use bevy::prelude::*;
use bevy_rapier3d::parry::query::DefaultQueryDispatcher;
use bevy_rapier3d::prelude::*;
use bevy_rapier3d::rapier::control::{DynamicRayCastVehicleController, WheelTuning};

use super::config::{PhysicsModel, SuspensionConfig, VehiclePhysicsConfig};
use super::visuals::VehicleVisualState;
use crate::GameState;
use crate::ui::physics_tuning::UiInputCapture;

pub const PHYSICS_HZ: f64 = 120.0;
pub const PHYSICS_DT: f32 = 1.0 / PHYSICS_HZ as f32;
pub const MODEL_WHEEL_RADIUS: f32 = 0.53;
pub const MOUNTS: [Vec3; 4] = [
    Vec3::new(-0.98, -0.22, 1.26),
    Vec3::new(0.98, -0.22, 1.26),
    Vec3::new(-0.98, -0.22, -1.27),
    Vec3::new(0.98, -0.22, -1.27),
];

#[derive(Component)]
pub struct Truck;

/// A collider can opt into a future surface-specific grip multiplier.
#[derive(Component, Clone, Copy)]
pub struct SurfaceGrip(pub f32);

#[derive(Clone, Copy, Default)]
pub struct WheelContact {
    pub grounded: bool,
    pub normal: Vec3,
    pub point: Vec3,
    pub suspension_length: f32,
    pub normal_load: f32,
    pub ground_velocity: Vec3,
    pub steer: f32,
}

#[derive(Resource)]
pub struct VehiclePhysics {
    pub controller: Option<DynamicRayCastVehicleController>,
    pub active_config: VehiclePhysicsConfig,
    pub wheels: [WheelContact; 4],
    pub throttle: f32,
    pub engine_force: f32,
    pub brake_force: f32,
    pub sleeping: bool,
    pub error: Option<String>,
    initialized: bool,
    wake_requested: bool,
}

impl Default for VehiclePhysics {
    fn default() -> Self {
        Self {
            controller: None,
            active_config: VehiclePhysicsConfig::baseline(),
            wheels: [WheelContact::default(); 4],
            throttle: 0.0,
            engine_force: 0.0,
            brake_force: 0.0,
            sleeping: false,
            error: None,
            initialized: false,
            wake_requested: true,
        }
    }
}

pub fn mass_properties(config: &VehiclePhysicsConfig) -> MassProperties {
    MassProperties {
        local_center_of_mass: Vec3::from_array(config.chassis.center_of_mass),
        mass: config.chassis.mass_kg,
        principal_inertia_local_frame: Quat::IDENTITY,
        principal_inertia: config.chassis.mass_kg / 3.0
            * Vec3::new(
                0.40_f32.powi(2) + 1.92_f32.powi(2),
                1.10_f32.powi(2) + 1.92_f32.powi(2),
                1.10_f32.powi(2) + 0.40_f32.powi(2),
            ),
    }
}

pub fn chassis_bundle(config: &VehiclePhysicsConfig) -> impl Bundle {
    (
        Truck,
        Visibility::default(),
        RigidBody::Dynamic,
        Collider::cuboid(1.10, 0.40, 1.92),
        ColliderMassProperties::MassProperties(mass_properties(config)),
        Damping {
            linear_damping: config.chassis.linear_damping,
            angular_damping: config.chassis.angular_damping,
        },
        Ccd::enabled(),
        Sleeping::disabled(),
        Velocity::zero(),
        ExternalForce::default(),
    )
}

pub fn spawn_height(config: &VehiclePhysicsConfig) -> f32 {
    let load = config.chassis.mass_kg * 9.81 / 4.0;
    let front = config.front.rest_length_m - load / config.front.stiffness_n_per_m;
    let rear = config.rear.rest_length_m - load / config.rear.stiffness_n_per_m;
    0.22 + config.tires.radius_m + (front + rear) * 0.5
}

