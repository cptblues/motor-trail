use super::*;
use bevy::prelude::Quat;
use bevy_rapier3d::parry::query::DefaultQueryDispatcher;
use bevy_rapier3d::rapier::prelude::*;

mod characterization;

const DT: f32 = 1.0 / 120.0;
const MOUNTS: [Vec3; 4] = [
    Vec3::new(-0.98, -0.22, 1.26),
    Vec3::new(0.98, -0.22, 1.26),
    Vec3::new(-0.98, -0.22, -1.27),
    Vec3::new(0.98, -0.22, -1.27),
];

struct Scene {
    chassis: RigidBodyHandle,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    broad_phase: BroadPhaseBvh,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    narrow_phase: NarrowPhase,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    ccd: CCDSolver,
}

impl Scene {
    fn new(position: Vec3, floor_height: Option<f32>, step: bool) -> Self {
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();
        let chassis = bodies.insert(RigidBodyBuilder::dynamic().translation(position));
        colliders.insert_with_parent(
            ColliderBuilder::cuboid(0.05, 0.05, 0.05).mass(1800.0),
            chassis,
            &mut bodies,
        );
        if let Some(height) = floor_height {
            colliders.insert(
                ColliderBuilder::cuboid(20.0, 0.5, 20.0).translation(Vec3::new(
                    0.0,
                    height - 0.5,
                    0.0,
                )),
            );
        }
        if step {
            colliders.insert(
                ColliderBuilder::cuboid(3.0, 0.125, 1.0).translation(Vec3::new(0.0, 0.125, 1.0)),
            );
        }
        let mut result = Self {
            chassis,
            bodies,
            colliders,
            broad_phase: BroadPhaseBvh::new(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
        };
        result.refresh();
        result
    }

    fn refresh(&mut self) {
        self.step(Vec3::ZERO);
    }

    fn step(&mut self, gravity: Vec3) {
        // Bodies retain island IDs and the broad phase retains pairs across steps.
        // Keep their managers alive for the full lifetime of this test world.
        self.pipeline.step(
            gravity,
            &IntegrationParameters {
                dt: DT,
                ..Default::default()
            },
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd,
            &(),
            &(),
        );
    }

    fn queries(&mut self) -> QueryPipelineMut<'_> {
        self.broad_phase.as_query_pipeline_mut(
            &DefaultQueryDispatcher,
            &mut self.bodies,
            &mut self.colliders,
            QueryFilter::default()
                .exclude_rigid_body(self.chassis)
                .exclude_sensors(),
        )
    }
}

fn tuning() -> WheelTuning {
    WheelTuning {
        suspension_stiffness: 72_000.0 / 1800.0,
        suspension_compression: 6500.0 / 1800.0,
        suspension_damping: 7500.0 / 1800.0,
        max_suspension_travel: 0.5,
        max_suspension_force: 25_000.0,
        friction_slip: 1.2,
        side_friction_stiffness: 0.8,
    }
}

fn controller(chassis: RigidBodyHandle, mode: TireContactMode, mounts: &[Vec3]) -> WheelController {
    let mut result = WheelController::new(chassis, mode);
    result.index_forward_axis = 2;
    for mount in mounts {
        result.add_wheel(*mount, Vec3::NEG_Y, Vec3::NEG_X, 0.36, 0.53, &tuning());
    }
    result
}

fn assert_scalar(actual: f32, expected: f32, epsilon: f32) {
    assert!(
        (actual - expected).abs() <= epsilon,
        "actual {actual}, expected {expected}"
    );
}

fn assert_wheel(actual: &Wheel, expected: &NativeWheel, epsilon: f32) {
    let a = actual.raycast_info();
    let b = expected.raycast_info();
    assert_eq!(a.is_in_contact, b.is_in_contact);
    assert_eq!(a.ground_object, b.ground_object);
    assert!(a.contact_normal_ws.distance(b.contact_normal_ws) <= epsilon);
    assert!(a.contact_point_ws.distance(b.contact_point_ws) <= epsilon);
    assert!(a.hard_point_ws.distance(b.hard_point_ws) <= epsilon);
    assert_scalar(a.suspension_length, b.suspension_length, epsilon);
    assert_scalar(
        actual.wheel_suspension_force,
        expected.wheel_suspension_force,
        epsilon,
    );
    assert_scalar(actual.forward_impulse, expected.forward_impulse, epsilon);
    assert_scalar(actual.side_impulse, expected.side_impulse, epsilon);
    assert_scalar(actual.rotation, expected.rotation, epsilon);
    assert!(actual.center().distance(expected.center()) <= epsilon);
    assert!(actual.suspension().distance(expected.suspension()) <= epsilon);
    assert!(actual.axle().distance(expected.axle()) <= epsilon);
}

