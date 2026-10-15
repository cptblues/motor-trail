//! Native GPU/GLB probe: --wheel-contact-probe <output-directory>.
use super::WheelDiagnostics;
use crate::terrain::config::TerrainPreset;
use crate::terrain::obstacles::{Obstacle, ObstacleKind};
use crate::terrain::runtime::{
    ActiveTerrain, ObstacleEntity, TerrainWorkshopState, WorkshopAction, WorkshopMode,
};
use crate::terrain::scene::ReferenceScene;
use crate::ui::UiInputCapture;
use crate::vehicle::config::VehiclePhysicsConfig;
use crate::vehicle::controller::{MOUNTS, Truck, VehiclePhysics};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy_rapier3d::prelude::*;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const MIN: Vec3 = Vec3::new(-2.5, 0.0, -17.25);
const MAX: Vec3 = Vec3::new(2.5, 0.25, -13.75);
const IMAGES: [&str; 4] = [
    "01-before.png",
    "02-encounter.png",
    "03-on-step.png",
    "04-crossed.png",
];
const WHEELS: [&str; 4] = [
    "WheelSpin_FL",
    "WheelSpin_FR",
    "WheelSpin_RL",
    "WheelSpin_RR",
];

pub struct WheelContactProbePlugin;
impl Plugin for WheelContactProbePlugin {
    fn build(&self, app: &mut App) {
        let mut args = std::env::args_os();
        if args.find(|arg| arg == "--wheel-contact-probe").is_none() {
            return;
        }
        let setup = (|| -> Result<Probe, String> {
            let output = args
                .next()
                .filter(|arg| !arg.to_string_lossy().starts_with("--"))
                .ok_or("--wheel-contact-probe requires an output directory")?;
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            // A fresh subdirectory prevents previous screenshots from satisfying this run.
            let output = PathBuf::from(output).join(format!("wheel-contact-{nonce}"));
            std::fs::create_dir_all(&output).map_err(|e| format!("Probe output: {e}"))?;
            info!("Wheel contact probe images: {}", output.display());
            Ok(Probe {
                output,
                stage: Stage::Load,
                started: Instant::now(),
                elapsed: 0.0,
                stable: 0.0,
                step: None,
                start: Vec3::ZERO,
                max_penetration: 0.0,
                top_wheels: 0,
                captured: [false; 4],
            })
        })();
        match setup {
            Ok(probe) => {
                app.insert_resource(probe).add_systems(
                    Update,
                    tick.after(crate::camera_follow)
                        .after(crate::vehicle::visuals::animate_wheels),
                );
            }
            Err(error) => {
                app.add_systems(Startup, move |mut exit: MessageWriter<AppExit>| {
                    error!("Wheel contact probe: {error}");
                    exit.write(AppExit::error());
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Load,
    Settle,
    Ready,
    Drive,
    Save,
}

#[derive(Resource)]
struct Probe {
    output: PathBuf,
    stage: Stage,
    started: Instant,
    elapsed: f32,
    stable: f32,
    step: Option<Entity>,
    start: Vec3,
    max_penetration: f32,
    top_wheels: u8,
    captured: [bool; 4],
}

fn penetration(center: Vec3, radius: f32) -> f32 {
    (radius - center.distance(center.clamp(MIN, MAX))).max(0.0)
}

fn capture(world: &mut World, probe: &mut Probe, index: usize) {
    if !probe.captured[index] {
        world
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(probe.output.join(IMAGES[index])));
        probe.captured[index] = true;
        info!("Wheel contact probe capture: {}", IMAGES[index]);
    }
}

fn image_exists(probe: &Probe, index: usize) -> bool {
    std::fs::metadata(probe.output.join(IMAGES[index])).is_ok_and(|m| m.is_file() && m.len() > 8)
}

fn spawn_step(world: &mut World) -> Result<Entity, String> {
    let object = Obstacle {
        id: u64::MAX,
        kind: ObstacleKind::Cube,
        position: [0.0, 0.125, -15.5],
        rotation_degrees: [0.0; 3],
        size: [5.0, 0.25, 3.5],
    };
    let (mesh, collider) = object.build()?;
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.63, 0.17),
            perceptual_roughness: 0.8,
            ..default()
        });
    let transform = object.transform();
    // Deliberately outside TerrainRoot and ActiveTerrain.scene: never a saved object.
    Ok(world
        .spawn((
            Name::new("Wheel contact probe step"),
            ObstacleEntity(object.id),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            collider,
            RigidBody::Fixed,
            Friction::coefficient(1.15),
            transform,
            GlobalTransform::from(transform),
        ))
        .id())
}

fn lock_camera(world: &mut World, car: &Transform) {
    let pose = {
        let mut orbit = world.resource_mut::<crate::OrbitCamera>();
        orbit.yaw_offset = 1.1;
        orbit.distance = 6.0;
        orbit.pitch = 0.25;
        orbit.focus = car.translation + Vec3::Y * 0.2;
        let forward = car.rotation * Vec3::Z;
        let (s, c) = (forward.x.atan2(forward.z) + orbit.yaw_offset).sin_cos();
        let horizontal = orbit.distance * orbit.pitch.cos();
        Transform::from_translation(
            orbit.focus
                + Vec3::new(
                    -s * horizontal,
                    orbit.distance * orbit.pitch.sin(),
                    -c * horizontal,
                ),
        )
        .looking_at(orbit.focus, Vec3::Y)
    };
    if let Ok(mut camera) = world
        .query_filtered::<&mut Transform, With<crate::ChaseCamera>>()
        .single_mut(world)
    {
        *camera = pose;
    }
}

fn tick(world: &mut World) {
    let Some(mut probe) = world.remove_resource::<Probe>() else {
        return;
    };
    match advance(world, &mut probe) {
        Ok(false) => {
            world.insert_resource(probe);
        }
        result => {
            world
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::KeyW);
            info!(
                "Wheel contact probe: native raycast (diagnosis only), max rubber penetration={:.5} m, top wheels={:04b}, stage={:?}",
                probe.max_penetration, probe.top_wheels, probe.stage
            );
            let exit = match result {
                Ok(true) => AppExit::Success,
                Err(error) => {
                    error!("Wheel contact probe failed: {error}");
                    AppExit::error()
                }
                Ok(false) => unreachable!(),
            };
            world.write_message(exit);
        }
    }
}

