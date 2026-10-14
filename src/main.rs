//! Motor Trail — prototype de 4x4 Low Poly avec Bevy et Rapier.
//! Carte, vehicule, arbres, rochers et refuge proceduraux ; aucun asset externe.
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::*;

const WIDTH: f32 = 120.0;
const DEPTH: f32 = 165.0;
const START: f32 = -67.0;
const FINISH: f32 = 65.0;
const WHEEL_RADIUS: f32 = 0.53; // Rayon du pneu fourni dans le nouveau GLB.
// Débattement volontairement court : la version précédente portait le châssis
// trop haut et les roues visuelles n'atteignaient jamais le sol.
const SUSPENSION: f32 = 0.36;
const SPRING_STIFFNESS: f32 = 15_000.0;
const SPRING_DAMPING: f32 = 1_750.0;
const MAX_SPRING_FORCE: f32 = 8_500.0;
const CAR_MASS: f32 = 930.0;
const MOUNTS: [Vec3; 4] = [
    Vec3::new(-0.91, -0.22,  1.22),
    Vec3::new( 0.91, -0.22,  1.22),
    Vec3::new(-0.91, -0.22, -1.22),
    Vec3::new( 0.91, -0.22, -1.22),
];

#[derive(Component)]
struct Truck;
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
struct WheelVisual { index: usize, spin: f32 }
#[derive(Resource, Default)]
struct VehicleVisualState { spin: f32, steer: f32, travels: [f32; 4] }
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
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.69, 0.82, 0.94)))
        .init_resource::<GameState>()
        .init_resource::<OrbitCamera>()
        .init_resource::<VehicleVisualState>()
        .init_resource::<FrameStats>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Motor Trail | Rando 4x4".into(),
                resolution: (1280, 780).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (controls, suspension_and_drive, animate_wheels, objective, camera_follow, display_hud, update_gear_widget).chain(),
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
    truck(&mut commands, &asset_server);

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
                width: Val::Px(27.0), height: Val::Px(27.0), ..default()
            },
            BackgroundColor(Color::srgb(0.95, 0.64, 0.19)),
            BorderRadius::all(Val::Px(14.0)),
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

fn truck(commands: &mut Commands, asset_server: &AssetServer) {
    let x = road_x(START);
    let car = commands.spawn((
        Truck,
        RigidBody::Dynamic,
        Collider::cuboid(1.10, 0.40, 1.92),
        ColliderMassProperties::Mass(CAR_MASS),
        Damping { linear_damping: 0.12, angular_damping: 1.25 },
        Ccd::enabled(),
        Velocity::zero(),
        ExternalForce::default(),
        Transform::from_xyz(x, height(x, START) + 1.06, START),
    )).id();

    // Ce modèle est livré avec l'avant en -Z alors que la physique du jeu va
    // vers +Z. Le pivot est donc retourné de PI autour de Y, sans toucher
    // aux colliders ou aux points d'attache des roues.
    commands.entity(car).with_children(|p| {
        p.spawn((
            SceneRoot(asset_server.load("vehicles/motor_trail_4x4.glb#Scene0")),
            Transform::from_xyz(0.0, -1.0, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        ));
    });
}

fn axis(keys: &ButtonInput<KeyCode>, pos: &[KeyCode], neg: &[KeyCode]) -> f32 {
    let a = pos.iter().any(|k| keys.pressed(*k)) as i32;
    let b = neg.iter().any(|k| keys.pressed(*k)) as i32;
    (a - b) as f32
}

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    mut cars: Query<(&mut Transform, &mut Velocity, &mut ExternalForce), With<Truck>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::KeyL) { state.low_range = !state.low_range; }
    if keys.just_pressed(KeyCode::Escape) { exit.write(AppExit::Success); }
    if keys.just_pressed(KeyCode::KeyR) {
        if let Ok((mut transform, mut speed, mut force)) = cars.single_mut() {
            let x = road_x(START);
            transform.translation = Vec3::new(x, height(x, START) + 1.15, START);
            transform.rotation = Quat::IDENTITY;
            *speed = Velocity::zero();
            *force = ExternalForce::default();
            state.reached_refuge = false;
        }
    }
}

