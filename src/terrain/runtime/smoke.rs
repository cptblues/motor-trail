//! Opt-in, real-renderer regression probe: --terrain-smoke <output-directory>.
use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
    mouse::MouseButtonInput,
};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::CursorMoved;
use std::path::PathBuf;

pub(super) struct TerrainSmokePlugin;
#[derive(Resource)]
struct Probe {
    output: PathBuf,
    stage: u8,
    elapsed: f32,
    brush_before: f32,
    objects_before: usize,
    drive_start: Vec3,
    preview_pose: Option<Transform>,
    preview_mesh: Option<Handle<Mesh>>,
    preview_material: Option<Handle<StandardMaterial>>,
    placed_object: Option<Obstacle>,
}

impl Plugin for TerrainSmokePlugin {
    fn build(&self, app: &mut App) {
        let mut args = std::env::args_os();
        let Some(output) = args
            .find(|arg| arg == "--terrain-smoke")
            .and_then(|_| args.next())
        else {
            return;
        };
        let output = PathBuf::from(output);
        if let Err(error) = std::fs::create_dir_all(&output) {
            error!("Terrain smoke output: {error}");
            return;
        }
        app.insert_resource(Probe {
            output,
            stage: 0,
            elapsed: 0.0,
            brush_before: 0.0,
            objects_before: 0,
            drive_start: Vec3::ZERO,
            preview_pose: None,
            preview_mesh: None,
            preview_material: None,
            placed_object: None,
        })
        .add_systems(Update, smoke_step.after(crate::controls));
    }
}

fn capture(world: &mut World, output: &std::path::Path, name: &str) {
    world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(output.join(name)));
}

fn mouse_at(world: &mut World, point: Vec3, pressed: bool) -> Result<(), String> {
    let position = {
        let mut cameras =
            world.query_filtered::<(&Camera, &GlobalTransform), With<crate::ChaseCamera>>();
        let (camera, transform) = cameras.single(world).map_err(|e| e.to_string())?;
        camera
            .world_to_viewport(transform, point)
            .map_err(|e| e.to_string())?
    };
    let window = {
        let mut windows = world.query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>();
        let (entity, mut window) = windows.single_mut(world).map_err(|e| e.to_string())?;
        if !position.is_finite()
            || position.x < 0.0
            || position.y < 0.0
            || position.x >= window.width()
            || position.y >= window.height()
        {
            return Err(format!(
                "Smoke target {point:?} projects outside the window: {position:?}"
            ));
        }
        window.focused = true;
        window.set_cursor_position(Some(position));
        entity
    };
    world.write_message(CursorMoved {
        window,
        position,
        delta: None,
    });
    world.write_message(MouseButtonInput {
        window,
        button: MouseButton::Left,
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
    });
    info!("Terrain smoke pointer: world {point:?}, viewport {position:?}, pressed {pressed}");
    Ok(())
}

fn left_button(world: &mut World, pressed: bool) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|e| e.to_string())?;
    world.write_message(MouseButtonInput {
        window,
        button: MouseButton::Left,
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
    });
    Ok(())
}

fn key_input(world: &mut World, key_code: KeyCode, pressed: bool) -> Result<(), String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|e| e.to_string())?;
    let logical_key = match key_code {
        KeyCode::KeyE => Key::Character("e".into()),
        KeyCode::KeyQ => Key::Character("q".into()),
        KeyCode::KeyW => Key::Character("w".into()),
        KeyCode::ShiftLeft => Key::Shift,
        KeyCode::Escape => Key::Escape,
        _ => return Err(format!("Unsupported smoke key: {key_code:?}")),
    };
    world.write_message(KeyboardInput {
        key_code,
        logical_key,
        state: if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        text: None,
        repeat: false,
        window,
    });
    Ok(())
}

