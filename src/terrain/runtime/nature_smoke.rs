//! Developer-only real-renderer probe: --nature-smoke <output> --user-data-dir <isolated>.
use super::smoke::{capture, key_input, left_button, mouse_at};
use super::*;
use crate::terrain::{legacy::NatureDecorAnchor, nature::NatureAsset};
use bevy::mesh::VertexAttributeValues;
use std::path::PathBuf;

pub(super) struct NatureSmokePlugin;
const SAVE: &str = "Nature verification";
const POINTER: Vec3 = Vec3::new(24.0, 0.0, 22.0);
const CAPTURES: [&str; 5] = [
    "01-gallery.png",
    "02-preview.png",
    "03-reloaded.png",
    "04-driving.png",
    "05-valley.png",
];

#[derive(Resource)]
struct Probe {
    output: PathBuf,
    stage: u8,
    started: Instant,
    created: std::time::SystemTime,
    since: Instant,
    preview: Option<Obstacle>,
    saved: Option<TerrainScene>,
    drive_start: Vec3,
}

impl Plugin for NatureSmokePlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<_> = std::env::args_os().collect();
        let Some(index) = args.iter().position(|arg| arg == "--nature-smoke") else {
            return;
        };
        let output = args
            .get(index + 1)
            .filter(|arg| !arg.is_empty() && !arg.to_string_lossy().starts_with("--"))
            .map(PathBuf::from)
            .unwrap_or_default();
        app.insert_resource(Probe {
            output,
            stage: 0,
            started: Instant::now(),
            created: std::time::SystemTime::now(),
            since: Instant::now(),
            preview: None,
            saved: None,
            drive_start: Vec3::ZERO,
        })
        .add_systems(Update, step.after(crate::controls));
    }
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn frame(world: &mut World, focus: Vec3) {
    *world.resource_mut::<EditorCamera>() = EditorCamera {
        focus,
        pitch: 0.8,
        yaw: 0.4,
        distance: 65.0,
    };
    let mut state = world.resource_mut::<TerrainWorkshopState>();
    state.mode = WorkshopMode::Edit;
    state.tool = EditorTool::Select;
    state.open = false;
    world
        .resource_mut::<crate::ui::physics_tuning::PhysicsTuningState>()
        .open = false;
}

fn gallery(world: &mut World) -> Result<(), String> {
    let data = crate::app_paths::command_line_data_root()?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    ensure(
        world.resource::<TerrainStore>().root == data.join("terrains"),
        "TerrainStore is not isolated",
    )?;
    let mut scene = TerrainScene::reference(ReferenceScene::Flat)?;
    scene.spawn.position = [0.0, 0.0, -26.0];
    for (index, asset) in NatureAsset::ALL.into_iter().enumerate() {
        let size = asset.default_size();
        scene.obstacles.push(Obstacle {
            id: index as u64 + 1,
            kind: ObstacleKind::Nature(asset),
            position: [
                (index % 6) as f32 * 9.0 - 22.5,
                size.y * 0.5,
                (index / 6) as f32 * 9.0 - 13.5,
            ],
            rotation_degrees: [0.0; 3],
            size: size.to_array(),
        });
    }
    // Resource scopes restore every resource even when installation returns an error.
    world.resource_scope(|world, mut active: Mut<ActiveTerrain>| {
        world.resource_scope(|world, mut state: Mut<TerrainWorkshopState>| {
            world.resource_scope(|world, mut sync: Mut<TerrainSync>| {
                world.resource_scope(|_, mut camera: Mut<EditorCamera>| {
                    install_scene(scene, None, &mut active, &mut state, &mut sync, &mut camera)
                })
            })
        })
    })?;
    frame(world, Vec3::new(0.0, 2.0, 0.0));
    Ok(())
}