pub fn apply_configuration(
    config: Res<VehiclePhysicsConfig>,
    mut physics: ResMut<VehiclePhysics>,
    mut visual: ResMut<VehicleVisualState>,
    mut cars: Query<
        (
            &mut ColliderMassProperties,
            &mut Damping,
            &mut ExternalForce,
        ),
        With<Truck>,
    >,
) {
    if physics.initialized && *config == physics.active_config {
        return;
    }
    if let Err(error) = config.validate() {
        physics.error = Some(error);
        return;
    }
    let Ok((mut mass, mut damping, mut force)) = cars.single_mut() else {
        return;
    };
    let geometry_changed = config.model != physics.active_config.model
        || config.tires.radius_m != physics.active_config.tires.radius_m
        || config.front.rest_length_m != physics.active_config.front.rest_length_m
        || config.rear.rest_length_m != physics.active_config.rear.rest_length_m;
    *mass = ColliderMassProperties::MassProperties(mass_properties(&config));
    *damping = Damping {
        linear_damping: config.chassis.linear_damping,
        angular_damping: config.chassis.angular_damping,
    };
    *force = ExternalForce::default();
    if geometry_changed {
        physics.controller = None;
        visual.previous_hubs = [None; 4];
    }
    physics.active_config = config.clone();
    physics.initialized = true;
    physics.wake_requested = true;
    physics.error = None;
}

fn axle_config(config: &VehiclePhysicsConfig, wheel: usize) -> &SuspensionConfig {
    if wheel < 2 {
        &config.front
    } else {
        &config.rear
    }
}

fn tune_controller(
    controller: &mut DynamicRayCastVehicleController,
    config: &VehiclePhysicsConfig,
) {
    for (i, wheel) in controller.wheels_mut().iter_mut().enumerate() {
        let axle = axle_config(config, i);
        wheel.suspension_stiffness = axle.stiffness_n_per_m / config.chassis.mass_kg;
        wheel.damping_compression = axle.compression_damping_ns_per_m / config.chassis.mass_kg;
        wheel.damping_relaxation = axle.rebound_damping_ns_per_m / config.chassis.mass_kg;
        wheel.suspension_rest_length = axle.rest_length_m;
        wheel.max_suspension_travel = axle.max_travel_m;
        wheel.max_suspension_force = axle.max_force_n;
        wheel.radius = config.tires.radius_m;
        wheel.friction_slip = config.tires.grip_coefficient;
        wheel.side_friction_stiffness = config.tires.lateral_stiffness;
    }
}

fn new_controller(
    handle: RapierRigidBodyHandle,
    config: &VehiclePhysicsConfig,
) -> DynamicRayCastVehicleController {
    let mut controller = DynamicRayCastVehicleController::new(handle.0);
    controller.index_forward_axis = 2;
    for (i, mount) in MOUNTS.into_iter().enumerate() {
        controller.add_wheel(
            mount,
            Vec3::NEG_Y,
            Vec3::NEG_X,
            axle_config(config, i).rest_length_m,
            config.tires.radius_m,
            &WheelTuning::default(),
        );
    }
    tune_controller(&mut controller, config);
    controller
}

fn axis(keys: &ButtonInput<KeyCode>, positive: &[KeyCode], negative: &[KeyCode]) -> f32 {
    positive.iter().any(|key| keys.pressed(*key)) as i32 as f32
        - negative.iter().any(|key| keys.pressed(*key)) as i32 as f32
}

