//! Static obstacles: dimensions are baked into both representations, never Transform scale.
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::Collider;
use serde::{Deserialize, Serialize};

use super::data::TerrainData;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObstacleKind {
    Cube,
    Cylinder,
    Sphere,
    Rock,
    Ramp,
}

impl ObstacleKind {
    pub const ALL: [Self; 5] = [
        Self::Cube,
        Self::Cylinder,
        Self::Sphere,
        Self::Rock,
        Self::Ramp,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cube => "Cube",
            Self::Cylinder => "Cylindre",
            Self::Sphere => "Sphere",
            Self::Rock => "Rocher",
            Self::Ramp => "Rampe",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obstacle {
    pub id: u64,
    pub kind: ObstacleKind,
    pub position: [f32; 3],
    /// Pitch (X), yaw (Y), roll (Z), composed in YXZ order.
    pub rotation_degrees: [f32; 3],
    /// Full local bounding-box dimensions, in metres.
    pub size: [f32; 3],
}

impl Obstacle {
    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0 {
            return Err("Obstacle ID must be greater than zero".into());
        }
        if !self
            .position
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 512.0)
        {
            return Err(format!(
                "Obstacle {} position must be finite and within +/-512 m",
                self.id
            ));
        }
        if !self.rotation_degrees.iter().all(|v| v.is_finite()) {
            return Err(format!("Obstacle {} rotation must be finite", self.id));
        }
        if !self
            .size
            .iter()
            .all(|v| v.is_finite() && (0.1..=32.0).contains(v))
        {
            return Err(format!(
                "Obstacle {} dimensions must be within 0.1..=32 m",
                self.id
            ));
        }
        Ok(())
    }

    pub fn transform(&self) -> Transform {
        // Reducing degrees first also avoids overflow for large finite inputs.
        let [pitch, yaw, roll] = self.rotation_degrees.map(|v| (v % 360.0).to_radians());
        Transform {
            translation: Vec3::from_array(self.position),
            rotation: Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll),
            scale: Vec3::ONE,
        }
    }

    /// Builds baked render geometry without constructing any physical shape.
    pub fn build_mesh(&self) -> Result<Mesh, String> {
        self.validate()?;
        let dimensions = Vec3::from_array(self.size);
        match self.kind {
            ObstacleKind::Cube => Ok(Mesh::from(Cuboid::new(
                dimensions.x,
                dimensions.y,
                dimensions.z,
            ))),
            ObstacleKind::Cylinder | ObstacleKind::Sphere => {
                let mut mesh = if self.kind == ObstacleKind::Sphere {
                    Sphere::new(0.5).mesh().uv(24, 16)
                } else {
                    Mesh::from(Cylinder::new(0.5, 1.0))
                };
                mesh.scale_by(dimensions);
                Ok(mesh)
            }
            ObstacleKind::Rock | ObstacleKind::Ramp => {
                let (points, triangles) = polyhedron_geometry(self.kind, dimensions);
                Ok(polyhedron_mesh(&points, triangles))
            }
        }
    }

    pub fn build(&self) -> Result<(Mesh, Collider), String> {
        let mesh = self.build_mesh()?;
        let dimensions = Vec3::from_array(self.size);
        let collider = match self.kind {
            ObstacleKind::Cube => {
                Collider::cuboid(dimensions.x / 2.0, dimensions.y / 2.0, dimensions.z / 2.0)
            }
            ObstacleKind::Cylinder | ObstacleKind::Sphere => {
                let (points, triangles) = mesh_geometry(&mesh)?;
                // Static exact triangles preserve ellipsoids and elliptic cylinders.
                Collider::trimesh(points, triangles)
                    .map_err(|e| format!("Cannot build obstacle {} collider: {e}", self.id))?
            }
            ObstacleKind::Rock | ObstacleKind::Ramp => {
                let (points, _) = polyhedron_geometry(self.kind, dimensions);
                Collider::convex_hull(&points)
                    .ok_or_else(|| format!("Cannot build obstacle {} convex hull", self.id))?
            }
        };
        Ok((mesh, collider))
    }

    pub fn align_to_ground(&mut self, terrain: &TerrainData) -> Result<(), String> {
        self.validate()?;
        terrain.validate()?;
        let [x, _, z] = self.position;
        let height = terrain
            .height(x, z)
            .ok_or("Obstacle lies outside the terrain")?;
        let normal = terrain.normal(x, z).ok_or("Cannot sample ground normal")?;
        if !normal.is_finite() || normal.y <= 1e-5 {
            return Err("Cannot align an obstacle to this ground normal".into());
        }
        let rotation = Quat::from_rotation_arc(Vec3::Y, normal)
            * Quat::from_rotation_y((self.rotation_degrees[1] % 360.0).to_radians());
        let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
        let mut aligned = self.clone();
        aligned.rotation_degrees = [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()];
        // Rotation maps local Y to the normal; every catalog shape reaches local -size.y/2.
        aligned.position[1] = height + self.size[1] * 0.5 / normal.y;
        aligned.validate()?;
        *self = aligned;
        Ok(())
    }
}

