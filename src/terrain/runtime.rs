//! Stage canonical edits before Rapier, acknowledge them only after a physics step.
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_rapier3d::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use super::brush::{Brush, HeightChange, apply_brush};
use super::config::{TerrainConfig, TerrainPreset};
use super::history::{EditHistory, EditOperation};
use super::obstacles::{Obstacle, ObstacleKind};
use super::persistence::TerrainStore;
use super::scene::{ReferenceScene, TerrainScene, VehicleSpawn};
use crate::ui::UiInputCapture;
use crate::vehicle::config::VehiclePhysicsConfig;
use crate::vehicle::equipment_runtime::EquipmentRuntime;
use crate::vehicle::telemetry::VehicleTelemetry;
use crate::vehicle::{Truck, VehiclePhysics, VehicleVisualState};
mod placement_preview;
mod smoke;
use placement_preview::PlacementPreview;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WorkshopMode {
    Edit,
    #[default]
    Drive,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EditorTool {
    #[default]
    Sculpt,
    Place,
    Select,
}

#[derive(Clone)]
pub enum WorkshopAction {
    SetMode(WorkshopMode),
    Generate,
    LoadReference(ReferenceScene),
    New,
    SaveAs(String),
    Save,
    Load(String),
    RefreshFiles,
    Undo,
    Redo,
    TransformObject(Obstacle),
    DeleteObject(u64),
    DuplicateObject(u64),
    AlignObject(u64),
    SetSpawn(VehicleSpawn),
    MoveVehicleToSpawn,
}

#[derive(Resource, Clone)]
pub struct TerrainWorkshopState {
    pub open: bool,
    pub mode: WorkshopMode,
    pub draft: TerrainConfig,
    pub brush: Brush,
    pub tool: EditorTool,
    pub object_kind: ObstacleKind,
    pub object_size: [f32; 3],
    placement_yaw_degrees: [f32; 5],
    pub selected: Option<u64>,
    pub scene_name: String,
    pub file_selection: String,
    pub files: Vec<String>,
    pub last_error: Option<String>,
    pub status: String,
    pub cursor: Option<Vec3>,
    pub actions: Vec<WorkshopAction>,
}

impl Default for TerrainWorkshopState {
    fn default() -> Self {
        Self {
            open: false,
            mode: WorkshopMode::Drive,
            draft: TerrainConfig::default(),
            brush: Brush::default(),
            tool: EditorTool::Sculpt,
            object_kind: ObstacleKind::Rock,
            object_size: [2.0, 1.0, 2.0],
            placement_yaw_degrees: [0.0; 5],
            selected: None,
            scene_name: "Mon terrain".into(),
            file_selection: String::new(),
            files: Vec::new(),
            last_error: None,
            status: String::new(),
            cursor: None,
            actions: Vec::new(),
        }
    }
}

impl TerrainWorkshopState {
    pub fn placement_yaw_degrees(&self, kind: ObstacleKind) -> f32 {
        self.placement_yaw_degrees[placement_preview::kind_index(kind)]
    }
}

#[derive(Resource)]
pub struct ActiveTerrain {
    pub scene: TerrainScene,
    pub revision: u64,
    pub applied_revision: u64,
    pub generation_ms: f64,
    pub history: EditHistory,
    pub file: Option<String>,
    pub collider_count: usize,
}
impl Default for ActiveTerrain {
    fn default() -> Self {
        Self {
            scene: TerrainScene::reference(ReferenceScene::Historical)
                .expect("valid built-in valley"),
            revision: 1,
            applied_revision: 0,
            generation_ms: 0.0,
            history: EditHistory::default(),
            file: None,
            collider_count: 0,
        }
    }
}

#[derive(Component)]
pub struct TerrainRoot;
#[derive(Component)]
pub struct TerrainGround;
#[derive(Component)]
pub struct ObstacleEntity(pub u64);

#[derive(Resource, Default)]
pub struct TerrainSync {
    root: Option<Entity>,
    ground: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    ground_material: Option<Handle<StandardMaterial>>,
    object_material: Option<Handle<StandardMaterial>>,
    objects: Vec<Entity>,
    dirty: bool,
    objects_dirty: bool,
    full_reload: bool,
    pending_revision: Option<u64>,
    last_publish: f64,
    reset_pending: bool,
    car_ready: bool,
    pub dirty_region: Option<[usize; 4]>,
}

#[derive(Resource, Default)]
struct EditorGesture {
    active: bool,
    center: Option<Vec2>,
    stroke: BTreeMap<usize, HeightChange>,
}
#[derive(Resource)]
struct EditorCamera {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
}
impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: 0.5,
            pitch: 0.8,
            distance: 48.0,
        }
    }
}

pub struct TerrainPlugin;
impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveTerrain>()
            .init_resource::<TerrainWorkshopState>()
            .init_resource::<TerrainSync>()
            .init_resource::<EditorGesture>()
            .init_resource::<EditorCamera>()
            .init_resource::<PlacementPreview>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Startup, refresh_files)
            .add_systems(
                Update,
                (shortcuts, process_actions, editor_camera)
                    .chain()
                    .before(crate::controls),
            )
            .add_systems(
                PreUpdate,
                editor_interaction
                    .after(crate::ui::input_capture::UiInputSet)
                    .after(bevy_egui::EguiPreUpdateSet::ProcessInput),
            )
            .add_systems(Update, draw_editor.after(editor_camera))
            .add_systems(
                PostUpdate,
                placement_preview::render
                    .after(bevy_egui::EguiPostUpdateSet::EndPass)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                FixedUpdate,
                (apply_stroke, synchronize_terrain)
                    .chain()
                    .after(crate::vehicle::equipment_runtime::sync)
                    .before(crate::vehicle::controller::apply_configuration)
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(FixedUpdate, acknowledge_sync.after(PhysicsSet::Writeback))
            .add_plugins(smoke::TerrainSmokePlugin);
    }
}

pub fn driving_enabled(
    state: Option<Res<TerrainWorkshopState>>,
    active: Option<Res<ActiveTerrain>>,
    sync: Option<Res<TerrainSync>>,
) -> bool {
    state.is_none_or(|s| s.mode == WorkshopMode::Drive)
        && active.is_none_or(|a| a.revision == a.applied_revision)
        && sync.is_none_or(|s| s.car_ready)
}

fn store() -> TerrainStore {
    TerrainStore::new("configs/terrains")
}
fn refresh_files(mut state: ResMut<TerrainWorkshopState>) {
    match store().list() {
        Ok(files) => state.files = files,
        Err(error) => state.last_error = Some(error),
    }
}

