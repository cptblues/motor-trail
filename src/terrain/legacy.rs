//! Historical valley geometry and scenery, preserved independently of the editor.
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::*;

use super::data::TerrainData;

#[derive(Component)]
pub struct LegacyRoad;

pub const WIDTH: f32 = 120.0;
pub const DEPTH: f32 = 165.0;
pub const START: f32 = -67.0;
pub const FINISH: f32 = 65.0;
pub fn road_x(z: f32) -> f32 {
    (z * 0.065).sin() * 4.0
}

pub fn height(x: f32, z: f32) -> f32 {
    let t = ((z + 38.0) / 102.0).clamp(0.0, 1.0);
    let climb = 15.0 * t * t * (3.0 - 2.0 * t);
    let bumps =
        0.55 * (x * 0.11).sin() * (z * 0.10).cos() + 0.38 * (x * 0.31).cos() * (z * 0.23).sin();
    climb + bumps + (x / 46.0).powi(2) * 2.0
}

pub fn normal(x: f32, z: f32) -> Vec3 {
    let d = 0.4;
    Vec3::new(
        height(x - d, z) - height(x + d, z),
        2.0 * d,
        height(x, z - d) - height(x, z + d),
    )
    .normalize()
}

pub fn geometry(vertices: Vec<[f32; 3]>, normals: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
pub fn ground() -> (Mesh, Collider) {
    const NX: usize = 96;
    const NZ: usize = 132;
    let mut points = Vec::new();
    let mut normals = Vec::new();
    let mut triangles = Vec::new();
    let mut indices = Vec::new();
    for iz in 0..=NZ {
        for ix in 0..=NX {
            let x = (ix as f32 / NX as f32 - 0.5) * WIDTH;
            let z = (iz as f32 / NZ as f32 - 0.5) * DEPTH;
            points.push(Vec3::new(x, height(x, z), z));
            normals.push(normal(x, z).to_array());
        }
    }
    for z in 0..NZ {
        for x in 0..NX {
            let a = (z * (NX + 1) + x) as u32;
            let b = a + 1;
            let c = a + (NX + 1) as u32;
            let d = c + 1;
            // Triangle winding: +Y.
            indices.extend_from_slice(&[a, c, b, b, c, d]);
            triangles.push([a, c, b]);
            triangles.push([b, c, d]);
        }
    }
    let collider = Collider::trimesh(points.clone(), triangles)
        .expect("Le terrain doit avoir un maillage physique valide");
    let verts = points.into_iter().map(|v| v.to_array()).collect();
    (geometry(verts, normals, indices), collider)
}

pub fn road() -> Mesh {
    let count = 150usize;
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for s in 0..=count {
        let z = START - 4.0 + s as f32 / count as f32 * (FINISH - START + 12.0);
        for side in [-1.0f32, 1.0] {
            let x = road_x(z) + side * 3.25;
            vertices.push([x, height(x, z) + 0.075, z]);
            normals.push(normal(x, z).to_array());
        }
        if s < count {
            let a = (s * 2) as u32;
            indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }
    }
    geometry(vertices, normals, indices)
}

/// Sample the canonical terrain, omitting cross-sections outside its domain.
pub fn road_on(data: &TerrainData) -> Mesh {
    let count = 150usize;
    let mut vertices = Vec::with_capacity((count + 1) * 2);
    let mut normals = Vec::with_capacity((count + 1) * 2);
    let mut indices = Vec::with_capacity(count * 6);
    let mut previous_row: Option<u32> = None;
    for s in 0..=count {
        let z = START - 4.0 + s as f32 / count as f32 * (FINISH - START + 12.0);
        let samples = [-1.0f32, 1.0].map(|side| {
            let x = road_x(z) + side * 3.25;
            Some((
                Vec3::new(x, data.height(x, z)? + 0.075, z),
                data.normal(x, z)?,
            ))
        });
        let [Some(left), Some(right)] = samples else {
            previous_row = None;
            continue;
        };
        let base = vertices.len() as u32;
        for (point, normal) in [left, right] {
            vertices.push(point.to_array());
            normals.push(normal.to_array());
        }
        if let Some(a) = previous_row {
            indices.extend_from_slice(&[a, base, a + 1, a + 1, base, base + 1]);
        }
        previous_row = Some(base);
    }
    geometry(vertices, normals, indices)
}

fn pseudo(seed: f32) -> f32 {
    (seed.sin() * 43758.547).fract().abs()
}

pub fn forest(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
) {
    let trunk_mat = materials.add(Color::srgb(0.31, 0.23, 0.17));
    let leaves = [
        materials.add(Color::srgb(0.15, 0.35, 0.23)),
        materials.add(Color::srgb(0.18, 0.41, 0.28)),
        materials.add(Color::srgb(0.21, 0.36, 0.29)),
    ];
    let trunk_mesh = meshes.add(Cylinder::new(0.28, 2.4));
    let pine_mesh = meshes.add(Cone::new(1.8, 4.0));
    for i in -6..=6 {
        for j in -8..=8 {
            let seed = (i * 71 + j * 127) as f32;
            let x = i as f32 * 9.1 + (pseudo(seed + 1.4) - 0.5) * 4.5;
            let z = j as f32 * 9.6 + (pseudo(seed + 7.7) - 0.5) * 4.5;
            if x.abs() > WIDTH * 0.47 || z.abs() > DEPTH * 0.47 || (x - road_x(z)).abs() < 8.0 {
                continue;
            }
            let y = height(x, z);
            let tree_height = 3.8 + pseudo(seed + 3.2) * 2.1;
            let stem_height = 1.8 + tree_height * 0.22;
            commands.spawn((
                ChildOf(root),
                Mesh3d(trunk_mesh.clone()),
                MeshMaterial3d(trunk_mat.clone()),
                Transform::from_xyz(x, y + stem_height * 0.5, z).with_scale(Vec3::new(
                    1.0,
                    stem_height / 2.4,
                    1.0,
                )),
                RigidBody::Fixed,
                Collider::cylinder(1.2, 0.28),
            ));
            commands.spawn((
                ChildOf(root),
                Mesh3d(pine_mesh.clone()),
                MeshMaterial3d(leaves[(pseudo(seed + 15.0) * 3.0) as usize].clone()),
                Transform::from_xyz(x, y + stem_height + tree_height * 0.35, z)
                    .with_scale(Vec3::new(0.82, tree_height / 4.0, 0.82)),
            ));
        }
    }
}

pub fn rocks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let colors = [
        materials.add(Color::srgb(0.43, 0.44, 0.43)),
        materials.add(Color::srgb(0.54, 0.52, 0.49)),
        materials.add(Color::srgb(0.40, 0.46, 0.43)),
    ];
    for i in 0..38 {
        let seed = i as f32;
        let z = -61.0 + pseudo(seed * 4.37 + 1.0) * 128.0;
        let offset = 5.0 + pseudo(seed * 9.57 + 6.0) * 40.0;
        let x = road_x(z) + if i % 2 == 0 { offset } else { -offset };
        let half = Vec3::new(
            0.6 + pseudo(seed + 4.0) * 1.2,
            0.45 + pseudo(seed + 7.0) * 1.2,
            0.6 + pseudo(seed + 11.0),
        );
        commands.spawn((
            ChildOf(root),
            Mesh3d(cube.clone()),
            MeshMaterial3d(colors[i % 3].clone()),
            Transform::from_xyz(x, height(x, z) + half.y * 0.7, z)
                .with_rotation(Quat::from_rotation_y(pseudo(seed + 12.0) * 2.0))
                .with_scale(half * 2.0),
            RigidBody::Fixed,
            Collider::cuboid(0.5, 0.5, 0.5),
        ));
    }
    // Rocher remarquable, juste sur le bord de la piste.
    let z = 24.0;
    let x = road_x(z) + 1.3;
    commands.spawn((
        ChildOf(root),
        Mesh3d(cube),
        MeshMaterial3d(colors[1].clone()),
        Transform::from_xyz(x, height(x, z) + 0.38, z).with_scale(Vec3::new(1.1, 0.76, 1.4)),
        RigidBody::Fixed,
        Collider::cuboid(0.5, 0.5, 0.5),
    ));
}