pub fn suspension_and_drive(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<UiInputCapture>,
    mut state: ResMut<GameState>,
    mut physics: ResMut<VehiclePhysics>,
    mut visual: ResMut<VehicleVisualState>,
    mut context: WriteRapierContext,
    configuration: Query<&RapierConfiguration>,
    surfaces: Query<&SurfaceGrip>,
    cars: Query<(Entity, &RapierRigidBodyHandle), With<Truck>>,
) {
    let Ok((entity, handle)) = cars.single() else {
        return;
    };
    let Ok(mut rapier) = context.single_mut() else {
        return;
    };
    let Some(body) = rapier.rigidbody_set.bodies.get(handle.0) else {
        return;
    };
    let config = physics.active_config.clone();
    let forward_speed = body.linvel().dot(body.position().rotation * Vec3::Z);
    let gas = if capture.keyboard {
        0.0
    } else {
        axis(
            &keys,
            &[KeyCode::KeyW, KeyCode::KeyZ, KeyCode::ArrowUp],
            &[KeyCode::KeyS, KeyCode::ArrowDown],
        )
    };
    let steer = if capture.keyboard {
        0.0
    } else {
        -axis(
            &keys,
            &[KeyCode::KeyD, KeyCode::ArrowRight],
            &[KeyCode::KeyA, KeyCode::KeyQ, KeyCode::ArrowLeft],
        )
    };
    let braking = (!capture.keyboard && keys.pressed(KeyCode::Space)) || gas * forward_speed < -0.4;
    physics.throttle += (gas.abs() - physics.throttle)
        * (1.0 - (-config.drivetrain.throttle_response_hz * PHYSICS_DT).exp());
    if gas.abs() < 0.01 && physics.throttle < 0.001 {
        physics.throttle = 0.0;
    }
    let max_speed = if gas < 0.0 {
        config.drivetrain.reverse_max_speed_mps
    } else if state.low_range {
        config.drivetrain.low_max_speed_mps
    } else {
        config.drivetrain.normal_max_speed_mps
    };
    let ratio = if state.low_range {
        config.drivetrain.low_ratio
    } else {
        config.drivetrain.normal_ratio
    };
    let engine = if gas.abs() > 0.01 && !braking && forward_speed.abs() < max_speed {
        gas.signum() * physics.throttle * config.drivetrain.engine_force_n * ratio
    } else {
        0.0
    };
    let drag = config.tires.rolling_resistance_n
        + config.tires.rolling_drag_ns_per_m * forward_speed.abs();
    let brake = if braking {
        config.drivetrain.brake_force_n
    } else if gas.abs() < 0.01 && forward_speed.abs() < 0.5 {
        config.drivetrain.brake_force_n * config.chassis.hill_hold_strength
            + config.drivetrain.engine_brake_force_n
            + drag
    } else if gas.abs() < 0.01 {
        config.drivetrain.engine_brake_force_n + drag
    } else {
        0.0
    };
    let target_steer = steer * 0.43 / (1.0 + forward_speed.abs() * 0.065);
    visual.steer += (target_steer - visual.steer) * (1.0 - (-9.0 * PHYSICS_DT).exp());
    physics.engine_force = engine;
    physics.brake_force = brake;
    let sleeping = body.is_sleeping();
    physics.sleeping = sleeping;
    // Native suspension impulses do not wake bodies: never mutate an idle sleeper.
    if sleeping && gas.abs() < 0.01 && !braking && !physics.wake_requested {
        for i in 0..2 {
            physics.wheels[i].steer = visual.steer;
        }
        return;
    }
    if gas.abs() > 0.01 || braking || physics.wake_requested {
        rapier
            .rigidbody_set
            .bodies
            .get_mut(handle.0)
            .unwrap()
            .wake_up(true);
    }
    if physics.wake_requested {
        let body = rapier.rigidbody_set.bodies.get_mut(handle.0).unwrap();
        body.reset_forces(false);
        body.reset_torques(false);
        physics.wake_requested = false;
    }
    let solver_substeps = rapier
        .simulation
        .integration_parameters
        .num_solver_iterations
        + rapier.rigidbody_set.bodies[handle.0].additional_solver_iterations();
    let gravity_dt = parking_gravity_dt(solver_substeps);
    if config.model == PhysicsModel::Legacy {
        legacy_step(
            &mut rapier,
            entity,
            *handle,
            &config,
            gas,
            engine,
            braking,
            visual.steer,
            &mut physics,
        );
    } else {
        if physics
            .controller
            .as_ref()
            .is_none_or(|controller| controller.chassis != handle.0)
        {
            physics.controller = Some(new_controller(*handle, &config));
        }
        let mut controller = physics.controller.take().unwrap();
        tune_controller(&mut controller, &config);
        for (i, wheel) in controller.wheels_mut().iter_mut().enumerate() {
            wheel.steering = if i < 2 { visual.steer } else { 0.0 };
            let surface = wheel
                .raycast_info()
                .ground_object
                .and_then(|handle| rapier.colliders.collider_entity(handle))
                .and_then(|entity| surfaces.get(entity).ok())
                .map(|grip| grip.0.clamp(0.05, 3.0))
                .unwrap_or(1.0);
            wheel.friction_slip *= surface;
            wheel.engine_force = if engine.abs() > 0.001 {
                engine.signum() * (engine.abs() - drag).max(0.0) / 4.0
            } else {
                0.0
            };
            let rolling_brake = if wheel.engine_force == 0.0 {
                (drag - engine.abs()).max(0.0)
            } else {
                0.0
            };
            wheel.brake = brake.max(rolling_brake) * PHYSICS_DT / 4.0;
        }
        RapierQueryPipelineMut::new_scoped(
            &rapier.simulation.broad_phase,
            &mut rapier.colliders,
            &mut rapier.rigidbody_set,
            &QueryFilter::default()
                .exclude_rigid_body(entity)
                .exclude_sensors(),
            &DefaultQueryDispatcher,
            |pipeline| controller.update_vehicle(PHYSICS_DT, pipeline.query_pipeline),
        );
        limit_tire_impulses(&mut controller, &mut rapier.rigidbody_set);
        if (gas.abs() < 0.01 || braking) && config.chassis.hill_hold_strength > 0.0 {
            let gravity = configuration
                .single()
                .map(|c| c.gravity)
                .unwrap_or(Vec3::NEG_Y * 9.81);
            apply_parking_assistance(
                &mut controller,
                &mut rapier.rigidbody_set,
                gravity,
                config.chassis.hill_hold_strength,
                gravity_dt,
            );
        }
        for (i, wheel) in controller.wheels().iter().enumerate() {
            let contact = wheel.raycast_info();
            let ground_velocity = contact
                .ground_object
                .and_then(|h| rapier.colliders.colliders[h].parent())
                .and_then(|h| rapier.rigidbody_set.bodies.get(h))
                .map(|ground| ground.velocity_at_point(contact.contact_point_ws))
                .unwrap_or(Vec3::ZERO);
            physics.wheels[i] = WheelContact {
                grounded: contact.is_in_contact,
                normal: contact.contact_normal_ws,
                point: contact.contact_point_ws,
                suspension_length: contact.suspension_length,
                normal_load: wheel.wheel_suspension_force.min(wheel.max_suspension_force),
                ground_velocity,
                steer: wheel.steering,
            };
        }
        physics.controller = Some(controller);
    }
    state.grounded_wheels = physics.wheels.iter().filter(|wheel| wheel.grounded).count();
    if config.model == PhysicsModel::Legacy
        && config.chassis.hill_hold_strength > 0.0
        && (gas.abs() < 0.01 || braking)
        && state.grounded_wheels >= 3
    {
        let normal = physics
            .wheels
            .iter()
            .filter(|wheel| wheel.grounded)
            .map(|wheel| wheel.normal)
            .sum::<Vec3>()
            .normalize_or_zero();
        let body = rapier.rigidbody_set.bodies.get_mut(handle.0).unwrap();
        let tangent_velocity = body.linvel() - normal * body.linvel().dot(normal);
        if normal.y > 0.90 && tangent_velocity.length() < 0.35 && body.angvel().length() < 0.20 {
            let gravity = configuration
                .single()
                .map(|c| c.gravity)
                .unwrap_or(Vec3::NEG_Y * 9.81);
            let tangent_gravity = gravity - normal * gravity.dot(normal);
            body.apply_impulse(
                -(tangent_velocity + tangent_gravity * gravity_dt)
                    * body.mass()
                    * config.chassis.hill_hold_strength,
                false,
            );
        }
    }
}