fn colored_mesh(world: &World, entity: Entity, visible: bool) -> Result<(), String> {
    let handle = world
        .get::<Mesh3d>(entity)
        .ok_or("Nature root has no Mesh3d")?;
    let mesh = world
        .resource::<Assets<Mesh>>()
        .get(&handle.0)
        .ok_or("Nature mesh asset is missing")?;
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        return Err("Nature mesh has no actual vertex colors".into());
    };
    let valid = colors.len() == mesh.count_vertices()
        && colors.len() >= 3
        && colors
            .iter()
            .flatten()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v));
    ensure(valid, "Invalid nature colors")?;
    let material = world
        .get::<MeshMaterial3d<StandardMaterial>>(entity)
        .ok_or("Nature material handle is missing")?;
    let material = world
        .resource::<Assets<StandardMaterial>>()
        .get(&material.0)
        .ok_or("Nature material is missing")?;
    ensure(
        material.base_color.to_srgba() == Color::WHITE.to_srgba(),
        "Nature colors are tinted",
    )?;
    ensure(
        !visible || world.get::<ViewVisibility>(entity).is_some_and(|v| v.get()),
        "Nature root is not visible",
    )
}

fn objects(world: &mut World, count: usize) -> Result<(), String> {
    let expected = world.resource::<ActiveTerrain>().scene.obstacles.clone();
    let roots: Vec<_> = world
        .query::<(Entity, &ObstacleEntity)>()
        .iter(world)
        .map(|(e, id)| (e, id.0))
        .collect();
    ensure(
        expected.len() == count && roots.len() == count,
        "Nature root count differs from canonical scene",
    )?;
    for object in expected {
        let matches: Vec<_> = roots.iter().filter(|(_, id)| *id == object.id).collect();
        ensure(matches.len() == 1, "Missing or duplicate nature root ID")?;
        let entity = matches[0].0;
        let ObstacleKind::Nature(asset) = object.kind else {
            return Err("Unexpected primitive in nature gallery".into());
        };
        colored_mesh(world, entity, true).map_err(|error| format!("{}: {error}", asset.id()))?;
        let physical = asset.collider(Vec3::from_array(object.size))?.is_some();
        ensure(
            world.get::<Collider>(entity).is_some() == physical
                && world.get::<RigidBody>(entity).is_some() == physical
                && (!physical || world.get::<RigidBody>(entity) == Some(&RigidBody::Fixed)),
            "Nature collision policy mismatch",
        )?;
        let expected_pose = object.transform();
        let pose = world
            .get::<Transform>(entity)
            .ok_or("Nature root pose is missing")?;
        let translation_error = pose.translation.distance(expected_pose.translation);
        let rotation_matches = pose.rotation.abs_diff_eq(expected_pose.rotation, 1e-5)
            || pose.rotation.abs_diff_eq(-expected_pose.rotation, 1e-5);
        if *pose != expected_pose {
            info!(
                "Nature pose {}: translation error {translation_error}, rotation {:?} vs {:?}",
                asset.id(),
                pose.rotation,
                expected_pose.rotation
            );
        }
        ensure(
            translation_error < 1e-4
                && rotation_matches
                && pose.scale.abs_diff_eq(expected_pose.scale, 1e-6),
            &format!(
                "Nature {} pose differs from scene: {pose:?} vs {expected_pose:?}",
                asset.id()
            ),
        )?;
    }
    Ok(())
}

fn preview(world: &mut World, yaw: f32) -> Result<Obstacle, String> {
    let object = world
        .resource::<PlacementPreview>()
        .object()
        .map_err(str::to_owned)?
        .clone();
    ensure(
        object.kind == ObstacleKind::Nature(NatureAsset::TreePineSmall)
            && (object.rotation_degrees[1] - yaw).abs() < 1e-4
            && Vec2::new(
                object.position[0] - POINTER.x,
                object.position[2] - POINTER.z,
            )
            .length()
                < 0.1
            && world.resource::<ActiveTerrain>().scene.obstacles.len() == 23,
        "Nature preview rotation/position/count mismatch",
    )?;
    let entity = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .filter(|(_, name)| name.as_str() == "Placement preview")
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    ensure(entity.len() == 1, "Expected one nature preview")?;
    let entity = entity[0];
    let handle = world.get::<Mesh3d>(entity).ok_or("Preview has no Mesh3d")?;
    let mesh = world
        .resource::<Assets<Mesh>>()
        .get(&handle.0)
        .ok_or("Preview mesh is missing")?;
    let expected = object.build_mesh()?;
    ensure(
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|v| v.as_float3())
            == expected
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|v| v.as_float3())
            && world.get::<Transform>(entity) == Some(&object.transform())
            && world.get::<ViewVisibility>(entity).is_some_and(|v| v.get())
            && world.get::<Collider>(entity).is_none()
            && world.get::<RigidBody>(entity).is_none(),
        "Nature preview geometry/visibility/physics mismatch",
    )?;
    Ok(object)
}