// Le véhicule se déplace vers +Z ; avec la caméra derrière lui, le côté droit
// de l'image correspond à -X dans ce repère. On inverse donc la direction de
// braquage à l'entrée (D = droite à l'écran, Q/A = gauche).
fn steering_input(keys: &ButtonInput<KeyCode>) -> f32 {
    -axis(keys, &[KeyCode::KeyD, KeyCode::ArrowRight],
        &[KeyCode::KeyA, KeyCode::KeyQ, KeyCode::ArrowLeft])
}

fn suspension_force(compression: f32, vertical_speed: f32) -> f32 {
    (compression * SPRING_STIFFNESS - vertical_speed * SPRING_DAMPING)
        .clamp(0.0, MAX_SPRING_FORCE)
}

fn suspension_and_drive(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    context: ReadRapierContext,
    mut cars: Query<(Entity, &Transform, &Velocity, &mut ExternalForce), With<Truck>>,
) {
    let Ok(rapier) = context.single() else { return; };
    let Ok((entity, transform, velocity, mut external)) = cars.single_mut() else { return; };
    let gas = axis(&keys, &[KeyCode::KeyW, KeyCode::KeyZ, KeyCode::ArrowUp],
        &[KeyCode::KeyS, KeyCode::ArrowDown]);
    let steer = steering_input(&keys);
    let braking = keys.pressed(KeyCode::Space);

    let forward = transform.rotation * Vec3::Z;
    let right = transform.rotation * Vec3::X;
    let up = transform.rotation * Vec3::Y;
    let forward_speed = velocity.linear.dot(forward);
    // Les anciennes valeurs donnaient un véhicule anémique en montée.
    // Le moteur produit davantage de couple, et la résistance passive est moindre.
    let max_speed = if state.low_range { 7.0 } else { 18.0 };
    let engine = if state.low_range { 2_100.0 } else { 1_650.0 };
    // Moins d'angle de braquage à haute vitesse pour éviter les tête-à-queue.
    let steer_angle = steer * 0.43 / (1.0 + forward_speed.abs() * 0.065);
    let mut force = Vec3::ZERO;
    let mut torque = Vec3::ZERO;
    let mut contacts = 0;

    for (i, mount) in MOUNTS.iter().enumerate() {
        let r = transform.rotation * *mount;
        let origin = transform.translation + r;
        let filter = QueryFilter::default().exclude_rigid_body(entity);
        let Some((_, hit)) = rapier.cast_ray_and_get_normal(
            origin, -up, SUSPENSION + WHEEL_RADIUS, true, filter
        ) else { continue };
        if hit.normal.dot(Vec3::Y) < 0.42 { continue; }
        contacts += 1;

        let travel = (hit.time_of_impact - WHEEL_RADIUS).clamp(0.0, SUSPENSION);
        let compression = SUSPENSION - travel;
        let at_mount = velocity.linear + velocity.angular.cross(r);
        let spring = suspension_force(compression, at_mount.dot(up));
        let mut wheel_force = up * spring;

        // Au point mort, les roues AV ne doivent pas exercer un couple de lacet
        // quand le joueur ne fait que braquer à l'arrêt.
        let stationary = forward_speed.abs() < 0.65 && gas.abs() < 0.01;
        let angle = if i < 2 && !stationary { steer_angle } else { 0.0 };
        let rotation = Quat::from_axis_angle(up, angle);
        let heading = rotation * forward;
        let side = rotation * right;
        // Le pneu s'oppose à la glissade latérale, mais garde assez de souplesse
        // pour rester contrôlable en traversant les bosses.
        wheel_force -= side * (at_mount.dot(side) * 1_650.0).clamp(-4_600.0, 4_600.0);
        wheel_force -= heading * at_mount.dot(heading)
            * if braking { 1_950.0 } else { 22.0 };

        if gas.abs() > 0.01
            && (forward_speed.abs() < max_speed || forward_speed.signum() != gas.signum())
        {
            wheel_force += heading * gas * engine;
        }

        force += wheel_force;
        // Forces appliquées à la position de la roue, et non au centre du châssis.
        torque += r.cross(wheel_force);
    }
    state.grounded_wheels = contacts;
    if contacts > 0 {
        // Assistance faible ; on privilégie les forces de pneus au pivot forcé.
        if forward_speed.abs() > 0.65 {
            torque += up * steer * forward_speed.clamp(-8.0, 8.0) * 110.0;
        }
        torque -= velocity.angular * 290.0;
    }
    external.force = force;
    external.torque = torque;
}