fn shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<UiInputCapture>,
    mut state: ResMut<TerrainWorkshopState>,
) {
    if capture.keyboard {
        return;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        state.open = !state.open;
    }
    if state.mode != WorkshopMode::Edit {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if control && keys.just_pressed(KeyCode::KeyZ) {
        state.actions.push(if shift {
            WorkshopAction::Redo
        } else {
            WorkshopAction::Undo
        });
    }
    if control && keys.just_pressed(KeyCode::KeyY) {
        state.actions.push(WorkshopAction::Redo);
    }
    if keys.just_pressed(KeyCode::Delete) {
        if let Some(id) = state.selected {
            state.actions.push(WorkshopAction::DeleteObject(id));
        }
    }
}

fn finish_stroke(gesture: &mut EditorGesture, active: &mut ActiveTerrain) {
    if !gesture.stroke.is_empty() {
        active.history.push(EditOperation::Sculpt(
            std::mem::take(&mut gesture.stroke).into_values().collect(),
        ));
    }
    gesture.active = false;
    gesture.center = None;
}

fn mark_terrain(active: &mut ActiveTerrain, sync: &mut TerrainSync) {
    active.revision += 1;
    sync.dirty = true;
    sync.car_ready = false;
}

fn validate_candidate(scene: &TerrainScene) -> Result<(), String> {
    scene.validate()?;
    super::mesh::build(&scene.terrain)?;
    for obstacle in &scene.obstacles {
        obstacle.build()?;
    }
    Ok(())
}

fn install_scene(
    scene: TerrainScene,
    file: Option<String>,
    active: &mut ActiveTerrain,
    state: &mut TerrainWorkshopState,
    sync: &mut TerrainSync,
    camera: &mut EditorCamera,
) -> Result<(), String> {
    validate_candidate(&scene)?;
    state.draft = scene.terrain.config.clone();
    state.scene_name = if file.is_some() {
        scene.name.clone()
    } else {
        "Mon terrain".into()
    };
    state.selected = None;
    state.mode = WorkshopMode::Edit;
    state.cursor = None;
    camera.focus = Vec3::from_array(scene.spawn.position) + Vec3::Z * 12.0;
    camera.distance = scene
        .terrain
        .config
        .width
        .max(scene.terrain.config.depth)
        .min(110.0)
        * 0.7;
    active.scene = scene;
    active.file = file;
    active.history = EditHistory::default();
    mark_terrain(active, sync);
    sync.full_reload = true;
    sync.reset_pending = true;
    Ok(())
}

fn process_actions(
    mut state: ResMut<TerrainWorkshopState>,
    mut active: ResMut<ActiveTerrain>,
    mut sync: ResMut<TerrainSync>,
    mut gesture: ResMut<EditorGesture>,
    mut camera: ResMut<EditorCamera>,
) {
    let actions = std::mem::take(&mut state.actions);
    for action in actions {
        finish_stroke(&mut gesture, &mut active);
        let start = Instant::now();
        let result = apply_action(action, &mut state, &mut active, &mut sync, &mut camera);
        match result {
            Ok(generated) => {
                if generated {
                    active.generation_ms = start.elapsed().as_secs_f64() * 1000.0;
                }
                state.last_error = None;
            }
            Err(error) => state.last_error = Some(error),
        }
    }
}

fn apply_action(
    action: WorkshopAction,
    state: &mut TerrainWorkshopState,
    active: &mut ActiveTerrain,
    sync: &mut TerrainSync,
    camera: &mut EditorCamera,
) -> Result<bool, String> {
    match action {
        WorkshopAction::SetMode(mode) => {
            state.mode = mode;
            sync.reset_pending = true;
            sync.car_ready = false;
            state.cursor = None;
        }
        WorkshopAction::Generate => {
            let scene = TerrainScene::generate(&state.scene_name, state.draft.clone())?;
            install_scene(scene, None, active, state, sync, camera)?;
            state.status = "Terrain genere".into();
            return Ok(true);
        }
        WorkshopAction::New | WorkshopAction::LoadReference(ReferenceScene::Flat) => {
            install_scene(
                TerrainScene::reference(ReferenceScene::Flat)?,
                None,
                active,
                state,
                sync,
                camera,
            )?;
            return Ok(true);
        }
        WorkshopAction::LoadReference(reference) => {
            install_scene(
                TerrainScene::reference(reference)?,
                None,
                active,
                state,
                sync,
                camera,
            )?;
            return Ok(true);
        }
        WorkshopAction::Load(name) => {
            let scene = store().load(&name)?;
            install_scene(scene, Some(name.clone()), active, state, sync, camera)?;
            state.file_selection = name;
            state.status = "Scene chargee".into();
            return Ok(true);
        }
        WorkshopAction::SaveAs(name) => {
            let mut candidate = active.scene.clone();
            candidate.name = name.clone();
            store().save(&name, &candidate)?;
            active.scene.name = name.clone();
            active.file = Some(name.clone());
            state.scene_name = name.clone();
            state.file_selection = name;
            state.files = store().list()?;
            state.status = "Scene sauvegardee".into();
        }
        WorkshopAction::Save => {
            let name = active
                .file
                .clone()
                .ok_or("Choisir un nom avec Sauvegarder sous")?;
            store().save(&name, &active.scene)?;
            state.status = "Scene sauvegardee".into();
        }
        WorkshopAction::RefreshFiles => {
            state.files = store().list()?;
        }
        WorkshopAction::Undo | WorkshopAction::Redo => {
            let changed = if matches!(action, WorkshopAction::Undo) {
                active.history.undo(&mut active.scene)
            } else {
                active.history.redo(&mut active.scene)
            };
            if changed {
                mark_terrain(active, sync);
                sync.objects_dirty = true;
                sync.reset_pending = true;
            }
        }
        WorkshopAction::SetSpawn(mut spawn) => {
            let y = active
                .scene
                .terrain
                .height(spawn.position[0], spawn.position[2])
                .ok_or("Depart hors du terrain")?;
            spawn.position[1] = y;
            let mut candidate = active.scene.clone();
            candidate.spawn = spawn;
            candidate.validate()?;
            active.scene.spawn = candidate.spawn;
        }
        WorkshopAction::MoveVehicleToSpawn => {
            sync.reset_pending = true;
            sync.car_ready = false;
        }
        action => {
            if state.mode != WorkshopMode::Edit {
                return Err("Passer en mode Edition pour modifier les objets".into());
            }
            let mut objects = active.scene.obstacles.clone();
            match action {
                WorkshopAction::TransformObject(object) => {
                    object.validate()?;
                    let target = objects
                        .iter_mut()
                        .find(|item| item.id == object.id)
                        .ok_or("Objet introuvable")?;
                    *target = object;
                }
                WorkshopAction::DeleteObject(id) => {
                    objects.retain(|item| item.id != id);
                    state.selected = None;
                }
                WorkshopAction::DuplicateObject(id) => {
                    let mut copy = objects
                        .iter()
                        .find(|item| item.id == id)
                        .cloned()
                        .ok_or("Objet introuvable")?;
                    copy.id = active.scene.next_id();
                    copy.position[0] += copy.size[0] + 0.25;
                    copy.align_to_ground(&active.scene.terrain)?;
                    state.selected = Some(copy.id);
                    objects.push(copy);
                }
                WorkshopAction::AlignObject(id) => {
                    objects
                        .iter_mut()
                        .find(|item| item.id == id)
                        .ok_or("Objet introuvable")?
                        .align_to_ground(&active.scene.terrain)?;
                }
                _ => return Ok(false),
            }
            commit_objects(objects, active, sync)?;
        }
    }
    Ok(false)
}

fn commit_objects(
    objects: Vec<Obstacle>,
    active: &mut ActiveTerrain,
    sync: &mut TerrainSync,
) -> Result<(), String> {
    let mut candidate = active.scene.clone();
    candidate.obstacles = objects;
    candidate.validate()?;
    for object in &candidate.obstacles {
        object.build()?;
    }
    if candidate.obstacles != active.scene.obstacles {
        active.history.push(EditOperation::Objects {
            before: active.scene.obstacles.clone(),
            after: candidate.obstacles.clone(),
        });
        active.scene.obstacles = candidate.obstacles;
        active.revision += 1;
        sync.objects_dirty = true;
        sync.reset_pending = true;
        sync.car_ready = false;
    }
    Ok(())
}

fn editor_interaction(
    capture: Res<UiInputCapture>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::ChaseCamera>>,
    context: ReadRapierContext,
    ground: Query<(), With<TerrainGround>>,
    objects: Query<&ObstacleEntity>,
    mut state: ResMut<TerrainWorkshopState>,
    mut active: ResMut<ActiveTerrain>,
    mut sync: ResMut<TerrainSync>,
    mut gesture: ResMut<EditorGesture>,
    mut preview: ResMut<PlacementPreview>,
) {
    preview.invalidate();
    gesture.center = None;
    let focused = windows.single().is_ok_and(|window| window.focused);
    if state.mode == WorkshopMode::Edit && focused && state.tool == EditorTool::Place {
        state.cursor = None;
        let escape = !capture.keyboard && keys.just_pressed(KeyCode::Escape);
        if escape || (!capture.pointer && mouse.just_pressed(MouseButton::Right)) {
            if escape {
                keys.clear_just_pressed(KeyCode::Escape);
            }
            state.tool = EditorTool::Select;
            finish_stroke(&mut gesture, &mut active);
            return;
        }
        if !capture.keyboard {
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let step = if shift { 5.0 } else { 15.0 };
            let rotate_left = keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::KeyA);
            let delta = (i32::from(keys.just_pressed(KeyCode::KeyE)) - i32::from(rotate_left))
                as f32
                * step;
            let index = placement_preview::kind_index(state.object_kind);
            let yaw = &mut state.placement_yaw_degrees[index];
            *yaw = (*yaw + delta) % 360.0;
        }
    }
    if state.mode != WorkshopMode::Edit
        || !focused
        || !mouse.pressed(MouseButton::Left)
        || mouse.pressed(MouseButton::Right)
        || mouse.pressed(MouseButton::Middle)
        || sync.full_reload
    {
        finish_stroke(&mut gesture, &mut active);
    }
    if state.mode != WorkshopMode::Edit
        || !focused
        || capture.pointer
        || mouse.pressed(MouseButton::Right)
        || mouse.pressed(MouseButton::Middle)
        || sync.full_reload
    {
        return;
    }
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
    let ground_only = |entity| ground.get(entity).is_ok();
    let hit = rapier.cast_ray(
        ray.origin,
        *ray.direction,
        2000.0,
        true,
        QueryFilter::default().predicate(&ground_only),
    );
    let point = hit
        .map(|(_, distance)| ray.origin + *ray.direction * distance)
        .filter(|point| point.is_finite());
    if state.tool == EditorTool::Place {
        state.cursor = point;
        preview.sample(&state, &active, ray, point);
    } else if point.is_some() {
        state.cursor = point;
    }

    if mouse.just_pressed(MouseButton::Left) {
        match state.tool {
            EditorTool::Sculpt => {
                gesture.active = point.is_some();
            }
            EditorTool::Select => {
                let objects_only = |entity| objects.get(entity).is_ok();
                let obstacle_hit = rapier.cast_ray(
                    ray.origin,
                    *ray.direction,
                    2000.0,
                    true,
                    QueryFilter::default().predicate(&objects_only),
                );
                state.selected = unoccluded_hit(obstacle_hit, hit)
                    .and_then(|entity| objects.get(entity).ok().map(|object| object.0));
            }
            EditorTool::Place => {
                let candidate = preview.object().cloned().map_err(str::to_owned);
                let result = candidate.and_then(|object| {
                    let id = object.id;
                    let mut placed = active.scene.obstacles.clone();
                    placed.push(object);
                    commit_objects(placed, &mut active, &mut sync)?;
                    Ok(id)
                });
                match result {
                    Ok(id) => {
                        state.selected = Some(id);
                        state.last_error = None;
                        preview.confirmed(active.revision);
                    }
                    Err(error) => {
                        preview.reject(error.clone());
                        state.last_error = Some(error);
                    }
                }
            }
        }
    }
    if gesture.active && state.tool == EditorTool::Sculpt {
        gesture.center = point.map(|point| Vec2::new(point.x, point.z));
    }
}

fn unoccluded_hit(object: Option<(Entity, f32)>, ground: Option<(Entity, f32)>) -> Option<Entity> {
    object
        .filter(|(_, distance)| ground.is_none_or(|(_, terrain)| *distance <= terrain + 0.001))
        .map(|(entity, _)| entity)
}