fn tire_axes(wheel: &bevy_rapier3d::rapier::control::Wheel) -> (Vec3, Vec3) {
    let normal = wheel.raycast_info().contact_normal_ws;
    let axle = (wheel.axle() - normal * wheel.axle().dot(normal)).normalize_or_zero();
    (normal.cross(axle).normalize_or_zero(), axle)
}

fn tire_impulse_limit(wheel: &bevy_rapier3d::rapier::control::Wheel) -> f32 {
    wheel.wheel_suspension_force.min(wheel.max_suspension_force) * wheel.friction_slip * PHYSICS_DT
}

fn parking_gravity_dt(solver_substeps: usize) -> f32 {
    let steps = solver_substeps.max(1) as f32;
    // Rapier integrates gravity before position in each internal solver substep.
    PHYSICS_DT * (steps + 1.0) / (2.0 * steps)
}

fn apply_tire_correction(
    body: &mut bevy_rapier3d::rapier::dynamics::RigidBody,
    wheel: &bevy_rapier3d::rapier::control::Wheel,
    correction: Vec2,
) {
    let point = wheel.raycast_info().contact_point_ws;
    let (forward, axle) = tire_axes(wheel);
    body.apply_impulse_at_point(forward * correction.x, point, false);
    // Match the pinned controller's internal roll_influence = 0.1.
    let up = body.position().rotation * Vec3::Y;
    let roll_point = point - up * up.dot(point - body.center_of_mass()) * 0.9;
    body.apply_impulse_at_point(axle * correction.y, roll_point, false);
}