fn polyhedron_geometry(kind: ObstacleKind, dimensions: Vec3) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let (vertices, triangles) = if kind == ObstacleKind::Ramp {
        (
            vec![
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
            ],
            vec![
                [0, 2, 1],
                [1, 2, 3],
                [0, 1, 4],
                [1, 5, 4],
                [2, 4, 3],
                [3, 4, 5],
                [0, 4, 2],
                [1, 3, 5],
            ],
        )
    } else {
        (
            vec![
                [0.08, 0.5, 0.04],
                [-0.03, -0.5, -0.06],
                [-0.5, -0.10, 0.0],
                [0.0, -0.12, -0.5],
                [0.5, -0.08, 0.0],
                [0.0, -0.14, 0.5],
            ],
            vec![
                [0, 2, 3],
                [0, 3, 4],
                [0, 4, 5],
                [0, 5, 2],
                [1, 3, 2],
                [1, 4, 3],
                [1, 5, 4],
                [1, 2, 5],
            ],
        )
    };
    let points = vertices
        .into_iter()
        .map(|v| Vec3::from_array(v) * dimensions)
        .collect();
    (points, triangles)
}

fn mesh_geometry(mesh: &Mesh) -> Result<(Vec<Vec3>, Vec<[u32; 3]>), String> {
    let Some(VertexAttributeValues::Float32x3(vertices)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err("Obstacle mesh is missing Float32x3 positions".into());
    };
    let points = vertices.iter().copied().map(Vec3::from_array).collect();
    let indices: Vec<usize> = match mesh.indices() {
        Some(indices) => indices.iter().collect(),
        None => (0..vertices.len()).collect(),
    };
    if indices.len() % 3 != 0 || indices.iter().any(|i| *i >= vertices.len()) {
        return Err("Obstacle mesh has invalid triangle indices".into());
    }
    let triangles = indices
        .chunks_exact(3)
        .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
        .collect();
    Ok((points, triangles))
}

