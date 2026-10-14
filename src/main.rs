//! Motor Trail — prototype de 4x4 Low Poly avec Bevy et Rapier.
//! Carte, vehicule, arbres, rochers et refuge proceduraux ; aucun asset externe.
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy_rapier3d::prelude::*;

const WIDTH: f32 = 120.0;
const DEPTH: f32 = 165.0;
const START: f32 = -67.0;
const FINISH: f32 = 65.0;
const WHEEL_RADIUS: f32 = 0.46;
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
#[derive(Component)]
struct WheelVisual { index: usize, spin: f32 }
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
            (controls, suspension_and_drive, animate_wheels, objective, camera_follow, display_hud).chain(),
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
    truck(&mut commands, &mut meshes, &mut materials);

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

fn truck(
    commands: &mut Commands, meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let paint = materials.add(Color::srgb(0.80, 0.32, 0.16));
    let glass = materials.add(Color::srgb(0.23, 0.36, 0.43));
    let rubber = materials.add(Color::srgb(0.12, 0.14, 0.15));
    let metal = materials.add(Color::srgb(0.27, 0.29, 0.28));
    let lights = materials.add(Color::srgb(0.97, 0.83, 0.48));
    let tire = meshes.add(Cylinder::new(WHEEL_RADIUS, 0.30));
    let car = commands.spawn((
        Truck,
        RigidBody::Dynamic,
        Collider::cuboid(1.02, 0.39, 1.77),
        ColliderMassProperties::Mass(CAR_MASS),
        Damping { linear_damping: 0.24, angular_damping: 1.25 },
        Ccd::enabled(),
        Velocity::zero(),
        ExternalForce::default(),
        Transform::from_xyz(road_x(START), height(road_x(START), START) + 1.05, START),
    )).id();
    commands.entity(car).with_children(|p| {
        p.spawn((Mesh3d(meshes.add(Cuboid::new(2.05, 0.75, 3.6))),
            MeshMaterial3d(paint.clone()), Transform::default()));
        p.spawn((Mesh3d(meshes.add(Cuboid::new(1.83, 0.8, 1.8))),
            MeshMaterial3d(paint), Transform::from_xyz(0.0, 0.65, -0.3)));
        p.spawn((Mesh3d(meshes.add(Cuboid::new(1.7, 0.5, 0.08))),
            MeshMaterial3d(glass), Transform::from_xyz(0.0, 0.72, 0.63)));
        p.spawn((Mesh3d(meshes.add(Cuboid::new(2.15, 0.22, 0.23))),
            MeshMaterial3d(metal), Transform::from_xyz(0.0, -0.26, 1.84)));
        for (index, position) in MOUNTS.iter().enumerate() {
            p.spawn((
                WheelVisual { index, spin: 0.0 },
                Mesh3d(tire.clone()),
                MeshMaterial3d(rubber.clone()),
                Transform::from_translation(*position - Vec3::Y * SUSPENSION)
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2)),
            ));
        }
        for x in [-0.72, 0.72] {
            p.spawn((Mesh3d(meshes.add(Cuboid::new(0.35, 0.2, 0.11))),
                MeshMaterial3d(lights.clone()), Transform::from_xyz(x, -0.03, 1.84)));
        }
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
    let max_speed = if state.low_range { 5.0 } else { 12.5 };
    let engine = if state.low_range { 1_450.0 } else { 1_090.0 };
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

        let angle = if i < 2 { steer_angle } else { 0.0 };
        let rotation = Quat::from_axis_angle(up, angle);
        let heading = rotation * forward;
        let side = rotation * right;
        // Le pneu s'oppose à la glissade latérale, mais garde assez de souplesse
        // pour rester contrôlable en traversant les bosses.
        wheel_force -= side * (at_mount.dot(side) * 1_650.0).clamp(-4_600.0, 4_600.0);
        wheel_force -= heading * at_mount.dot(heading)
            * if braking { 1_950.0 } else { 95.0 };

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
        torque += up * steer * forward_speed.clamp(-8.0, 8.0) * 140.0;
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
    mut wheels: Query<(&mut WheelVisual, &mut Transform), Without<Truck>>,
) {
    let Ok(rapier) = context.single() else { return; };
    let Ok((car_entity, car, velocity)) = cars.single() else { return; };
    let forward = car.rotation * Vec3::Z;
    let up = car.rotation * Vec3::Y;
    let wheel_speed = velocity.linear.dot(forward) / WHEEL_RADIUS;
    let steer = steering_input(&keys);
    let steer_angle = steer * 0.43 / (1.0 + velocity.linear.dot(forward).abs() * 0.065);
    for (mut wheel, mut transform) in &mut wheels {
        let mount = MOUNTS[wheel.index];
        let origin = car.translation + car.rotation * mount;
        let travel = rapier.cast_ray_and_get_normal(
            origin, -up, SUSPENSION + WHEEL_RADIUS, true,
            QueryFilter::default().exclude_rigid_body(car_entity),
        ).filter(|(_, hit)| hit.normal.dot(Vec3::Y) >= 0.42)
            .map(|(_, hit)| (hit.time_of_impact - WHEEL_RADIUS).clamp(0.0, SUSPENSION))
            .unwrap_or(SUSPENSION);

        transform.translation = mount - Vec3::Y * travel;
        let angle = if wheel.index < 2 { steer_angle } else { 0.0 };
        wheel.spin = (wheel.spin + wheel_speed * time.delta_secs()).rem_euclid(TAU);
        transform.rotation = Quat::from_rotation_y(angle)
            * Quat::from_rotation_z(FRAC_PI_2)
            * Quat::from_rotation_y(wheel.spin);
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

fn camera_follow(
    time: Res<Time>,
    cars: Query<&Transform, With<Truck>>,
    mut cameras: Query<&mut Transform, (With<ChaseCamera>, Without<Truck>)>,
) {
    let (Ok(car), Ok(mut camera)) = (cars.single(), cameras.single_mut()) else { return; };
    // Caméra stable : elle suit le cap horizontal, sans hériter du roulis.
    let forward = (car.rotation * Vec3::Z).with_y(0.0).normalize_or_zero();
    let forward = if forward.length_squared() < 0.01 { Vec3::Z } else { forward };
    let destination = car.translation - forward * 9.8 + Vec3::Y * 4.5;
    let alpha = 1.0 - (-4.5 * time.delta_secs()).exp();
    camera.translation = camera.translation.lerp(destination, alpha);
    camera.look_at(car.translation + Vec3::Y * 0.9 + forward * 3.0, Vec3::Y);
}

fn display_hud(
    state: Res<GameState>,
    cars: Query<&Velocity, With<Truck>>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let (Ok(speed), Ok(mut text)) = (cars.single(), hud.single_mut()) else { return; };
    let range = if state.low_range { "COURTE" } else { "NORMALE" };
    let wheels_on_ground = state.grounded_wheels;
    let mission = if state.reached_refuge { "REFUGE ATTEINT ! Explore librement." }
        else { "OBJECTIF : rejoindre le refuge au sommet de la piste" };
    text.0 = format!(
        "MOTOR TRAIL  |  {:.0} km/h  |  GAMME {range}  |  Roues au sol: {wheels_on_ground}/4\n{mission}\n\
        Z/W avancer  S reculer  Q/A gauche  D droite  Espace frein\n\
        L gamme courte  R recommencer  Echap quitter",
        speed.linear.length() * 3.6
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
    fn road_stays_inside_map() {
        for z in -70..=70 {
            assert!(road_x(z as f32).abs() < WIDTH * 0.5 - 4.0);
        }
    }
}
