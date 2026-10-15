//! Deterministic native renderer/GLB check: --vehicle-body-probe <directory>.
//! Runs only on explicit request and writes artifacts solely below that directory.
use super::controller::{Truck, VehiclePhysics};
use super::diagnostics::WheelDiagnostics;
use super::equipment_runtime::{EquipmentRequest, EquipmentRuntime, MountedEquipment};
use super::wheel_backend::TireContactMode;
use super::wheel_settings::WheelContactSettings;
use crate::terrain::config::TerrainPreset;
use crate::terrain::runtime::{ActiveTerrain, TerrainWorkshopState, WorkshopAction, WorkshopMode};
use crate::terrain::scene::ReferenceScene;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::*;
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const IMAGES: [&str; 4] = [
    "01-upright.png",
    "02-roof.png",
    "03-left-flank.png",
    "04-right-flank.png",
];
const ROLLS: [f32; 4] = [
    0.0,
    std::f32::consts::PI,
    std::f32::consts::FRAC_PI_2,
    -std::f32::consts::FRAC_PI_2,
];
const DEMO_LOCAL: Vec3 = Vec3::new(0.0, 1.74, -0.42);

pub struct VehicleBodyProbePlugin;

impl Plugin for VehicleBodyProbePlugin {
    fn build(&self, app: &mut App) {
        let mut args = std::env::args_os();
        if args.find(|arg| arg == "--vehicle-body-probe").is_none() {
            return;
        }
        let setup = (|| -> Result<BodyProbe, String> {
            if std::env::args_os().any(|arg| arg == "--wheel-contact-probe") {
                return Err("Run wheel and body probes in separate processes".into());
            }
            let output = args
                .next()
                .filter(|arg| !arg.to_string_lossy().starts_with("--"))
                .ok_or("--vehicle-body-probe requires an output directory")?;
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let output = PathBuf::from(output).join(format!("vehicle-body-{nonce}"));
            std::fs::create_dir_all(&output).map_err(|e| format!("Body probe output: {e}"))?;
            info!("Vehicle body probe images: {}", output.display());
            Ok(BodyProbe {
                output,
                started: Instant::now(),
                stage: Stage::Load,
                case: 0,
                elapsed: 0.0,
                stable: 0.0,
                car: None,
                equipment: None,
                captured: [false; 4],
                records: Vec::new(),
                mode: app.world().resource::<WheelContactSettings>().mode(),
            })
        })();
        match setup {
            Ok(probe) => {
                app.insert_resource(probe)
                    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                        1.0 / 60.0,
                    )))
                    .add_systems(
                        PostUpdate,
                        tick.after(bevy::transform::TransformSystems::Propagate),
                    );
            }
            Err(error) => {
                app.add_systems(Startup, move |mut exit: MessageWriter<AppExit>| {
                    error!("Vehicle body probe: {error}");
                    exit.write(AppExit::error());
                });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Load,
    Prepare,
    Drop,
    Settle,
    Capture,
}

#[derive(Resource)]
struct BodyProbe {
    output: PathBuf,
    started: Instant,
    stage: Stage,
    case: usize,
    elapsed: f32,
    stable: f32,
    car: Option<Entity>,
    equipment: Option<Entity>,
    captured: [bool; 4],
    records: Vec<Value>,
    mode: TireContactMode,
}

fn tick(world: &mut World) {
    let Some(mut probe) = world.remove_resource::<BodyProbe>() else {
        return;
    };
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    let result = advance(world, &mut probe);
    if matches!(result, Ok(false)) {
        world.insert_resource(probe);
        return;
    }
    let report = json!({
        "success": matches!(&result, Ok(true)), "error": result.as_ref().err(),
        "backend": format!("{:?}", probe.mode), "cases": probe.records,
        "captured": probe.captured,
    });
    let saved = serde_json::to_vec_pretty(&report)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            std::fs::write(probe.output.join("report.json"), bytes).map_err(|e| e.to_string())
        });
    match result.and_then(|done| saved.map(|()| done)) {
        Ok(true) => {
            info!("Vehicle body probe complete: {}", probe.output.display());
            world.write_message(AppExit::Success);
        }
        Err(error) => {
            error!(
                "Vehicle body probe failed in {:?}, case {}: {error}",
                probe.stage, probe.case
            );
            world.write_message(AppExit::error());
        }
        Ok(false) => unreachable!(),
    }
}

