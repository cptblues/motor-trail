//! Fixed, embedded nature models with shared canonical CPU geometry.
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::Collider;
use gltf::mesh::Mode;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

mod data;
#[cfg(test)]
mod tests;
mod validation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NatureAsset {
    TreeOakRound,
    TreeOakWide,
    TreeOakTall,
    TreeYoung,
    TreePineSmall,
    TreePineMedium,
    TreePineTall,
    RockBoulderSmall,
    RockBoulderMedium,
    RockBoulderLarge,
    RockSlabFlat,
    RockCluster,
    BushLow,
    BushRound,
    BushWild,
    GrassShort,
    GrassTall,
    GrassWild,
    FernSmall,
    FernLarge,
    LogFallenSmall,
    LogFallenLarge,
    StumpOld,
}

struct Geometry {
    // Independent triangle vertices retain material boundaries and flat shading.
    positions: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
    solid_triangles: Vec<[u32; 3]>,
    size: Vec3,
}

static GEOMETRY: [OnceLock<Result<Geometry, String>>; 23] = [const { OnceLock::new() }; 23];

impl NatureAsset {
    pub const TREES: [Self; 7] = [
        Self::TreeOakRound,
        Self::TreeOakWide,
        Self::TreeOakTall,
        Self::TreeYoung,
        Self::TreePineSmall,
        Self::TreePineMedium,
        Self::TreePineTall,
    ];
    pub const ROCKS: [Self; 5] = [
        Self::RockBoulderSmall,
        Self::RockBoulderMedium,
        Self::RockBoulderLarge,
        Self::RockSlabFlat,
        Self::RockCluster,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn asset_path(self) -> String {
        format!("3D/assets/{}.glb", self.id())
    }

    /// Measured full Y-up dimensions. Invalid embedded data yields an unusable
    /// zero size; mesh/collider construction returns the cached decoding error.
    pub fn default_size(self) -> Vec3 {
        self.geometry().map_or(Vec3::ZERO, |geometry| geometry.size)
    }

    pub fn build_mesh(self, size: Vec3) -> Result<Mesh, String> {
        let points = self.scaled_positions(size)?;
        let geometry = self.geometry()?;
        let mut normals = Vec::with_capacity(points.len());
        for triangle in points.chunks_exact(3) {
            let normal = face_normal(triangle[0], triangle[1], triangle[2])?;
            normals.extend([normal.to_array(); 3]);
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_indices(Indices::U32((0..points.len() as u32).collect()));
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            points.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, geometry.colors.clone());
        Ok(mesh)
    }

    pub fn collider(self, size: Vec3) -> Result<Option<Collider>, String> {
        let points = self.scaled_positions(size)?;
        let geometry = self.geometry()?;
        if geometry.solid_triangles.is_empty() {
            return Ok(None);
        }
        // Compact the physical vertices too, so foliage never enlarges its AABB.
        let mut vertices = Vec::with_capacity(geometry.solid_triangles.len() * 3);
        let mut triangles = Vec::with_capacity(geometry.solid_triangles.len());
        for triangle in &geometry.solid_triangles {
            let offset = vertices.len() as u32;
            vertices.extend(triangle.map(|index| points[index as usize]));
            triangles.push([offset, offset + 1, offset + 2]);
        }
        Collider::trimesh(vertices, triangles)
            .map(Some)
            .map_err(|error| format!("Nature {} collider: {error}", self.id()))
    }

    fn geometry(self) -> Result<&'static Geometry, String> {
        GEOMETRY[self.index()]
            .get_or_init(|| {
                decode(self, self.source_bytes())
                    .map_err(|error| format!("Nature {}: {error}", self.id()))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn scaled_positions(self, size: Vec3) -> Result<Vec<Vec3>, String> {
        if !size.is_finite()
            || size.cmplt(Vec3::splat(0.1)).any()
            || size.cmpgt(Vec3::splat(32.0)).any()
        {
            return Err("Nature dimensions must be finite and within 0.1..=32 m".into());
        }
        let geometry = self.geometry()?;
        let scale = size / geometry.size;
        let points: Vec<_> = geometry
            .positions
            .iter()
            .map(|point| *point * scale)
            .collect();
        if !scale.is_finite() || points.iter().any(|point| !point.is_finite()) {
            return Err("Nature dimensions overflow the baked geometry".into());
        }
        Ok(points)
    }

    fn is_tree(self) -> bool {
        Self::TREES.contains(&self)
    }

    fn is_solid(self, material: &str) -> bool {
        if self.is_tree() {
            matches!(material, "bark" | "bark_dark" | "bark_light" | "dead_bark")
        } else {
            Self::ROCKS.contains(&self)
                || matches!(
                    self,
                    Self::LogFallenSmall | Self::LogFallenLarge | Self::StumpOld
                )
        }
    }
}

fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Result<Vec3, String> {
    (b - a)
        .cross(c - a)
        .try_normalize()
        .filter(|normal| normal.is_finite())
        .ok_or_else(|| "Nature contains a degenerate or nonfinite triangle".into())
}

fn decode(asset: NatureAsset, bytes: &[u8]) -> Result<Geometry, String> {
    let source = validation::document(bytes)?;
    let blob = source
        .blob
        .as_deref()
        .ok_or("Nature GLB has no binary payload")?;
    let scene = source
        .default_scene()
        .or_else(|| source.scenes().next())
        .ok_or("Nature GLB has no scene")?;
    let mut geometry = Geometry {
        positions: Vec::new(),
        colors: Vec::new(),
        solid_triangles: Vec::new(),
        size: Vec3::ZERO,
    };
    let up = Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let mut pending: Vec<_> = scene.nodes().map(|node| (node, up)).collect();
    let mut visited = vec![false; source.nodes().len()];
    while let Some((node, parent)) = pending.pop() {
        if std::mem::replace(&mut visited[node.index()], true) {
            return Err("Nature scene has a cycle or a multiply parented node".into());
        }
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let determinant = transform.determinant();
        if !transform.is_finite()
            || !determinant.is_finite()
            || determinant == 0.0
            || transform.row(3) != Vec4::W
        {
            return Err("Nature node transform must be finite, affine and invertible".into());
        }
        pending.extend(node.children().map(|child| (child, transform)));
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                append_primitive(
                    asset,
                    primitive,
                    blob,
                    transform,
                    determinant < 0.0,
                    &mut geometry,
                )?;
            }
        }
    }
    if geometry.positions.is_empty()
        || (asset.is_solid("bark") && geometry.solid_triangles.is_empty())
    {
        return Err("Nature scene has no required render or solid geometry".into());
    }
    let min = geometry
        .positions
        .iter()
        .copied()
        .fold(Vec3::splat(f32::INFINITY), Vec3::min);
    let max = geometry
        .positions
        .iter()
        .copied()
        .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
    geometry.size = max - min;
    if !geometry.size.is_finite() || geometry.size.cmple(Vec3::splat(1e-6)).any() {
        return Err("Nature geometry has invalid bounding dimensions".into());
    }
    let center = (min + max) * 0.5;
    for point in &mut geometry.positions {
        *point -= center;
    }
    Ok(geometry)
}

fn append_primitive(
    asset: NatureAsset,
    primitive: gltf::Primitive<'_>,
    blob: &[u8],
    transform: Mat4,
    mirrored: bool,
    geometry: &mut Geometry,
) -> Result<(), String> {
    if primitive.mode() != Mode::Triangles
        || primitive.morph_targets().next().is_some()
        || primitive.attributes().any(|(semantic, _)| {
            !matches!(
                semantic,
                gltf::Semantic::Positions | gltf::Semantic::Normals
            )
        })
    {
        return Err("Nature requires static triangle positions and optional normals only".into());
    }
    let material = primitive.material();
    let color = material.pbr_metallic_roughness().base_color_factor();
    let name = material.name().unwrap_or("");
    if asset.is_tree()
        && !asset.is_solid(name)
        && !name.starts_with("leaf_")
        && !name.starts_with("pine_")
    {
        return Err(format!("Unsupported tree material: {name}"));
    }
    let accessor = primitive
        .get(&gltf::Semantic::Positions)
        .ok_or("Nature primitive has no positions")?;
    if accessor.data_type() != gltf::accessor::DataType::F32
        || accessor.dimensions() != gltf::accessor::Dimensions::Vec3
        || accessor.normalized()
    {
        return Err("Nature positions must be unnormalized float VEC3 values".into());
    }
    if let Some(indices) = primitive.indices() {
        if indices.dimensions() != gltf::accessor::Dimensions::Scalar
            || indices.normalized()
            || !matches!(
                indices.data_type(),
                gltf::accessor::DataType::U8
                    | gltf::accessor::DataType::U16
                    | gltf::accessor::DataType::U32
            )
        {
            return Err("Nature indices must be unsigned scalar values".into());
        }
    }
    let reader = primitive.reader(|_| Some(blob));
    let points: Vec<_> = reader
        .read_positions()
        .ok_or("Cannot read nature positions")?
        .map(|point| transform.transform_point3(Vec3::from_array(point)))
        .collect();
    if points
        .iter()
        .any(|point| !point.is_finite() || point.abs().max_element() > 1e6)
    {
        return Err("Nature coordinates must be finite and within +/-1000000 m".into());
    }
    let indices: Vec<_> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..points.len() as u32).collect(),
    };
    if indices.is_empty()
        || indices.len() % 3 != 0
        || indices.iter().any(|index| *index as usize >= points.len())
        || geometry
            .positions
            .len()
            .checked_add(indices.len())
            .is_none_or(|count| count > 100_000)
    {
        return Err("Nature contains invalid or excessive triangle indices".into());
    }
    for triangle in indices.chunks_exact(3) {
        let mut corners = [triangle[0], triangle[1], triangle[2]];
        if mirrored {
            corners.swap(1, 2);
        }
        let points = corners.map(|index| points[index as usize]);
        face_normal(points[0], points[1], points[2])?;
        let offset = geometry.positions.len() as u32;
        geometry.positions.extend(points);
        geometry.colors.extend([color; 3]);
        if asset.is_solid(name) {
            geometry
                .solid_triangles
                .push([offset, offset + 1, offset + 2]);
        }
    }
    Ok(())
}
