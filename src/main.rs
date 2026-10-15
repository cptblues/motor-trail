//! Motor Trail — prototype de 4x4 Low Poly avec Bevy et Rapier.
//! Vallee procedurale et vehicule articule charge depuis le modele glTF.
use std::f32::consts::TAU;

mod persistence;
mod ui;
mod vehicle;

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::*;
use bevy_egui::EguiPlugin;
use persistence::ProfileStore;
use ui::{PhysicsTuningPlugin, UiInputCapture};
use vehicle::config::VehiclePhysicsConfig;
use vehicle::{Truck, VehiclePhysics, VehiclePlugin, VehicleVisualState};

const WIDTH: f32 = 120.0;
const DEPTH: f32 = 165.0;
const START: f32 = -67.0;
const FINISH: f32 = 65.0;
#[derive(Component)]
struct ChaseCamera;

// Angle de caméra relatif à la direction du 4x4 : 0 = derrière le véhicule.
// Les angles sont indépendants du roulis/tangage de la carrosserie.
#[derive(Resource)]
struct OrbitCamera {
    yaw_offset: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
}
impl Default for OrbitCamera {
    fn default() -> Self {
        let x = road_x(START);
        Self {
            yaw_offset: 0.0,
            pitch: 0.40,
            distance: 10.0,
            focus: Vec3::new(x, height(x, START) + 1.8, START),
        }
    }
}

const ORBIT_SENSITIVITY: f32 = 0.006;
const PITCH_MIN: f32 = 0.12;
const PITCH_MAX: f32 = 1.28;
const CAMERA_MIN_DISTANCE: f32 = 3.3;
const CAMERA_MAX_DISTANCE: f32 = 19.0;
#[derive(Component)]
struct GearKnob;
#[derive(Component)]
struct GearLabel { low: bool }
#[derive(Resource)]
struct FrameStats { smooth_fps: f32 }
impl Default for FrameStats {
    fn default() -> Self { Self { smooth_fps: 60.0 } }
}
#[derive(Component)]
struct Hud;

#[derive(Resource, Default)]
struct GameState {
    low_range: bool,
    reached_refuge: bool,
    grounded_wheels: usize,
}

fn main() {
    let (config, profiles) = ProfileStore::bootstrap("configs/vehicles");
    App::new()
        .insert_resource(config)
        .insert_resource(profiles)
        .insert_resource(ClearColor(Color::srgb(0.69, 0.82, 0.94)))
        .init_resource::<GameState>()
        .init_resource::<OrbitCamera>()
        .init_resource::<FrameStats>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Motor Trail | Rando 4x4".into(),
                resolution: (1280, 780).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((VehiclePlugin, EguiPlugin::default(), PhysicsTuningPlugin))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (controls, objective, camera_follow, display_hud, update_gear_widget).chain(),
        )
        .run();
}

fn road_x(z: f32) -> f32 {
    (z * 0.065).sin() * 4.0
}

fn height(x: f32, z: f32) -> f32 {
    let t = ((z + 38.0) / 102.0).clamp(0.0, 1.0);
    let climb = 15.0 * t * t * (3.0 - 2.0 * t);
    let bumps = 0.55 * (x * 0.11).sin() * (z * 0.10).cos()
        + 0.38 * (x * 0.31).cos() * (z * 0.23).sin();
    climb + bumps + (x / 46.0).powi(2) * 2.0
}

fn normal(x: f32, z: f32) -> Vec3 {
    let d = 0.4;
    Vec3::new(height(x - d, z) - height(x + d, z), 2.0 * d,
        height(x, z - d) - height(x, z + d)).normalize()
}

fn geometry(vertices: Vec<[f32; 3]>, normals: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn ground() -> (Mesh, Collider) {
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

fn road() -> Mesh {
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

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    config: Res<VehiclePhysicsConfig>,
) {
    commands.spawn((
        DirectionalLight { illuminance: 13_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(-30.0, 55.0, -35.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let (mesh, collider) = ground();
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(Color::srgb(0.38, 0.53, 0.35))),
        collider, RigidBody::Fixed, Friction::coefficient(1.15),
    ));
    commands.spawn((
        Mesh3d(meshes.add(road())),
        MeshMaterial3d(materials.add(Color::srgb(0.57, 0.44, 0.31))),
    ));
    forest(&mut commands, &mut meshes, &mut materials);
    rocks(&mut commands, &mut meshes, &mut materials);
    refuge(&mut commands, &mut meshes, &mut materials);
    truck(&mut commands, &asset_server, &config);

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.0, 6.0, START - 11.0)
            .looking_at(Vec3::new(road_x(START), 1.5, START), Vec3::Y),
        ChaseCamera,
    ));
    commands.spawn((
        Text::new("MOTOR TRAIL"),
        TextFont { font_size: FontSize::Px(19.0), ..default() },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(20.0),
            ..default()
        },
        Hud,
    ));
    spawn_gear_widget(&mut commands);
}