fn apply_stroke(
    time: Res<Time<Fixed>>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<UiInputCapture>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: ResMut<TerrainWorkshopState>,
    mut active: ResMut<ActiveTerrain>,
    mut sync: ResMut<TerrainSync>,
    mut gesture: ResMut<EditorGesture>,
) {
    if state.mode == WorkshopMode::Edit
        && gesture.active
        && state.tool == EditorTool::Sculpt
        && mouse.pressed(MouseButton::Left)
        && !mouse.pressed(MouseButton::Right)
        && !mouse.pressed(MouseButton::Middle)
        && !capture.pointer
        && windows.single().is_ok_and(|window| window.focused)
    {
        if let Some(center) = gesture.center {
            match apply_brush(
                &mut active.scene.terrain,
                center,
                &state.brush,
                time.delta_secs(),
            ) {
                Ok(changes) if !changes.is_empty() => {
                    for change in changes {
                        let x = change.index % (active.scene.terrain.cells_x + 1);
                        let z = change.index / (active.scene.terrain.cells_x + 1);
                        let region = sync.dirty_region.get_or_insert([x, z, x, z]);
                        region[0] = region[0].min(x);
                        region[1] = region[1].min(z);
                        region[2] = region[2].max(x);
                        region[3] = region[3].max(z);
                        gesture
                            .stroke
                            .entry(change.index)
                            .and_modify(|old| old.after = change.after)
                            .or_insert(change);
                    }
                    mark_terrain(&mut active, &mut sync);
                }
                Ok(_) => {}
                Err(error) => {
                    state.last_error = Some(error);
                    finish_stroke(&mut gesture, &mut active);
                }
            }
        }
    }
}

fn editor_camera(
    state: Res<TerrainWorkshopState>,
    capture: Res<UiInputCapture>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut orbit: ResMut<EditorCamera>,
    mut cameras: Query<&mut Transform, With<crate::ChaseCamera>>,
) {
    if state.mode != WorkshopMode::Edit {
        return;
    }
    let focused = windows.single().is_ok_and(|window| window.focused);
    if !capture.pointer && focused {
        if mouse.pressed(MouseButton::Right) {
            orbit.yaw -= motion.delta.x * 0.006;
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006).clamp(0.15, 1.48);
        }
        if mouse.pressed(MouseButton::Middle) {
            let right = Vec3::new(orbit.yaw.cos(), 0.0, -orbit.yaw.sin());
            let forward = Vec3::new(orbit.yaw.sin(), 0.0, orbit.yaw.cos());
            let distance = orbit.distance;
            orbit.focus += (-right * motion.delta.x + forward * motion.delta.y) * distance * 0.0015;
        }
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y * 0.025,
        };
        orbit.distance = (orbit.distance * (-lines * 0.12).exp()).clamp(5.0, 250.0);
    }
    if let Ok(mut camera) = cameras.single_mut() {
        let horizontal = orbit.distance * orbit.pitch.cos();
        camera.translation = orbit.focus
            + Vec3::new(
                -orbit.yaw.sin() * horizontal,
                orbit.distance * orbit.pitch.sin(),
                -orbit.yaw.cos() * horizontal,
            );
        camera.look_at(orbit.focus, Vec3::Y);
    }
}

fn draw_editor(state: Res<TerrainWorkshopState>, active: Res<ActiveTerrain>, mut gizmos: Gizmos) {
    if state.mode != WorkshopMode::Edit {
        return;
    }
    if let Some(point) = state.cursor {
        if state.tool == EditorTool::Sculpt {
            let radius = state.brush.radius;
            let ring = (0..=64).filter_map(|i| {
                let angle = i as f32 / 64.0 * std::f32::consts::TAU;
                let x = point.x + angle.cos() * radius;
                let z = point.z + angle.sin() * radius;
                active
                    .scene
                    .terrain
                    .height(x, z)
                    .map(|y| Vec3::new(x, y + 0.06, z))
            });
            gizmos.linestrip(ring, Color::srgb(0.95, 0.83, 0.16));
            gizmos.cross(point + Vec3::Y * 0.08, 0.3, Color::WHITE);
        }
    }
    if let Some(object) = active
        .scene
        .obstacles
        .iter()
        .find(|item| Some(item.id) == state.selected)
    {
        gizmos.cube(
            object
                .transform()
                .with_scale(Vec3::from_array(object.size) + Vec3::splat(0.06)),
            Color::srgb(0.95, 0.75, 0.12),
        );
    }
    let spawn = Vec3::from_array(active.scene.spawn.position);
    gizmos.arrow(
        spawn + Vec3::Y * 0.2,
        spawn
            + Vec3::Y * 0.2
            + Quat::from_rotation_y(active.scene.spawn.yaw_degrees.to_radians()) * Vec3::Z * 3.0,
        Color::srgb(0.2, 0.85, 0.95),
    );
}

/// Conservative bound over every cell intersecting the rotated chassis footprint.
pub fn checked_spawn_transform(
    scene: &TerrainScene,
    config: &VehiclePhysicsConfig,
    bounds: (Vec3, Vec3),
) -> Result<Transform, String> {
    if !bounds.0.is_finite() || !bounds.1.is_finite() || bounds.0.cmpge(bounds.1).any() {
        return Err("Enveloppe du vehicule invalide".into());
    }
    let rotation = Quat::from_rotation_y(scene.spawn.yaw_degrees.to_radians());
    let position = Vec3::from_array(scene.spawn.position);
    for x in [bounds.0.x, bounds.1.x] {
        for z in [bounds.0.z, bounds.1.z] {
            let point = position + rotation * Vec3::new(x, 0.0, z);
            if point.x.abs() > scene.terrain.config.width * 0.5 - 0.05
                || point.z.abs() > scene.terrain.config.depth * 0.5 - 0.05
            {
                return Err(
                    "Le vehicule et ses equipements depassent les limites du terrain au depart"
                        .into(),
                );
            }
        }
    }
    Ok(spawn_with_bounds(scene, config, bounds))
}

pub fn spawn_transform(scene: &TerrainScene, config: &VehiclePhysicsConfig) -> Transform {
    spawn_with_bounds(scene, config, crate::vehicle::collision::body_bounds())
}

fn spawn_with_bounds(
    scene: &TerrainScene,
    config: &VehiclePhysicsConfig,
    bounds: (Vec3, Vec3),
) -> Transform {
    let position = Vec3::from_array(scene.spawn.position);
    let rotation = Quat::from_rotation_y(scene.spawn.yaw_degrees.to_radians());
    let mut ground = scene
        .terrain
        .height(position.x, position.z)
        .unwrap_or(position.y);
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for x in [bounds.0.x, bounds.1.x] {
        for z in [bounds.0.z, bounds.1.z] {
            let point = position + rotation * Vec3::new(x, 0.0, z);
            min = min.min(Vec2::new(point.x, point.z));
            max = max.max(Vec2::new(point.x, point.z));
        }
    }
    let data = &scene.terrain;
    let grid = |point: Vec2| {
        (point / Vec2::new(data.config.width, data.config.depth) + Vec2::splat(0.5))
            * Vec2::new(data.cells_x as f32, data.cells_z as f32)
    };
    let first = grid(min).floor().as_ivec2() - IVec2::ONE;
    let last = grid(max).ceil().as_ivec2() + IVec2::ONE;
    for z in first.y.max(0)..=last.y.min(data.cells_z as i32) {
        for x in first.x.max(0)..=last.x.min(data.cells_x as i32) {
            ground = ground.max(data.heights[z as usize * (data.cells_x + 1) + x as usize]);
        }
    }
    for object in &scene.obstacles {
        let center = Vec3::from_array(object.position);
        let extent = Vec3::from_array(object.size).length() * 0.5;
        let radius = Vec2::new(
            bounds.0.x.abs().max(bounds.1.x.abs()),
            bounds.0.z.abs().max(bounds.1.z.abs()),
        )
        .length();
        if Vec2::new(center.x - position.x, center.z - position.z).length() < extent + radius {
            ground = ground.max(center.y + extent);
        }
    }
    Transform::from_xyz(
        position.x,
        ground + (crate::vehicle::controller::spawn_height(config) + 0.18).max(-bounds.0.y + 0.18),
        position.z,
    )
    .with_rotation(rotation)
}

fn reset_car(world: &mut World, dynamic: bool) -> bool {
    let bounds = world.get_resource::<EquipmentRuntime>().map_or_else(
        crate::vehicle::collision::body_bounds,
        EquipmentRuntime::bounds,
    );
    let config = crate::vehicle::controller::effective_configuration(
        world.resource::<VehiclePhysicsConfig>(),
        world.resource::<EquipmentRuntime>(),
    )
    .map(|(effective, _)| effective)
    .unwrap_or_else(|_| world.resource::<VehiclePhysics>().active_config.clone());
    let pose =
        match checked_spawn_transform(&world.resource::<ActiveTerrain>().scene, &config, bounds) {
            Ok(pose) => Some(pose),
            Err(error) => {
                let mut state = world.resource_mut::<TerrainWorkshopState>();
                state.mode = WorkshopMode::Edit;
                state.last_error = Some(error);
                None
            }
        };
    let cars: Vec<_> = world
        .query_filtered::<Entity, With<Truck>>()
        .iter(world)
        .collect();
    for entity in cars {
        world.entity_mut(entity).insert((
            Velocity::zero(),
            ExternalForce::default(),
            if dynamic && pose.is_some() {
                RigidBody::Dynamic
            } else {
                RigidBody::Fixed
            },
        ));
        if let Some(pose) = pose {
            // Rapier's fixed-step propagation omits Bevy's dirty-tree marking.
            world
                .entity_mut(entity)
                .insert((pose, TransformTreeChanged));
        }
    }
    world.resource_mut::<VehiclePhysics>().reset_motion();
    *world.resource_mut::<VehicleVisualState>() = VehicleVisualState::default();
    world
        .resource_mut::<VehicleTelemetry>()
        .reset_motion_history();
    if let Some(mut state) = world.get_resource_mut::<crate::GameState>() {
        state.grounded_wheels = 0;
        state.reached_refuge = false;
    }
    if let Some(pose) = pose
        && let Some(mut orbit) = world.get_resource_mut::<crate::OrbitCamera>()
    {
        orbit.focus = pose.translation + Vec3::Y * 0.8;
    }
    pose.is_some()
}

fn collect_descendants(world: &World, entity: Entity, entities: &mut Vec<Entity>) {
    entities.push(entity);
    if let Some(children) = world.get::<Children>(entity) {
        for child in children.iter() {
            collect_descendants(world, child, entities);
        }
    }
}