fn preview_snapshot(
    world: &mut World,
    valid: bool,
    yaw: f32,
) -> Result<(Entity, Transform), String> {
    let state = world.resource::<TerrainWorkshopState>();
    if state.tool != EditorTool::Place
        || state.object_kind != ObstacleKind::Ramp
        || (state.placement_yaw_degrees(ObstacleKind::Ramp) - yaw).abs() > 1e-4
    {
        return Err(format!(
            "Ramp preview did not retain the expected yaw {yaw}"
        ));
    }
    if state.last_error.is_some() {
        return Err("Hovering a preview must not publish a placement error".into());
    }
    let candidate_pose = world
        .resource::<PlacementPreview>()
        .object()
        .ok()
        .map(Obstacle::transform);
    if candidate_pose.is_some() != valid || (!valid && state.cursor.is_some()) {
        return Err("Preview validity disagrees with the current ground pick".into());
    }
    let expected_rotation = if valid {
        let point = state
            .cursor
            .ok_or("Valid preview has no fresh ground pick")?;
        let normal = world
            .resource::<ActiveTerrain>()
            .scene
            .terrain
            .normal(point.x, point.z)
            .ok_or("Preview ground normal is missing")?;
        Quat::from_rotation_arc(Vec3::Y, normal) * Quat::from_rotation_y(yaw.to_radians())
    } else {
        Quat::from_rotation_y(yaw.to_radians())
    };
    let entities: Vec<_> = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .filter(|(_, name)| name.as_str() == "Placement preview")
        .map(|(entity, _)| entity)
        .collect();
    let [entity] = entities.as_slice() else {
        return Err(format!(
            "Expected exactly one preview entity, found {}",
            entities.len()
        ));
    };
    let entity = *entity;
    let mesh = world
        .get::<Mesh3d>(entity)
        .ok_or("Preview is not a real Mesh3d")?;
    if world.resource::<Assets<Mesh>>().get(&mesh.0).is_none()
        || world.get::<Collider>(entity).is_some()
        || world.get::<RigidBody>(entity).is_some()
        || world.get::<ObstacleEntity>(entity).is_some()
    {
        return Err("Preview has missing geometry or a physical/canonical component".into());
    }
    if !world
        .get::<ViewVisibility>(entity)
        .is_some_and(|visibility| visibility.get())
    {
        return Err("Preview mesh is not visible to the renderer".into());
    }
    let handle = &world
        .get::<MeshMaterial3d<StandardMaterial>>(entity)
        .ok_or("Preview material is missing")?
        .0;
    let material = world
        .resource::<Assets<StandardMaterial>>()
        .get(handle)
        .ok_or("Preview material asset is missing")?;
    let color = material.base_color.to_srgba();
    if material.alpha_mode != AlphaMode::Blend
        || !(0.0 < color.alpha && color.alpha < 1.0)
        || (valid && color.green <= color.red)
        || (!valid && color.red <= color.green)
    {
        return Err("Preview must be transparent green when valid, red when invalid".into());
    }
    let pose = *world
        .get::<Transform>(entity)
        .ok_or("Preview pose is missing")?;
    if !pose.translation.is_finite() || !pose.rotation.is_finite() || pose.scale != Vec3::ONE {
        return Err("Preview pose must be finite with baked unit-scale dimensions".into());
    }
    if pose.rotation.dot(expected_rotation).abs() <= 1.0 - 1e-5 {
        return Err(format!(
            "Rendered ramp rotation does not match yaw {yaw} and ground alignment"
        ));
    }
    if candidate_pose.is_some_and(|candidate| !same_pose(candidate, pose)) {
        return Err("Displayed mesh differs from the committable preview candidate".into());
    }
    Ok((entity, pose))
}

fn same_pose(first: Transform, second: Transform) -> bool {
    (first.translation - second.translation).length() < 1e-4
        && first.rotation.dot(second.rotation).abs() > 1.0 - 1e-5
        && first.scale == second.scale
}

fn require(world: &mut World, condition: bool, message: &str) -> bool {
    if !condition {
        error!("Terrain smoke failed: {message}");
        world.write_message(AppExit::error());
    }
    condition
}