fn queue(world: &mut World, action: WorkshopAction) {
    world
        .resource_mut::<TerrainWorkshopState>()
        .actions
        .push(action);
}

fn grass(world: &World) -> Result<Obstacle, String> {
    let scene = &world.resource::<ActiveTerrain>().scene;
    scene
        .obstacles
        .iter()
        .find(|o| o.kind == ObstacleKind::Nature(NatureAsset::GrassShort))
        .cloned()
        .ok_or("Grass is missing".into())
}

fn truck_position(world: &mut World) -> Result<Vec3, String> {
    world
        .query_filtered::<&Transform, With<Truck>>()
        .single(world)
        .map(|pose| pose.translation)
        .map_err(|error| error.to_string())
}

fn advance(world: &mut World, probe: &mut Probe) -> Result<bool, String> {
    match probe.stage {
        0 => {
            ensure(
                !probe.output.as_os_str().is_empty(),
                "--nature-smoke requires an output directory",
            )?;
            ensure(
                std::env::args_os().any(|arg| arg == "--user-data-dir"),
                "Explicit --user-data-dir is required",
            )?;
            std::fs::create_dir_all(&probe.output).map_err(|error| error.to_string())?;
            gallery(world)?;
        }
        1 => {
            objects(world, 23)?;
            capture(world, &probe.output, CAPTURES[0]);
            let mut state = world.resource_mut::<TerrainWorkshopState>();
            state.tool = EditorTool::Place;
            state.object_kind = ObstacleKind::Nature(NatureAsset::TreePineSmall);
            state.object_size = NatureAsset::TreePineSmall.default_size().to_array();
            mouse_at(world, POINTER, false)?;
            key_input(world, KeyCode::KeyE, true)?;
        }
        2 => {
            preview(world, 15.0)?;
            key_input(world, KeyCode::KeyE, false)?;
            key_input(world, KeyCode::ShiftLeft, true)?;
            key_input(world, KeyCode::KeyQ, true)?;
        }
        3 => {
            probe.preview = Some(preview(world, 10.0)?);
            key_input(world, KeyCode::KeyQ, false)?;
            key_input(world, KeyCode::ShiftLeft, false)?;
            capture(world, &probe.output, CAPTURES[1]);
        }
        4 => left_button(world, true)?,
        5 => {
            left_button(world, false)?;
            let active = world.resource::<ActiveTerrain>();
            info!(
                "Nature confirmation: count {}, actual {:?}, preview {:?}",
                active.scene.obstacles.len(),
                active.scene.obstacles.last(),
                probe.preview
            );
            ensure(
                active.scene.obstacles.len() == 24
                    && active.scene.obstacles.last() == probe.preview.as_ref(),
                "Nature confirmation lost preview pose",
            )?;
            world.resource_mut::<TerrainWorkshopState>().tool = EditorTool::Select;
            let mut saved = world.resource::<ActiveTerrain>().scene.clone();
            saved.name = SAVE.into();
            probe.saved = Some(saved);
            queue(world, WorkshopAction::SaveAs(SAVE.into()));
        }
        6 => {
            let saved = world.resource::<TerrainStore>().load(SAVE)?;
            ensure(
                Some(&saved) == probe.saved.as_ref(),
                "Saved nature JSON differs from scene",
            )?;
            info!(
                "Nature smoke saved to {}",
                world.resource::<TerrainStore>().root.display()
            );
            queue(world, WorkshopAction::LoadReference(ReferenceScene::Flat));
        }
        7 => {
            let cleared = world.resource::<ActiveTerrain>().scene.obstacles.is_empty();
            ensure(cleared, "Flat reference did not clear gallery")?;
            queue(world, WorkshopAction::Load(SAVE.into()));
        }
        8 => {
            let scene = &world.resource::<ActiveTerrain>().scene;
            ensure(
                Some(scene) == probe.saved.as_ref(),
                "Reload changed kinds/IDs/rotations/poses/dimensions",
            )?;
            frame(world, Vec3::new(0.0, 2.0, 0.0));
        }
        9 => {
            objects(world, 24)?;
            capture(world, &probe.output, CAPTURES[2]);
            let grass = grass(world)?;
            mouse_at(world, Vec3::from_array(grass.position), true)?;
        }
        10 => {
            let grass = grass(world)?;
            let selected = world.resource::<TerrainWorkshopState>().selected;
            ensure(
                selected == Some(grass.id),
                "Real mouse did not select decorative grass",
            )?;
            mouse_at(world, Vec3::from_array(grass.position), false)?;
            queue(world, WorkshopAction::SetMode(WorkshopMode::Drive));
        }
        11 => {
            let driving = world.resource::<TerrainWorkshopState>().mode == WorkshopMode::Drive;
            ensure(
                driving && world.resource::<TerrainSync>().car_ready,
                "Drive transition is not ready",
            )?;
            probe.drive_start = truck_position(world)?;
            key_input(world, KeyCode::KeyW, true)?;
        }
        12 => {
            key_input(world, KeyCode::KeyW, false)?;
            let position = truck_position(world)?;
            let delta = position - probe.drive_start;
            let travel = Vec2::new(delta.x, delta.z).length();
            ensure(
                position.is_finite() && travel > 1.0,
                "Vehicle did not travel more than 1 m",
            )?;
            info!("Nature smoke driving: {travel} m");
            capture(world, &probe.output, CAPTURES[3]);
        }
        13 => queue(
            world,
            WorkshopAction::LoadReference(ReferenceScene::Historical),
        ),
        14 => {
            let height = world
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .height(20.0, 0.0)
                .ok_or("Valley ground is missing")?;
            frame(world, Vec3::new(20.0, height + 2.0, 0.0));
        }
        15 => {
            let roots: Vec<_> = world
                .query_filtered::<Entity, With<NatureDecorAnchor>>()
                .iter(world)
                .collect();
            ensure(
                roots.len() > 100,
                "Historical valley has insufficient imported nature decoration",
            )?;
            for entity in &roots {
                colored_mesh(world, *entity, false)?;
                ensure(
                    world.get::<Collider>(*entity).is_some()
                        && world.get::<RigidBody>(*entity) == Some(&RigidBody::Fixed),
                    "Valley nature collider is missing",
                )?;
            }
            ensure(
                roots
                    .iter()
                    .any(|e| world.get::<ViewVisibility>(*e).is_some_and(|v| v.get())),
                "Valley nature is not visible",
            )?;
            info!("Nature smoke valley: {} imported roots", roots.len());
            capture(world, &probe.output, CAPTURES[4]);
        }
        _ => {
            if !CAPTURES.iter().all(|name| {
                std::fs::metadata(probe.output.join(name)).is_ok_and(|m| {
                    m.len() > 0 && m.modified().is_ok_and(|time| time >= probe.created)
                })
            }) {
                return Ok(false);
            }
            info!(
                "Nature smoke complete: 23 models, yaw 15 -> 10, 24 reloaded objects, decorative picking, driving, valley, 5 captures"
            );
            world.write_message(AppExit::Success);
            return Ok(true);
        }
    }
    info!("Nature smoke stage {} complete", probe.stage);
    probe.stage += 1;
    probe.since = Instant::now();
    Ok(false)
}

fn step(world: &mut World) {
    let Some(mut probe) = world.remove_resource::<Probe>() else {
        return;
    };
    let result = (|| {
        ensure(
            probe.started.elapsed().as_secs_f32() < 90.0,
            "90-second global timeout",
        )?;
        ensure(
            world.resource::<Messages<AppExit>>().is_empty(),
            "Unexpected AppExit before nature probe completion",
        )?;
        if let Some(error) = &world.resource::<TerrainWorkshopState>().last_error {
            return Err(error.clone());
        }
        let active = world.resource::<ActiveTerrain>();
        let delay = if probe.stage == 12 { 3.0 } else { 2.5 };
        if active.revision != active.applied_revision {
            probe.since = Instant::now();
            return Ok(false);
        }
        if probe.since.elapsed().as_secs_f32() < delay {
            return Ok(false);
        }
        advance(world, &mut probe)
    })();
    match result {
        Ok(false) => {
            world.insert_resource(probe);
        }
        Ok(true) => {}
        Err(error) => {
            error!("Nature smoke failed at stage {}: {error}", probe.stage);
            world.write_message(AppExit::error());
        }
    }
}