fn remove_scene(world: &mut World, sync: &mut TerrainSync) {
    placement_preview::remove(world);
    if let Some(root) = sync.root.take() {
        let mut entities = Vec::new();
        collect_descendants(world, root, &mut entities);
        let mut meshes = HashSet::new();
        let mut materials = HashSet::new();
        for entity in entities {
            if let Some(mesh) = world.get::<Mesh3d>(entity) {
                meshes.insert(mesh.0.id());
            }
            if let Some(material) = world.get::<MeshMaterial3d<StandardMaterial>>(entity) {
                materials.insert(material.0.id());
            }
        }
        world.despawn(root);
        for id in meshes {
            world.resource_mut::<Assets<Mesh>>().remove(id);
        }
        for id in materials {
            if sync
                .ground_material
                .as_ref()
                .is_none_or(|handle| handle.id() != id)
                && sync
                    .object_material
                    .as_ref()
                    .is_none_or(|handle| handle.id() != id)
            {
                world.resource_mut::<Assets<StandardMaterial>>().remove(id);
            }
        }
    }
    sync.ground = None;
    sync.mesh = None;
    sync.objects.clear();
}

fn synchronize_terrain(world: &mut World) {
    let mut sync = world.remove_resource::<TerrainSync>().unwrap_or_default();
    if sync.root.is_none() {
        sync.full_reload = true;
        sync.dirty = true;
        sync.objects_dirty = true;
        sync.reset_pending = true;
    }
    let time = world.resource::<Time>().elapsed_secs_f64();
    let gesture_active = world.resource::<EditorGesture>().active;
    let mode = world.resource::<TerrainWorkshopState>().mode;
    let needs_publish = sync.dirty || sync.objects_dirty || sync.full_reload;
    let throttled = gesture_active && !sync.full_reload && time - sync.last_publish < 0.05;
    if needs_publish && !throttled {
        let built = {
            let active = world.resource::<ActiveTerrain>();
            let terrain = if sync.dirty || sync.full_reload {
                Some(super::mesh::build(&active.scene.terrain))
            } else {
                None
            };
            let objects = if sync.objects_dirty || sync.full_reload {
                active
                    .scene
                    .obstacles
                    .iter()
                    .map(|object| {
                        object
                            .build()
                            .map(|(mesh, collider)| (object.id, object.transform(), mesh, collider))
                    })
                    .collect::<Result<Vec<_>, _>>()
            } else {
                Ok(Vec::new())
            };
            let road = if active.scene.terrain.config.preset == TerrainPreset::Legacy
                && (sync.dirty || sync.full_reload)
            {
                Some(super::legacy::road_on(&active.scene.terrain))
            } else {
                None
            };
            (terrain.transpose(), objects, road)
        };
        match built {
            (Ok(terrain), Ok(objects), mut road) => {
                let reset_ok = reset_car(world, false);
                sync.car_ready = false;
                if sync.full_reload {
                    remove_scene(world, &mut sync);
                }
                let root = *sync.root.get_or_insert_with(|| {
                    world
                        .spawn((
                            TerrainRoot,
                            Name::new("Terrain scene"),
                            Transform::default(),
                            Visibility::default(),
                        ))
                        .id()
                });
                let ground_material = sync
                    .ground_material
                    .get_or_insert_with(|| {
                        world
                            .resource_mut::<Assets<StandardMaterial>>()
                            .add(Color::srgb(0.38, 0.53, 0.35))
                    })
                    .clone();
                if let Some((mesh, collider)) = terrain {
                    if let (Some(entity), Some(handle)) = (sync.ground, sync.mesh.as_ref()) {
                        let _ = world
                            .resource_mut::<Assets<Mesh>>()
                            .insert(handle.id(), mesh);
                        world.entity_mut(entity).insert(collider);
                    } else {
                        let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
                        let entity = world
                            .spawn((
                                ChildOf(root),
                                TerrainGround,
                                Mesh3d(handle.clone()),
                                MeshMaterial3d(ground_material),
                                RigidBody::Fixed,
                                collider,
                                Friction::coefficient(1.15),
                                Transform::default(),
                            ))
                            .id();
                        sync.mesh = Some(handle);
                        sync.ground = Some(entity);
                    }
                }
                if sync.objects_dirty || sync.full_reload {
                    for entity in sync.objects.drain(..) {
                        if let Some(mesh) = world.get::<Mesh3d>(entity).map(|mesh| mesh.0.id()) {
                            world.resource_mut::<Assets<Mesh>>().remove(mesh);
                        }
                        world.despawn(entity);
                    }
                    let material = sync
                        .object_material
                        .get_or_insert_with(|| {
                            world
                                .resource_mut::<Assets<StandardMaterial>>()
                                .add(Color::srgb(0.48, 0.49, 0.47))
                        })
                        .clone();
                    for (id, transform, mesh, collider) in objects {
                        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
                        sync.objects.push(
                            world
                                .spawn((
                                    ChildOf(root),
                                    ObstacleEntity(id),
                                    Mesh3d(mesh),
                                    MeshMaterial3d(material.clone()),
                                    transform,
                                    // Scene roots are identity; Rapier needs the world pose on first sync.
                                    GlobalTransform::from(transform),
                                    collider,
                                    RigidBody::Fixed,
                                    Friction::coefficient(1.15),
                                ))
                                .id(),
                        );
                    }
                }
                if sync.full_reload
                    && world
                        .resource::<ActiveTerrain>()
                        .scene
                        .terrain
                        .config
                        .preset
                        == TerrainPreset::Legacy
                {
                    let road_mesh = road.take().unwrap_or_else(super::legacy::road);
                    world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
                        world.resource_scope(
                            |world, mut materials: Mut<Assets<StandardMaterial>>| {
                                let mut commands = world.commands();
                                super::legacy::spawn_decor(
                                    &mut commands,
                                    &mut meshes,
                                    &mut materials,
                                    root,
                                    road_mesh,
                                );
                            },
                        );
                    });
                    world.flush();
                } else if let Some(road) = road {
                    let handle = world
                        .query_filtered::<&Mesh3d, With<super::legacy::LegacyRoad>>()
                        .iter(world)
                        .next()
                        .map(|mesh| mesh.0.id());
                    if let Some(handle) = handle {
                        let _ = world.resource_mut::<Assets<Mesh>>().insert(handle, road);
                    }
                }
                sync.pending_revision = Some(world.resource::<ActiveTerrain>().revision);
                sync.dirty = false;
                sync.objects_dirty = false;
                sync.full_reload = false;
                sync.dirty_region = None;
                sync.last_publish = time;
                sync.reset_pending = reset_ok && mode == WorkshopMode::Drive;
            }
            (terrain, objects, _) => {
                let error = terrain
                    .err()
                    .or_else(|| objects.err())
                    .unwrap_or_else(|| "Terrain build failed".into());
                world.resource_mut::<TerrainWorkshopState>().last_error = Some(error);
            }
        }
    } else if !needs_publish && sync.pending_revision.is_none() {
        if sync.reset_pending || (!sync.car_ready && mode == WorkshopMode::Drive) {
            let reset_ok = reset_car(world, mode == WorkshopMode::Drive);
            sync.reset_pending = false;
            sync.car_ready = reset_ok && mode == WorkshopMode::Drive;
        }
    }
    world.insert_resource(sync);
}