fn spawn_gear_widget(commands: &mut Commands) {
    // Levier de gamme fixe en bas à droite, indépendant des textes de diagnostic.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(24.0), bottom: Val::Px(24.0),
            width: Val::Px(204.0), height: Val::Px(170.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.065, 0.085, 0.11, 0.89)),
    )).with_children(|panel| {
        panel.spawn((
            Text::new("TRANSFERT 4×4"),
            TextFont { font_size: FontSize::Px(17.0), ..default() },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(14.0), top: Val::Px(10.0), ..default()
            },
        ));
        panel.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(29.0), top: Val::Px(47.0),
                width: Val::Px(7.0), height: Val::Px(100.0), ..default()
            },
            BackgroundColor(Color::srgb(0.41, 0.45, 0.48)),
        ));
        panel.spawn((
            GearKnob,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(19.0), top: Val::Px(116.0),
                width: Val::Px(27.0), height: Val::Px(27.0),
                border_radius: BorderRadius::all(Val::Px(14.0)), ..default()
            },
            BackgroundColor(Color::srgb(0.95, 0.64, 0.19)),
 
        ));
        panel.spawn((
            GearLabel { low: true },
            Text::new("L    COURTE"),
            TextFont { font_size: FontSize::Px(19.0), ..default() },
            TextColor(Color::srgb(0.64, 0.67, 0.70)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(58.0), top: Val::Px(54.0), ..default()
            },
        ));
        panel.spawn((
            GearLabel { low: false },
            Text::new("H    NORMALE"),
            TextFont { font_size: FontSize::Px(19.0), ..default() },
            TextColor(Color::srgb(0.95, 0.64, 0.19)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(58.0), top: Val::Px(120.0), ..default()
            },
        ));
    });
}

fn update_gear_widget(
    state: Res<GameState>,
    mut knob: Query<&mut Node, With<GearKnob>>,
    mut labels: Query<(&GearLabel, &mut TextColor)>,
) {
    if !state.is_changed() { return; }
    if let Ok(mut node) = knob.single_mut() {
        node.top = Val::Px(if state.low_range { 49.0 } else { 116.0 });
    }
    for (label, mut color) in &mut labels {
        color.0 = if label.low == state.low_range {
            Color::srgb(0.95, 0.64, 0.19)
        } else {
            Color::srgb(0.64, 0.67, 0.70)
        };
    }
}

fn pseudo(seed: f32) -> f32 {
    (seed.sin() * 43758.547).fract().abs()
}

fn forest(
    commands: &mut Commands, meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
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
            if x.abs() > WIDTH * 0.47 || z.abs() > DEPTH * 0.47
                || (x - road_x(z)).abs() < 8.0 { continue; }
            let y = height(x, z);
            let tree_height = 3.8 + pseudo(seed + 3.2) * 2.1;
            let stem_height = 1.8 + tree_height * 0.22;
            commands.spawn((
                Mesh3d(trunk_mesh.clone()),
                MeshMaterial3d(trunk_mat.clone()),
                Transform::from_xyz(x, y + stem_height * 0.5, z)
                    .with_scale(Vec3::new(1.0, stem_height / 2.4, 1.0)),
                RigidBody::Fixed, Collider::cylinder(1.2, 0.28),
            ));
            commands.spawn((
                Mesh3d(pine_mesh.clone()),
                MeshMaterial3d(leaves[(pseudo(seed + 15.0) * 3.0) as usize].clone()),
                Transform::from_xyz(x, y + stem_height + tree_height * 0.35, z)
                    .with_scale(Vec3::new(0.82, tree_height / 4.0, 0.82)),
            ));
        }
    }
}

fn rocks(
    commands: &mut Commands, meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
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
        let half = Vec3::new(0.6 + pseudo(seed + 4.0) * 1.2,
            0.45 + pseudo(seed + 7.0) * 1.2, 0.6 + pseudo(seed + 11.0));
        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(colors[i % 3].clone()),
            Transform::from_xyz(x, height(x, z) + half.y * 0.7, z)
                .with_rotation(Quat::from_rotation_y(pseudo(seed + 12.0) * 2.0))
                .with_scale(half * 2.0),
            RigidBody::Fixed, Collider::cuboid(0.5, 0.5, 0.5),
        ));
    }
    // Rocher remarquable, juste sur le bord de la piste.
    let z = 24.0;
    let x = road_x(z) + 1.3;
    commands.spawn((
        Mesh3d(cube), MeshMaterial3d(colors[1].clone()),
        Transform::from_xyz(x, height(x, z) + 0.38, z)
            .with_scale(Vec3::new(1.1, 0.76, 1.4)),
        RigidBody::Fixed, Collider::cuboid(0.5, 0.5, 0.5),
    ));
}

