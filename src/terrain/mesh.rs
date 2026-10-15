use super::data::TerrainData;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::Collider;

pub fn build(data: &TerrainData) -> Result<(Mesh, Collider), String> {
    data.validate()?;
    let points = data.points();
    let triangles = data.triangles();
    let mut normals = vec![Vec3::ZERO; points.len()];
    for &[a, b, c] in &triangles {
        let normal = (points[b as usize] - points[a as usize])
            .cross(points[c as usize] - points[a as usize]);
        for index in [a, b, c] {
            normals[index as usize] += normal;
        }
    }
    let collider = Collider::trimesh(points.clone(), triangles.clone())
        .map_err(|error| format!("Terrain collider: {error}"))?;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        points.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        normals
            .into_iter()
            .map(|n| n.normalize_or(Vec3::Y).to_array())
            .collect::<Vec<_>>(),
    );
    mesh.insert_indices(Indices::U32(triangles.into_iter().flatten().collect()));
    Ok((mesh, collider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{
        config::{TerrainConfig, TerrainPreset},
        generation,
    };
    #[test]
    fn mesh_and_physics_share_every_vertex_and_triangle() {
        let data = generation::generate(&TerrainConfig::preset(TerrainPreset::Trial)).unwrap();
        let (mesh, collider) = build(&data).unwrap();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(positions.len(), data.vertex_count());
        for (index, point) in positions.iter().enumerate() {
            assert_eq!(*point, data.vertex_position(index).to_array());
        }
        let shape = collider.as_trimesh().unwrap();
        assert_eq!(shape.vertices().len(), positions.len());
        assert_eq!(shape.indices(), data.triangles());
        for (position, physics) in positions.iter().zip(shape.vertices()) {
            assert_eq!(*position, physics.to_array());
        }
        assert_eq!(mesh.indices().unwrap().len(), data.triangle_count() * 3);
    }
}