fn acknowledge_sync(
    mut active: ResMut<ActiveTerrain>,
    mut sync: ResMut<TerrainSync>,
    colliders: Query<(&ChildOf, &RapierColliderHandle)>,
) {
    if let Some(revision) = sync.pending_revision.take() {
        active.applied_revision = revision;
        active.collider_count = colliders
            .iter()
            .filter(|(parent, _)| Some(parent.parent()) == sync.root)
            .count();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::equipment_runtime::{EquipmentRequest, MountedEquipment};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    #[test]
    fn respawn_equipment_footprint_rejects_outside_map_and_invalid_bounds() {
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let config = VehiclePhysicsConfig::baseline();
        assert!(
            checked_spawn_transform(&scene, &config, (Vec3::splat(-100.0), Vec3::splat(100.0)))
                .is_err()
        );
        assert!(
            checked_spawn_transform(&scene, &config, (Vec3::splat(f32::NAN), Vec3::ONE)).is_err()
        );
    }

    #[test]
    fn respawn_equipment_lowest_part_gets_real_geometric_clearance() {
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let config = VehiclePhysicsConfig::baseline();
        let pose = checked_spawn_transform(
            &scene,
            &config,
            (Vec3::new(-1.0, -3.0, -2.0), Vec3::new(1.0, 2.0, 2.0)),
        )
        .unwrap();
        assert!(pose.translation.y - 3.0 >= 0.179);
    }

    fn app() -> App {
        let mut app = App::new();
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let config = VehiclePhysicsConfig::baseline();
        let pose = spawn_transform(&scene, &config);
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            crate::vehicle::VehiclePlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 120.0,
        )))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<crate::GameState>()
        .init_resource::<TerrainWorkshopState>()
        .init_resource::<TerrainSync>()
        .init_resource::<EditorGesture>()
        .init_resource::<EditorCamera>()
        .init_resource::<PlacementPreview>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(ActiveTerrain {
            scene,
            revision: 1,
            applied_revision: 0,
            generation_ms: 0.0,
            history: EditHistory::default(),
            file: None,
            collider_count: 0,
        })
        .add_systems(Update, process_actions.before(crate::controls))
        .add_systems(
            PostUpdate,
            placement_preview::render.before(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(
            FixedUpdate,
            (apply_stroke, synchronize_terrain)
                .chain()
                .after(crate::vehicle::equipment_runtime::sync)
                .before(crate::vehicle::controller::apply_configuration)
                .before(PhysicsSet::SyncBackend),
        )
        .add_systems(FixedUpdate, acknowledge_sync.after(PhysicsSet::Writeback));
        app.world_mut()
            .spawn((crate::vehicle::controller::chassis_bundle(&config), pose));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app
    }
    fn step(app: &mut App, count: usize) {
        for _ in 0..count {
            app.update();
        }
    }
    fn queue(app: &mut App, action: WorkshopAction) {
        app.world_mut()
            .resource_mut::<TerrainWorkshopState>()
            .actions
            .push(action);
    }
    fn car_position(app: &mut App) -> Vec3 {
        app.world_mut()
            .query_filtered::<&Transform, With<Truck>>()
            .single(app.world())
            .unwrap()
            .translation
    }

    fn extended_equipment_app() -> (App, Entity, Entity) {
        let mut app = app();
        app.world_mut().resource_mut::<TerrainWorkshopState>().mode = WorkshopMode::Edit;
        let car = app
            .world_mut()
            .query_filtered::<Entity, With<Truck>>()
            .single(app.world())
            .unwrap();
        app.world_mut().entity_mut(car).insert(GravityScale(0.0));
        let mut request = EquipmentRequest::demo();
        request.catalog.definitions[0].colliders[0]
            .local_pose
            .translation = [4.0, -5.0, 0.0];
        app.insert_resource(request);
        step(&mut app, 4);
        let equipment = app
            .world_mut()
            .query_filtered::<Entity, With<MountedEquipment>>()
            .single(app.world())
            .unwrap();
        let runtime = app.world().resource::<EquipmentRuntime>();
        assert!(runtime.error.is_none(), "{:?}", runtime.error);
        let bounds = runtime.bounds();
        assert!(bounds.0.y < -3.0 && bounds.1.x > 4.0, "{bounds:?}");
        (app, car, equipment)
    }

    fn assert_equipment_spawn_clearance(app: &App, car: Entity, support_height: f32) {
        let minimum = app.world().resource::<EquipmentRuntime>().bounds().0;
        let pose = app.world().get::<Transform>(car).unwrap();
        assert!(
            pose.translation.y + minimum.y >= support_height + 0.17,
            "installed equipment intersects support: pose {pose:?}, minimum {minimum:?}, support {support_height}"
        );
    }

    #[test]
    fn reset_rejected_equipment_profile_keeps_last_valid_controller_on_drive_and_reload() {
        for action in [
            WorkshopAction::SetMode(WorkshopMode::Drive),
            WorkshopAction::LoadReference(ReferenceScene::Flat),
        ] {
            let mut app = app();
            app.insert_resource(EquipmentRequest::demo());
            step(&mut app, 5);
            queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Edit));
            step(&mut app, 3);
            let valid = app
                .world()
                .resource::<VehiclePhysics>()
                .active_config
                .clone();
            app.world_mut()
                .resource_mut::<VehiclePhysicsConfig>()
                .chassis
                .center_of_mass[1] = 1.0;
            app.world()
                .resource::<VehiclePhysicsConfig>()
                .validate()
                .unwrap();
            step(&mut app, 2);
            assert!(app.world().resource::<VehiclePhysics>().error.is_some());
            queue(&mut app, action);
            step(&mut app, 4);
            assert_eq!(
                app.world().resource::<VehiclePhysics>().active_config,
                valid
            );
        }
    }

    #[test]
    fn presentation_follows_workshop_reset_drive_and_full_reload_without_old_spawn_history() {
        let (mut app, car, _) = extended_equipment_app();
        for action in [
            WorkshopAction::SetMode(WorkshopMode::Drive),
            WorkshopAction::SetMode(WorkshopMode::Edit),
            WorkshopAction::LoadReference(ReferenceScene::Steps),
            WorkshopAction::LoadReference(ReferenceScene::Flat),
            WorkshopAction::SetMode(WorkshopMode::Drive),
        ] {
            queue(&mut app, action);
            for _ in 0..4 {
                step(&mut app, 1);
                let physical = *app.world().get::<Transform>(car).unwrap();
                let displayed = app
                    .world()
                    .resource::<crate::vehicle::presentation::VehiclePresentation>()
                    .pose_for(car, Transform::IDENTITY);
                // This fixture has zero gravity and no drive input: it should snap
                // to each new spawn, not sweep across the scene from the old one.
                assert!(displayed.translation.distance(physical.translation) < 1.0e-4);
            }
        }
    }

    #[test]
    fn respawn_equipment_runtime_edit_drive_and_reload_preserve_installed_entities() {
        let (mut app, car, equipment) = extended_equipment_app();
        let local_pose = *app.world().get::<Transform>(equipment).unwrap();
        let installed = app.world().resource::<EquipmentRuntime>().active().clone();
        let revision = app.world().resource::<EquipmentRuntime>().revision;
        let original_root = app.world().resource::<TerrainSync>().root.unwrap();
        assert_equipment_spawn_clearance(&app, car, 0.0);
        for action in [
            WorkshopAction::SetMode(WorkshopMode::Drive),
            WorkshopAction::SetMode(WorkshopMode::Edit),
            WorkshopAction::LoadReference(ReferenceScene::Steps),
            WorkshopAction::LoadReference(ReferenceScene::Flat),
            WorkshopAction::SetMode(WorkshopMode::Drive),
        ] {
            queue(&mut app, action);
            step(&mut app, 4);
            assert_eq!(app.world().get::<ChildOf>(equipment).unwrap().parent(), car);
            assert_eq!(
                *app.world().get::<Transform>(equipment).unwrap(),
                local_pose
            );
            assert!(app.world().get::<RapierColliderHandle>(equipment).is_some());
            assert_eq!(
                app.world().resource::<EquipmentRuntime>().active(),
                &installed
            );
            assert_eq!(
                app.world().resource::<EquipmentRuntime>().revision,
                revision
            );
            assert_equipment_spawn_clearance(&app, car, 0.0);
            let driving =
                app.world().resource::<TerrainWorkshopState>().mode == WorkshopMode::Drive;
            assert_eq!(app.world().resource::<TerrainSync>().car_ready, driving);
            assert_eq!(
                *app.world().get::<RigidBody>(car).unwrap(),
                if driving {
                    RigidBody::Dynamic
                } else {
                    RigidBody::Fixed
                }
            );
        }
        assert!(app.world().get_entity(original_root).is_err());
    }

    #[test]
    fn respawn_equipment_runtime_extended_rotated_footprint_clears_ground_and_obstacles() {
        for obstacle_support in [false, true] {
            let (mut app, car, equipment) = extended_equipment_app();
            let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
            scene.spawn.position = [0.0; 3];
            scene.spawn.yaw_degrees = 90.0;
            let support_height = if obstacle_support {
                let size = Vec3::splat(0.5);
                scene.obstacles.push(Obstacle {
                    id: 1,
                    kind: ObstacleKind::Cube,
                    position: [0.0, 3.0, -4.0],
                    rotation_degrees: [0.0; 3],
                    size: size.to_array(),
                });
                3.0 + size.length() * 0.5
            } else {
                let peak = (0..scene.terrain.vertex_count())
                    .find(|&index| {
                        let point = scene.terrain.vertex_position(index);
                        point.x == 0.0 && point.z == -4.0
                    })
                    .unwrap();
                scene.terrain.heights[peak] = 4.0;
                4.0
            };
            scene.validate().unwrap();
            app.world_mut().resource_mut::<ActiveTerrain>().scene = scene;
            app.world_mut().resource_mut::<ActiveTerrain>().revision += 1;
            {
                let mut sync = app.world_mut().resource_mut::<TerrainSync>();
                sync.dirty = true;
                sync.full_reload = true;
                sync.reset_pending = true;
            }
            step(&mut app, 4);
            assert_equipment_spawn_clearance(&app, car, support_height);
            assert_eq!(app.world().get::<ChildOf>(equipment).unwrap().parent(), car);
            let pose = app.world().get::<Transform>(car).unwrap();
            assert!((pose.rotation * Vec3::X).distance(Vec3::NEG_Z) < 1e-5);
        }
    }

    #[test]
    fn respawn_equipment_runtime_rejected_drive_keeps_pose_fixed_and_can_recover() {
        let (mut app, car, equipment) = extended_equipment_app();
        let before = *app.world().get::<Transform>(car).unwrap();
        queue(
            &mut app,
            WorkshopAction::SetSpawn(VehicleSpawn {
                position: [29.0, 0.0, 0.0],
                yaw_degrees: 0.0,
            }),
        );
        queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Drive));
        step(&mut app, 4);
        assert_eq!(
            *app.world().get::<Transform>(car).unwrap(),
            before,
            "rejected equipment footprint must not teleport the vehicle"
        );
        assert_eq!(
            *app.world().get::<RigidBody>(car).unwrap(),
            RigidBody::Fixed
        );
        assert_eq!(app.world().get::<Velocity>(car).unwrap().linear, Vec3::ZERO);
        let state = app.world().resource::<TerrainWorkshopState>();
        assert_eq!(state.mode, WorkshopMode::Edit);
        assert!(state.last_error.is_some());
        assert!(!app.world().resource::<TerrainSync>().car_ready);
        assert_eq!(app.world().get::<ChildOf>(equipment).unwrap().parent(), car);
        queue(
            &mut app,
            WorkshopAction::SetSpawn(VehicleSpawn {
                position: [0.0; 3],
                yaw_degrees: 0.0,
            }),
        );
        queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Drive));
        step(&mut app, 4);
        assert!(
            app.world()
                .resource::<TerrainWorkshopState>()
                .last_error
                .is_none()
        );
        assert!(app.world().resource::<TerrainSync>().car_ready);
        assert_eq!(
            *app.world().get::<RigidBody>(car).unwrap(),
            RigidBody::Dynamic
        );
        assert_equipment_spawn_clearance(&app, car, 0.0);
    }

    #[test]
    fn respawn_equipment_runtime_rejected_reload_never_marks_the_car_ready() {
        let (mut app, car, equipment) = extended_equipment_app();
        let before = *app.world().get::<Transform>(car).unwrap();
        {
            let mut state = app.world_mut().resource_mut::<TerrainWorkshopState>();
            state.draft.width = 8.0;
            state.draft.depth = 8.0;
            state.draft.resolution = 0.5;
        }
        queue(&mut app, WorkshopAction::Generate);
        step(&mut app, 4);
        assert_eq!(
            app.world()
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .config
                .width,
            8.0
        );
        assert_eq!(*app.world().get::<Transform>(car).unwrap(), before);
        assert_eq!(
            *app.world().get::<RigidBody>(car).unwrap(),
            RigidBody::Fixed
        );
        assert!(
            app.world()
                .resource::<TerrainWorkshopState>()
                .last_error
                .is_some()
        );
        assert_eq!(
            app.world().resource::<TerrainWorkshopState>().mode,
            WorkshopMode::Edit
        );
        assert!(!app.world().resource::<TerrainSync>().car_ready);
        assert_eq!(app.world().get::<ChildOf>(equipment).unwrap().parent(), car);
        queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Drive));
        step(&mut app, 4);
        assert_eq!(*app.world().get::<Transform>(car).unwrap(), before);
        assert_eq!(
            *app.world().get::<RigidBody>(car).unwrap(),
            RigidBody::Fixed
        );
        assert_eq!(
            app.world().resource::<TerrainWorkshopState>().mode,
            WorkshopMode::Edit
        );
        assert!(
            app.world()
                .resource::<TerrainWorkshopState>()
                .last_error
                .is_some()
        );
        assert!(!app.world().resource::<TerrainSync>().car_ready);
    }

    fn floor_hit(app: &mut App) -> f32 {
        app.world_mut()
            .run_system_once(
                |context: ReadRapierContext, ground: Query<(), With<TerrainGround>>| {
                    let only_ground = |entity| ground.get(entity).is_ok();
                    let (_, distance) = context
                        .single()
                        .unwrap()
                        .cast_ray(
                            Vec3::new(0.0, 100.0, 0.0),
                            Vec3::NEG_Y,
                            200.0,
                            true,
                            QueryFilter::default().predicate(&only_ground),
                        )
                        .unwrap();
                    100.0 - distance
                },
            )
            .unwrap()
    }

    #[test]
    fn vehicle_stabilizes_on_generated_flat_terrain() {
        let mut app = app();
        step(&mut app, 720);
        let position = car_position(&mut app);
        step(&mut app, 240);
        assert!((car_position(&mut app) - position).length() < 0.01);
        let speed = app
            .world_mut()
            .query_filtered::<&Velocity, With<Truck>>()
            .single(app.world())
            .unwrap();
        // Substep gravity leaves a small vertical velocity sample even at positional equilibrium.
        assert!(Vec2::new(speed.linear.x, speed.linear.z).length() < 0.01);
        assert!(speed.linear.y.abs() < 0.05, "speed {:?}", speed.linear);
        assert!(app.world().resource::<crate::GameState>().grounded_wheels >= 3);
        assert!((car_position(&mut app).y - 1.06).abs() < 0.15);
    }
    #[test]
    fn the_backend_queries_new_collisions_after_publication() {
        let mut app = app();
        step(&mut app, 5);
        assert!(floor_hit(&mut app).abs() < 0.01);
        app.world_mut()
            .resource_mut::<ActiveTerrain>()
            .scene
            .terrain
            .heights
            .fill(3.0);
        app.world_mut().resource_mut::<ActiveTerrain>().revision += 1;
        app.world_mut().resource_mut::<TerrainSync>().dirty = true;
        step(&mut app, 1);
        assert!((floor_hit(&mut app) - 3.0).abs() < 0.01);
        let active = app.world().resource::<ActiveTerrain>();
        assert_eq!(active.revision, active.applied_revision);
    }

    #[test]
    fn placed_objects_keep_their_world_pose_and_are_raycastable_after_publication() {
        let mut app = app();
        step(&mut app, 5);
        let objects: Vec<_> = ObstacleKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| Obstacle {
                id: index as u64 + 1,
                kind,
                position: [-8.0 + index as f32 * 4.0, 1.0, 8.0],
                rotation_degrees: [10.0, 25.0, 0.0],
                size: [2.0, 1.0, 2.0],
            })
            .collect();
        {
            let mut active = app.world_mut().resource_mut::<ActiveTerrain>();
            active.scene.obstacles = objects.clone();
            active.revision += 1;
        }
        app.world_mut().resource_mut::<TerrainSync>().objects_dirty = true;
        step(&mut app, 3);
        for object in objects {
            let (entity, pose) = app
                .world_mut()
                .query::<(Entity, &ObstacleEntity, &Transform)>()
                .iter(app.world())
                .find(|(_, id, _)| id.0 == object.id)
                .map(|(entity, _, pose)| (entity, *pose))
                .unwrap();
            assert!(
                (pose.translation - Vec3::from_array(object.position)).length() < 1e-4,
                "{:?} moved from {:?} to {:?}",
                object.kind,
                object.position,
                pose.translation
            );
            let origin = pose.translation + Vec3::Y * 5.0;
            let hit = app
                .world_mut()
                .run_system_once(move |context: ReadRapierContext| {
                    let only_object = |candidate| candidate == entity;
                    context.single().unwrap().cast_ray(
                        origin,
                        Vec3::NEG_Y,
                        10.0,
                        true,
                        QueryFilter::default().predicate(&only_object),
                    )
                })
                .unwrap();
            assert!(
                hit.is_some(),
                "{:?} has no collider at its pose",
                object.kind
            );
        }
    }

    #[test]
    fn the_historical_road_reuses_its_mesh_and_follows_edited_heights() {
        let mut app = app();
        step(&mut app, 4);
        queue(
            &mut app,
            WorkshopAction::LoadReference(ReferenceScene::Historical),
        );
        step(&mut app, 4);
        let handle = app
            .world_mut()
            .query_filtered::<&Mesh3d, With<super::super::legacy::LegacyRoad>>()
            .single(app.world())
            .unwrap()
            .0
            .clone();
        {
            let mut active = app.world_mut().resource_mut::<ActiveTerrain>();
            active
                .scene
                .terrain
                .heights
                .iter_mut()
                .for_each(|h| *h += 2.0);
            active.revision += 1;
        }
        app.world_mut().resource_mut::<TerrainSync>().dirty = true;
        step(&mut app, 2);
        let current = app
            .world_mut()
            .query_filtered::<&Mesh3d, With<super::super::legacy::LegacyRoad>>()
            .single(app.world())
            .unwrap()
            .0
            .clone();
        assert_eq!(current.id(), handle.id());
        let positions = app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let data = &app.world().resource::<ActiveTerrain>().scene.terrain;
        for &[x, y, z] in positions {
            assert!((y - data.height(x, z).unwrap() - 0.075).abs() < 1e-5);
        }
    }
    #[test]
    fn repeated_scene_replacements_remove_colliders_meshes_and_materials() {
        let mut app = app();
        step(&mut app, 5);
        for _ in 0..3 {
            queue(
                &mut app,
                WorkshopAction::LoadReference(ReferenceScene::Historical),
            );
            step(&mut app, 4);
            queue(
                &mut app,
                WorkshopAction::LoadReference(ReferenceScene::Steps),
            );
            step(&mut app, 4);
            queue(
                &mut app,
                WorkshopAction::LoadReference(ReferenceScene::Flat),
            );
            step(&mut app, 4);
            assert_eq!(
                app.world_mut()
                    .query_filtered::<Entity, With<TerrainRoot>>()
                    .iter(app.world())
                    .count(),
                1
            );
            assert_eq!(
                app.world_mut()
                    .query::<&RapierContextColliders>()
                    .single(app.world())
                    .unwrap()
                    .colliders
                    .len(),
                2
            );
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
            assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 2);
            assert!(floor_hit(&mut app).abs() < 0.01);
        }
    }
    #[test]
    fn edit_freezes_driving_then_drive_restores_dynamic_simulation() {
        let mut app = app();
        step(&mut app, 360);
        queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Edit));
        step(&mut app, 4);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        let position = car_position(&mut app);
        step(&mut app, 120);
        assert_eq!(position, car_position(&mut app));
        assert_eq!(
            *app.world_mut()
                .query_filtered::<&RigidBody, With<Truck>>()
                .single(app.world())
                .unwrap(),
            RigidBody::Fixed
        );
        queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Drive));
        step(&mut app, 180);
        assert!(car_position(&mut app).z > position.z + 0.5);
    }
    #[test]
    fn an_invalid_load_does_not_change_the_active_scene_or_history() {
        let mut app = app();
        step(&mut app, 5);
        let original = app.world().resource::<ActiveTerrain>().scene.clone();
        queue(&mut app, WorkshopAction::Load("../unsafe".into()));
        step(&mut app, 2);
        assert_eq!(app.world().resource::<ActiveTerrain>().scene, original);
        assert!(
            app.world()
                .resource::<TerrainWorkshopState>()
                .last_error
                .is_some()
        );
    }
    #[test]
    fn a_complete_stroke_merges_intermediate_samples_into_one_undo_operation() {
        let mut active = ActiveTerrain::default();
        let original = active.scene.terrain.heights[0];
        let mut gesture = EditorGesture::default();
        gesture.active = true;
        gesture.stroke.insert(
            0,
            HeightChange {
                index: 0,
                before: original,
                after: original + 1.0,
            },
        );
        gesture.stroke.get_mut(&0).unwrap().after = original + 2.0;
        active.scene.terrain.heights[0] = original + 2.0;
        finish_stroke(&mut gesture, &mut active);
        assert_eq!(active.history.undo_len(), 1);
        assert!(active.history.undo(&mut active.scene));
        assert_eq!(active.scene.terrain.heights[0], original);
        assert!(active.history.redo(&mut active.scene));
        assert_eq!(active.scene.terrain.heights[0], original + 2.0);
    }

    #[test]
    fn spawning_clears_peaks_between_the_old_nine_sample_positions() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        scene.spawn.position = [0.0; 3];
        let peak = (0..scene.terrain.vertex_count())
            .find(|&i| {
                let point = scene.terrain.vertex_position(i);
                point.x == 0.5 && point.z == 0.5
            })
            .unwrap();
        scene.terrain.heights[peak] = 2.0;
        for yaw in [0.0, 45.0, 93.0] {
            scene.spawn.yaw_degrees = yaw;
            let pose = spawn_transform(&scene, &VehiclePhysicsConfig::baseline());
            assert!(pose.translation.y - 0.40 > 2.0);
        }
    }

    #[test]
    fn an_object_behind_the_ground_is_not_selectable() {
        let mut world = World::new();
        let object = world.spawn_empty().id();
        let floor = world.spawn_empty().id();
        assert_eq!(
            unoccluded_hit(Some((object, 12.0)), Some((floor, 10.0))),
            None
        );
        assert_eq!(
            unoccluded_hit(Some((object, 8.0)), Some((floor, 10.0))),
            Some(object)
        );
        assert_eq!(unoccluded_hit(Some((object, 8.0)), None), Some(object));
    }

    fn placement_app() -> App {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
        let mut app = app();
        step(&mut app, 5);
        {
            let mut state = app.world_mut().resource_mut::<TerrainWorkshopState>();
            state.mode = WorkshopMode::Edit;
            state.tool = EditorTool::Place;
            state.object_kind = ObstacleKind::Ramp;
            state.object_size = [2.0, 1.4, 3.0];
        }
        {
            let mut windows = app
                .world_mut()
                .query_filtered::<&mut Window, With<PrimaryWindow>>();
            let mut window = windows.single_mut(app.world_mut()).unwrap();
            window.resolution = (800, 600).into();
            window.set_cursor_position(Some(Vec2::new(400.0, 300.0)));
        }
        let pose = Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
        app.world_mut().spawn((
            Camera {
                computed: ComputedCameraValues {
                    clip_from_view: Mat4::perspective_infinite_reverse_rh(
                        std::f32::consts::FRAC_PI_3,
                        4.0 / 3.0,
                        0.1,
                    ),
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(800, 600),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            pose,
            GlobalTransform::from(pose),
            crate::ChaseCamera,
        ));
        app
    }

    fn placement_tick(app: &mut App) {
        app.world_mut().run_system_once(editor_interaction).unwrap();
        step(app, 1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
    }

    fn preview_entity(app: &mut App) -> Option<Entity> {
        app.world_mut()
            .query::<(Entity, &Name)>()
            .iter(app.world())
            .find(|(_, name)| name.as_str() == "Placement preview")
            .map(|(entity, _)| entity)
    }

    fn preview_pose(app: &mut App) -> Transform {
        let entity = preview_entity(app).expect("placement must render a real mesh");
        *app.world().get::<Transform>(entity).unwrap()
    }

    fn preview_color(app: &mut App) -> Srgba {
        let entity = preview_entity(app).expect("invalid current picks must render red");
        let handle = &app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .unwrap()
            .0;
        let material = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap();
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
        material.base_color.to_srgba()
    }

    fn move_placement_camera(app: &mut App, x: f32) {
        let pose = Transform::from_xyz(x, 20.0, 0.0).looking_at(Vec3::new(x, 0.0, 0.0), Vec3::Z);
        let mut cameras = app
            .world_mut()
            .query_filtered::<(&mut Transform, &mut GlobalTransform), With<crate::ChaseCamera>>();
        let (mut local, mut global) = cameras.single_mut(app.world_mut()).unwrap();
        *local = pose;
        *global = GlobalTransform::from(pose);
    }

    #[test]
    fn placement_preview_matches_catalog_geometry_and_exact_committed_pose() {
        for kind in ObstacleKind::ALL {
            let mut app = placement_app();
            app.world_mut()
                .resource_mut::<TerrainWorkshopState>()
                .object_kind = kind;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyE);
            placement_tick(&mut app);
            let entity = preview_entity(&mut app).expect("placement needs a transparent mesh");
            assert!(app.world().get::<Collider>(entity).is_none());
            assert!(app.world().get::<RigidBody>(entity).is_none());
            assert!(app.world().get::<ObstacleEntity>(entity).is_none());
            let pose = preview_pose(&mut app);
            let mut expected = Obstacle {
                id: 1,
                kind,
                size: [2.0, 1.4, 3.0],
                position: [0.0; 3],
                rotation_degrees: [0.0, 15.0, 0.0],
            };
            expected
                .align_to_ground(&app.world().resource::<ActiveTerrain>().scene.terrain)
                .unwrap();
            assert!((pose.translation - expected.transform().translation).length() < 1e-4);
            assert!(pose.rotation.angle_between(expected.transform().rotation) < 1e-4);
            assert_eq!(pose.scale, Vec3::ONE);
            let handle = &app.world().get::<Mesh3d>(entity).unwrap().0;
            let mesh = app.world().resource::<Assets<Mesh>>().get(handle).unwrap();
            let (final_mesh, _) = expected.build().unwrap();
            assert_eq!(
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .as_float3(),
                final_mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .as_float3()
            );
            assert_eq!(
                mesh.indices()
                    .map(|indices| indices.iter().collect::<Vec<_>>()),
                final_mesh
                    .indices()
                    .map(|indices| indices.iter().collect::<Vec<_>>())
            );
            let color = preview_color(&mut app);
            assert!(color.green > color.red && color.alpha > 0.0 && color.alpha < 1.0);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            placement_tick(&mut app);
            let active = app.world().resource::<ActiveTerrain>();
            let placed = active.scene.obstacles.last().unwrap();
            assert_eq!(
                placed.transform(),
                pose,
                "confirmation changed the displayed pose"
            );
            assert_eq!(active.history.undo_len(), 1);
            assert_eq!(preview_pose(&mut app), pose, "repeated placement lost yaw");
        }
    }

    #[test]
    fn placement_preview_accumulates_yaw_per_kind_and_reuses_assets() {
        let mut app = placement_app();
        placement_tick(&mut app);
        let entity = preview_entity(&mut app).unwrap();
        let mesh = app.world().get::<Mesh3d>(entity).unwrap().0.clone();
        let material = app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .unwrap()
            .0
            .clone();
        let counts = (
            app.world().resource::<Assets<Mesh>>().len(),
            app.world().resource::<Assets<StandardMaterial>>().len(),
        );
        for key in [KeyCode::KeyE, KeyCode::KeyE, KeyCode::KeyQ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            placement_tick(&mut app);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftRight);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyQ);
        move_placement_camera(&mut app, 4.0);
        placement_tick(&mut app);
        let ramp_pose = preview_pose(&mut app);
        assert!(
            ramp_pose
                .rotation
                .angle_between(Quat::from_rotation_y(10_f32.to_radians()))
                < 1e-4
        );
        assert!((ramp_pose.translation.x - 4.0).abs() < 1e-4);
        app.world_mut()
            .resource_mut::<TerrainWorkshopState>()
            .object_kind = ObstacleKind::Rock;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        placement_tick(&mut app);
        assert!(
            preview_pose(&mut app)
                .rotation
                .angle_between(Quat::from_rotation_y(15_f32.to_radians()))
                < 1e-4
        );
        app.world_mut()
            .resource_mut::<TerrainWorkshopState>()
            .object_kind = ObstacleKind::Ramp;
        app.world_mut()
            .resource_mut::<TerrainWorkshopState>()
            .object_size = [3.0, 1.0, 4.0];
        placement_tick(&mut app);
        assert!(
            preview_pose(&mut app)
                .rotation
                .angle_between(ramp_pose.rotation)
                < 1e-4
        );
        assert_eq!(preview_entity(&mut app), Some(entity));
        assert_eq!(app.world().get::<Mesh3d>(entity).unwrap().0, mesh);
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0,
            material
        );
        assert_eq!(
            (
                app.world().resource::<Assets<Mesh>>().len(),
                app.world().resource::<Assets<StandardMaterial>>().len()
            ),
            counts
        );
        assert!(
            app.world()
                .resource::<ActiveTerrain>()
                .scene
                .obstacles
                .is_empty()
        );
    }

    #[test]
    fn placement_rotation_accepts_the_azerty_q_physical_position() {
        let mut app = placement_app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        placement_tick(&mut app);
        assert_eq!(
            app.world()
                .resource::<TerrainWorkshopState>()
                .placement_yaw_degrees(ObstacleKind::Ramp),
            -15.0
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyA);
        placement_tick(&mut app);
        assert_eq!(
            app.world()
                .resource::<TerrainWorkshopState>()
                .placement_yaw_degrees(ObstacleKind::Ramp),
            -20.0
        );
    }

    #[test]
    fn placement_preview_outside_terrain_is_red_and_cannot_confirm_a_stale_pick() {
        let mut app = placement_app();
        placement_tick(&mut app);
        move_placement_camera(&mut app, 45.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        placement_tick(&mut app);
        let color = preview_color(&mut app);
        assert!(color.red > color.green);
        assert!((preview_pose(&mut app).translation.x - 45.0).abs() < 1e-3);
        assert_eq!(app.world().resource::<TerrainWorkshopState>().cursor, None);
        assert!(
            app.world()
                .resource::<ActiveTerrain>()
                .scene
                .obstacles
                .is_empty()
        );
        for size in [[f32::NAN, 1.0, 2.0], [0.0, 1.0, 2.0], [f32::INFINITY; 3]] {
            move_placement_camera(&mut app, 0.0);
            app.world_mut()
                .resource_mut::<TerrainWorkshopState>()
                .object_size = size;
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            placement_tick(&mut app);
            let color = preview_color(&mut app);
            assert!(color.red > color.green);
            let pose = preview_pose(&mut app);
            assert!(
                pose.translation.is_finite() && pose.rotation.is_finite() && pose.scale.is_finite()
            );
            assert!(
                app.world()
                    .resource::<ActiveTerrain>()
                    .scene
                    .obstacles
                    .is_empty()
            );
        }
    }

    #[test]
    fn placement_preview_ui_capture_blocks_confirmation_and_rotation() {
        let mut app = placement_app();
        placement_tick(&mut app);
        *app.world_mut().resource_mut::<UiInputCapture>() = UiInputCapture {
            pointer: true,
            keyboard: true,
        };
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        placement_tick(&mut app);
        assert!(
            app.world()
                .resource::<ActiveTerrain>()
                .scene
                .obstacles
                .is_empty()
        );
        assert!(preview_entity(&mut app).is_none());
        *app.world_mut().resource_mut::<UiInputCapture>() = UiInputCapture::default();
        placement_tick(&mut app);
        assert!(
            preview_pose(&mut app)
                .rotation
                .angle_between(Quat::IDENTITY)
                < 1e-4
        );
    }

    #[test]
    fn placement_preview_aligns_to_slopes_and_confirmation_uses_the_fresh_pick() {
        let mut app = placement_app();
        {
            let mut active = app.world_mut().resource_mut::<ActiveTerrain>();
            let terrain = &mut active.scene.terrain;
            for i in 0..terrain.vertex_count() {
                let point = terrain.vertex_position(i);
                terrain.heights[i] = point.x * 0.15 + point.z * 0.10;
            }
            active.revision += 1;
        }
        app.world_mut().resource_mut::<TerrainSync>().dirty = true;
        step(&mut app, 2);
        for _ in 0..2 {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ShiftLeft);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyE);
            placement_tick(&mut app);
        }
        let old_pose = preview_pose(&mut app);
        move_placement_camera(&mut app, 4.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        placement_tick(&mut app);
        let active = app.world().resource::<ActiveTerrain>();
        let placed = active.scene.obstacles.last().unwrap();
        let mut expected = Obstacle {
            id: placed.id,
            kind: ObstacleKind::Ramp,
            position: [4.0, 0.6, 0.0],
            rotation_degrees: [0.0, 10.0, 0.0],
            size: [2.0, 1.4, 3.0],
        };
        expected.align_to_ground(&active.scene.terrain).unwrap();
        let placed_pose = placed.transform();
        assert!((placed_pose.translation - expected.transform().translation).length() < 1e-4);
        assert!(
            placed_pose
                .rotation
                .angle_between(expected.transform().rotation)
                < 1e-4
        );
        assert_eq!(preview_pose(&mut app), placed_pose);
        assert!((placed_pose.translation - old_pose.translation).length() > 3.0);
    }

    #[test]
    fn placement_preview_cancel_consumes_escape_and_cleans_temporary_assets() {
        for escape in [true, false] {
            let mut app = placement_app();
            placement_tick(&mut app);
            if escape {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape);
            } else {
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Right);
            }
            app.world_mut().run_system_once(editor_interaction).unwrap();
            assert!(
                !app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .just_pressed(KeyCode::Escape)
            );
            app.add_message::<AppExit>();
            app.world_mut().run_system_once(crate::controls).unwrap();
            assert!(
                app.world().resource::<Messages<AppExit>>().is_empty(),
                "placement cancellation quit the game"
            );
            step(&mut app, 1);
            assert_eq!(
                app.world().resource::<TerrainWorkshopState>().tool,
                EditorTool::Select
            );
            assert!(preview_entity(&mut app).is_none());
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
            assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 2);
        }
    }

    #[test]
    fn placement_preview_is_removed_on_tool_mode_and_full_scene_changes() {
        for change in 0..4 {
            let mut app = placement_app();
            placement_tick(&mut app);
            assert!(preview_entity(&mut app).is_some());
            match change {
                0 => {
                    app.world_mut().resource_mut::<TerrainWorkshopState>().tool = EditorTool::Select
                }
                1 => queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Drive)),
                2 => queue(
                    &mut app,
                    WorkshopAction::LoadReference(ReferenceScene::Flat),
                ),
                _ => {
                    let mut sync = app.world_mut().remove_resource::<TerrainSync>().unwrap();
                    remove_scene(app.world_mut(), &mut sync);
                    app.world_mut().insert_resource(sync);
                    assert!(
                        preview_entity(&mut app).is_none(),
                        "scene unload orphaned the preview"
                    );
                }
            }
            step(&mut app, 3);
            assert!(preview_entity(&mut app).is_none());
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
            assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 2);
        }
    }

    #[test]
    fn hovering_ui_preserves_the_last_sample_without_editing_terrain() {
        let mut app = app();
        step(&mut app, 4);
        let point = Vec3::new(2.0, 3.0, 4.0);
        {
            let mut state = app.world_mut().resource_mut::<TerrainWorkshopState>();
            state.mode = WorkshopMode::Edit;
            state.cursor = Some(point);
        }
        app.world_mut().resource_mut::<UiInputCapture>().pointer = true;
        let before = app
            .world()
            .resource::<ActiveTerrain>()
            .scene
            .terrain
            .heights
            .clone();
        app.world_mut().run_system_once(editor_interaction).unwrap();
        assert_eq!(
            app.world().resource::<TerrainWorkshopState>().cursor,
            Some(point)
        );
        assert_eq!(
            app.world()
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .heights,
            before
        );
    }

    #[test]
    fn the_first_resumed_step_preserves_the_selected_physics_profile() {
        for name in ["Origine", "Franchissement"] {
            let mut app = app();
            step(&mut app, 10);
            queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Edit));
            step(&mut app, 3);
            let config = VehiclePhysicsConfig::preset(name).unwrap();
            app.world_mut().insert_resource(config.clone());
            step(&mut app, 2);
            queue(&mut app, WorkshopAction::SetMode(WorkshopMode::Drive));
            step(&mut app, 1);
            step(&mut app, 1);
            assert_eq!(
                app.world().resource::<VehiclePhysics>().active_config,
                config,
                "first resumed step changed {name}"
            );
        }
    }

    #[test]
    fn fixed_rate_smoothing_matches_at_30_and_144_render_fps() {
        fn smooth(fps: usize) -> Vec<f32> {
            let mut app = app();
            step(&mut app, 4);
            let mut config = TerrainConfig::preset(TerrainPreset::Flat);
            config.width = 8.0;
            config.depth = 8.0;
            config.resolution = 1.0;
            config.amplitude = 0.0;
            let mut scene = TerrainScene::generate("Smooth test", config).unwrap();
            scene.terrain.heights[40] = 9.0;
            app.world_mut().resource_mut::<ActiveTerrain>().scene = scene;
            {
                let mut state = app.world_mut().resource_mut::<TerrainWorkshopState>();
                state.mode = WorkshopMode::Edit;
                state.brush = Brush {
                    kind: crate::terrain::brush::BrushKind::Smooth,
                    radius: 2.0,
                    strength: 20.0,
                    falloff: 1.0,
                    flatten_height: 0.0,
                };
            }
            {
                let mut gesture = app.world_mut().resource_mut::<EditorGesture>();
                gesture.active = true;
                gesture.center = Some(Vec2::ZERO);
            }
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            app.world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                    1.0 / fps as f64,
                )));
            step(&mut app, fps);
            // Align nanosecond-rounded render durations to the same fixed-step boundary.
            app.world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_micros(1)));
            step(&mut app, 1);
            app.world()
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .heights
                .clone()
        }
        let slow = smooth(30);
        let fast = smooth(144);
        assert!(slow[40] < 1.0);
        for (a, b) in slow.iter().zip(fast) {
            assert!((a - b).abs() < 1e-5, "{a} != {b}");
        }
    }

    #[test]
    fn a_one_frame_mouse_press_sculpts_before_release_at_30_and_60_fps() {
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
        use bevy::input::{ButtonState, InputPlugin, InputSystems, mouse::MouseButtonInput};
        for fps in [30, 60] {
            let mut app = app();
            app.add_plugins(InputPlugin)
                .add_systems(PreUpdate, editor_interaction.after(InputSystems));
            step(&mut app, 4);
            let window = {
                let mut query = app
                    .world_mut()
                    .query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>();
                let (entity, mut window) = query.single_mut(app.world_mut()).unwrap();
                window.resolution = (800, 600).into();
                window.set_cursor_position(Some(Vec2::new(400.0, 300.0)));
                entity
            };
            let pose = Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
            app.world_mut().spawn((
                Camera {
                    computed: ComputedCameraValues {
                        clip_from_view: Mat4::perspective_infinite_reverse_rh(
                            std::f32::consts::FRAC_PI_3,
                            4.0 / 3.0,
                            0.1,
                        ),
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(800, 600),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
                pose,
                GlobalTransform::from(pose),
                crate::ChaseCamera,
            ));
            app.world_mut().resource_mut::<TerrainWorkshopState>().mode = WorkshopMode::Edit;
            app.world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                    1.0 / fps as f64,
                )));
            app.world_mut().write_message(MouseButtonInput {
                window,
                button: MouseButton::Left,
                state: ButtonState::Pressed,
            });
            step(&mut app, 1);
            let raised = app
                .world()
                .resource::<ActiveTerrain>()
                .scene
                .terrain
                .height(0.0, 0.0)
                .unwrap();
            assert!(raised > 0.0, "one-frame click at {fps} FPS was lost");
            app.world_mut().write_message(MouseButtonInput {
                window,
                button: MouseButton::Left,
                state: ButtonState::Released,
            });
            step(&mut app, 1);
            let active = app.world().resource::<ActiveTerrain>();
            assert_eq!(active.scene.terrain.height(0.0, 0.0), Some(raised));
            assert_eq!(active.history.undo_len(), 1);
            assert!(!app.world().resource::<EditorGesture>().active);
        }
    }
}