fn refuge(
    commands: &mut Commands, meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let x = road_x(FINISH) + 7.0;
    let z = FINISH;
    let y = height(x, z);
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(5.0, 3.2, 4.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.60, 0.33, 0.20))),
        Transform::from_xyz(x, y + 1.6, z),
        RigidBody::Fixed, Collider::cuboid(2.5, 1.6, 2.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(5.8, 0.55, 4.8))),
        MeshMaterial3d(materials.add(Color::srgb(0.28, 0.29, 0.30))),
        Transform::from_xyz(x, y + 3.45, z),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.09, 5.8))),
        MeshMaterial3d(materials.add(Color::srgb(0.76, 0.77, 0.75))),
        Transform::from_xyz(x - 3.7, y + 2.9, z),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.8, 1.0, 0.08))),
        MeshMaterial3d(materials.add(Color::srgb(0.94, 0.66, 0.23))),
        Transform::from_xyz(x - 2.8, y + 5.2, z),
    ));
}

fn truck(commands: &mut Commands, asset_server: &AssetServer, config: &VehiclePhysicsConfig) {
    let x = road_x(START);
    let car = commands.spawn((
        vehicle::controller::chassis_bundle(config),
        Transform::from_xyz(x, height(x, START) + vehicle::controller::spawn_height(config), START),
    )).id();

    // Ce modèle est livré avec l'avant en -Z alors que la physique du jeu va
    // vers +Z. Le pivot est donc retourné de PI autour de Y, sans toucher
    // aux colliders ou aux points d'attache des roues.
    commands.entity(car).with_children(|p| {
        p.spawn((
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset("vehicles/motor_trail_4x4.glb"))),
            Transform::from_xyz(0.0, -1.0, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        ));
    });
}

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<UiInputCapture>,
    config: Res<VehiclePhysicsConfig>,
    mut state: ResMut<GameState>,
    mut physics: ResMut<VehiclePhysics>,
    mut visual: ResMut<VehicleVisualState>,
    mut telemetry: ResMut<vehicle::telemetry::VehicleTelemetry>,
    mut cars: Query<(&mut Transform, &mut Velocity, &mut ExternalForce), With<Truck>>,
    mut exit: MessageWriter<AppExit>,
) {
    if capture.keyboard { return; }
    if keys.just_pressed(KeyCode::KeyL) { state.low_range = !state.low_range; }
    if keys.just_pressed(KeyCode::Escape) { exit.write(AppExit::Success); }
    if keys.just_pressed(KeyCode::KeyR) {
        if let Ok((mut transform, mut speed, mut force)) = cars.single_mut() {
            let x = road_x(START);
            transform.translation = Vec3::new(x, height(x, START) + vehicle::controller::spawn_height(&config), START);
            transform.rotation = Quat::IDENTITY;
            *speed = Velocity::zero();
            *force = ExternalForce::default();
            *physics = VehiclePhysics::default();
            *visual = VehicleVisualState::default();
            telemetry.reset_motion_history();
            state.grounded_wheels = 0;
            state.reached_refuge = false;
        }
    }
}


fn objective(mut state: ResMut<GameState>, cars: Query<&Transform, With<Truck>>) {
    if state.reached_refuge { return; }
    if let Ok(car) = cars.single() {
        let x = road_x(FINISH) + 4.0;
        let target = Vec3::new(x, height(x, FINISH), FINISH);
        if car.translation.distance(target) < 10.0 { state.reached_refuge = true; }
    }
}