fn edited_fixture(world: &mut World, output: &std::path::Path) -> Result<(), String> {
    world.resource_scope(|world, mut active: Mut<ActiveTerrain>| {
        world.resource_scope(|_world, mut sync: Mut<TerrainSync>| {
            let brush = Brush {
                kind: crate::terrain::brush::BrushKind::Lower,
                radius: 3.0,
                strength: 2.0,
                ..Brush::default()
            };
            let changes = apply_brush(&mut active.scene.terrain, Vec2::new(0.0, 2.0), &brush, 0.6)?;
            active.history.push(EditOperation::Sculpt(changes));
            mark_terrain(&mut active, &mut sync);
            let mut objects = active.scene.obstacles.clone();
            let mut ramp = Obstacle {
                id: active.scene.next_id(),
                kind: ObstacleKind::Ramp,
                position: [-2.8, 0.0, 5.0],
                rotation_degrees: [0.0, 15.0, 0.0],
                size: [2.4, 1.1, 4.0],
            };
            ramp.align_to_ground(&active.scene.terrain)?;
            objects.push(ramp);
            commit_objects(objects, &mut active, &mut sync)?;
            let store = TerrainStore::new(output.join("scenes"));
            let mut saved = active.scene.clone();
            saved.name = "Verification terrain".into();
            store.save(&saved.name, &saved)?;
            if store.load(&saved.name)? != saved {
                return Err("Smoke snapshot roundtrip changed the scene".into());
            }
            info!("Terrain smoke: sculpt, ramp and snapshot roundtrip verified");
            Ok(())
        })
    })
}

