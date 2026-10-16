//! Historical valley geometry and scenery, preserved independently of the editor.
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::*;

use super::data::TerrainData;
use super::nature::NatureAsset;

#[derive(Component)]
pub struct LegacyRoad;

/// Canonical terrain sample and vertical offset used to reground scenery after edits.
#[derive(Component)]
pub struct NatureDecorAnchor {
    pub x: f32,
    pub z: f32,
    pub offset_y: f32,
}

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

struct NatureDecorModel {
    mesh: Handle<Mesh>,
    collider: Collider,
    size: Vec3,
}

impl NatureDecorModel {
    fn new(asset: NatureAsset, meshes: &mut Assets<Mesh>) -> Self {
        let size = asset.default_size();
        Self {
            mesh: meshes.add(
                asset
                    .build_mesh(size)
                    .expect("Historical nature mesh must be valid"),
            ),
            collider: asset
                .collider(size)
                .expect("Historical nature collider must be valid")
                .expect("Historical trees and rocks must have colliders"),
            size,
        }
    }

    fn spawn(
        &self,
        commands: &mut Commands,
        material: &Handle<StandardMaterial>,
        root: Entity,
        mut transform: Transform,
        offset_y: f32,
    ) {
        let anchor = NatureDecorAnchor {
            x: transform.translation.x,
            z: transform.translation.z,
            offset_y,
        };
        transform.translation.y += offset_y;
        commands.spawn((
            ChildOf(root),
            anchor,
            Mesh3d(self.mesh.clone()),
            MeshMaterial3d(material.clone()),
            transform,
            RigidBody::Fixed,
            self.collider.clone(),
        ));
    }
}

pub fn forest(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    root: Entity,
    data: &TerrainData,
) {
    let mut models: [Option<NatureDecorModel>; 7] = std::array::from_fn(|_| None);
    for i in -6..=6 {
        for j in -8..=8 {
            let seed = (i * 71 + j * 127) as f32;
            let x = i as f32 * 9.1 + (pseudo(seed + 1.4) - 0.5) * 4.5;
            let z = j as f32 * 9.6 + (pseudo(seed + 7.7) - 0.5) * 4.5;
            if x.abs() > WIDTH * 0.47 || z.abs() > DEPTH * 0.47 || (x - road_x(z)).abs() < 8.0 {
                continue;
            }
            let Some(y) = data.height(x, z) else {
                continue;
            };
            let index = (pseudo(seed + 15.0) * models.len() as f32) as usize;
            let model = models[index]
                .get_or_insert_with(|| NatureDecorModel::new(NatureAsset::TREES[index], meshes));
            let scale = 0.85 + pseudo(seed + 3.2) * 0.3;
            model.spawn(
                commands,
                material,
                root,
                Transform::from_xyz(x, y, z)
                    .with_rotation(Quat::from_rotation_y(
                        pseudo(seed + 21.3) * std::f32::consts::TAU,
                    ))
                    .with_scale(Vec3::splat(scale)),
                model.size.y * scale * 0.5,
            );
        }
    }
}