pub fn refuge(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
) {
    let x = road_x(FINISH) + 7.0;
    let z = FINISH;
    let y = height(x, z);
    commands.spawn((
        ChildOf(root),
        Mesh3d(meshes.add(Cuboid::new(5.0, 3.2, 4.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.60, 0.33, 0.20))),
        Transform::from_xyz(x, y + 1.6, z),
        RigidBody::Fixed,
        Collider::cuboid(2.5, 1.6, 2.0),
    ));
    commands.spawn((
        ChildOf(root),
        Mesh3d(meshes.add(Cuboid::new(5.8, 0.55, 4.8))),
        MeshMaterial3d(materials.add(Color::srgb(0.28, 0.29, 0.30))),
        Transform::from_xyz(x, y + 3.45, z),
    ));
    commands.spawn((
        ChildOf(root),
        Mesh3d(meshes.add(Cylinder::new(0.09, 5.8))),
        MeshMaterial3d(materials.add(Color::srgb(0.76, 0.77, 0.75))),
        Transform::from_xyz(x - 3.7, y + 2.9, z),
    ));
    commands.spawn((
        ChildOf(root),
        Mesh3d(meshes.add(Cuboid::new(1.8, 1.0, 0.08))),
        MeshMaterial3d(materials.add(Color::srgb(0.94, 0.66, 0.23))),
        Transform::from_xyz(x - 2.8, y + 5.2, z),
    ));
}