fn polyhedron_mesh(points: &[Vec3], mut triangles: Vec<[u32; 3]>) -> Mesh {
    let interior = points.iter().copied().sum::<Vec3>() / points.len() as f32;
    for triangle in &mut triangles {
        let [a, b, c] = triangle.map(|i| points[i as usize]);
        if (b - a).cross(c - a).dot((a + b + c) / 3.0 - interior) < 0.0 {
            triangle.swap(1, 2);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        points.iter().map(|v| v.to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_indices(Indices::U32(triangles.into_iter().flatten().collect()));
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obstacle(kind: ObstacleKind) -> Obstacle {
        Obstacle {
            id: 1,
            kind,
            position: [0.0; 3],
            rotation_degrees: [0.0; 3],
            size: [2.0, 3.0, 4.0],
        }
    }

    #[test]
    fn all_shapes_have_baked_dimensions_unit_scale_and_normals() {
        for kind in ObstacleKind::ALL {
            let object = obstacle(kind);
            let mesh = object.build_mesh().unwrap();
            let (points, _) = mesh_geometry(&mesh).unwrap();
            let min = points
                .iter()
                .copied()
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let max = points
                .iter()
                .copied()
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            assert!(
                (max - min - Vec3::from_array(object.size)).length() < 1e-4,
                "{kind:?}"
            );
            assert_eq!(object.transform().scale, Vec3::ONE);
            let Some(VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("missing normals for {kind:?}");
            };
            assert!(
                normals
                    .iter()
                    .all(|n| (Vec3::from_array(*n).length() - 1.0).abs() < 1e-4)
            );
        }
    }

    #[test]
    fn placement_mesh_factory_matches_full_factory_for_every_catalog_shape() {
        for kind in ObstacleKind::ALL {
            for size in [[2.0, 3.0, 4.0], [0.1, 32.0, 0.3], [32.0, 0.1, 7.0]] {
                let mut object = obstacle(kind);
                object.size = size;
                let preview = object.build_mesh().unwrap();
                let (committed, _) = object.build().unwrap();
                assert_eq!(
                    mesh_geometry(&preview).unwrap(),
                    mesh_geometry(&committed).unwrap(),
                    "{kind:?}, size {size:?}"
                );
                assert_eq!(preview.primitive_topology(), committed.primitive_topology());
                assert_eq!(
                    preview
                        .attribute(Mesh::ATTRIBUTE_NORMAL)
                        .unwrap()
                        .as_float3(),
                    committed
                        .attribute(Mesh::ATTRIBUTE_NORMAL)
                        .unwrap()
                        .as_float3(),
                    "{kind:?}, size {size:?}"
                );
                match (
                    preview.attribute(Mesh::ATTRIBUTE_UV_0),
                    committed.attribute(Mesh::ATTRIBUTE_UV_0),
                ) {
                    (
                        Some(VertexAttributeValues::Float32x2(preview)),
                        Some(VertexAttributeValues::Float32x2(committed)),
                    ) => assert_eq!(preview, committed, "{kind:?}, size {size:?}"),
                    (None, None) => {}
                    _ => panic!("UV mismatch for {kind:?}, size {size:?}"),
                }
                assert_eq!(object.transform().scale, Vec3::ONE);
            }
        }
    }

    #[test]
    fn curved_shapes_use_exact_baked_triangle_colliders() {
        for kind in [ObstacleKind::Cylinder, ObstacleKind::Sphere] {
            let (mesh, collider) = obstacle(kind).build().unwrap();
            let (points, triangles) = mesh_geometry(&mesh).unwrap();
            let physical = collider.as_trimesh().unwrap();
            assert_eq!(physical.vertices().collect::<Vec<_>>(), points);
            assert_eq!(physical.indices(), triangles);
        }
    }

    #[test]
    fn ramp_is_closed_outward_and_rises_towards_positive_z() {
        let (mesh, collider) = obstacle(ObstacleKind::Ramp).build().unwrap();
        assert!(collider.as_convex_polyhedron().is_some());
        let (points, triangles) = mesh_geometry(&mesh).unwrap();
        let interior = Vec3::new(0.0, -0.5, 0.5);
        for [a, b, c] in triangles {
            let [a, b, c] = [points[a as usize], points[b as usize], points[c as usize]];
            assert!((b - a).cross(c - a).dot((a + b + c) / 3.0 - interior) > 0.0);
        }
        assert!(points.iter().filter(|p| p.z < 0.0).all(|p| p.y == -1.5));
        assert!(points.iter().any(|p| p.z == 2.0 && p.y == 1.5));
    }

    #[test]
    fn rejects_bad_ids_dimensions_and_nonfinite_transforms() {
        let mut object = obstacle(ObstacleKind::Cube);
        object.id = 0;
        assert!(object.validate().is_err());
        assert!(object.build_mesh().is_err());
        object.id = 1;
        for value in [0.0, 0.09, 32.1, f32::NAN, f32::INFINITY] {
            object.size[0] = value;
            assert!(object.build().is_err());
            assert!(object.build_mesh().is_err());
        }
        object.size[0] = 1.0;
        object.rotation_degrees[0] = f32::NAN;
        assert!(object.validate().is_err());
        assert!(object.build_mesh().is_err());
        object.rotation_degrees[0] = 0.0;
        object.position[2] = 513.0;
        assert!(object.validate().is_err());
        assert!(object.build_mesh().is_err());
    }

    #[test]
    fn ground_alignment_rotates_up_and_places_lowest_vertex_on_flat_ground() {
        let mut terrain = super::super::generation::generate(
            &super::super::config::TerrainConfig::preset(super::super::config::TerrainPreset::Flat),
        )
        .unwrap();
        terrain.heights.fill(2.0);
        let mut object = obstacle(ObstacleKind::Ramp);
        object.rotation_degrees = [20.0, 35.0, -15.0];
        object.align_to_ground(&terrain).unwrap();
        let transform = object.transform();
        assert!((transform.rotation * Vec3::Y - Vec3::Y).length() < 1e-5);
        let (mesh, _) = object.build().unwrap();
        let (points, _) = mesh_geometry(&mesh).unwrap();
        let bottom = points
            .iter()
            .map(|p| transform.transform_point(*p).y)
            .fold(f32::INFINITY, f32::min);
        assert!((bottom - 2.0).abs() < 1e-4);
        object.position[0] = 512.0;
        let before = object.clone();
        assert!(object.align_to_ground(&terrain).is_err());
        assert_eq!(object, before);
    }

    #[test]
    fn aligned_rock_matches_sloping_ground_normal_and_convex_mesh_points() {
        let mut terrain = super::super::generation::generate(
            &super::super::config::TerrainConfig::preset(super::super::config::TerrainPreset::Flat),
        )
        .unwrap();
        for index in 0..terrain.heights.len() {
            terrain.heights[index] = terrain.vertex_position(index).x * 0.25;
        }
        let mut object = obstacle(ObstacleKind::Rock);
        object.rotation_degrees[1] = 35.0;
        object.align_to_ground(&terrain).unwrap();
        let normal = terrain.normal(0.0, 0.0).unwrap();
        let transform = object.transform();
        assert!((transform.rotation * Vec3::Y - normal).length() < 1e-5);
        let (mesh, collider) = object.build().unwrap();
        let (points, _) = mesh_geometry(&mesh).unwrap();
        let hull = collider.as_convex_polyhedron().unwrap();
        assert!(points.iter().all(|p| hull.points().any(|h| h == *p)));
        let min_distance = points
            .iter()
            .map(|p| transform.transform_point(*p).dot(normal))
            .fold(f32::INFINITY, f32::min);
        assert!(min_distance.abs() < 1e-4);
    }

    #[test]
    fn analytic_alignment_matches_built_geometry_for_all_catalog_shapes() {
        use super::super::config::{TerrainConfig, TerrainPreset};

        let config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            resolution: 4.0,
            ..TerrainConfig::preset(TerrainPreset::Flat)
        };
        for [slope_x, slope_z] in [[0.0, 0.0], [0.25, 0.0], [-0.2, 0.4], [1.25, -0.75]] {
            let mut terrain = TerrainData::from_heights(config.clone(), vec![0.0; 9]).unwrap();
            for index in 0..terrain.heights.len() {
                let point = terrain.vertex_position(index);
                terrain.heights[index] = 1.25 + point.x * slope_x + point.z * slope_z;
            }
            terrain.validate().unwrap();
            let [x, z] = [0.5, -0.75];
            let height = terrain.height(x, z).unwrap();
            let normal = terrain.normal(x, z).unwrap();
            for kind in ObstacleKind::ALL {
                for size in [
                    [2.0, 3.0, 4.0],
                    [0.1, 32.0, 0.3],
                    [32.0, 0.1, 7.0],
                    [0.7, 5.0, 32.0],
                ] {
                    let mut base = obstacle(kind);
                    base.position = [x, -40.0, z];
                    base.size = size;
                    let (mesh, _) = base.build().unwrap();
                    let (points, _) = mesh_geometry(&mesh).unwrap();
                    for yaw in [0.0_f32, 35.0, -130.0] {
                        let mut aligned = base.clone();
                        aligned.rotation_degrees = [20.0, yaw, -15.0];
                        let expected_rotation = Quat::from_rotation_arc(Vec3::Y, normal)
                            * Quat::from_rotation_y(yaw.to_radians());
                        aligned.align_to_ground(&terrain).unwrap();
                        let transform = aligned.transform();
                        let geometry_support = points
                            .iter()
                            .map(|point| (transform.rotation * *point).dot(normal))
                            .fold(f32::INFINITY, f32::min);
                        let analytic_support = -size[1] / 2.0;
                        let case = format!(
                            "{kind:?}, size {size:?}, slope [{slope_x}, {slope_z}], yaw {yaw}"
                        );
                        assert!((geometry_support - analytic_support).abs() < 1e-4, "{case}");
                        assert!(
                            (aligned.position[1] - (height - geometry_support / normal.y)).abs()
                                < 2e-4,
                            "{case}"
                        );
                        assert!(
                            (aligned.position[1] - (height + size[1] / (2.0 * normal.y))).abs()
                                < 1e-4,
                            "{case}"
                        );
                        assert!(
                            transform.rotation.dot(expected_rotation).abs() > 1.0 - 1e-5,
                            "{case}"
                        );
                        assert!(
                            (transform.rotation * Vec3::Y - normal).length() < 1e-5,
                            "{case}"
                        );
                        assert_eq!([aligned.position[0], aligned.position[2]], [x, z]);
                        assert_eq!(transform.scale, Vec3::ONE);
                    }
                }
            }
        }
    }
}