fn limit_tire_impulses(
    controller: &mut DynamicRayCastVehicleController,
    bodies: &mut RapierRigidBodySet,
) {
    let body = &mut bodies.bodies[controller.chassis];
    for wheel in controller.wheels_mut() {
        let impulse = Vec2::new(wheel.forward_impulse, wheel.side_impulse);
        let limited = impulse.clamp_length_max(tire_impulse_limit(wheel));
        let correction = limited - impulse;
        if correction.length_squared() == 0.0 {
            continue;
        }
        // Rapier 0.33-alpha can bypass its longitudinal limit. Undo only the excess.
        apply_tire_correction(body, wheel, correction);
        wheel.forward_impulse = limited.x;
        wheel.side_impulse = limited.y;
    }
}

fn apply_parking_assistance(
    controller: &mut DynamicRayCastVehicleController,
    bodies: &mut RapierRigidBodySet,
    gravity: Vec3,
    strength: f32,
    gravity_dt: f32,
) {
    let contacts = controller
        .wheels()
        .iter()
        .filter(|w| w.raycast_info().is_in_contact)
        .count();
    if contacts < 3 {
        return;
    }
    let normal = controller
        .wheels()
        .iter()
        .filter(|w| w.raycast_info().is_in_contact)
        .map(|w| w.raycast_info().contact_normal_ws)
        .sum::<Vec3>()
        .normalize_or_zero();
    let total_load: f32 = controller
        .wheels()
        .iter()
        .filter(|w| w.raycast_info().is_in_contact)
        .map(|w| w.wheel_suspension_force.min(w.max_suspension_force))
        .sum();
    let body = &mut bodies.bodies[controller.chassis];
    let tangent_velocity = body.linvel() - normal * body.linvel().dot(normal);
    if normal.y <= 0.90
        || tangent_velocity.length() >= 0.35
        || body.angvel().length() >= 0.20
        || total_load <= 0.0
    {
        return;
    }
    let tangent_gravity = gravity - normal * gravity.dot(normal);
    let holding_impulse =
        -(tangent_velocity + tangent_gravity * gravity_dt) * body.mass() * strength;
    for wheel in controller.wheels_mut() {
        if !wheel.raycast_info().is_in_contact {
            continue;
        }
        let wheel_normal = wheel.raycast_info().contact_normal_ws;
        let load = wheel.wheel_suspension_force.min(wheel.max_suspension_force);
        let share = holding_impulse * (load / total_load);
        let tangent_share = share - wheel_normal * share.dot(wheel_normal);
        let (forward, axle) = tire_axes(wheel);
        let existing = forward * wheel.forward_impulse + axle * wheel.side_impulse;
        let limited = (existing + tangent_share).clamp_length_max(tire_impulse_limit(wheel));
        let correction = limited - existing;
        apply_tire_correction(
            body,
            wheel,
            Vec2::new(correction.dot(forward), correction.dot(axle)),
        );
        wheel.forward_impulse = limited.dot(forward);
        wheel.side_impulse = limited.dot(axle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chassis_propagates_visibility_to_the_gltf_children() {
        let mut world = World::new();
        let car = world
            .spawn((
                chassis_bundle(&VehiclePhysicsConfig::baseline()),
                Transform::default(),
            ))
            .id();
        assert!(world.entity(car).contains::<InheritedVisibility>());
    }

    #[test]
    fn parking_compensation_matches_internal_gravity_integration() {
        for steps in [1, 4, 8] {
            let h = PHYSICS_DT / steps as f32;
            let gravity_displacement: f32 = (1..=steps).map(|step| step as f32 * h * h).sum();
            let compensated = gravity_displacement - parking_gravity_dt(steps) * PHYSICS_DT;
            assert!(compensated.abs() < 1.0e-9);
        }
    }
}

fn legacy_step(
    rapier: &mut RapierContextMut,
    entity: Entity,
    handle: RapierRigidBodyHandle,
    config: &VehiclePhysicsConfig,
    gas: f32,
    engine: f32,
    braking: bool,
    steer: f32,
    physics: &mut VehiclePhysics,
) {
    let body = &rapier.rigidbody_set.bodies[handle.0];
    let pose = *body.position();
    let forward = pose.rotation * Vec3::Z;
    let right = pose.rotation * Vec3::X;
    let up = pose.rotation * Vec3::Y;
    let velocity = body.linvel();
    let angular = body.angvel();
    let speed = velocity.dot(forward);
    let com = body.center_of_mass();
    let mut force = Vec3::ZERO;
    let mut torque = Vec3::ZERO;
    for (i, mount) in MOUNTS.into_iter().enumerate() {
        let axle = axle_config(config, i);
        let origin = pose.translation + pose.rotation * mount;
        let hit = rapier.with_query_pipeline(
            QueryFilter::default().exclude_rigid_body(entity),
            |pipeline| {
                pipeline.cast_ray_and_get_normal(
                    origin,
                    -up,
                    axle.rest_length_m + config.tires.radius_m,
                    true,
                )
            },
        );
        let Some((_, hit)) = hit.filter(|(_, hit)| hit.normal.dot(Vec3::Y) >= 0.42) else {
            physics.wheels[i] = WheelContact {
                suspension_length: axle.rest_length_m,
                ..default()
            };
            continue;
        };
        let length = (hit.time_of_impact - config.tires.radius_m).clamp(
            (axle.rest_length_m - axle.max_travel_m).max(0.0),
            axle.rest_length_m,
        );
        let point = origin - up * hit.time_of_impact;
        let point_velocity = velocity + angular.cross(origin - com);
        let vertical_speed = point_velocity.dot(up);
        let damper = if vertical_speed < 0.0 {
            axle.compression_damping_ns_per_m
        } else {
            axle.rebound_damping_ns_per_m
        };
        let spring = ((axle.rest_length_m - length) * axle.stiffness_n_per_m
            - vertical_speed * damper)
            .clamp(0.0, axle.max_force_n);
        let angle = if i < 2 && !(speed.abs() < 0.65 && gas.abs() < 0.01) {
            steer
        } else {
            0.0
        };
        let rotation = Quat::from_axis_angle(up, angle);
        let heading = rotation * forward;
        let side = rotation * right;
        let side_force = (point_velocity.dot(side) * 1_650.0 * config.tires.lateral_stiffness)
            .clamp(
                -4_600.0 * config.tires.grip_coefficient / 1.35,
                4_600.0 * config.tires.grip_coefficient / 1.35,
            );
        let longitudinal = point_velocity.dot(heading);
        let drag = longitudinal * config.tires.rolling_drag_ns_per_m / 4.0
            + longitudinal.signum() * config.tires.rolling_resistance_n / 4.0;
        let brake_force = if braking {
            let requested = (config.drivetrain.brake_force_n
                + longitudinal.abs() * config.drivetrain.legacy_brake_damping_ns_per_m)
                / 4.0;
            longitudinal.signum()
                * requested.min(longitudinal.abs() * config.chassis.mass_kg / (4.0 * PHYSICS_DT))
        } else if gas.abs() < 0.01 {
            longitudinal.signum() * config.drivetrain.engine_brake_force_n / 4.0
        } else {
            0.0
        };
        let wheel_force = up * spring - side * side_force - heading * (drag + brake_force)
            + heading * engine / 4.0;
        force += wheel_force;
        torque += (origin - com).cross(wheel_force);
        physics.wheels[i] = WheelContact {
            grounded: true,
            normal: hit.normal,
            point,
            suspension_length: length,
            normal_load: spring,
            steer: angle,
            ..default()
        };
    }
    if physics.wheels.iter().any(|wheel| wheel.grounded) {
        // Historical yaw assistance is retained only for the Origine comparison.
        if speed.abs() > 0.65 {
            torque += up * steer * speed.clamp(-8.0, 8.0) * 110.0;
        }
        torque -= angular * 290.0;
    }
    let body = rapier.rigidbody_set.bodies.get_mut(handle.0).unwrap();
    body.reset_forces(false);
    body.reset_torques(false);
    body.add_force(force, false);
    body.add_torque(torque, false);
}