pub fn spawn_decor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    road_mesh: Mesh,
) {
    commands.spawn((
        ChildOf(root),
        LegacyRoad,
        Mesh3d(meshes.add(road_mesh)),
        MeshMaterial3d(materials.add(Color::srgb(0.57, 0.44, 0.31))),
    ));
    forest(commands, meshes, materials, root);
    rocks(commands, meshes, materials, root);
    refuge(commands, meshes, materials, root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::config::{TerrainConfig, TerrainPreset};
    use crate::terrain::data::TerrainData;

    #[test]
    fn canonical_road_follows_edited_heights_and_normals_including_a_local_peak() {
        let config = TerrainConfig::preset(TerrainPreset::Legacy);
        let (nx, nz) = config.cells();
        let mut data = TerrainData::from_heights(config, vec![1.0; (nx + 1) * (nz + 1)]).unwrap();
        let before = road_on(&data);
        let target = before
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap()[150];
        let ix = ((target[0] / data.config.width + 0.5) * nx as f32).round() as usize;
        let iz = ((target[2] / data.config.depth + 0.5) * nz as f32).round() as usize;
        data.heights.fill(2.0);
        data.heights[iz * (nx + 1) + ix] = 32.0;

        let mesh = road_on(&data);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let normals = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
            .as_float3()
            .unwrap();
        assert_eq!(positions.len(), 302);
        assert_eq!(normals.len(), positions.len());
        assert!(positions[150][1] > 3.0, "edited peak must affect the road");
        for (point, normal) in positions.iter().zip(normals) {
            assert_eq!(point[1], data.height(point[0], point[2]).unwrap() + 0.075);
            assert_eq!(*normal, data.normal(point[0], point[2]).unwrap().to_array());
            assert!(normal.iter().all(|value| value.is_finite()));
            assert!(normal[1] > 0.0);
        }
        let Indices::U32(indices) = mesh.indices().unwrap() else {
            panic!("road indices must be u32")
        };
        assert_eq!(indices.len(), 900);
        assert!(
            indices
                .iter()
                .all(|&index| (index as usize) < positions.len())
        );
        for triangle in indices.chunks_exact(3) {
            let a = Vec3::from_array(positions[triangle[0] as usize]);
            let b = Vec3::from_array(positions[triangle[1] as usize]);
            let c = Vec3::from_array(positions[triangle[2] as usize]);
            assert!((b - a).cross(c - a).y > 0.0);
        }
    }

    #[test]
    fn canonical_road_omits_sections_outside_a_smaller_terrain_grid() {
        let config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            resolution: 1.0,
            ..TerrainConfig::preset(TerrainPreset::Flat)
        };
        let data = TerrainData::from_heights(config, vec![7.0; 81]).unwrap();
        let mesh = road_on(&data);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        assert!(!positions.is_empty() && positions.len() < 302);
        for point in positions {
            assert_eq!(point[1], data.height(point[0], point[2]).unwrap() + 0.075);
        }
        let indices = mesh.indices().unwrap();
        assert!(indices.len() > 0);
        assert!(indices.iter().all(|index| index < positions.len()));
    }
}