fn parity(mode: TireContactMode) {
    let position = Vec3::new(0.0, 1.07, 0.0);
    let mut facade_scene = Scene::new(position, Some(0.0), false);
    let mut upstream_scene = Scene::new(position, Some(0.0), false);
    let mut facade = controller(facade_scene.chassis, mode, &MOUNTS);
    let mut upstream = DynamicRayCastVehicleController::new(upstream_scene.chassis);
    upstream.index_forward_axis = 2;
    for mount in MOUNTS {
        upstream.add_wheel(mount, Vec3::NEG_Y, Vec3::NEG_X, 0.36, 0.53, &tuning());
    }
    let cache = facade.wheels().as_ptr();
    for (speed, engine, brake, steering) in [
        (0.0, 0.0, 0.0, 0.0),
        (3.0, 1700.0, 0.0, 0.0),
        (3.0, 0.0, 25.0, 0.0),
        (3.0, 1700.0, 0.0, 0.31),
        (-2.0, -1300.0, 0.0, -0.24),
        (-2.0, 0.0, 20.0, 0.19),
    ] {
        let velocity = Vec3::new(0.15, -0.12, speed);
        for scene in [&mut facade_scene, &mut upstream_scene] {
            scene.bodies[scene.chassis].set_linvel(velocity, true);
            scene.bodies[scene.chassis].set_angvel(Vec3::new(0.02, 0.03, -0.01), true);
        }
        for i in 0..4 {
            let wheel = &mut facade.wheels_mut()[i];
            wheel.engine_force = engine;
            wheel.brake = brake;
            wheel.steering = if i < 2 { steering } else { 0.0 };
            wheel.friction_slip = 1.05 + i as f32 * 0.03;
            wheel.damping_relaxation = 4.0 + i as f32 * 0.2;
            wheel.suspension_stiffness = 35.0 + i as f32;
            wheel.damping_compression = 3.5 + i as f32 * 0.1;
            wheel.side_friction_stiffness = 0.6 + i as f32 * 0.05;
            wheel.max_suspension_force = 20_000.0 + i as f32 * 500.0;
            // Configure the reference independently of facade copy helpers.
            let reference = &mut upstream.wheels_mut()[i];
            reference.engine_force = engine;
            reference.brake = brake;
            reference.steering = if i < 2 { steering } else { 0.0 };
            reference.friction_slip = 1.05 + i as f32 * 0.03;
            reference.damping_relaxation = 4.0 + i as f32 * 0.2;
            reference.suspension_stiffness = 35.0 + i as f32;
            reference.damping_compression = 3.5 + i as f32 * 0.1;
            reference.side_friction_stiffness = 0.6 + i as f32 * 0.05;
            reference.max_suspension_force = 20_000.0 + i as f32 * 500.0;
        }
        facade.update_vehicle(DT, facade_scene.queries());
        upstream.update_vehicle(DT, upstream_scene.queries());
        // Equivalent flat contacts must keep the native hit and its exact force pass.
        let epsilon = 0.0;
        for (actual, expected) in facade.wheels().iter().zip(upstream.wheels()) {
            assert_wheel(actual, expected, epsilon);
        }
        let actual = &facade_scene.bodies[facade_scene.chassis];
        let expected = &upstream_scene.bodies[upstream_scene.chassis];
        assert!(actual.linvel().distance(expected.linvel()) <= epsilon);
        assert!(actual.angvel().distance(expected.angvel()) <= epsilon);
        assert_scalar(
            facade.current_vehicle_speed,
            upstream.current_vehicle_speed,
            epsilon,
        );
        assert_eq!(
            facade.wheels().as_ptr(),
            cache,
            "cache must not reallocate during updates"
        );
    }
}

#[test]
fn default_contact_mode_is_native() {
    assert_eq!(TireContactMode::default(), TireContactMode::Native);
}