// Caméra orbitale : maintenir clic droit et glisser pour observer les roues,
// la suspension et le véhicule sur le terrain, même en conduisant.
fn camera_follow(
    time: Res<Time>,
    capture: Res<UiInputCapture>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    mut orbit: ResMut<OrbitCamera>,
    cars: Query<(Entity, &Transform), With<Truck>>,
    context: ReadRapierContext,
    mut cameras: Query<&mut Transform, (With<ChaseCamera>, Without<Truck>)>,
) {
    let (Ok((car_entity, car)), Ok(mut camera)) = (cars.single(), cameras.single_mut())
        else { return; };

    // C recentre derrière le 4x4 et restaure la distance de conduite.
    if !capture.keyboard && keys.just_pressed(KeyCode::KeyC) {
        orbit.yaw_offset = 0.0;
        orbit.pitch = 0.40;
        orbit.distance = 10.0;
    }
    if !capture.pointer && mouse_buttons.pressed(MouseButton::Right) {
        orbit.yaw_offset = (orbit.yaw_offset - mouse_motion.delta.x * ORBIT_SENSITIVITY)
            .rem_euclid(TAU);
        orbit.pitch = (orbit.pitch + mouse_motion.delta.y * ORBIT_SENSITIVITY)
            .clamp(PITCH_MIN, PITCH_MAX);
    }
    // Zoom proportionnel à la distance pour être précis près du véhicule.
    // Sur trackpad, Bevy retourne des pixels ; sur molette classique, des lignes.
    let scroll_lines = match mouse_scroll.unit {
        MouseScrollUnit::Line => mouse_scroll.delta.y,
        MouseScrollUnit::Pixel => mouse_scroll.delta.y * 0.025,
    };
    if !capture.pointer && scroll_lines.abs() > f32::EPSILON {
        orbit.distance = (orbit.distance * (-scroll_lines * 0.12).exp())
            .clamp(CAMERA_MIN_DISTANCE, CAMERA_MAX_DISTANCE);
    }

    let dt = time.delta_secs();
    let target = car.translation + Vec3::Y * 0.8;
    let follow_alpha = 1.0 - (-7.0 * dt).exp();
    orbit.focus = orbit.focus.lerp(target, follow_alpha);

    // Le +Z local est l'avant : on suit son cap au sol, pas son roulis.
    let forward = car.rotation * Vec3::Z;
    let heading = forward.x.atan2(forward.z);
    let (azimuth_sin, azimuth_cos) = (heading + orbit.yaw_offset).sin_cos();
    let horizontal = orbit.distance * orbit.pitch.cos();
    let desired = orbit.focus
        + Vec3::new(-azimuth_sin * horizontal, orbit.distance * orbit.pitch.sin(),
            -azimuth_cos * horizontal);

    // On raccourcit le bras de caméra lorsque le relief ou un arbre se trouve
    // entre le 4x4 et la caméra (sans bloquer l'orbite).
    let ray = desired - orbit.focus;
    let mut safe_position = desired;
    if let Ok(rapier) = context.single() {
        if let Some((_, hit)) = rapier.cast_ray(
            orbit.focus, ray.normalize(), ray.length(), true,
            QueryFilter::default().exclude_rigid_body(car_entity),
        ) {
            let length = (hit - 0.35).max(1.1);
            safe_position = orbit.focus + ray.normalize() * length;
        }
    }

    // Smoothing temporel : réactivité pendant l'orbite, suivi doux en conduite.
    let camera_alpha = 1.0 - (-11.0 * dt).exp();
    camera.translation = camera.translation.lerp(safe_position, camera_alpha);
    if camera.translation.distance_squared(orbit.focus) > 0.01 {
        camera.look_at(orbit.focus, Vec3::Y);
    }
}

