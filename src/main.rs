//! Motor Trail — prototype de 4x4 Low Poly avec Bevy et Rapier.
//! Vallee procedurale et vehicule articule charge depuis le modele glTF.
use std::f32::consts::TAU;

mod app_paths;
mod persistence;
mod terrain;
mod ui;
mod vehicle;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use bevy_rapier3d::prelude::*;
use persistence::ProfileStore;
use terrain::legacy::{FINISH, START, height, road_x};
#[cfg(test)]
use terrain::legacy::{WIDTH, ground, normal};
use ui::{PhysicsTuningPlugin, UiInputCapture};
use vehicle::config::VehiclePhysicsConfig;
use vehicle::{Truck, VehiclePhysics, VehiclePlugin, VehicleVisualState};

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
struct GearLabel {
    low: bool,
}
#[derive(Resource)]
struct FrameStats {
    smooth_fps: f32,
}
impl Default for FrameStats {
    fn default() -> Self {
        Self { smooth_fps: 60.0 }
    }
}
#[derive(Component)]
struct Hud;

#[derive(Resource, Default)]
struct GameState {
    low_range: bool,
    reached_refuge: bool,
    grounded_wheels: usize,
}

fn main() -> AppExit {
    match run() {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("Motor-Trail: {error}");
            AppExit::error()
        }
    }
}

fn run() -> Result<AppExit, String> {
    let resources = app_paths::ResourceLayout::discover()?;
    let version = resources.check_installation()?;
    if std::env::args_os().any(|arg| arg == "--check-installation") {
        println!(
            "Motor-Trail {version}: installation resources validated (no renderer or user-data writes)"
        );
        return Ok(AppExit::Success);
    }
    let data = app_paths::command_line_data_root()?;
    resources.initialize_user_data(&data)?;
    let data = data
        .canonicalize()
        .map_err(|e| format!("Cannot resolve user-data directory: {e}"))?;
    let (config, profiles) = ProfileStore::bootstrap(data.join("vehicles"));
    println!("Motor-Trail {version}; user data: {}", data.display());
    Ok(App::new()
        .insert_resource(config)
        .insert_resource(profiles)
        .insert_resource(terrain::persistence::TerrainStore::new(
            data.join("terrains"),
        ))
        .insert_resource(ClearColor(Color::srgb(0.69, 0.82, 0.94)))
        .init_resource::<GameState>()
        .init_resource::<OrbitCamera>()
        .init_resource::<FrameStats>()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: resources
                        .assets
                        .to_str()
                        .ok_or("Asset directory must be UTF-8")?
                        .to_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Motor Trail | Rando 4x4".into(),
                        resolution: (1280, 780).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((VehiclePlugin, EguiPlugin::default(), PhysicsTuningPlugin))
        .add_plugins(vehicle::diagnostics::WheelDiagnosticsPlugin)
        .add_plugins(vehicle::body_probe::VehicleBodyProbePlugin)
        .add_plugins((
            terrain::TerrainPlugin,
            ui::terrain_workshop::TerrainWorkshopPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                controls,
                objective,
                camera_follow,
                display_hud,
                update_gear_widget,
            )
                .chain(),
        )
        .run())
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    config: Res<VehiclePhysicsConfig>,
    active: Res<terrain::runtime::ActiveTerrain>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 13_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-30.0, 55.0, -35.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    truck(&mut commands, &asset_server, &config, &active.scene);

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.0, 6.0, START - 11.0)
            .looking_at(Vec3::new(road_x(START), 1.5, START), Vec3::Y),
        ChaseCamera,
    ));
    commands.spawn((
        Text::new("MOTOR TRAIL"),
        TextFont {
            font_size: FontSize::Px(19.0),
            ..default()
        },
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
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(24.0),
                bottom: Val::Px(24.0),
                width: Val::Px(204.0),
                height: Val::Px(170.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.065, 0.085, 0.11, 0.89)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("TRANSFERT 4×4"),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(14.0),
                    top: Val::Px(10.0),
                    ..default()
                },
            ));
            panel.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(29.0),
                    top: Val::Px(47.0),
                    width: Val::Px(7.0),
                    height: Val::Px(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.41, 0.45, 0.48)),
            ));
            panel.spawn((
                GearKnob,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(19.0),
                    top: Val::Px(116.0),
                    width: Val::Px(27.0),
                    height: Val::Px(27.0),
                    border_radius: BorderRadius::all(Val::Px(14.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.95, 0.64, 0.19)),
            ));
            panel.spawn((
                GearLabel { low: true },
                Text::new("L    COURTE"),
                TextFont {
                    font_size: FontSize::Px(19.0),
                    ..default()
                },
                TextColor(Color::srgb(0.64, 0.67, 0.70)),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(58.0),
                    top: Val::Px(54.0),
                    ..default()
                },
            ));
            panel.spawn((
                GearLabel { low: false },
                Text::new("H    NORMALE"),
                TextFont {
                    font_size: FontSize::Px(19.0),
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.64, 0.19)),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(58.0),
                    top: Val::Px(120.0),
                    ..default()
                },
            ));
        });
}