#[test]
fn native_facade_matches_actual_upstream_engine_brake_steering_and_reverse() {
    parity(TireContactMode::Native);
}

#[test]
fn enabled_flat_matches_actual_upstream_engine_brake_steering_and_reverse() {
    parity(TireContactMode::Envelope);
}

#[test]
fn enabled_step_detects_support_before_the_center_ray_reaches_the_edge() {
    let position = Vec3::new(0.0, 0.85, -0.4);
    let mut native_scene = Scene::new(position, Some(0.0), true);
    let mut enabled_scene = Scene::new(position, Some(0.0), true);
    let mut native = controller(native_scene.chassis, TireContactMode::Native, &[Vec3::ZERO]);
    let mut enabled = controller(
        enabled_scene.chassis,
        TireContactMode::Envelope,
        &[Vec3::ZERO],
    );
    native.update_vehicle(DT, native_scene.queries());
    enabled.update_vehicle(DT, enabled_scene.queries());
    let ray = native.wheels()[0].raycast_info();
    let envelope = enabled.wheels()[0].raycast_info();
    assert_scalar(ray.suspension_length, 0.32, 1e-5);
    let expected = 0.85 - (0.25 + (0.53_f32.powi(2) - 0.4_f32.powi(2)).sqrt());
    assert!(
        envelope.suspension_length < ray.suspension_length - 0.03,
        "enabled tires must select early edge support: enabled {envelope:?}, native {ray:?}"
    );
    assert_scalar(envelope.suspension_length, expected, 0.004);
    assert!(envelope.contact_normal_ws.y > 0.6 && envelope.contact_normal_ws.z < -0.6);
    assert!((envelope.contact_point_ws.y - 0.25).abs() < 0.004);
}

#[test]
fn airborne_wheels_do_not_create_support_or_drive_impulses() {
    for mode in [TireContactMode::Native, TireContactMode::Envelope] {
        let mut scene = Scene::new(Vec3::new(0.0, 4.0, 0.0), None, false);
        let mut facade = controller(scene.chassis, mode, &MOUNTS);
        for wheel in facade.wheels_mut() {
            wheel.engine_force = 2500.0;
            wheel.brake = 30.0;
            wheel.steering = 0.3;
        }
        let velocity = Vec3::new(1.0, -2.0, 3.0);
        scene.bodies[scene.chassis].set_linvel(velocity, true);
        facade.update_vehicle(DT, scene.queries());
        assert_eq!(scene.bodies[scene.chassis].linvel(), velocity);
        for wheel in facade.wheels() {
            assert!(!wheel.raycast_info().is_in_contact);
            assert!(wheel.raycast_info().ground_object.is_none());
            assert_eq!(wheel.wheel_suspension_force, 0.0);
            assert_eq!(wheel.forward_impulse, 0.0);
            assert_eq!(wheel.side_impulse, 0.0);
            assert_eq!(
                wheel.raycast_info().suspension_length,
                wheel.suspension_rest_length
            );
        }
    }
}

#[test]
fn compressed_travel_can_be_negative_and_is_clamped_to_the_native_limits() {
    for mode in [TireContactMode::Native, TireContactMode::Envelope] {
        let mut scene = Scene::new(Vec3::new(0.0, 0.85, -0.4), Some(0.8), false);
        let mut facade = controller(scene.chassis, mode, &[Vec3::ZERO]);
        facade.wheels_mut()[0].max_suspension_travel = 0.5;
        facade.update_vehicle(DT, scene.queries());
        let wheel = &facade.wheels()[0];
        assert!(wheel.raycast_info().is_in_contact);
        assert_scalar(wheel.raycast_info().suspension_length, -0.14, 1e-5);
        assert!(wheel.wheel_suspension_force.is_finite() && wheel.wheel_suspension_force > 0.0);
        assert!(scene.bodies[scene.chassis].linvel().is_finite());
    }
}