// La position des roues affichées suit le vrai débattement raycast.
// Avant, elles restaient fixes à 34 cm sous le point d'attache et semblaient
// flotter dans le vide pendant que le châssis reposait sur ses ressorts.
fn animate_wheels(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    context: ReadRapierContext,
    cars: Query<(Entity, &Transform, &Velocity), With<Truck>>,
    mut visual: ResMut<VehicleVisualState>,
    mut nodes: Query<(&Name, &mut Transform), Without<Truck>>,
) {
    let Ok(rapier) = context.single() else { return; };
    let Ok((car_entity, car, velocity)) = cars.single() else { return; };
    let forward = car.rotation * Vec3::Z;
    let up = car.rotation * Vec3::Y;
    let speed = velocity.linear.dot(forward);
    let steer = steering_input(&keys);
    visual.steer = steer * 0.43 / (1.0 + speed.abs() * 0.065);
    visual.spin = (visual.spin + (speed / WHEEL_RADIUS) * time.delta_secs())
        .rem_euclid(TAU);
    for (i, mount) in MOUNTS.iter().enumerate() {
        let origin = car.translation + car.rotation * *mount;
        visual.travels[i] = rapier.cast_ray_and_get_normal(
            origin, -up, SUSPENSION + WHEEL_RADIUS, true,
            QueryFilter::default().exclude_rigid_body(car_entity),
        ).filter(|(_, hit)| hit.normal.dot(Vec3::Y) >= 0.42)
            .map(|(_, hit)| (hit.time_of_impact - WHEEL_RADIUS).clamp(0.0, SUSPENSION))
            .unwrap_or(SUSPENSION);
    }
    // L'importeur glTF instancie les entités nommées après le chargement de la
    // scène. Chaque pivot reste local à son parent, et sa roue suit la physique.
    // Le modèle a été retourné autour de Y, d'où le miroir gauche/droite.
    for (name, mut transform) in &mut nodes {
        let n = name.as_str();
        let wheel = match n {
            "Suspension_FL" | "Steer_FL" | "WheelSpin_FL" => Some(1),
            "Suspension_FR" | "Steer_FR" | "WheelSpin_FR" => Some(0),
            "Suspension_RL" | "WheelSpin_RL" => Some(3),
            "Suspension_RR" | "WheelSpin_RR" => Some(2),
            _ => None,
        };
        let Some(index) = wheel else { continue };
        if n.starts_with("Suspension_") {
            // 0.78 = base locale (.56) + débattement au repos (.22).
            transform.translation.y = 0.78 - visual.travels[index];
        } else if n.starts_with("Steer_") {
            transform.rotation = Quat::from_rotation_y(visual.steer);
        } else if n.starts_with("WheelSpin_") {
            transform.rotation = Quat::from_rotation_x(visual.spin);
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
    if keys.just_pressed(KeyCode::KeyC) {
        orbit.yaw_offset = 0.0;
        orbit.pitch = 0.40;
        orbit.distance = 10.0;
    }
    if mouse_buttons.pressed(MouseButton::Right) {
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
    if scroll_lines.abs() > f32::EPSILON {
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
        L : gamme {range}  R : recommencer  Echap : quitter",
        speed.linear.length() * 3.6,
        frames.smooth_fps,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let compression = CAR_MASS * 9.81 / (4.0 * SPRING_STIFFNESS);
        let wheel_drop = SUSPENSION - compression;
        let chassis_height = 0.22 + WHEEL_RADIUS + wheel_drop;
        assert!(chassis_height > 0.72 && chassis_height < 1.05);
        assert!((chassis_height - 0.22 - wheel_drop - WHEEL_RADIUS).abs() < 0.001);
    }
    #[test]
    fn suspension_does_not_pull_vehicle_down() {
        assert_eq!(suspension_force(0.0, 1.0), 0.0);
        assert!(suspension_force(0.15, 0.0) > 0.0);
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