fn belongs_to(world: &World, mut entity: Entity, root: Entity) -> bool {
    for _ in 0..64 {
        if entity == root {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
    false
}

fn glb_root(world: &mut World, car: Entity) -> Option<Entity> {
    let mut roots = world.query::<(Entity, &WorldAssetRoot)>();
    let server = world.resource::<AssetServer>();
    roots.iter(world).find_map(|(entity, root)| {
        (belongs_to(world, entity, car) && server.is_loaded_with_dependencies(root.0.id()))
            .then_some(entity)
    })
}

fn glb_ready(world: &mut World, car: Entity) -> bool {
    let Some(root) = glb_root(world, car) else {
        return false;
    };
    let mut names = world.query::<(Entity, &Name)>();
    [
        "WheelSpin_FL",
        "WheelSpin_FR",
        "WheelSpin_RL",
        "WheelSpin_RR",
    ]
    .iter()
    .all(|name| {
        names
            .iter(world)
            .any(|(entity, label)| label.as_str() == *name && belongs_to(world, entity, root))
    })
}

fn suspended_wheel(world: &World, mut entity: Entity) -> bool {
    for _ in 0..64 {
        if world.get::<Name>(entity).is_some_and(|name| {
            matches!(
                name.as_str(),
                "WheelSpin_FL" | "WheelSpin_FR" | "WheelSpin_RL" | "WheelSpin_RR"
            )
        }) {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
    false
}

struct RenderedClearance {
    body: f32,
    wheels: f32,
}

fn rendered_lowest(world: &mut World, car: Entity) -> Result<RenderedClearance, String> {
    let root = glb_root(world, car).ok_or("Truck GLB scene is not loaded")?;
    let mut query = world.query::<(
        Entity,
        &Mesh3d,
        &GlobalTransform,
        Option<&InheritedVisibility>,
    )>();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut minimum = f32::INFINITY;
    let mut body_minimum = f32::INFINITY;
    let mut wheel_minimum = f32::INFINITY;
    let mut lowest_entity = None;
    let mut count = 0;
    for (entity, mesh, pose, visible) in query.iter(world) {
        if !belongs_to(world, entity, root) || visible.is_some_and(|v| !v.get()) {
            continue;
        }
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x3(vertices)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let wheel = suspended_wheel(world, entity);
        for vertex in vertices {
            let point = pose.transform_point(Vec3::from_array(*vertex));
            if !point.is_finite() {
                return Err("Non-finite rendered GLB vertex".into());
            }
            if point.y < minimum {
                minimum = point.y;
                lowest_entity = Some(entity);
            }
            if wheel {
                wheel_minimum = wheel_minimum.min(point.y);
            } else {
                body_minimum = body_minimum.min(point.y);
            }
            count += 1;
        }
    }
    if count == 0 || !body_minimum.is_finite() || !wheel_minimum.is_finite() {
        return Err("No CPU-readable rendered truck GLB vertices".into());
    }
    let mut labels = Vec::new();
    let mut entity = lowest_entity.unwrap();
    for _ in 0..64 {
        if let Some(name) = world.get::<Name>(entity) {
            labels.push(name.as_str());
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            break;
        };
        entity = parent.parent();
    }
    info!("Rendered lowest GLB vertex: {minimum:.5} m, ancestry {labels:?}");
    Ok(RenderedClearance {
        body: body_minimum,
        wheels: wheel_minimum,
    })
}

fn attachment(world: &mut World, car: Entity) -> Result<(Entity, f32, f32, f32), String> {
    let mut equipment = world.query::<(
        Entity,
        &MountedEquipment,
        &ChildOf,
        &Transform,
        &GlobalTransform,
        &Collider,
    )>();
    let items: Vec<_> = equipment.iter(world).collect();
    if items.len() != 1 {
        return Err(format!(
            "Expected one dummy attachment, got {}",
            items.len()
        ));
    }
    let (entity, item, parent, local, global, collider) = items[0];
    if item.instance_id != "demo_roof_1"
        || parent.parent() != car
        || world.get::<RigidBody>(entity).is_some()
    {
        return Err("Demo must remain a rigid child of the one truck body".into());
    }
    if local.translation.distance(DEMO_LOCAL) > 1e-5
        || local.rotation.angle_between(Quat::IDENTITY).abs() > 1e-5
        || local.scale != Vec3::ONE
    {
        return Err("Demo local mounting pose changed".into());
    }
    if world.get::<ColliderMassProperties>(entity) != Some(&ColliderMassProperties::Density(0.0)) {
        return Err("Demo collider must keep zero density".into());
    }
    let expected = world
        .get::<GlobalTransform>(car)
        .ok_or("Missing truck global pose")?
        .mul_transform(*local);
    let follow_error = [Vec3::ZERO, Vec3::X * 0.3, Vec3::Y * 0.2, Vec3::Z * 0.4]
        .into_iter()
        .map(|point| {
            expected
                .transform_point(point)
                .distance(global.transform_point(point))
        })
        .fold(0.0_f32, f32::max);
    if !follow_error.is_finite() || follow_error > 0.002 {
        return Err(format!(
            "Attachment failed to follow chassis: {follow_error} m"
        ));
    }
    let pose = global.compute_transform();
    let floor = collider
        .raw
        .compute_aabb(&bevy_rapier3d::rapier::math::Pose::from_parts(
            pose.translation,
            pose.rotation,
        ))
        .mins
        .y;
    let mut bodies = world.query::<&RapierRigidBodySet>();
    let mut colliders = world.query::<&RapierContextColliders>();
    let bodies = bodies.single(world).map_err(|e| e.to_string())?;
    let colliders = colliders.single(world).map_err(|e| e.to_string())?;
    let handle = world
        .get::<RapierRigidBodyHandle>(car)
        .ok_or("Missing truck rigid-body handle")?
        .0;
    let collider = world
        .get::<RapierColliderHandle>(entity)
        .ok_or("Missing demo collider handle")?
        .0;
    if colliders.colliders[collider].parent() != Some(handle) {
        return Err("Demo collider does not share the chassis Rapier body".into());
    }
    if bodies
        .bodies
        .iter()
        .filter(|(_, body)| body.is_dynamic())
        .count()
        != 1
    {
        return Err("The probe must have exactly one dynamic rigid body".into());
    }
    let mass = bodies.bodies[handle].mass();
    if (mass - 1840.0).abs() > 0.05 {
        return Err(format!("Loaded chassis mass {mass} kg, expected 1840 kg"));
    }
    Ok((entity, floor, mass, follow_error))
}

fn png_exists(path: &Path) -> bool {
    let mut header = [0_u8; 24];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header[..8] == b"\x89PNG\r\n\x1a\n"
        && &header[12..16] == b"IHDR"
        && u32::from_be_bytes(header[16..20].try_into().unwrap()) >= 64
        && u32::from_be_bytes(header[20..24].try_into().unwrap()) >= 64
}

fn orientation_valid(case: usize, rotation: Quat) -> bool {
    match case {
        0 => (rotation * Vec3::Y).y > 0.8,
        1 => (rotation * Vec3::Y).y < -0.5,
        2 => (rotation * Vec3::X).y > 0.5 && (rotation * Vec3::Y).y < 0.65,
        3 => (rotation * Vec3::X).y < -0.5 && (rotation * Vec3::Y).y < 0.65,
        _ => false,
    }
}

fn body_floor_contact(world: &mut World, car: Entity) -> Result<bool, String> {
    let mut contexts = world.query::<(
        &RapierContextSimulation,
        &RapierContextColliders,
        &RapierRigidBodySet,
    )>();
    let (simulation, colliders, bodies) = contexts.single(world).map_err(|e| e.to_string())?;
    // The synchronized Flat reference has no obstacles, and same-body equipment
    // cannot collide with the chassis. An active root-collider pair is floor support.
    Ok(simulation
        .contact_pairs_with(colliders, bodies, car)
        .any(|pair| pair.has_any_active_contact()))
}

fn capture_ready(
    case: usize,
    rotation: Quat,
    supported: bool,
    body_contact: bool,
    stable: f32,
) -> bool {
    supported
        && orientation_valid(case, rotation)
        && if case >= 2 {
            body_contact
        } else {
            stable >= 0.5
        }
}

fn capture(world: &mut World, probe: &mut BodyProbe) {
    if !probe.captured[probe.case] {
        world
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(probe.output.join(IMAGES[probe.case])));
        probe.captured[probe.case] = true;
        info!("Vehicle body probe capture: {}", IMAGES[probe.case]);
    }
}

fn lock_camera(world: &mut World, car: &Transform) {
    let focus = car.translation + Vec3::Y * 0.25;
    let pose =
        Transform::from_translation(focus + Vec3::new(6.0, 2.8, 6.8)).looking_at(focus, Vec3::Y);
    if let Ok((mut transform, mut global)) = world
        .query_filtered::<(&mut Transform, &mut GlobalTransform), With<crate::ChaseCamera>>()
        .single_mut(world)
    {
        *transform = pose;
        *global = GlobalTransform::from(pose);
    }
    let mut orbit = world.resource_mut::<crate::OrbitCamera>();
    orbit.focus = focus;
    orbit.distance = 9.5;
    orbit.pitch = 0.3;
}

fn advance(world: &mut World, probe: &mut BodyProbe) -> Result<bool, String> {
    if probe.started.elapsed().as_secs_f32() > 180.0 {
        return Err("180 s wall-clock deadline".into());
    }
    if world.resource::<WheelContactSettings>().mode() != probe.mode {
        return Err("Tire contact mode changed during body probe".into());
    }
    if let Some(error) = &world.resource::<EquipmentRuntime>().error {
        return Err(format!("Equipment: {error}"));
    }
    if let Some(error) = &world.resource::<VehiclePhysics>().error {
        return Err(format!("Vehicle: {error}"));
    }
    if let Some(error) = &world.resource::<TerrainWorkshopState>().last_error {
        return Err(format!("Workshop: {error}"));
    }
    if probe.stage == Stage::Load {
        world.insert_resource(super::config::VehiclePhysicsConfig::baseline());
        world.insert_resource(EquipmentRequest::demo());
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
        probe.stage = Stage::Prepare;
        return Ok(false);
    }
    let active = world.resource::<ActiveTerrain>();
    let synced = active.revision == active.applied_revision
        && active.collider_count > 0
        && active.scene.terrain.config.preset == TerrainPreset::Flat
        && active.scene.name == ReferenceScene::Flat.name()
        && active.scene.obstacles.is_empty();
    if !synced {
        if probe.stage == Stage::Prepare {
            return Ok(false);
        }
        return Err("Probe flat terrain changed".into());
    }
    if *world.resource::<super::config::VehiclePhysicsConfig>()
        != super::config::VehiclePhysicsConfig::baseline()
        || *world.resource::<EquipmentRequest>() != EquipmentRequest::demo()
        || world.resource::<TerrainWorkshopState>().mode != WorkshopMode::Drive
    {
        return Err("Probe configuration, loadout or Drive mode changed".into());
    }
    let Ok((car_entity, car, velocity)) = world
        .query_filtered::<(Entity, &Transform, &Velocity), With<Truck>>()
        .single(world)
        .map(|(entity, car, velocity)| (entity, *car, *velocity))
    else {
        return if probe.stage == Stage::Prepare {
            Ok(false)
        } else {
            Err("The single probe truck disappeared".into())
        };
    };
    if probe.stage == Stage::Prepare {
        if !glb_ready(world, car_entity)
            || world.resource::<EquipmentRuntime>().active() != &EquipmentRequest::demo()
            || world
                .get::<ReadMassProperties>(car_entity)
                .is_none_or(|mass| (mass.mass - 1840.0).abs() > 0.05)
        {
            return Ok(false);
        }
        let (equipment, _, _, _) = attachment(world, car_entity)?;
        probe.car = Some(car_entity);
        probe.equipment = Some(equipment);
        probe.stage = Stage::Drop;
    }
    if probe.car != Some(car_entity) {
        return Err("Truck entity was recreated during probe".into());
    }
    if probe.stage == Stage::Drop {
        *world
            .get_mut::<Transform>(car_entity)
            .ok_or("Missing truck pose")? = Transform::from_xyz(0.0, 3.6, -15.0)
            .with_rotation(Quat::from_rotation_z(ROLLS[probe.case]));
        *world
            .get_mut::<Velocity>(car_entity)
            .ok_or("Missing truck velocity")? = Velocity::zero();
        *world
            .get_mut::<ExternalForce>(car_entity)
            .ok_or("Missing truck force")? = ExternalForce::default();
        world.entity_mut(car_entity).insert(Sleeping::disabled());
        world.resource_mut::<VehiclePhysics>().controller = None;
        *world.resource_mut::<super::VehicleVisualState>() = super::VehicleVisualState::default();
        probe.stage = Stage::Settle;
        probe.elapsed = 0.0;
        probe.stable = 0.0;
        return Ok(false);
    }
    if !car.translation.is_finite()
        || !car.rotation.is_finite()
        || car.scale != Vec3::ONE
        || !velocity.linear.is_finite()
        || !velocity.angular.is_finite()
    {
        return Err("Non-finite truck state or unexpected scaling".into());
    }
    let (equipment, equipment_lowest, mass, follow_error) = attachment(world, car_entity)?;
    if probe.equipment != Some(equipment) {
        return Err("Dummy attachment was recreated during inversion".into());
    }
    let body_lowest = super::collision::lowest_point(&car);
    if !body_lowest.is_finite()
        || !equipment_lowest.is_finite()
        || body_lowest < -0.07
        || equipment_lowest < -0.07
    {
        return Err(format!(
            "Severe floor penetration: body {body_lowest:.5}, equipment {equipment_lowest:.5} m"
        ));
    }
    lock_camera(world, &car);
    let dt = world.resource::<Time>().delta_secs();
    probe.elapsed += dt;
    match probe.stage {
        Stage::Settle => {
            if probe.elapsed > 12.0 {
                return Err(format!(
                    "Required support snapshot was not reached in case {}: {velocity:?}",
                    probe.case
                ));
            }
            let supported = if probe.case == 0 {
                world
                    .resource::<VehiclePhysics>()
                    .wheels
                    .iter()
                    .filter(|wheel| wheel.grounded)
                    .count()
                    >= 3
            } else {
                body_lowest.min(equipment_lowest) < 0.06
            };
            let stable = probe.elapsed > 1.0
                && supported
                && velocity.linear.length() < 0.12
                && velocity.angular.length() < 0.12;
            probe.stable = if stable { probe.stable + dt } else { 0.0 };
            let body_contact = body_floor_contact(world, car_entity)?;
            if capture_ready(
                probe.case,
                car.rotation,
                supported,
                body_contact,
                probe.stable,
            ) {
                let visual = rendered_lowest(world, car_entity)?;
                if visual.body < -0.07 || body_lowest < -0.04 || equipment_lowest < -0.04 {
                    return Err(format!(
                        "Capture penetration: GLB body {:.5}, collider {body_lowest:.5}, equipment {equipment_lowest:.5} m",
                        visual.body
                    ));
                }
                probe.records.push(json!({
                    "image": IMAGES[probe.case], "mass_kg": mass,
                    "position": car.translation.to_array(), "rotation_xyzw": car.rotation.to_array(),
                    "body_lowest_m": body_lowest, "equipment_lowest_m": equipment_lowest,
                    "rendered_glb_lowest_m": visual.body.min(visual.wheels),
                    "rendered_body_lowest_m": visual.body,
                    "rendered_suspended_wheels_lowest_m": visual.wheels,
                    "suspended_wheels_have_no_lateral_collision": true,
                    "attachment_follow_error_m": follow_error,
                    "shared_chassis_body": true, "active_body_floor_contact": body_contact,
                    "capture_state": if probe.case >= 2 { "flank-contact" } else { "settled" },
                    "linear_velocity": velocity.linear.to_array(), "angular_velocity": velocity.angular.to_array(),
                }));
                // Request this frame's image before a free flank can roll back
                // onto its tires. Physics continues normally during GPU readback.
                capture(world, probe);
                probe.stage = Stage::Capture;
            }
        }
        Stage::Capture => {
            if png_exists(&probe.output.join(IMAGES[probe.case])) {
                if probe.case == IMAGES.len() - 1 {
                    return Ok(true);
                }
                probe.case += 1;
                probe.stage = Stage::Drop;
            }
        }
        Stage::Load | Stage::Prepare | Stage::Drop => unreachable!(),
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_probe_only_separates_suspended_wheels_not_the_spare_or_body() {
        let mut world = World::new();
        let car = world.spawn(Name::new("Truck")).id();
        let spin = world.spawn((Name::new("WheelSpin_RR"), ChildOf(car))).id();
        let cap = world.spawn((Name::new("Hub cap"), ChildOf(spin))).id();
        let spare = world.spawn((Name::new("SpareWheel"), ChildOf(car))).id();
        let roof = world.spawn((Name::new("Body"), ChildOf(car))).id();
        assert!(suspended_wheel(&world, cap));
        assert!(suspended_wheel(&world, spin));
        assert!(!suspended_wheel(&world, spare));
        assert!(!suspended_wheel(&world, roof));
        assert!(!suspended_wheel(&world, car));
    }

    #[test]
    fn body_probe_requires_each_intended_orientation() {
        for (case, roll) in ROLLS.into_iter().enumerate() {
            assert!(orientation_valid(case, Quat::from_rotation_z(roll)));
        }
        for case in 1..4 {
            assert!(!orientation_valid(case, Quat::IDENTITY));
        }
    }

    #[test]
    fn flank_capture_requires_real_contact_but_does_not_require_settling() {
        for case in [2, 3] {
            let flank = Quat::from_rotation_z(ROLLS[case]);
            assert!(capture_ready(case, flank, true, true, 0.0));
            assert!(!capture_ready(case, flank, true, false, 1.0));
            assert!(!capture_ready(case, flank, false, true, 1.0));
            assert!(!capture_ready(case, Quat::IDENTITY, true, true, 1.0));
        }
        for case in [0, 1] {
            let rotation = Quat::from_rotation_z(ROLLS[case]);
            assert!(!capture_ready(case, rotation, true, true, 0.0));
            assert!(capture_ready(case, rotation, true, true, 0.5));
        }
    }

    #[test]
    fn dummy_follows_the_chassis_through_roof_and_both_flanks() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, super::super::VehiclePlugin))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<crate::GameState>()
            .insert_resource(EquipmentRequest::demo())
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 120.0,
            )));
        let car = app
            .world_mut()
            .spawn((
                super::super::controller::chassis_bundle(
                    &super::super::config::VehiclePhysicsConfig::baseline(),
                ),
                Transform::from_xyz(2.0, 4.0, -4.0),
            ))
            .id();
        for _ in 0..5 {
            app.update();
        }
        let (original, _, _, _) = attachment(app.world_mut(), car).unwrap();
        for roll in ROLLS {
            *app.world_mut().get_mut::<Transform>(car).unwrap() =
                Transform::from_xyz(2.0, 4.0, -4.0).with_rotation(Quat::from_rotation_z(roll));
            for _ in 0..3 {
                app.update();
            }
            let (entity, _, mass, follow_error) = attachment(app.world_mut(), car).unwrap();
            assert_eq!(entity, original);
            assert!((mass - 1840.0).abs() < 0.05);
            assert!(follow_error < 1e-5);
            assert_eq!(
                *app.world()
                    .resource::<super::super::config::VehiclePhysicsConfig>(),
                super::super::config::VehiclePhysicsConfig::baseline()
            );
        }
    }
}