#[test]
fn negative_raw_flat_travel_keeps_the_native_contact_before_clamping() {
    let position = Vec3::new(0.0, 0.85, -0.4);
    let mut native_scene = Scene::new(position, Some(0.8), false);
    let mut enabled_scene = Scene::new(position, Some(0.8), false);
    let mut native = controller(native_scene.chassis, TireContactMode::Native, &[Vec3::ZERO]);
    let mut enabled = controller(
        enabled_scene.chassis,
        TireContactMode::Envelope,
        &[Vec3::ZERO],
    );
    native.update_vehicle(DT, native_scene.queries());
    enabled.update_vehicle(DT, enabled_scene.queries());
    let actual = &enabled.wheels()[0];
    let expected = &native.wheels()[0];
    assert_eq!(actual.raycast_info(), expected.raycast_info());
    assert_eq!(
        actual.wheel_suspension_force,
        expected.wheel_suspension_force
    );
    assert_eq!(actual.forward_impulse, expected.forward_impulse);
    assert_eq!(actual.side_impulse, expected.side_impulse);
    assert_eq!(actual.rotation, expected.rotation);
    assert_scalar(
        enabled.wheels()[0].raycast_info().suspension_length,
        -0.14,
        1e-5,
    );
    assert_eq!(
        enabled_scene.bodies[enabled_scene.chassis].linvel(),
        native_scene.bodies[native_scene.chassis].linvel()
    );
    assert_eq!(
        enabled_scene.bodies[enabled_scene.chassis].angvel(),
        native_scene.bodies[native_scene.chassis].angvel()
    );
}

#[test]
fn deeper_negative_raw_edge_support_wins_even_when_both_lengths_clamp_identically() {
    let position = Vec3::new(0.0, 0.30, -0.2);
    let mut native_scene = Scene::new(position, Some(0.0), true);
    let mut enabled_scene = Scene::new(position, Some(0.0), true);
    let mut native = controller(native_scene.chassis, TireContactMode::Native, &[Vec3::ZERO]);
    let mut enabled = controller(
        enabled_scene.chassis,
        TireContactMode::Envelope,
        &[Vec3::ZERO],
    );
    native.update_vehicle(DT, native_scene.queries());
    enabled.update_vehicle(DT, enabled_scene.queries());
    let ray = native.wheels()[0].raycast_info();
    let envelope = enabled.wheels()[0].raycast_info();
    assert_scalar(ray.suspension_length, -0.14, 1e-5);
    assert_eq!(envelope.suspension_length, ray.suspension_length);
    assert_scalar(ray.contact_point_ws.y, 0.0, 1e-5);
    assert_scalar(envelope.contact_point_ws.y, 0.25, 0.004);
    assert!(envelope.contact_normal_ws.z < -0.2, "{envelope:?}");
    assert!(envelope.contact_normal_ws.y > 0.8, "{envelope:?}");
    assert!(
        enabled_scene.bodies[enabled_scene.chassis]
            .linvel()
            .is_finite()
    );
}

#[test]
fn enabled_envelope_finds_edge_support_without_a_center_ray_hit() {
    let position = Vec3::new(0.0, 0.85, -0.4);
    let mut native_scene = Scene::new(position, None, true);
    let mut enabled_scene = Scene::new(position, None, true);
    let mut native = controller(native_scene.chassis, TireContactMode::Native, &[Vec3::ZERO]);
    let mut enabled = controller(
        enabled_scene.chassis,
        TireContactMode::Envelope,
        &[Vec3::ZERO],
    );
    native.update_vehicle(DT, native_scene.queries());
    enabled.update_vehicle(DT, enabled_scene.queries());
    assert!(!native.wheels()[0].raycast_info().is_in_contact);
    let wheel = &enabled.wheels()[0];
    assert!(wheel.raycast_info().is_in_contact);
    assert!(wheel.raycast_info().ground_object.is_some());
    assert!(wheel.wheel_suspension_force > 0.0);
    assert!(wheel.raycast_info().suspension_length < 0.30);
}

#[test]
fn enabled_step_uses_the_steered_finite_tire_axle() {
    use crate::vehicle::contacts::{MODEL_TIRE_HALF_WIDTH, WheelCast, cast_envelope};
    let position = Vec3::new(0.0, 0.85, -0.4);
    let mut scene = Scene::new(position, Some(0.0), true);
    let axle = Quat::from_rotation_y(0.3) * Vec3::NEG_X;
    let expected = cast_envelope(
        &scene.queries().as_ref(),
        WheelCast {
            mount: position,
            down: Vec3::NEG_Y,
            axle,
            radius: 0.53,
            half_width: MODEL_TIRE_HALF_WIDTH,
            rest_length: 0.36,
        },
    )
    .unwrap();
    assert!(expected.length < 0.30, "{expected:?}");
    let mut enabled = controller(scene.chassis, TireContactMode::Envelope, &[Vec3::ZERO]);
    enabled.wheels_mut()[0].steering = 0.3;
    enabled.update_vehicle(DT, scene.queries());
    let wheel = &enabled.wheels()[0];
    assert!(wheel.axle().distance(axle) < 1e-6);
    assert_scalar(
        wheel.raycast_info().suspension_length,
        expected.length,
        1e-5,
    );
    assert!(
        wheel
            .raycast_info()
            .contact_normal_ws
            .distance(expected.normal)
            < 1e-5
    );
    assert!(
        wheel
            .raycast_info()
            .contact_point_ws
            .distance(expected.point)
            < 1e-5
    );
}

