use super::config::PhysicsModel;
use super::controller::{PHYSICS_DT, Truck, VehiclePhysics};
use super::visuals::VehicleVisualState;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

#[derive(Clone, Copy, Default)]
pub struct WheelTelemetry {
    pub grounded: bool,
    pub compression_m: f32,
    pub suspension_length_m: f32,
    pub normal_load_n: f32,
    pub longitudinal_speed_mps: f32,
    pub lateral_speed_mps: f32,
    pub slip_estimate_mps: f32,
}

#[derive(Resource, Default)]
pub struct VehicleTelemetry {
    pub fps: f32,
    pub frame_ms: f32,
    pub speed_kmh: f32,
    pub longitudinal_acceleration_mps2: f32,
    pub yaw_rate_rad_s: f32,
    pub steering_rad: f32,
    pub pitch_rad: f32,
    pub roll_rad: f32,
    pub grounded_wheels: usize,
    pub engine_force_n: f32,
    pub brake_force_n: f32,
    pub model: PhysicsModel,
    pub wheels: [WheelTelemetry; 4],
    previous_velocity: Option<Vec3>,
}

impl VehicleTelemetry {
    pub fn reset_motion_history(&mut self) {
        self.previous_velocity = None;
        self.longitudinal_acceleration_mps2 = 0.0;
    }
}

pub fn sample_frame(time: Res<Time>, mut telemetry: ResMut<VehicleTelemetry>) {
    let dt = time.delta_secs();
    if dt > 0.0 {
        telemetry.frame_ms = dt * 1_000.0;
        let fps = 1.0 / dt;
        telemetry.fps = if telemetry.fps == 0.0 {
            fps
        } else {
            telemetry.fps + (fps - telemetry.fps) * 0.08
        };
    }
}

pub fn sample_vehicle(
    physics: Res<VehiclePhysics>,
    visual: Res<VehicleVisualState>,
    mut telemetry: ResMut<VehicleTelemetry>,
    cars: Query<(&Transform, &Velocity), With<Truck>>,
) {
    let Ok((car, velocity)) = cars.single() else {
        return;
    };
    let forward = car.rotation * Vec3::Z;
    let com = car.translation
        + car.rotation * Vec3::from_array(physics.active_config.chassis.center_of_mass);
    telemetry.speed_kmh = velocity.linear.length() * 3.6;
    telemetry.longitudinal_acceleration_mps2 = telemetry
        .previous_velocity
        .map(|previous| (velocity.linear - previous).dot(forward) / PHYSICS_DT)
        .unwrap_or(0.0);
    telemetry.previous_velocity = Some(velocity.linear);
    telemetry.yaw_rate_rad_s = velocity.angular.dot(car.rotation * Vec3::Y);
    let (_, pitch, roll) = car.rotation.to_euler(EulerRot::YXZ);
    telemetry.pitch_rad = pitch;
    telemetry.roll_rad = roll;
    telemetry.steering_rad = visual.steer;
    telemetry.grounded_wheels = physics.wheels.iter().filter(|wheel| wheel.grounded).count();
    telemetry.engine_force_n = physics.engine_force;
    telemetry.brake_force_n = physics.brake_force;
    telemetry.model = physics.active_config.model;
    for (i, contact) in physics.wheels.iter().enumerate() {
        let axle = car.rotation * Quat::from_rotation_y(contact.steer) * Vec3::NEG_X;
        let heading = contact.normal.cross(axle).normalize_or_zero();
        let side = heading.cross(contact.normal).normalize_or_zero();
        let relative_velocity =
            velocity.linear + velocity.angular.cross(contact.point - com) - contact.ground_velocity;
        let suspension = if i < 2 {
            &physics.active_config.front
        } else {
            &physics.active_config.rear
        };
        let lateral_speed = relative_velocity.dot(side);
        telemetry.wheels[i] = WheelTelemetry {
            grounded: contact.grounded,
            compression_m: (suspension.rest_length_m - contact.suspension_length).max(0.0),
            suspension_length_m: contact.suspension_length,
            normal_load_n: contact.normal_load,
            longitudinal_speed_mps: relative_velocity.dot(heading),
            lateral_speed_mps: lateral_speed,
            slip_estimate_mps: lateral_speed.abs(),
        };
    }
}