pub fn rocks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    root: Entity,
    data: &TerrainData,
) {
    let mut models: [Option<NatureDecorModel>; 5] = std::array::from_fn(|_| None);
    for i in 0..38 {
        let seed = i as f32;
        let z = -61.0 + pseudo(seed * 4.37 + 1.0) * 128.0;
        let offset = 5.0 + pseudo(seed * 9.57 + 6.0) * 40.0;
        let x = road_x(z) + if i % 2 == 0 { offset } else { -offset };
        let Some(y) = data.height(x, z) else {
            continue;
        };
        let index = i % models.len();
        let model = models[index]
            .get_or_insert_with(|| NatureDecorModel::new(NatureAsset::ROCKS[index], meshes));
        let scale = 0.8 + pseudo(seed + 4.0) * 0.6;
        model.spawn(
            commands,
            material,
            root,
            Transform::from_xyz(x, y, z)
                .with_rotation(Quat::from_rotation_y(pseudo(seed + 12.0) * 2.0))
                .with_scale(Vec3::splat(scale)),
            model.size.y * scale * 0.35,
        );
    }
    // Rocher remarquable, juste sur le bord de la piste.
    let z = 24.0;
    let x = road_x(z) + 1.3;
    if let Some(y) = data.height(x, z) {
        let index = NatureAsset::ROCKS
            .iter()
            .position(|&asset| asset == NatureAsset::RockBoulderMedium)
            .expect("Historical rocks include the medium boulder");
        let model = models[index]
            .get_or_insert_with(|| NatureDecorModel::new(NatureAsset::ROCKS[index], meshes));
        let scale = 0.76 / model.size.y;
        model.spawn(
            commands,
            material,
            root,
            Transform::from_xyz(x, y, z).with_scale(Vec3::splat(scale)),
            model.size.y * scale * 0.5,
        );
    }
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
    data: &TerrainData,
) {
    commands.spawn((
        ChildOf(root),
        LegacyRoad,
        Mesh3d(meshes.add(road_mesh)),
        MeshMaterial3d(materials.add(Color::srgb(0.57, 0.44, 0.31))),
    ));
    let nature_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        metallic: 0.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    forest(commands, meshes, &nature_material, root, data);
    rocks(commands, meshes, &nature_material, root, data);
    refuge(commands, meshes, materials, root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::config::{TerrainConfig, TerrainPreset};
    use crate::terrain::data::TerrainData;
    use crate::terrain::generation;
    use bevy::ecs::world::CommandQueue;
    use std::collections::{HashMap, HashSet};

    fn decor_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>();
        app
    }

    fn spawn_test_decor(app: &mut App, data: &TerrainData) -> Entity {
        let root = app.world_mut().spawn_empty().id();
        let mut queue = CommandQueue::default();
        app.world_mut()
            .resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
                world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
                    let mut commands = Commands::new(&mut queue, world);
                    spawn_decor(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        root,
                        road_on(data),
                        data,
                    );
                });
            });
        queue.apply(app.world_mut());
        root
    }

    fn nature_meshes(app: &App) -> HashMap<AssetId<Mesh>, NatureAsset> {
        let expected: Vec<_> = NatureAsset::TREES
            .into_iter()
            .chain(NatureAsset::ROCKS)
            .map(|asset| (asset, asset.build_mesh(asset.default_size()).unwrap()))
            .collect();
        app.world()
            .resource::<Assets<Mesh>>()
            .iter()
            .filter(|(_, mesh)| mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some())
            .map(|(id, mesh)| {
                let positions = mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .as_float3();
                let (asset, expected_mesh) = expected
                    .iter()
                    .find(|(_, expected)| {
                        expected
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                            .unwrap()
                            .as_float3()
                            == positions
                    })
                    .expect("decoration must use a natural-size tree or rock mesh");
                assert_eq!(
                    mesh.indices().unwrap().iter().collect::<Vec<_>>(),
                    expected_mesh.indices().unwrap().iter().collect::<Vec<_>>()
                );
                assert_eq!(
                    mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap().len(),
                    positions.unwrap().len()
                );
                (id, *asset)
            })
            .collect()
    }

    #[test]
    fn historical_nature_preserves_density_clearance_and_wood_only_tree_colliders() {
        let data = generation::generate(&TerrainConfig::preset(TerrainPreset::Legacy)).unwrap();
        let mut app = decor_app();
        let root = spawn_test_decor(&mut app, &data);
        let models = nature_meshes(&app);
        let mut tree_positions = Vec::new();
        let mut rock_count = 0;
        let mut landmark_count = 0;
        let mut variants = HashSet::new();
        let meshes = app.world().resource::<Assets<Mesh>>();
        for entity in app.world().iter_entities() {
            let Some(anchor) = entity.get::<NatureDecorAnchor>() else {
                continue;
            };
            let transform = entity.get::<Transform>().unwrap();
            let mesh = entity.get::<Mesh3d>().unwrap();
            let collider = entity.get::<Collider>().unwrap();
            let asset = models[&mesh.0.id()];
            variants.insert(asset.index());
            assert_eq!(entity.get::<ChildOf>().unwrap().parent(), root);
            assert_eq!(entity.get::<RigidBody>(), Some(&RigidBody::Fixed));
            assert!(entity.get::<Sensor>().is_none());
            assert_eq!(transform.scale, Vec3::splat(transform.scale.x));
            assert!(transform.scale.x > 0.0);
            assert!((transform.rotation * Vec3::Y - Vec3::Y).length() < 1e-6);
            assert_eq!(transform.translation.x, anchor.x);
            assert_eq!(transform.translation.z, anchor.z);
            assert_eq!(
                transform.translation.y,
                data.height(anchor.x, anchor.z).unwrap() + anchor.offset_y
            );
            let expected = asset.collider(asset.default_size()).unwrap().unwrap();
            let physical = collider
                .as_trimesh()
                .expect("nature uses exact triangle surfaces");
            let expected = expected.as_trimesh().unwrap();
            assert_eq!(
                physical.vertices().collect::<Vec<_>>(),
                expected.vertices().collect::<Vec<_>>()
            );
            assert_eq!(physical.indices(), expected.indices());
            let mesh_triangles = meshes.get(&mesh.0).unwrap().indices().unwrap().len() / 3;
            let clearance = (anchor.x - road_x(anchor.z)).abs();
            if NatureAsset::TREES.contains(&asset) {
                tree_positions.push((anchor.x, anchor.z));
                assert!(clearance >= 8.0);
                assert!(
                    physical.indices().len() < mesh_triangles,
                    "foliage must not collide"
                );
                assert_eq!(
                    anchor.offset_y,
                    asset.default_size().y * transform.scale.y * 0.5
                );
                for point in physical.vertices() {
                    let point = transform.transform_point(point);
                    assert!((point.x - road_x(point.z)).abs() > 3.25);
                }
            } else {
                rock_count += 1;
                assert_eq!(physical.indices().len(), mesh_triangles);
                if clearance < 5.0 {
                    landmark_count += 1;
                    assert_eq!(anchor.z, 24.0);
                    assert_eq!(anchor.x, road_x(24.0) + 1.3);
                }
            }
        }
        let mut expected_trees = Vec::new();
        for i in -6..=6 {
            for j in -8..=8 {
                let seed = (i * 71 + j * 127) as f32;
                let x = i as f32 * 9.1 + (pseudo(seed + 1.4) - 0.5) * 4.5;
                let z = j as f32 * 9.6 + (pseudo(seed + 7.7) - 0.5) * 4.5;
                if x.abs() <= WIDTH * 0.47
                    && z.abs() <= DEPTH * 0.47
                    && (x - road_x(z)).abs() >= 8.0
                {
                    expected_trees.push((x, z));
                }
            }
        }
        tree_positions.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        expected_trees.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        assert_eq!(tree_positions, expected_trees);
        assert_eq!(rock_count, 39);
        assert_eq!(landmark_count, 1);
        assert_eq!(variants.len(), 12);
    }

    #[test]
    fn historical_nature_shares_scene_assets_and_keeps_separate_spawns_independent() {
        let data = generation::generate(&TerrainConfig::preset(TerrainPreset::Legacy)).unwrap();
        let mut app = decor_app();
        let roots = [
            spawn_test_decor(&mut app, &data),
            spawn_test_decor(&mut app, &data),
        ];
        let mut scene_meshes = [HashSet::new(), HashSet::new()];
        let mut scene_materials = [HashSet::new(), HashSet::new()];
        let mut first_colliders: HashMap<AssetId<Mesh>, &Collider> = HashMap::new();
        for entity in app.world().iter_entities() {
            if entity.get::<NatureDecorAnchor>().is_none() {
                continue;
            }
            let parent = entity.get::<ChildOf>().unwrap().parent();
            let scene = roots.iter().position(|&root| root == parent).unwrap();
            let mesh_id = entity.get::<Mesh3d>().unwrap().0.id();
            let material_id = entity
                .get::<MeshMaterial3d<StandardMaterial>>()
                .unwrap()
                .0
                .id();
            scene_meshes[scene].insert(mesh_id);
            scene_materials[scene].insert(material_id);
            let collider = entity.get::<Collider>().unwrap();
            if let Some(first) = first_colliders.insert(mesh_id, collider) {
                assert!(
                    std::ptr::eq(&*first.raw, &*collider.raw),
                    "instances must share CPU collider geometry"
                );
            }
        }
        for scene in 0..2 {
            assert_eq!(scene_meshes[scene].len(), 12);
            assert_eq!(scene_materials[scene].len(), 1);
            let material = app
                .world()
                .resource::<Assets<StandardMaterial>>()
                .get(*scene_materials[scene].iter().next().unwrap())
                .unwrap();
            assert_eq!(material.base_color, Color::WHITE);
            assert_eq!(material.perceptual_roughness, 1.0);
            assert_eq!(material.metallic, 0.0);
            assert!(material.double_sided);
            assert!(material.cull_mode.is_none());
        }
        assert!(scene_meshes[0].is_disjoint(&scene_meshes[1]));
        assert!(scene_materials[0].is_disjoint(&scene_materials[1]));
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 34);
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 12);
    }

    #[test]
    fn historical_nature_anchors_use_edited_canonical_heights() {
        let mut data = generation::generate(&TerrainConfig::preset(TerrainPreset::Legacy)).unwrap();
        data.heights.iter_mut().for_each(|height| *height += 19.0);
        let mut app = decor_app();
        spawn_test_decor(&mut app, &data);
        let mut count = 0;
        for entity in app.world().iter_entities() {
            let Some(anchor) = entity.get::<NatureDecorAnchor>() else {
                continue;
            };
            let transform = entity.get::<Transform>().unwrap();
            let ground = transform.translation.y - anchor.offset_y;
            assert!((ground - data.height(anchor.x, anchor.z).unwrap()).abs() < 1e-5);
            assert!((ground - height(anchor.x, anchor.z)).abs() > 18.0);
            count += 1;
        }
        assert!(count > 180);
    }

    #[test]
    fn historical_nature_omits_anchors_outside_the_canonical_domain() {
        let config = TerrainConfig {
            width: 24.0,
            depth: 32.0,
            resolution: 1.0,
            ..TerrainConfig::preset(TerrainPreset::Flat)
        };
        let data = generation::generate(&config).unwrap();
        let mut app = decor_app();
        spawn_test_decor(&mut app, &data);
        let mut count = 0;
        for entity in app.world().iter_entities() {
            let Some(anchor) = entity.get::<NatureDecorAnchor>() else {
                continue;
            };
            assert!(data.height(anchor.x, anchor.z).is_some());
            assert!(anchor.x.abs() <= 12.0 && anchor.z.abs() <= 16.0);
            count += 1;
        }
        assert!(count > 0 && count < 30);
    }

    #[test]
    fn historical_nature_small_maps_do_not_allocate_unreferenced_meshes() {
        let data = generation::generate(&TerrainConfig {
            width: 8.0,
            depth: 8.0,
            resolution: 1.0,
            ..TerrainConfig::preset(TerrainPreset::Flat)
        })
        .unwrap();
        let mut app = decor_app();
        spawn_test_decor(&mut app, &data);
        let referenced: HashSet<_> = app
            .world_mut()
            .query::<&Mesh3d>()
            .iter(app.world())
            .map(|mesh| mesh.0.id())
            .collect();
        assert_eq!(
            app.world().resource::<Assets<Mesh>>().len(),
            referenced.len()
        );
    }

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