#[test]
fn rejected_overhead_and_side_hits_fall_back_to_exact_native_support() {
    for (center, half_extents) in [
        (Vec3::new(0.0, 1.4, -0.4), Vec3::new(1.0, 0.2, 1.0)),
        (Vec3::new(0.0, 0.8, 0.12), Vec3::new(1.0, 0.8, 0.1)),
    ] {
        let position = Vec3::new(0.0, 0.85, -0.4);
        let mut native_scene = Scene::new(position, Some(0.0), false);
        let mut enabled_scene = Scene::new(position, Some(0.0), false);
        for scene in [&mut native_scene, &mut enabled_scene] {
            scene.colliders.insert(
                ColliderBuilder::cuboid(half_extents.x, half_extents.y, half_extents.z)
                    .translation(center),
            );
            scene.refresh();
        }
        let mut reference = DynamicRayCastVehicleController::new(native_scene.chassis);
        reference.index_forward_axis = 2;
        reference.add_wheel(Vec3::ZERO, Vec3::NEG_Y, Vec3::NEG_X, 0.36, 0.53, &tuning());
        let mut enabled = controller(
            enabled_scene.chassis,
            TireContactMode::Envelope,
            &[Vec3::ZERO],
        );
        reference.update_vehicle(DT, native_scene.queries());
        enabled.update_vehicle(DT, enabled_scene.queries());
        assert_wheel(&enabled.wheels()[0], &reference.wheels()[0], 0.0);
        assert_eq!(
            enabled_scene.bodies[enabled_scene.chassis].linvel(),
            native_scene.bodies[native_scene.chassis].linvel()
        );
        assert_eq!(
            enabled_scene.bodies[enabled_scene.chassis].angvel(),
            native_scene.bodies[native_scene.chassis].angvel()
        );
    }
}

#[test]
fn envelope_width_scales_with_actual_radius_and_does_not_invent_outside_support() {
    for (radius, mount_y, mount_z, inside_x, outside_x) in [
        (0.53, 0.85, -0.3, 3.10, 3.20),
        (0.265, 0.425, -0.15, 3.06, 3.10),
    ] {
        for (x, inside) in [(inside_x, true), (outside_x, false)] {
            let mut scene = Scene::new(Vec3::new(x, mount_y, mount_z), Some(0.0), true);
            let mut enabled = controller(scene.chassis, TireContactMode::Envelope, &[Vec3::ZERO]);
            enabled.wheels_mut()[0].radius = radius;
            enabled.wheels_mut()[0].steering = 0.0;
            enabled.update_vehicle(DT, scene.queries());
            let contact = enabled.wheels()[0].raycast_info();
            assert!(contact.is_in_contact, "{contact:?}");
            if inside {
                assert!(
                    contact.contact_point_ws.y > 0.15,
                    "radius {radius}, x {x}: {contact:?}"
                );
            } else {
                assert_scalar(contact.contact_point_ws.y, 0.0, 1e-5);
                assert_scalar(contact.suspension_length, mount_y - radius, 1e-5);
            }
        }
    }
}

#[test]
fn steering_updates_the_public_axle_snapshot() {
    let mut scene = Scene::new(Vec3::new(0.0, 0.85, -0.4), Some(0.0), false);
    let mut facade = controller(scene.chassis, TireContactMode::Native, &[Vec3::ZERO]);
    facade.wheels_mut()[0].steering = 0.3;
    facade.update_vehicle(DT, scene.queries());
    assert!(
        facade.wheels()[0]
            .axle()
            .distance(Quat::from_rotation_y(0.3) * Vec3::NEG_X)
            < 1e-6
    );
}