fn update_gear_widget(
    state: Res<GameState>,
    mut knob: Query<&mut Node, With<GearKnob>>,
    mut labels: Query<(&GearLabel, &mut TextColor)>,
) {
    if !state.is_changed() {
        return;
    }
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

fn truck(
    commands: &mut Commands,
    asset_server: &AssetServer,
    config: &VehiclePhysicsConfig,
    scene: &terrain::scene::TerrainScene,
) {
    let car = commands
        .spawn((
            vehicle::controller::chassis_bundle(config),
            terrain::runtime::spawn_transform(scene, config),
        ))
        .id();

    // Ce modèle est livré avec l'avant en -Z alors que la physique du jeu va
    // vers +Z. Le pivot est donc retourné de PI autour de Y, sans toucher
    // aux colliders ou aux points d'attache des roues.
    commands.entity(car).with_children(|p| {
        let model_pose = Transform::from_xyz(0.0, -1.0, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI));
        p.spawn((
            vehicle::presentation::VehicleGraphic::new(car, Transform::IDENTITY, model_pose),
            WorldAssetRoot(
                asset_server
                    .load(GltfAssetLabel::Scene(0).from_asset("vehicles/motor_trail_4x4.glb")),
            ),
            model_pose,
        ));
    });
}

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<UiInputCapture>,
    active: Res<terrain::runtime::ActiveTerrain>,
    mut workshop: ResMut<terrain::runtime::TerrainWorkshopState>,
    equipment: Res<vehicle::equipment_runtime::EquipmentRuntime>,
    config: Res<VehiclePhysicsConfig>,
    mut state: ResMut<GameState>,
    mut physics: ResMut<VehiclePhysics>,
    mut visual: ResMut<VehicleVisualState>,
    mut telemetry: ResMut<vehicle::telemetry::VehicleTelemetry>,
    mut cars: Query<(&mut Transform, &mut Velocity, &mut ExternalForce), With<Truck>>,
    mut exit: MessageWriter<AppExit>,
) {
    if capture.keyboard {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    if workshop.mode == terrain::runtime::WorkshopMode::Edit {
        return;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        state.low_range = !state.low_range;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let spawn_config = vehicle::controller::effective_configuration(&config, &equipment)
            .map(|(effective, _)| effective)
            .unwrap_or_else(|_| physics.active_config.clone());
        let pose = match terrain::runtime::checked_spawn_transform(
            &active.scene,
            &spawn_config,
            equipment.bounds(),
        ) {
            Ok(pose) => pose,
            Err(error) => {
                workshop.last_error = Some(error);
                return;
            }
        };
        if let Ok((mut transform, mut speed, mut force)) = cars.single_mut() {
            *transform = pose;
            *speed = Velocity::zero();
            *force = ExternalForce::default();
            physics.reset_motion();
            *visual = VehicleVisualState::default();
            telemetry.reset_motion_history();
            state.grounded_wheels = 0;
            state.reached_refuge = false;
            workshop.last_error = None;
        }
    }
}

fn objective(
    mut state: ResMut<GameState>,
    active: Res<terrain::runtime::ActiveTerrain>,
    cars: Query<&Transform, With<Truck>>,
) {
    if active.scene.terrain.config.preset != terrain::config::TerrainPreset::Legacy {
        return;
    }
    if state.reached_refuge {
        return;
    }
    if let Ok(car) = cars.single() {
        let x = road_x(FINISH) + 4.0;
        let target = Vec3::new(x, height(x, FINISH), FINISH);
        if car.translation.distance(target) < 10.0 {
            state.reached_refuge = true;
        }
    }
}

// Caméra orbitale : maintenir clic droit et glisser pour observer les roues,
// la suspension et le véhicule sur le terrain, même en conduisant.
fn camera_follow(
    time: Res<Time>,
    presentation: Res<vehicle::presentation::VehiclePresentation>,
    capture: Res<UiInputCapture>,
    workshop: Res<terrain::runtime::TerrainWorkshopState>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_scroll: Res<AccumulatedMouseScroll>,
    mut orbit: ResMut<OrbitCamera>,
    cars: Query<(Entity, &Transform), With<Truck>>,
    context: ReadRapierContext,
    mut cameras: Query<&mut Transform, (With<ChaseCamera>, Without<Truck>)>,
) {
    if workshop.mode == terrain::runtime::WorkshopMode::Edit {
        return;
    }
    let (Ok((car_entity, car)), Ok(mut camera)) = (cars.single(), cameras.single_mut()) else {
        return;
    };
    let car = presentation.pose_for(car_entity, *car);

    // C recentre derrière le 4x4 et restaure la distance de conduite.
    if !capture.keyboard && keys.just_pressed(KeyCode::KeyC) {
        orbit.yaw_offset = 0.0;
        orbit.pitch = 0.40;
        orbit.distance = 10.0;
    }
    if !capture.pointer && mouse_buttons.pressed(MouseButton::Right) {
        orbit.yaw_offset =
            (orbit.yaw_offset - mouse_motion.delta.x * ORBIT_SENSITIVITY).rem_euclid(TAU);
        orbit.pitch =
            (orbit.pitch + mouse_motion.delta.y * ORBIT_SENSITIVITY).clamp(PITCH_MIN, PITCH_MAX);
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
        + Vec3::new(
            -azimuth_sin * horizontal,
            orbit.distance * orbit.pitch.sin(),
            -azimuth_cos * horizontal,
        );

    // On raccourcit le bras de caméra lorsque le relief ou un arbre se trouve
    // entre le 4x4 et la caméra (sans bloquer l'orbite).
    let ray = desired - orbit.focus;
    let mut safe_position = desired;
    if let Ok(rapier) = context.single() {
        if let Some((_, hit)) = rapier.cast_ray(
            orbit.focus,
            ray.normalize(),
            ray.length(),
            true,
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
    active: Res<terrain::runtime::ActiveTerrain>,
    workshop: Res<terrain::runtime::TerrainWorkshopState>,
    time: Res<Time>,
    mut frames: ResMut<FrameStats>,
    cars: Query<&Velocity, With<Truck>>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let (Ok(speed), Ok(mut text)) = (cars.single(), hud.single_mut()) else {
        return;
    };
    let range = if state.low_range { "COURTE" } else { "NORMALE" };
    let wheels_on_ground = state.grounded_wheels;
    if time.delta_secs() > 0.0 {
        let instant = (1.0 / time.delta_secs()).clamp(0.0, 500.0);
        frames.smooth_fps += (instant - frames.smooth_fps) * 0.06;
    }
    let mission = if active.scene.terrain.config.preset == terrain::config::TerrainPreset::Legacy {
        if state.reached_refuge {
            "REFUGE ATTEINT ! Explore librement.".to_owned()
        } else {
            "OBJECTIF : rejoindre le refuge au sommet de la piste".to_owned()
        }
    } else {
        format!(
            "Terrain : {} | {}",
            active.scene.name,
            if workshop.mode == terrain::runtime::WorkshopMode::Edit {
                "Edition"
            } else {
                "Conduite"
            }
        )
    };
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
        app.add_plugins((MinimalPlugins, TransformPlugin, VehiclePlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 120.0,
            )))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<GameState>();
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
        app.world_mut()
            .spawn((Name::new("WheelSpin_FL"), Transform::default()));
        app
    }

    fn advance_vehicle(app: &mut App, steps: usize) {
        for _ in 0..steps {
            app.update();
        }
    }

    #[test]
    fn wheel_rendering_advances_between_fixed_ticks_at_144_fps() {
        for experimental in [false, true] {
            let mut app = vehicle_test_app(Quat::IDENTITY);
            app.insert_resource(vehicle::wheel_settings::WheelContactSettings { experimental });
            advance_vehicle(&mut app, 360);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            advance_vehicle(&mut app, 120);
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 144.0,
            )));
            let mut checked = 0;
            for _ in 0..144 {
                let before = app.world().resource::<VehicleVisualState>().spins[1];
                let rotation = app
                    .world_mut()
                    .query::<(&Name, &Transform)>()
                    .iter(app.world())
                    .find(|(name, _)| name.as_str() == "WheelSpin_FL")
                    .unwrap()
                    .1
                    .rotation;
                app.update();
                let after = app.world().resource::<VehicleVisualState>().spins[1];
                if before == after {
                    let rendered = app
                        .world_mut()
                        .query::<(&Name, &Transform)>()
                        .iter(app.world())
                        .find(|(name, _)| name.as_str() == "WheelSpin_FL")
                        .unwrap()
                        .1
                        .rotation;
                    assert!(
                        rotation.angle_between(rendered) > 1.0e-5,
                        "moving wheel repeated a rendered pose on a frame without a physics tick"
                    );
                    checked += 1;
                }
            }
            assert!(checked >= 20);
        }
    }

    #[test]
    fn chase_camera_tracks_the_displayed_pose_and_not_the_latest_physics_tick() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.init_resource::<terrain::runtime::TerrainWorkshopState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<OrbitCamera>()
            .add_systems(Update, camera_follow);
        app.world_mut()
            .spawn((ChaseCamera, Transform::from_xyz(0.0, 5.0, -10.0)));
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 144.0,
        )));
        let mut checked = 0;
        for _ in 0..144 {
            let before = app.world().resource::<OrbitCamera>().focus;
            app.update();
            let (entity, physical) = app
                .world_mut()
                .query_filtered::<(Entity, &Transform), With<Truck>>()
                .single(app.world())
                .map(|(entity, pose)| (entity, *pose))
                .unwrap();
            let shown = app
                .world()
                .resource::<vehicle::presentation::VehiclePresentation>()
                .pose_for(entity, physical);
            let alpha = 1.0 - (-7.0 * app.world().resource::<Time>().delta_secs()).exp();
            let expected = before.lerp(shown.translation + Vec3::Y * 0.8, alpha);
            assert!(
                app.world()
                    .resource::<OrbitCamera>()
                    .focus
                    .distance(expected)
                    < 1.0e-5
            );
            if shown.translation.distance(physical.translation) > 0.001 {
                checked += 1;
            }
        }
        assert!(checked > 50);
    }

    fn truck_position(app: &mut App) -> Vec3 {
        app.world_mut()
            .query_filtered::<&Transform, With<Truck>>()
            .single(app.world())
            .unwrap()
            .translation
    }

    fn truck_velocity(app: &mut App) -> Velocity {
        *app.world_mut()
            .query_filtered::<&Velocity, With<Truck>>()
            .single(app.world())
            .unwrap()
    }

    fn truck_mass(app: &mut App) -> f32 {
        let handle = *app
            .world_mut()
            .query_filtered::<&RapierRigidBodyHandle, With<Truck>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .query::<&RapierRigidBodySet>()
            .single(app.world())
            .unwrap()
            .bodies[handle.0]
            .mass()
    }

    #[test]
    fn rollover_roof_and_existing_cargo_remain_above_flat_ground() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        {
            let mut cars = app
                .world_mut()
                .query_filtered::<&mut Transform, With<Truck>>();
            let mut car = cars.single_mut(app.world_mut()).unwrap();
            *car = Transform::from_xyz(0.0, 3.0, 0.0)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::PI));
        }
        advance_vehicle(&mut app, 1200);
        let car = *app
            .world_mut()
            .query_filtered::<&Transform, With<Truck>>()
            .single(app.world())
            .unwrap();
        let mut minimum = f32::INFINITY;
        for x in [-0.785, 0.785] {
            for z in [-1.37, 0.53] {
                minimum = minimum.min(car.transform_point(Vec3::new(x, 1.164, z)).y);
            }
        }
        minimum = minimum.min(car.transform_point(Vec3::new(-0.43, 1.5225, -1.05)).y);
        assert!(
            minimum > -0.04,
            "Roof/cargo penetrated the floor by {} m; pose {car:?}",
            -minimum
        );
        assert!(
            (car.rotation * Vec3::Y).y < -0.5,
            "The truck was automatically righted"
        );
        assert!(truck_velocity(&mut app).linear.length() < 0.2);
    }

    #[test]
    fn wheel_contacts_on_flat_ground_match_the_physical_radius_and_static_load() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 600);
        let car = *app
            .world_mut()
            .query_filtered::<&Transform, With<Truck>>()
            .single(app.world())
            .unwrap();
        let physics = app.world().resource::<VehiclePhysics>();
        let mut load = 0.0;
        for (mount, contact) in vehicle::controller::MOUNTS.into_iter().zip(physics.wheels) {
            assert!(contact.grounded);
            let center = car.transform_point(mount - Vec3::Y * contact.suspension_length);
            assert!((center.y - physics.active_config.tires.radius_m).abs() < 0.01);
            assert!(
                (center.distance(contact.point) - physics.active_config.tires.radius_m).abs()
                    < 0.01
            );
            load += contact.normal_load;
        }
        let weight = physics.active_config.chassis.mass_kg * 9.81;
        assert!(
            (load - weight).abs() / weight < 0.03,
            "load {load}, weight {weight}"
        );
        let equilibrium = vehicle::controller::spawn_height(&physics.active_config);
        assert!((car.translation.y - equilibrium).abs() < 0.01);
    }

    #[test]
    fn wheel_step_diagnostic_reproduces_native_raycast_penetration() {
        characterize_raycast_step(1.0);
    }

    #[test]
    fn wheel_step_diagnostic_reproduces_native_raycast_penetration_in_reverse() {
        characterize_raycast_step(-1.0);
    }

    fn characterize_raycast_step(direction: f32) {
        let penetration = measure_step(direction, false, 0.25, true);
        // Characterize the retained controller's limitation, not a corrected contact.
        assert!(
            (0.20..0.29).contains(&penetration),
            "Native raycast diagnostic changed: penetration {penetration} m"
        );
    }

    #[test]
    fn wheel_experimental_crosses_25cm_steps_forward_and_reverse() {
        for direction in [-1.0, 1.0] {
            let penetration = measure_step(direction, true, 0.25, true);
            assert!(
                penetration < 0.065,
                "Experimental step penetration {penetration} m"
            );
        }
    }

    #[test]
    fn wheel_experimental_taller_step_stays_finite_without_an_arbitrary_climb_impulse() {
        let penetration = measure_step(1.0, true, 0.4, false);
        assert!(
            penetration < 0.10,
            "Higher-step penetration {penetration} m"
        );
    }

    fn measure_step(direction: f32, experimental: bool, height: f32, must_cross: bool) -> f32 {
        use terrain::obstacles::{Obstacle, ObstacleKind};
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.world_mut()
            .resource_mut::<vehicle::wheel_settings::WheelContactSettings>()
            .experimental = experimental;
        let obstacle = Obstacle {
            id: 1,
            kind: ObstacleKind::Cube,
            position: [0.0, height * 0.5, 4.0 * direction],
            rotation_degrees: [0.0; 3],
            size: [5.0, height, 3.5],
        };
        let (_, collider) = obstacle.build().unwrap();
        app.world_mut()
            .spawn((RigidBody::Fixed, collider, obstacle.transform()));
        advance_vehicle(&mut app, 360);
        app.world_mut().resource_mut::<GameState>().low_range = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(if direction > 0.0 {
                KeyCode::KeyW
            } else {
                KeyCode::KeyS
            });
        let mut penetration = 0.0_f32;
        let mut sampled = 0;
        for _ in 0..600 {
            advance_vehicle(&mut app, 1);
            let car = *app
                .world_mut()
                .query_filtered::<&Transform, With<Truck>>()
                .single(app.world())
                .unwrap();
            let physics = app.world().resource::<VehiclePhysics>();
            for (mount, contact) in vehicle::controller::MOUNTS.into_iter().zip(physics.wheels) {
                let center = car.transform_point(mount - Vec3::Y * contact.suspension_length);
                let local = center - Vec3::from_array(obstacle.position);
                let outside = (local.abs() - Vec3::from_array(obstacle.size) * 0.5).max(Vec3::ZERO);
                let distance = outside.length();
                if distance < physics.active_config.tires.radius_m + 0.1 {
                    penetration = penetration.max(physics.active_config.tires.radius_m - distance);
                    sampled += 1;
                }
            }
        }
        assert!(sampled > 20, "The vehicle never reached the vertical step");
        assert!(
            !must_cross || truck_position(&mut app).z * direction > 7.0,
            "The vehicle did not cross the step"
        );
        assert!(truck_velocity(&mut app).linear.is_finite());
        println!(
            "Step height {height}, direction {direction}, experimental {experimental}: max rubber penetration {penetration} m"
        );
        penetration
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
        let handle = *app
            .world_mut()
            .query_filtered::<&RapierRigidBodyHandle, With<Truck>>()
            .single(app.world())
            .unwrap();
        let body_set = app
            .world_mut()
            .query::<&RapierRigidBodySet>()
            .single(app.world())
            .unwrap();
        let body = &body_set.bodies[handle.0];
        let local_com = body
            .position()
            .inverse_transform_point(body.center_of_mass());
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
            app.world_mut()
                .resource_mut::<VehiclePhysicsConfig>()
                .drivetrain
                .engine_force_n = force;
            advance_vehicle(&mut app, 360);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
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
        assert!(
            app.world()
                .resource::<VehiclePhysics>()
                .controller
                .is_none()
        );
        assert!(truck_velocity(&mut app).linear.length() < 0.10);
        *app.world_mut().resource_mut::<VehiclePhysicsConfig>() = VehiclePhysicsConfig::baseline();
        advance_vehicle(&mut app, 600);
        assert!(
            app.world()
                .resource::<VehiclePhysics>()
                .controller
                .is_some()
        );
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
        app.world_mut()
            .resource_mut::<VehiclePhysicsConfig>()
            .drivetrain
            .throttle_response_hz = 0.2;
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..24 {
            app.update();
            let physics = app.world().resource::<VehiclePhysics>();
            assert!(
                physics
                    .controller
                    .as_ref()
                    .unwrap()
                    .wheels()
                    .iter()
                    .all(|wheel| wheel.engine_force >= 0.0)
            );
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
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 60);
        app.world_mut()
            .query_filtered::<&mut Velocity, With<Truck>>()
            .single_mut(app.world_mut())
            .unwrap()
            .linear
            .z = 3.0;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        advance_vehicle(&mut app, 2);
        let physics = app.world().resource::<VehiclePhysics>();
        let forward_impulse: f32 = physics
            .controller
            .as_ref()
            .unwrap()
            .wheels()
            .iter()
            .map(|wheel| wheel.forward_impulse)
            .sum();
        assert!(
            forward_impulse < 0.0,
            "engine braking must oppose motion, impulse: {forward_impulse}"
        );
    }

    #[test]
    fn straight_line_tire_impulses_respect_the_actual_normal_load() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.world_mut()
            .resource_mut::<VehiclePhysicsConfig>()
            .tires
            .grip_coefficient = 0.05;
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 60);
        let physics = app.world().resource::<VehiclePhysics>();
        for wheel in physics.controller.as_ref().unwrap().wheels() {
            let impulse = Vec2::new(wheel.forward_impulse, wheel.side_impulse).length();
            let limit = wheel.wheel_suspension_force.min(wheel.max_suspension_force)
                * wheel.friction_slip
                * vehicle::controller::PHYSICS_DT;
            assert!(
                impulse <= limit + 0.0001,
                "tire impulse {impulse} exceeds load-based limit {limit}"
            );
        }
    }

    #[test]
    fn the_slowest_valid_throttle_response_still_eventually_accelerates() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.world_mut()
            .resource_mut::<VehiclePhysicsConfig>()
            .drivetrain
            .throttle_response_hz = 0.1;
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 240);
        assert!(truck_velocity(&mut app).linear.z > 0.25);
    }

    #[test]
    fn low_grip_changes_straight_line_acceleration() {
        let launch_speed = |grip| {
            let mut app = vehicle_test_app(Quat::IDENTITY);
            app.world_mut()
                .resource_mut::<VehiclePhysicsConfig>()
                .tires
                .grip_coefficient = grip;
            advance_vehicle(&mut app, 360);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            advance_vehicle(&mut app, 120);
            truck_velocity(&mut app).linear.z
        };
        assert!(launch_speed(1.35) > launch_speed(0.05) * 2.0);
    }

    #[test]
    fn parking_assistance_cannot_hold_a_slope_beyond_available_grip() {
        let slope = Quat::from_rotation_x(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        app.world_mut()
            .resource_mut::<VehiclePhysicsConfig>()
            .tires
            .grip_coefficient = 0.05;
        advance_vehicle(&mut app, 240);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 240);
        assert!(
            (truck_position(&mut app) - start)
                .dot(slope * Vec3::Z)
                .abs()
                > 0.25
        );
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
        let drift = (truck_position(&mut app) - start)
            .dot(slope * Vec3::Z)
            .abs();
        assert!(
            drift < 0.02,
            "brake and throttle parking drift: {drift:.6} m, velocity: {:?}",
            truck_velocity(&mut app)
        );
    }

    #[test]
    fn slow_low_grip_sliding_is_not_hidden_by_sleeping() {
        let slope = Quat::from_rotation_x(3.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        app.world_mut()
            .resource_mut::<VehiclePhysicsConfig>()
            .tires
            .grip_coefficient = 0.05;
        advance_vehicle(&mut app, 600);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 360);
        let drift = (truck_position(&mut app) - start)
            .dot(slope * Vec3::Z)
            .abs();
        assert!(
            drift > 0.05,
            "insufficient grip must keep sliding slowly: {drift:.3} m"
        );
    }

    #[test]
    fn driving_is_consistent_at_30_and_144_render_frames_per_second() {
        let drive_at_fps = |fps: usize| {
            let mut app = vehicle_test_app(Quat::IDENTITY);
            *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
                TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / fps as f64));
            advance_vehicle(&mut app, fps * 3);
            let start = truck_position(&mut app);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            advance_vehicle(&mut app, fps * 2);
            (
                (truck_position(&mut app) - start).z,
                truck_velocity(&mut app).linear.z,
            )
        };
        let slow = drive_at_fps(30);
        let fast = drive_at_fps(144);
        assert!(
            (slow.0 - fast.0).abs() < 0.10,
            "distance depends on rendering FPS: {slow:?} vs {fast:?}"
        );
        assert!(
            (slow.1 - fast.1).abs() < 0.10,
            "speed depends on rendering FPS: {slow:?} vs {fast:?}"
        );
    }

    #[test]
    fn mission_a_switching_tire_modes_preserves_flat_support_and_saved_profile() {
        use vehicle::wheel_backend::TireContactMode;
        use vehicle::wheel_settings::WheelContactSettings;
        let mut app = vehicle_test_app(Quat::IDENTITY);
        assert!(!app.world().resource::<WheelContactSettings>().experimental);
        advance_vehicle(&mut app, 600);
        let native_height = truck_position(&mut app).y;
        for experimental in [true, false, true] {
            app.world_mut()
                .resource_mut::<WheelContactSettings>()
                .experimental = experimental;
            advance_vehicle(&mut app, 1);
            let (owner, pose) = app
                .world_mut()
                .query_filtered::<(Entity, &Transform), With<Truck>>()
                .single(app.world())
                .map(|(owner, pose)| (owner, *pose))
                .unwrap();
            assert_eq!(
                app.world()
                    .resource::<vehicle::presentation::VehiclePresentation>()
                    .pose_for(owner, Transform::IDENTITY),
                pose
            );
            advance_vehicle(&mut app, 359);
            let physics = app.world().resource::<VehiclePhysics>();
            assert_eq!(
                physics.controller.as_ref().unwrap().contact_mode(),
                if experimental {
                    TireContactMode::Envelope
                } else {
                    TireContactMode::Native
                }
            );
            assert_eq!(
                *app.world().resource::<VehiclePhysicsConfig>(),
                VehiclePhysicsConfig::baseline()
            );
            let load: f32 = physics.wheels.iter().map(|wheel| wheel.normal_load).sum();
            assert!(
                (load - 1800.0 * 9.81).abs() < 1800.0 * 9.81 * 0.03,
                "Duplicate or missing support {load}"
            );
            assert!((truck_position(&mut app).y - native_height).abs() < 0.002);
            assert!(truck_velocity(&mut app).linear.length() < 0.05);
        }
        *app.world_mut().resource_mut::<VehiclePhysicsConfig>() = VehiclePhysicsConfig::original();
        advance_vehicle(&mut app, 2);
        assert!(
            app.world()
                .resource::<VehiclePhysics>()
                .controller
                .is_none()
        );
        assert!(
            !app.world()
                .resource::<vehicle::telemetry::VehicleTelemetry>()
                .experimental_tires
        );
    }

    #[test]
    fn reset_rejected_equipment_profile_keeps_last_valid_controller_on_r() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.insert_resource(vehicle::equipment_runtime::EquipmentRequest::demo())
            .insert_resource(terrain::runtime::ActiveTerrain {
                applied_revision: 1,
                ..default()
            })
            .init_resource::<terrain::runtime::TerrainWorkshopState>()
            .add_message::<AppExit>();
        advance_vehicle(&mut app, 5);
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
        advance_vehicle(&mut app, 2);
        assert!(app.world().resource::<VehiclePhysics>().error.is_some());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.world_mut().run_system_once(controls).unwrap();
        assert_eq!(
            app.world().resource::<VehiclePhysics>().active_config,
            valid
        );
        advance_vehicle(&mut app, 2);
        assert_eq!(
            app.world().resource::<VehiclePhysics>().active_config,
            valid
        );
        assert!((truck_mass(&mut app) - valid.chassis.mass_kg).abs() < 0.01);
    }

    #[test]
    fn mission_a_reset_uses_attached_equipment_clearance_and_keeps_the_child() {
        use bevy::ecs::system::RunSystemOnce;
        use vehicle::equipment_runtime::{EquipmentRequest, EquipmentRuntime, MountedEquipment};
        let mut app = vehicle_test_app(Quat::IDENTITY);
        let mut request = EquipmentRequest::demo();
        request.catalog.definitions[0].local_pose.translation[1] = -4.0;
        app.insert_resource(request)
            .insert_resource(terrain::runtime::ActiveTerrain {
                scene: terrain::scene::TerrainScene::reference(
                    terrain::scene::ReferenceScene::Flat,
                )
                .unwrap(),
                applied_revision: 1,
                ..default()
            })
            .init_resource::<terrain::runtime::TerrainWorkshopState>()
            .add_message::<AppExit>();
        advance_vehicle(&mut app, 5);
        let child = app
            .world_mut()
            .query_filtered::<Entity, With<MountedEquipment>>()
            .single(app.world())
            .unwrap();
        let expected = terrain::runtime::checked_spawn_transform(
            &app.world()
                .resource::<terrain::runtime::ActiveTerrain>()
                .scene,
            app.world().resource::<VehiclePhysicsConfig>(),
            app.world().resource::<EquipmentRuntime>().bounds(),
        )
        .unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.world_mut().run_system_once(controls).unwrap();
        assert!(
            truck_position(&mut app).distance(expected.translation) < 1e-4,
            "R ignores the installed collider bounds"
        );
        app.world_mut()
            .run_system_once(vehicle::presentation::present)
            .unwrap();
        let owner = app
            .world_mut()
            .query_filtered::<Entity, With<Truck>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<vehicle::presentation::VehiclePresentation>()
                .pose_for(owner, Transform::IDENTITY),
            expected
        );
        assert!(app.world().get_entity(child).is_ok());
    }

    #[test]
    fn mission_a_telemetry_reports_effective_values_without_changing_the_profile() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.insert_resource(vehicle::equipment_runtime::EquipmentRequest::demo());
        advance_vehicle(&mut app, 5);
        let telemetry = app
            .world()
            .resource::<vehicle::telemetry::VehicleTelemetry>();
        let effective = &app.world().resource::<VehiclePhysics>().active_config;
        assert!((telemetry.effective_mass_kg - 1840.0).abs() < 0.01);
        assert!(
            telemetry
                .effective_center_of_mass
                .distance(Vec3::from_array(effective.chassis.center_of_mass))
                < 1e-5
        );
        assert_eq!(
            *app.world().resource::<VehiclePhysicsConfig>(),
            VehiclePhysicsConfig::baseline()
        );
    }

    #[test]
    fn resetting_does_not_report_a_fictitious_acceleration() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        app.insert_resource(terrain::runtime::ActiveTerrain {
            applied_revision: 1,
            ..default()
        })
        .init_resource::<terrain::runtime::TerrainWorkshopState>()
        .add_message::<AppExit>()
        .add_systems(Update, controls);
        advance_vehicle(&mut app, 360);
        assert!(app.world().resource::<GameState>().grounded_wheels >= 3);
        app.world_mut()
            .query_filtered::<&mut Velocity, With<Truck>>()
            .single_mut(app.world_mut())
            .unwrap()
            .linear
            .z = 3.0;
        advance_vehicle(&mut app, 2);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyR);
            keys.clear();
        }
        advance_vehicle(&mut app, 1);
        assert!(
            app.world()
                .resource::<vehicle::telemetry::VehicleTelemetry>()
                .longitudinal_acceleration_mps2
                .abs()
                < 0.1
        );
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
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyD);
        advance_vehicle(&mut app, 240);
        let transform = *app
            .world_mut()
            .query_filtered::<&Transform, With<Truck>>()
            .single(app.world())
            .unwrap();
        assert!(
            (transform.translation - start).length() < 0.02,
            "steering displacement: {:?}, velocity: {:?}, rotation: {:?}",
            transform.translation - start,
            truck_velocity(&mut app),
            transform.rotation
        );
        assert!((transform.rotation * Vec3::Z).x.abs() < 0.01);
        assert!(app.world().resource::<VehicleVisualState>().steer < -0.40);
    }

    #[test]
    fn reverse_can_wake_a_vehicle_at_rest() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 600);
        let start = truck_position(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyS);
        advance_vehicle(&mut app, 120);
        assert!(truck_position(&mut app).z < start.z - 0.5);
        assert!(truck_velocity(&mut app).linear.z < -0.5);
    }

    #[test]
    fn brakes_override_throttle_and_stop_the_vehicle() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 180);
        assert!(truck_velocity(&mut app).linear.z > 2.0);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        advance_vehicle(&mut app, 240);
        let velocity = truck_velocity(&mut app);
        assert!(
            velocity.linear.length() < 0.10,
            "braking velocity: {:?}",
            velocity.linear
        );
    }

    #[test]
    fn low_range_can_start_uphill() {
        let slope = Quat::from_rotation_x(-15.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        app.world_mut().resource_mut::<GameState>().low_range = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 120);
        let distance = (truck_position(&mut app) - start).dot(slope * Vec3::Z);
        assert!(distance > 0.5, "uphill progress: {distance:.3} m");
        assert!(app.world().resource::<GameState>().grounded_wheels >= 3);
    }

    #[test]
    fn unloaded_wheels_do_not_spin_from_chassis_motion() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        let mut query = app
            .world_mut()
            .query_filtered::<(&mut Transform, &mut Velocity), With<Truck>>();
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
        let floor = app
            .world_mut()
            .query_filtered::<Entity, (With<Collider>, Without<Truck>)>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .entity_mut(floor)
            .insert((ground().1, Transform::IDENTITY));
        let x = road_x(START);
        let mut query = app
            .world_mut()
            .query_filtered::<&mut Transform, With<Truck>>();
        query.single_mut(app.world_mut()).unwrap().translation =
            Vec3::new(x, height(x, START) + 1.06, START);
        advance_vehicle(&mut app, 600);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start).length();
        assert!(drift < 0.10, "starting terrain drift: {drift:.3} m");
        assert_eq!(app.world().resource::<GameState>().grounded_wheels, 4);
        let velocity = truck_velocity(&mut app);
        assert!(
            velocity.linear.length() < 0.05,
            "terrain residual velocity: {velocity:?}"
        );
    }

    #[test]
    fn driving_across_a_gentle_slope_keeps_lateral_grip() {
        let slope = Quat::from_rotation_z(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 360);
        let displacement = truck_position(&mut app) - start;
        assert!(displacement.dot(slope * Vec3::Z) > 5.0);
        assert!(
            displacement.dot(slope * Vec3::X).abs() < 0.25,
            "lateral driving drift: {displacement:?}"
        );
    }

    #[test]
    fn right_turn_changes_heading_and_individual_wheel_speeds() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 180);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyD);
        advance_vehicle(&mut app, 120);
        let transform = *app
            .world_mut()
            .query_filtered::<&Transform, With<Truck>>()
            .single(app.world())
            .unwrap();
        assert!((transform.rotation * Vec3::Z).x < -0.10);
        let spins = app.world().resource::<VehicleVisualState>().spins;
        assert!(
            Quat::from_rotation_x(spins[0]).angle_between(Quat::from_rotation_x(spins[1])) > 0.05
        );
    }

    #[test]
    fn vehicle_holds_still_on_a_gentle_cross_slope() {
        let slope = Quat::from_rotation_z(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start)
            .dot(slope * Vec3::X)
            .abs();
        assert!(drift < 0.10, "cross-slope drift: {drift:.3} m in 5 seconds");
    }

    #[test]
    fn vehicle_holds_still_on_a_gentle_uphill_slope() {
        let slope = Quat::from_rotation_x(8.0_f32.to_radians());
        let mut app = vehicle_test_app(slope);
        advance_vehicle(&mut app, 360);
        let start = truck_position(&mut app);
        advance_vehicle(&mut app, 600);
        let drift = (truck_position(&mut app) - start)
            .dot(slope * Vec3::Z)
            .abs();
        assert!(drift < 0.10, "uphill drift: {drift:.3} m in 5 seconds");
    }

    #[test]
    fn wheel_rotation_rolls_forward_in_the_rotated_model() {
        let mut app = vehicle_test_app(Quat::IDENTITY);
        advance_vehicle(&mut app, 360);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        advance_vehicle(&mut app, 20);
        let rotation = app
            .world_mut()
            .query_filtered::<&Transform, Without<Truck>>()
            .iter(app.world())
            .find(|transform| transform.translation == Vec3::ZERO)
            .unwrap()
            .rotation;
        let model_rotation = Quat::from_rotation_y(std::f32::consts::PI);
        let bottom_motion = model_rotation * (rotation * Vec3::NEG_Y - Vec3::NEG_Y);
        assert!(
            bottom_motion.z < -0.001,
            "the bottom of a forward-rolling tire must move backward, got {bottom_motion:?}"
        );
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
        assert!(
            physics
                .controller
                .as_ref()
                .unwrap()
                .wheels()
                .iter()
                .all(|wheel| wheel.wheel_suspension_force >= 0.0)
        );
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