fn smoke_step(world: &mut World) {
    let Some(mut probe) = world.remove_resource::<Probe>() else {
        return;
    };
    let no_exit = world.resource::<Messages<AppExit>>().is_empty();
    if !require(
        world,
        no_exit,
        "Unexpected AppExit before native probe completion",
    ) {
        return;
    }
    probe.elapsed += world.resource::<Time>().delta_secs();
    if probe.elapsed < 2.5 {
        world.insert_resource(probe);
        return;
    }
    if let Some(error) = world.resource::<TerrainWorkshopState>().last_error.as_ref() {
        error!("Terrain smoke failed: {error}");
        world.write_message(AppExit::error());
        return;
    }
    if world.resource::<ActiveTerrain>().revision
        != world.resource::<ActiveTerrain>().applied_revision
        && !world.resource::<EditorGesture>().active
    {
        world.insert_resource(probe);
        return;
    }
    match probe.stage {
        0 => {
            let mut state = world.resource_mut::<TerrainWorkshopState>();
            state.open = true;
            state
                .actions
                .push(WorkshopAction::LoadReference(ReferenceScene::Rocks));
        }
        1 => {
            capture(world, &probe.output, "01-editor.png");
            if let Err(error) = edited_fixture(world, &probe.output) {
                error!("Terrain smoke failed: {error}");
                world.write_message(AppExit::error());
                return;
            }
            probe.brush_before = world
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .height(8.0, 2.0)
                .unwrap_or(0.0);
            let mut state = world.resource_mut::<TerrainWorkshopState>();
            state.tool = EditorTool::Sculpt;
            state.brush.kind = crate::terrain::brush::BrushKind::Lower;
            if let Err(error) = mouse_at(world, Vec3::new(8.0, probe.brush_before, 2.0), true) {
                require(world, false, &error);
                return;
            }
        }
        2 => {
            capture(world, &probe.output, "02-sculpt-objects.png");
            let height = world
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .height(8.0, 2.0)
                .unwrap_or(0.0);
            let focus = world
                .query_filtered::<&Window, With<PrimaryWindow>>()
                .single(world)
                .map(|w| w.focused);
            info!(
                "Terrain smoke stroke: height {height}, before {}, cursor {:?}, gesture {}, capture {:?}, left {}, focus {:?}",
                probe.brush_before,
                world.resource::<TerrainWorkshopState>().cursor,
                world.resource::<EditorGesture>().active,
                world.resource::<UiInputCapture>(),
                world
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left),
                focus
            );
            if !require(
                world,
                height < probe.brush_before - 0.1,
                "3D mouse stroke did not lower the terrain",
            ) {
                return;
            }
            if let Err(error) = mouse_at(world, Vec3::new(8.0, height, 2.0), false) {
                require(world, false, &error);
                return;
            }
        }
        3 => {
            probe.objects_before = world.resource::<ActiveTerrain>().scene.obstacles.len();
            let mut state = world.resource_mut::<TerrainWorkshopState>();
            state.open = false;
            state.tool = EditorTool::Place;
            state.object_kind = ObstacleKind::Ramp;
            state.object_size = [2.4, 1.1, 4.0];
            world.resource_mut::<EditorCamera>().distance = 70.0;
            world
                .resource_mut::<crate::ui::physics_tuning::PhysicsTuningState>()
                .open = false;
        }
        4 => {
            // Waited one stage for the wider editor camera before projecting targets.
            if let Err(error) = mouse_at(world, Vec3::new(8.0, 0.0, 8.0), false)
                .and_then(|_| key_input(world, KeyCode::KeyE, true))
            {
                require(world, false, &error);
                return;
            }
        }
        5 => {
            if let Err(error) = preview_snapshot(world, true, 15.0) {
                require(world, false, &error);
                return;
            }
            for (key, pressed) in [
                (KeyCode::KeyE, false),
                (KeyCode::ShiftLeft, true),
                (KeyCode::KeyQ, true),
            ] {
                if let Err(error) = key_input(world, key, pressed) {
                    require(world, false, &error);
                    return;
                }
            }
        }
        6 => {
            let entity = match preview_snapshot(world, true, 10.0) {
                Ok((entity, _)) => entity,
                Err(error) => {
                    require(world, false, &error);
                    return;
                }
            };
            let unchanged =
                world.resource::<ActiveTerrain>().scene.obstacles.len() == probe.objects_before;
            if !require(
                world,
                unchanged,
                "Hovering/rotating a preview committed an object",
            ) {
                return;
            }
            probe.preview_mesh = world.get::<Mesh3d>(entity).map(|mesh| mesh.0.clone());
            probe.preview_material = world
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .map(|material| material.0.clone());
            capture(world, &probe.output, "03-placement-preview.png");
            for key in [KeyCode::KeyQ, KeyCode::ShiftLeft] {
                if let Err(error) = key_input(world, key, false) {
                    require(world, false, &error);
                    return;
                }
            }
        }
        7 => {
            if let Err(error) = mouse_at(world, Vec3::new(36.0, 0.0, 4.0), false) {
                require(world, false, &error);
                return;
            }
        }
        8 => {
            let (entity, pose) = match preview_snapshot(world, false, 10.0) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    require(world, false, &error);
                    return;
                }
            };
            let foot = pose.translation - Vec3::Y * 0.55;
            let projected = world
                .query_filtered::<(&Camera, &GlobalTransform), With<crate::ChaseCamera>>()
                .single(world)
                .ok()
                .and_then(|(camera, transform)| camera.world_to_viewport(transform, foot).ok());
            let cursor = world
                .query_filtered::<&Window, With<PrimaryWindow>>()
                .single(world)
                .ok()
                .and_then(Window::cursor_position);
            let terrain = &world.resource::<ActiveTerrain>().scene.terrain.config;
            let outside = foot.x.abs() > terrain.width * 0.5 || foot.z.abs() > terrain.depth * 0.5;
            let follows_pointer = projected
                .zip(cursor)
                .is_some_and(|(a, b)| a.distance(b) < 0.1);
            let same_assets = world.get::<Mesh3d>(entity).map(|mesh| &mesh.0)
                == probe.preview_mesh.as_ref()
                && world
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .map(|material| &material.0)
                    == probe.preview_material.as_ref();
            let unchanged =
                world.resource::<ActiveTerrain>().scene.obstacles.len() == probe.objects_before;
            if !require(
                world,
                outside && follows_pointer && same_assets && unchanged,
                "Invalid hover used a stale pose, replaced assets, or committed an object",
            ) {
                return;
            }
            capture(world, &probe.output, "04-invalid-preview.png");
        }
        9 => {
            if let Err(error) = key_input(world, KeyCode::Escape, true) {
                require(world, false, &error);
                return;
            }
        }
        10 => {
            let preview_exists = world
                .query::<&Name>()
                .iter(world)
                .any(|name| name.as_str() == "Placement preview");
            let assets_removed = probe
                .preview_mesh
                .as_ref()
                .is_some_and(|handle| world.resource::<Assets<Mesh>>().get(handle).is_none())
                && probe.preview_material.as_ref().is_some_and(|handle| {
                    world
                        .resource::<Assets<StandardMaterial>>()
                        .get(handle)
                        .is_none()
                });
            let state = world.resource::<TerrainWorkshopState>();
            let cancelled = state.tool == EditorTool::Select
                && state.cursor.is_none()
                && world.resource::<PlacementPreview>().object().is_err()
                && world.resource::<ActiveTerrain>().scene.obstacles.len() == probe.objects_before;
            if !require(
                world,
                cancelled && !preview_exists && assets_removed,
                "Escape did not cleanly cancel the preview without committing",
            ) {
                return;
            }
            probe.preview_mesh = None;
            probe.preview_material = None;
            if let Err(error) = key_input(world, KeyCode::Escape, false) {
                require(world, false, &error);
                return;
            }
        }
        11 => {
            let mut state = world.resource_mut::<TerrainWorkshopState>();
            state.tool = EditorTool::Place;
            state.object_kind = ObstacleKind::Ramp;
            if let Err(error) = mouse_at(world, Vec3::new(8.0, 0.0, 8.0), false) {
                require(world, false, &error);
                return;
            }
        }
        12 => {
            probe.preview_pose = match preview_snapshot(world, true, 10.0) {
                Ok((_, pose)) => Some(pose),
                Err(error) => {
                    require(world, false, &error);
                    return;
                }
            };
            if let Err(error) = left_button(world, true) {
                require(world, false, &error);
                return;
            }
        }
        13 => {
            let count = world.resource::<ActiveTerrain>().scene.obstacles.len();
            let object = world
                .resource::<ActiveTerrain>()
                .scene
                .obstacles
                .last()
                .cloned();
            let exact_pose = object.as_ref().is_some_and(|object| {
                object.kind == ObstacleKind::Ramp
                    && object.size == [2.4, 1.1, 4.0]
                    && probe
                        .preview_pose
                        .is_some_and(|pose| same_pose(object.transform(), pose))
            });
            info!(
                "Terrain smoke confirmation: count {count}, expected {}, preview {:?}, object {:?}, capture {:?}, cursor {:?}",
                probe.objects_before + 1,
                probe.preview_pose,
                object,
                world.resource::<UiInputCapture>(),
                world.resource::<TerrainWorkshopState>().cursor,
            );
            if !require(
                world,
                count == probe.objects_before + 1 && exact_pose,
                "3D confirmation did not commit the exact displayed ramp pose",
            ) {
                return;
            }
            probe.placed_object = object;
            let point = Vec3::from_array(
                world
                    .resource::<ActiveTerrain>()
                    .scene
                    .obstacles
                    .last()
                    .unwrap()
                    .position,
            );
            info!("Terrain smoke placed object center: {point:?}");
            if let Err(error) = mouse_at(world, point, false) {
                require(world, false, &error);
                return;
            }
        }
        14 => {
            world.resource_mut::<TerrainWorkshopState>().tool = EditorTool::Select;
            world.resource_mut::<TerrainWorkshopState>().selected = None;
            let point = Vec3::from_array(
                world
                    .resource::<ActiveTerrain>()
                    .scene
                    .obstacles
                    .last()
                    .unwrap()
                    .position,
            );
            if let Err(error) = mouse_at(world, point, true) {
                require(world, false, &error);
                return;
            }
        }
        15 => {
            let selected = world.resource::<TerrainWorkshopState>().selected;
            let expected = world
                .resource::<ActiveTerrain>()
                .scene
                .obstacles
                .last()
                .map(|o| o.id);
            info!(
                "Terrain smoke selection: selected {selected:?}, expected {expected:?}, capture {:?}, left {}",
                world.resource::<UiInputCapture>(),
                world
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left)
            );
            let _ = world.run_system_once(
                |context: ReadRapierContext,
                 windows: Query<&Window, With<PrimaryWindow>>,
                 cameras: Query<(&Camera, &GlobalTransform), With<crate::ChaseCamera>>,
                 objects: Query<&ObstacleEntity>,
                 ground: Query<(), With<TerrainGround>>| {
                    let (Ok(window), Ok((camera, transform)), Ok(rapier)) =
                        (windows.single(), cameras.single(), context.single())
                    else {
                        return;
                    };
                    let Some(position) = window.cursor_position() else {
                        return;
                    };
                    let Ok(ray) = camera.viewport_to_world(transform, position) else {
                        return;
                    };
                    let objects_only = |entity| objects.get(entity).is_ok();
                    let ground_only = |entity| ground.get(entity).is_ok();
                    let object = rapier.cast_ray(
                        ray.origin,
                        *ray.direction,
                        2000.0,
                        true,
                        QueryFilter::default().predicate(&objects_only),
                    );
                    let terrain = rapier.cast_ray(
                        ray.origin,
                        *ray.direction,
                        2000.0,
                        true,
                        QueryFilter::default().predicate(&ground_only),
                    );
                    info!(
                        "Terrain smoke selection ray: cursor {position:?}, object {:?}, terrain {terrain:?}",
                        object.map(|(entity, distance)| (objects.get(entity).map(|o| o.0), distance))
                    );
                },
            );
            if !require(
                world,
                selected == expected && selected.is_some(),
                "3D object selection failed",
            ) {
                return;
            }
            let point = Vec3::from_array(
                world
                    .resource::<ActiveTerrain>()
                    .scene
                    .obstacles
                    .last()
                    .unwrap()
                    .position,
            );
            if let Err(error) = mouse_at(world, point, false) {
                require(world, false, &error);
                return;
            }
            info!("Terrain smoke: real camera raycast sculpt, placement and selection verified");
            let mut state = world.resource_mut::<TerrainWorkshopState>();
            state.actions.push(WorkshopAction::Undo);
            state.actions.push(WorkshopAction::Redo);
            state
                .actions
                .push(WorkshopAction::SetMode(WorkshopMode::Drive));
            state.open = false;
            world
                .resource_mut::<crate::ui::physics_tuning::PhysicsTuningState>()
                .open = true;
        }
        16 => {
            let restored = world.resource::<ActiveTerrain>().scene.obstacles.last()
                == probe.placed_object.as_ref();
            let driving = world.resource::<TerrainWorkshopState>().mode == WorkshopMode::Drive;
            if !require(
                world,
                restored && driving,
                "Undo/Redo or the Edit-to-Drive transition lost the ramp",
            ) {
                return;
            }
            probe.drive_start = world
                .query_filtered::<&Transform, With<Truck>>()
                .single(world)
                .map(|pose| pose.translation)
                .unwrap_or(Vec3::NAN);
            if let Err(error) = key_input(world, KeyCode::KeyW, true) {
                require(world, false, &error);
                return;
            }
        }
        17 => {
            if let Err(error) = key_input(world, KeyCode::KeyW, false) {
                require(world, false, &error);
                return;
            }
            let position = world
                .query_filtered::<&Transform, With<Truck>>()
                .single(world)
                .map(|pose| pose.translation)
                .unwrap_or(Vec3::NAN);
            let displacement = position - probe.drive_start;
            let travel = Vec2::new(displacement.x, displacement.z).length();
            info!("Terrain smoke driving: travelled {travel} m, position {position:?}");
            if !require(
                world,
                position.is_finite() && travel > 0.1,
                "The vehicle did not drive after leaving Edit mode",
            ) {
                return;
            }
            let physics_open = world
                .resource::<crate::ui::physics_tuning::PhysicsTuningState>()
                .open;
            if !require(
                world,
                physics_open,
                "Physics workshop closed before its screenshot",
            ) {
                return;
            }
            capture(world, &probe.output, "05-drive-physics.png");
        }
        18 => {
            // Keep the physics panel open through the preceding capture frame.
            world
                .resource_mut::<crate::ui::physics_tuning::PhysicsTuningState>()
                .open = false;
            world
                .resource_mut::<TerrainWorkshopState>()
                .actions
                .push(WorkshopAction::SetMode(WorkshopMode::Edit));
        }
        19 => {
            let state = world.resource::<TerrainWorkshopState>();
            let scene_ready = state.mode == WorkshopMode::Edit
                && !state.open
                && !world
                    .resource::<crate::ui::physics_tuning::PhysicsTuningState>()
                    .open
                && world.resource::<ActiveTerrain>().scene.obstacles.len()
                    == probe.objects_before + 1;
            if !require(
                world,
                scene_ready,
                "Final scene must retain the ramp in Edit mode with both panels closed",
            ) {
                return;
            }
            capture(world, &probe.output, "06-scene.png");
        }
        _ => {
            let files = [
                "01-editor.png",
                "02-sculpt-objects.png",
                "03-placement-preview.png",
                "04-invalid-preview.png",
                "05-drive-physics.png",
                "06-scene.png",
            ];
            if !files.iter().all(|name| probe.output.join(name).is_file()) {
                world.insert_resource(probe);
                return;
            }
            let active = world.resource::<ActiveTerrain>();
            info!(
                "Terrain smoke complete: {} captures, preview yaw 15 -> 10, {} objects, {} scene colliders, revision {}",
                files.len(),
                active.scene.obstacles.len(),
                active.collider_count,
                active.applied_revision
            );
            world.write_message(AppExit::Success);
            return;
        }
    }
    probe.stage += 1;
    probe.elapsed = 0.0;
    world.insert_resource(probe);
}