fn advance(world: &mut World, probe: &mut Probe) -> Result<bool, String> {
    if probe.started.elapsed().as_secs_f32() > 90.0 {
        return Err(format!(
            "90 s deadline in {:?}; captures {:?}",
            probe.stage, probe.captured
        ));
    }
    if let Some(error) = &world.resource::<TerrainWorkshopState>().last_error {
        return Err(format!("Workshop: {error}"));
    }
    if let Some(error) = &world.resource::<VehiclePhysics>().error {
        return Err(format!("Vehicle: {error}"));
    }
    let dt = world.resource::<Time>().delta_secs();
    probe.elapsed += dt;
    if probe.stage == Stage::Load {
        let mut config = VehiclePhysicsConfig::baseline();
        config.drivetrain.low_max_speed_mps = 3.0;
        config.drivetrain.normal_max_speed_mps = 3.0;
        config.validate()?;
        world.insert_resource(config);
        world.resource_mut::<crate::GameState>().low_range = true;
        world.resource_mut::<WheelDiagnostics>().enabled = true;
        world
            .resource_mut::<crate::ui::physics_tuning::PhysicsTuningState>()
            .open = false;
        let mut workshop = world.resource_mut::<TerrainWorkshopState>();
        workshop.open = false;
        workshop.actions.extend([
            WorkshopAction::LoadReference(ReferenceScene::Flat),
            WorkshopAction::SetMode(WorkshopMode::Drive),
        ]);
        probe.stage = Stage::Settle;
        return Ok(false);
    }
    let Ok((car, speed)) = world
        .query_filtered::<(&Transform, &Velocity), With<Truck>>()
        .single(world)
        .map(|(car, speed)| (*car, *speed))
    else {
        return Ok(false);
    };
    let physics = world.resource::<VehiclePhysics>();
    let wheels = physics.wheels;
    let radius = physics.active_config.tires.radius_m;
    let centers = std::array::from_fn::<_, 4, _>(|i| {
        car.transform_point(MOUNTS[i] - Vec3::Y * wheels[i].suspension_length)
    });
    if !car.translation.is_finite()
        || !car.rotation.is_finite()
        || !car.scale.is_finite()
        || !speed.linear.is_finite()
        || !speed.angular.is_finite()
        || !radius.is_finite()
        || !Vec3::new(physics.throttle, physics.engine_force, physics.brake_force).is_finite()
        || wheels.iter().any(|w| {
            !w.suspension_length.is_finite()
                || !w.normal_load.is_finite()
                || !w.point.is_finite()
                || !w.normal.is_finite()
                || !w.ground_velocity.is_finite()
                || !w.steer.is_finite()
        })
        || centers.iter().any(|c| !c.is_finite())
    {
        return Err("Non-finite vehicle or wheel state".into());
    }
    let active = world.resource::<ActiveTerrain>();
    let synced = active.revision == active.applied_revision
        && active.collider_count > 0
        && active.scene.terrain.config.preset == TerrainPreset::Flat
        && active.scene.name == ReferenceScene::Flat.name()
        && active.scene.obstacles.is_empty();
    if world.resource::<TerrainWorkshopState>().mode != WorkshopMode::Drive {
        return Err("Workshop left Drive mode".into());
    }
    let configured = physics.active_config == *world.resource::<VehiclePhysicsConfig>();
    lock_camera(world, &car);
    if probe.step.is_some() {
        for (i, center) in centers.iter().enumerate() {
            probe.max_penetration = probe.max_penetration.max(penetration(*center, radius));
            let point = wheels[i].point;
            if wheels[i].grounded
                && point.x.abs() < MAX.x
                && (MIN.z..=MAX.z).contains(&point.z)
                && (point.y - MAX.y).abs() < 0.04
            {
                probe.top_wheels |= 1 << i;
            }
        }
    }
    match probe.stage {
        Stage::Settle => {
            let mut roots = world.query::<&WorldAssetRoot>();
            let server = world.resource::<AssetServer>();
            let loaded = roots
                .iter(world)
                .any(|root| server.is_loaded_with_dependencies(root.0.id()));
            let mut names = world.query::<&Name>();
            let glb_ready = loaded
                && WHEELS
                    .iter()
                    .all(|label| names.iter(world).any(|n| n.as_str() == *label));
            let settled = synced
                && configured
                && glb_ready
                && wheels.iter().filter(|w| w.grounded).count() >= 3
                && speed.linear.length() < 0.15
                && speed.angular.length() < 0.15;
            probe.stable = if settled { probe.stable + dt } else { 0.0 };
            if probe.stable >= 0.5 {
                probe.step = Some(spawn_step(world)?);
                probe.stage = Stage::Ready;
                probe.elapsed = 0.0;
            }
        }
        Stage::Ready => {
            if probe.elapsed < 0.5
                || world
                    .get::<RapierColliderHandle>(probe.step.unwrap())
                    .is_none()
            {
                return Ok(false);
            }
            capture(world, probe, 0);
            if image_exists(probe, 0) && !world.resource::<UiInputCapture>().keyboard {
                probe.start = car.translation;
                probe.elapsed = 0.0;
                probe.stage = Stage::Drive;
                world
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::KeyW);
            }
        }
        Stage::Drive => {
            if !synced
                || !configured
                || !world.resource::<crate::GameState>().low_range
                || world.resource::<UiInputCapture>().keyboard
            {
                return Err(
                    "Scene/configuration/range changed or UI captured driving input".into(),
                );
            }
            world
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            if centers[..2].iter().any(|c| c.z >= MIN.z - 0.20) {
                capture(world, probe, 1);
            }
            if probe.top_wheels & 0b0011 == 0b0011 {
                capture(world, probe, 2);
            }
            if centers.iter().all(|c| c.z - radius > MAX.z) {
                capture(world, probe, 3);
            }
            if probe.elapsed >= 4.8 {
                world
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .release(KeyCode::KeyW);
                let progress = car.translation.z - probe.start.z;
                info!(
                    "Wheel contact probe drove {:.3} m in {:.3} s",
                    progress, probe.elapsed
                );
                if progress < 8.0
                    || probe.top_wheels != 0b1111
                    || !probe.captured.iter().all(|v| *v)
                {
                    return Err("Did not contact the step with all four wheels and cross it during the drive".into());
                }
                probe.stage = Stage::Save;
            }
        }
        Stage::Save => {
            if !(0..IMAGES.len()).all(|i| image_exists(probe, i)) {
                return Ok(false);
            }
            info!("Wheel contact probe complete: {}", probe.output.display());
            return Ok(true);
        }
        Stage::Load => unreachable!(),
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn penetration_measures_the_corner_instead_of_a_downward_ray() {
        let center = Vec3::new(0.0, 0.55, MIN.z - 0.4);
        assert!((penetration(center, 0.53) - 0.03).abs() < 1e-5);
        assert_eq!(penetration(Vec3::new(0.0, 0.78, -15.5), 0.53), 0.0);
        assert_eq!(penetration(Vec3::new(4.0, 0.5, -15.5), 0.53), 0.0);
    }
}