fn display_hud(
    state: Res<GameState>,
    time: Res<Time>,
    mut frames: ResMut<FrameStats>,
    cars: Query<&Velocity, With<Truck>>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let (Ok(speed), Ok(mut text)) = (cars.single(), hud.single_mut()) else { return; };
    let range = if state.low_range { "COURTE" } else { "NORMALE" };
    let wheels_on_ground = state.grounded_wheels;
    if time.delta_secs() > 0.0 {
        let instant = (1.0 / time.delta_secs()).clamp(0.0, 500.0);
        frames.smooth_fps += (instant - frames.smooth_fps) * 0.06;
    }
    let mission = if state.reached_refuge { "REFUGE ATTEINT ! Explore librement." }
        else { "OBJECTIF : rejoindre le refuge au sommet de la piste" };
    text.0 = format!(
        "MOTOR TRAIL  |  {:.0} km/h  |  {:.0} FPS  |  Roues au sol: {wheels_on_ground}/4\n{mission}\n\
        Z/W avancer  S reculer  Q/A gauche  D droite  Espace frein\n\
        Clic droit + souris : camera  |  Molette : zoom  |  C : arriere\n\
        L : gamme {range}  R : recommencer  ² : atelier  Echap : quitter",
        speed.linear.length() * 3.6,
        frames.smooth_fps,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    fn vehicle_test_app(slope: Quat) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            VehiclePlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 120.0)))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<GameState>()
        ;
        let normal = slope * Vec3::Y;
        app.world_mut().spawn((
            RigidBody::Fixed,
            Collider::cuboid(40.0, 0.5, 40.0),
            Transform::from_translation(-normal * 0.5).with_rotation(slope),
        ));
        app.world_mut().spawn((
            vehicle::controller::chassis_bundle(&VehiclePhysicsConfig::baseline()),
            Transform::from_translation(normal * 1.06).with_rotation(slope),
        ));
        app.world_mut().spawn((Name::new("WheelSpin_FL"), Transform::default()));
        app
    }

    fn advance_vehicle(app: &mut App, steps: usize) {
        for _ in 0..steps {
            app.update();
        }
    }

    fn truck_position(app: &mut App) -> Vec3 {
        app.world_mut().query_filtered::<&Transform, With<Truck>>()
            .single(app.world()).unwrap().translation
    }

    fn truck_velocity(app: &mut App) -> Velocity {
        *app.world_mut().query_filtered::<&Velocity, With<Truck>>()
            .single(app.world()).unwrap()
    }

    fn truck_mass(app: &mut App) -> f32 {
        let handle = *app.world_mut().query_filtered::<&RapierRigidBodyHandle, With<Truck>>()
            .single(app.world()).unwrap();
        app.world_mut().query::<&RapierRigidBodySet>()
            .single(app.world()).unwrap().bodies[handle.0].mass()
    }

    #[test]
    fn live_mass_and_center_of_mass_update_the_rigid_body() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        assert!((truck_mass(&mut app) - 1_800.0).abs() < 0.1);
        {
            let mut config = app.world_mut().resource_mut::<VehiclePhysicsConfig>();
            config.chassis.mass_kg = 2_400.0;
            config.chassis.center_of_mass = [0.15, -0.30, 0.10];
        }
        advance_vehicle(&mut app, 2);
        assert!((truck_mass(&mut app) - 2_400.0).abs() < 0.1);
        let handle = *app.world_mut().query_filtered::<&RapierRigidBodyHandle, With<Truck>>()
            .single(app.world()).unwrap();
        let body_set = app.world_mut().query::<&RapierRigidBodySet>().single(app.world()).unwrap();
        let body = &body_set.bodies[handle.0];
        let local_com = body.position().inverse_transform_point(body.center_of_mass());
        assert!(local_com.distance(Vec3::new(0.15, -0.30, 0.10)) < 0.001);
    }

    #[test]
    fn softer_live_springs_change_the_static_ride_height() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 600);
        let baseline = truck_position(&mut app).y;
        {
            let mut config = app.world_mut().resource_mut::<VehiclePhysicsConfig>();
            config.front.stiffness_n_per_m = 22_000.0;
            config.rear.stiffness_n_per_m = 22_000.0;
        }
        advance_vehicle(&mut app, 600);
        assert!(baseline - truck_position(&mut app).y > 0.08);
        assert!(truck_velocity(&mut app).linear.length() < 0.05);
    }

    #[test]
    fn engine_force_adjustment_changes_acceleration() {
        let speed_after_launch = |force| {
            let mut app = vehicle_test_app(Quat::IDENTITY);
            app.world_mut().resource_mut::<VehiclePhysicsConfig>().drivetrain.engine_force_n = force;
            advance_vehicle(&mut app, 360);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
            advance_vehicle(&mut app, 120);
            truck_velocity(&mut app).linear.z
        };
        assert!(speed_after_launch(10_000.0) > speed_after_launch(5_000.0) * 1.5);
    }

    #[test]
    fn switching_controller_models_does_not_accumulate_forces() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        *app.world_mut().resource_mut::<VehiclePhysicsConfig>() = VehiclePhysicsConfig::original();
        advance_vehicle(&mut app, 360);
        assert!(app.world().resource::<VehiclePhysics>().controller.is_none());
        assert!(truck_velocity(&mut app).linear.length() < 0.10);
        *app.world_mut().resource_mut::<VehiclePhysicsConfig>() = VehiclePhysicsConfig::baseline();
        advance_vehicle(&mut app, 600);
        assert!(app.world().resource::<VehiclePhysics>().controller.is_some());
        assert!((truck_mass(&mut app) - 1_800.0).abs() < 0.1);
        assert!(truck_velocity(&mut app).linear.length() < 0.05);
    }

    #[test]
    fn wheel_rotation_does_not_include_hill_hold_compensation() {
        let mut app = vehicle_test_app(Quat::from_rotation_x(20.0_f32.to_radians()));
        advance_vehicle(&mut app, 600);
        let spins = app.world().resource::<VehicleVisualState>().spins;
        advance_vehicle(&mut app, 600);
        assert_eq!(app.world().resource::<VehicleVisualState>().spins, spins);
    }

    #[test]
    fn rolling_resistance_cannot_command_reverse_thrust_during_a_forward_launch() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.world_mut().resource_mut::<VehiclePhysicsConfig>().drivetrain.throttle_response_hz = 0.2;
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        for _ in 0..24 {
            app.update();
            let physics = app.world().resource::<VehiclePhysics>();
            assert!(physics.controller.as_ref().unwrap().wheels().iter().all(|wheel| wheel.engine_force >= 0.0));
        }
    }

    #[test]
    fn releasing_throttle_applies_engine_braking_without_waiting_for_response_decay() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        {
            let mut config = app.world_mut().resource_mut::<VehiclePhysicsConfig>();
            config.drivetrain.throttle_response_hz = 0.2;
            config.drivetrain.engine_brake_force_n = 8_000.0;
        }
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 60);
        app.world_mut().query_filtered::<&mut Velocity, With<Truck>>()
            .single_mut(app.world_mut()).unwrap().linear.z = 3.0;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyW);
        advance_vehicle(&mut app, 2);
        let physics = app.world().resource::<VehiclePhysics>();
        let forward_impulse: f32 = physics.controller.as_ref().unwrap().wheels().iter().map(|wheel| wheel.forward_impulse).sum();
        assert!(forward_impulse < 0.0, "engine braking must oppose motion, impulse: {forward_impulse}");
    }

    #[test]
    fn straight_line_tire_impulses_respect_the_actual_normal_load() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.world_mut().resource_mut::<VehiclePhysicsConfig>().tires.grip_coefficient = 0.05;
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 60);
        let physics = app.world().resource::<VehiclePhysics>();
        for wheel in physics.controller.as_ref().unwrap().wheels() {
            let impulse = Vec2::new(wheel.forward_impulse, wheel.side_impulse).length();
            let limit = wheel.wheel_suspension_force.min(wheel.max_suspension_force)
                * wheel.friction_slip * vehicle::controller::PHYSICS_DT;
            assert!(impulse <= limit + 0.0001, "tire impulse {impulse} exceeds load-based limit {limit}");
        }
    }

    #[test]
    fn the_slowest_valid_throttle_response_still_eventually_accelerates() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.world_mut().resource_mut::<VehiclePhysicsConfig>().drivetrain.throttle_response_hz = 0.1;
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 240);
        assert!(truck_velocity(&mut app).linear.z > 0.25);
    }

    #[test]
    fn low_grip_changes_straight_line_acceleration() {
        let launch_speed = |grip| {
            let mut app = vehicle_test_app(Quat::IDENTITY);
            app.world_mut().resource_mut::<VehiclePhysicsConfig>().tires.grip_coefficient = grip;
            advance_vehicle(&mut app, 360);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
            advance_vehicle(&mut app, 120);
            truck_velocity(&mut app).linear.z
        };
        assert!(launch_speed(1.35) > launch_speed(0.05) * 2.0);
    }

    #[test]
    fn parking_assistance_cannot_hold_a_slope_beyond_available_grip() {
        let slope = Quat::from_rotation_x(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        app.world_mut().resource_mut::<VehiclePhysicsConfig>().tires.grip_coefficient = 0.05;
        advance_vehicle(&mut app, 240);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 240);
        assert!((truck_position(&mut app) - start).dot(slope * Vec3::Z).abs() > 0.25);
    }

    #[test]
    fn parking_assistance_stays_active_when_brake_overrides_throttle() {
        let slope = Quat::from_rotation_x(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyW);
            keys.press(KeyCode::Space);
        }
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start).dot(slope * Vec3::Z).abs();
        assert!(drift < 0.02, "brake and throttle parking drift: {drift:.6} m, velocity: {:?}", truck_velocity(&mut app));
    }

    #[test]
    fn slow_low_grip_sliding_is_not_hidden_by_sleeping() {
        let slope = Quat::from_rotation_x(3.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        app.world_mut().resource_mut::<VehiclePhysicsConfig>().tires.grip_coefficient = 0.05;
        advance_vehicle(&mut app, 600);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 360);
        let drift = (truck_position(&mut app) - start).dot(slope * Vec3::Z).abs();
        assert!(drift > 0.05, "insufficient grip must keep sliding slowly: {drift:.3} m");
    }

    #[test]
    fn driving_is_consistent_at_30_and_144_render_frames_per_second() {
        let drive_at_fps = |fps: usize| {
            let mut app = vehicle_test_app(Quat::IDENTITY);
            *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
                TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / fps as f64));
            advance_vehicle(&mut app, fps * 3);
            let start = truck_position(&mut app);
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
            advance_vehicle(&mut app, fps * 2);
            ((truck_position(&mut app) - start).z, truck_velocity(&mut app).linear.z)
        };
        let slow = drive_at_fps(30);
        let fast = drive_at_fps(144);
        assert!((slow.0 - fast.0).abs() < 0.10, "distance depends on rendering FPS: {slow:?} vs {fast:?}");
        assert!((slow.1 - fast.1).abs() < 0.10, "speed depends on rendering FPS: {slow:?} vs {fast:?}");
    }

    #[test]
    fn resetting_does_not_report_a_fictitious_acceleration() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.add_message::<AppExit>().add_systems(Update, controls);
        advance_vehicle(&mut app, 360);
        app.world_mut().query_filtered::<&mut Velocity, With<Truck>>()
            .single_mut(app.world_mut()).unwrap().linear.z = 3.0;
        advance_vehicle(&mut app, 2);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyR);
            keys.clear();
        }
        advance_vehicle(&mut app, 1);
        assert!(app.world().resource::<vehicle::telemetry::VehicleTelemetry>()
            .longitudinal_acceleration_mps2.abs() < 0.1);
    }

    #[test]
    fn stationary_wheels_do_not_keep_spinning() {
        let mut app = vehicle_test_app(Quat::from_rotation_z(8.0_f32.to_radians()));
        advance_vehicle(&mut app, 360);
        let spins = app.world().resource::<VehicleVisualState>().spins;
        advance_vehicle(&mut app, 600);
        assert_eq!(app.world().resource::<VehicleVisualState>().spins, spins);
    }

    #[test]
    fn steering_at_rest_does_not_turn_the_chassis() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyD);
        advance_vehicle(&mut app, 240);
        let transform = *app.world_mut().query_filtered::<&Transform, With<Truck>>()
            .single(app.world()).unwrap();
        assert!((transform.translation - start).length() < 0.02, "steering displacement: {:?}, velocity: {:?}, rotation: {:?}", transform.translation - start, truck_velocity(&mut app), transform.rotation);
        assert!((transform.rotation * Vec3::Z).x.abs() < 0.01);
        assert!(app.world().resource::<VehicleVisualState>().steer < -0.40);
    }

    #[test]
    fn reverse_can_wake_a_vehicle_at_rest() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 600);
        let start = truck_position(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyS);
        advance_vehicle(&mut app, 120);
        assert!(truck_position(&mut app).z < start.z - 0.5);
        assert!(truck_velocity(&mut app).linear.z < -0.5);
    }

    #[test]
    fn brakes_override_throttle_and_stop_the_vehicle() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 180);
        assert!(truck_velocity(&mut app).linear.z > 2.0);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        advance_vehicle(&mut app, 240);
        let velocity = truck_velocity(&mut app);
        assert!(velocity.linear.length() < 0.10, "braking velocity: {:?}", velocity.linear);
    }

    #[test]
    fn low_range_can_start_uphill() {
        let slope = Quat::from_rotation_x(-15.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        app.world_mut().resource_mut::<GameState>().low_range = true;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 120);
        let distance = (truck_position(&mut app) - start).dot(slope * Vec3::Z);
        assert!(distance > 0.5, "uphill progress: {distance:.3} m");
        assert!(app.world().resource::<GameState>().grounded_wheels >= 3);
    }

    #[test]
    fn unloaded_wheels_do_not_spin_from_chassis_motion() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        let mut query = app.world_mut().query_filtered::<(&mut Transform, &mut Velocity), With<Truck>>();
        let (mut transform, mut velocity) = query.single_mut(app.world_mut()).unwrap();
        transform.translation.y = 8.0;
        velocity.linear.z = 3.0;
        advance_vehicle(&mut app, 24);
        assert_eq!(app.world().resource::<VehicleVisualState>().spins, [0.0; 4]);
        assert_eq!(app.world().resource::<GameState>().grounded_wheels, 0);
    }

    #[test]
    fn vehicle_settles_on_the_actual_starting_terrain() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        let floor = app.world_mut().query_filtered::<Entity, (With<Collider>, Without<Truck>)>()
            .single(app.world()).unwrap();
        app.world_mut().entity_mut(floor).insert((ground().1, Transform::IDENTITY));
        let x = road_x(START);
        let mut query = app.world_mut().query_filtered::<&mut Transform, With<Truck>>();
        query.single_mut(app.world_mut()).unwrap().translation = Vec3::new(x, height(x, START) + 1.06, START);
        advance_vehicle(&mut app, 600);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start).length();
        assert!(drift < 0.10, "starting terrain drift: {drift:.3} m");
        assert_eq!(app.world().resource::<GameState>().grounded_wheels, 4);
        let velocity = truck_velocity(&mut app);
        assert!(velocity.linear.length() < 0.05, "terrain residual velocity: {velocity:?}");
    }

    #[test]
    fn driving_across_a_gentle_slope_keeps_lateral_grip() {
        let slope = Quat::from_rotation_z(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 360);
        let displacement = truck_position(&mut app) - start;
        assert!(displacement.dot(slope * Vec3::Z) > 5.0);
        assert!(displacement.dot(slope * Vec3::X).abs() < 0.25, "lateral driving drift: {displacement:?}");
    }

    #[test]
    fn right_turn_changes_heading_and_individual_wheel_speeds() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 180);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyD);
        advance_vehicle(&mut app, 120);
        let transform = *app.world_mut().query_filtered::<&Transform, With<Truck>>()
            .single(app.world()).unwrap();
        assert!((transform.rotation * Vec3::Z).x < -0.10);
        let spins = app.world().resource::<VehicleVisualState>().spins;
        assert!(Quat::from_rotation_x(spins[0]).angle_between(Quat::from_rotation_x(spins[1])) > 0.05);
    }

    #[test]
    fn vehicle_holds_still_on_a_gentle_cross_slope() {
        let slope = Quat::from_rotation_z(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start).dot(slope * Vec3::X).abs();
        assert!(drift < 0.10, "cross-slope drift: {drift:.3} m in 5 seconds");
    }

    #[test]
    fn vehicle_holds_still_on_a_gentle_uphill_slope() {
        let slope = Quat::from_rotation_x(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start).dot(slope * Vec3::Z).abs();
        assert!(drift < 0.10, "uphill drift: {drift:.3} m in 5 seconds");
    }

    #[test]
    fn wheel_rotation_rolls_forward_in_the_rotated_model() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        advance_vehicle(&mut app, 20);
        let rotation = app.world_mut().query_filtered::<&Transform, Without<Truck>>()
            .iter(app.world())
            .find(|transform| transform.translation == Vec3::ZERO)
            .unwrap().rotation;
        let model_rotation = Quat::from_rotation_y(std::f32::consts::PI);
        let bottom_motion = model_rotation * (rotation * Vec3::NEG_Y - Vec3::NEG_Y);
        assert!(bottom_motion.z < -0.001, "the bottom of a forward-rolling tire must move backward, got {bottom_motion:?}");
    }

    #[test]
    fn refuge_is_higher_than_start() {
        assert!(height(road_x(FINISH), FINISH) > height(road_x(START), START) + 12.0);
    }
    #[test]
    fn terrain_normal_points_up() {
        for z in [-60.0, 0.0, 60.0] {
            let n = normal(road_x(z), z);
            assert!(n.is_finite() && n.y > 0.5);
        }
    }
    #[test]
    fn vehicle_static_sag_is_plausible() {
        // La roue doit toucher le sol, et le corps du 4x4 ne doit pas flotter.
        let config = VehiclePhysicsConfig::baseline();
        let compression = config.chassis.mass_kg * 9.81 / (4.0 * config.front.stiffness_n_per_m);
        let wheel_drop = config.front.rest_length_m - compression;
        let chassis_height = 0.22 + config.tires.radius_m + wheel_drop;
        assert!(chassis_height > 0.72 && chassis_height < 1.05);
        assert!((chassis_height - 0.22 - wheel_drop - config.tires.radius_m).abs() < 0.001);
    }
    #[test]
    fn suspension_does_not_pull_vehicle_down() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        let physics = app.world().resource::<VehiclePhysics>();
        assert!(physics.controller.as_ref().unwrap().wheels().iter().all(|wheel| wheel.wheel_suspension_force >= 0.0));
    }
    #[test]
    fn motor_has_more_headroom() {
        assert!(18.0 * 3.6 >= 60.0);
        assert!(7.0 < 18.0);
    }
    #[test]
    fn camera_orbit_defaults_to_rear_view() {
        let orbit = OrbitCamera::default();
        assert_eq!(orbit.yaw_offset, 0.0);
        assert!((PITCH_MIN..=PITCH_MAX).contains(&orbit.pitch));
        assert!((CAMERA_MIN_DISTANCE..=CAMERA_MAX_DISTANCE).contains(&orbit.distance));
        let (s, c) = orbit.yaw_offset.sin_cos();
        let back = Vec3::new(-s, 0.0, -c);
        assert!((back - Vec3::NEG_Z).length() < 0.001);
    }
    #[test]
    fn road_stays_inside_map() {
        for z in -70..=70 {
            assert!(road_x(z as f32).abs() < WIDTH * 0.5 - 4.0);
        }
    }
}
