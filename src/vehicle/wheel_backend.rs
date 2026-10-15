//! Opt-in wheel-controller facade; Native delegates to the pinned upstream Rapier.
//!
//! Wheel field names and contact snapshots follow rapier3d 0.33.0-alpha's
//! src/control/ray_cast_vehicle_controller.rs (Apache-2.0; see
//! third_party/rapier-LICENSE). Rapier does not re-export RayCastInfo from its
//! private module, so the facade must expose a field-compatible local snapshot.
use bevy::prelude::Vec3;
use bevy_rapier3d::rapier::{
    control::{DynamicRayCastVehicleController, Wheel as NativeWheel, WheelTuning},
    dynamics::RigidBodyHandle,
    geometry::ColliderHandle,
    pipeline::QueryPipelineMut,
};

use super::experimental_tires::ExperimentalTires;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TireContactMode {
    #[default]
    Native,
    Envelope,
}

/// Source-compatible snapshot of Rapier's otherwise unnameable contact type.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RayCastInfo {
    pub contact_normal_ws: Vec3,
    pub contact_point_ws: Vec3,
    pub suspension_length: f32,
    pub hard_point_ws: Vec3,
    pub is_in_contact: bool,
    pub ground_object: Option<ColliderHandle>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wheel {
    pub chassis_connection_point_cs: Vec3,
    pub direction_cs: Vec3,
    pub axle_cs: Vec3,
    pub suspension_rest_length: f32,
    pub max_suspension_travel: f32,
    pub radius: f32,
    pub suspension_stiffness: f32,
    pub damping_compression: f32,
    pub damping_relaxation: f32,
    pub friction_slip: f32,
    pub side_friction_stiffness: f32,
    pub rotation: f32,
    pub max_suspension_force: f32,
    pub forward_impulse: f32,
    pub side_impulse: f32,
    pub steering: f32,
    pub engine_force: f32,
    pub brake: f32,
    pub wheel_suspension_force: f32,
    pub(super) raycast_info: RayCastInfo,
    pub(super) center: Vec3,
    pub(super) wheel_direction_ws: Vec3,
    pub(super) wheel_axle_ws: Vec3,
    pub(super) delta_rotation: f32,
    pub(super) roll_influence: f32,
    pub(super) clipped_inv_contact_dot_suspension: f32,
    pub(super) suspension_relative_velocity: f32,
    pub(super) skid_info: f32,
}

impl Wheel {
    fn new(
        chassis_connection_cs: Vec3,
        direction_cs: Vec3,
        axle_cs: Vec3,
        suspension_rest_length: f32,
        radius: f32,
        tuning: &WheelTuning,
    ) -> Self {
        Self {
            chassis_connection_point_cs: chassis_connection_cs,
            direction_cs,
            axle_cs,
            suspension_rest_length,
            max_suspension_travel: tuning.max_suspension_travel,
            radius,
            suspension_stiffness: tuning.suspension_stiffness,
            damping_compression: tuning.suspension_compression,
            damping_relaxation: tuning.suspension_damping,
            friction_slip: tuning.friction_slip,
            side_friction_stiffness: tuning.side_friction_stiffness,
            rotation: 0.0,
            max_suspension_force: tuning.max_suspension_force,
            forward_impulse: 0.0,
            side_impulse: 0.0,
            steering: 0.0,
            engine_force: 0.0,
            brake: 0.0,
            wheel_suspension_force: 0.0,
            raycast_info: RayCastInfo::default(),
            center: Vec3::ZERO,
            wheel_direction_ws: direction_cs,
            wheel_axle_ws: axle_cs,
            delta_rotation: 0.0,
            roll_influence: 0.1,
            clipped_inv_contact_dot_suspension: 0.0,
            suspension_relative_velocity: 0.0,
            skid_info: 0.0,
        }
    }

    pub fn raycast_info(&self) -> &RayCastInfo {
        &self.raycast_info
    }

    pub fn center(&self) -> Vec3 {
        self.center
    }

    pub fn suspension(&self) -> Vec3 {
        self.wheel_direction_ws
    }

    pub fn axle(&self) -> Vec3 {
        self.wheel_axle_ws
    }

    fn from_native(wheel: &NativeWheel) -> Self {
        let contact = wheel.raycast_info();
        Self {
            chassis_connection_point_cs: wheel.chassis_connection_point_cs,
            direction_cs: wheel.direction_cs,
            axle_cs: wheel.axle_cs,
            suspension_rest_length: wheel.suspension_rest_length,
            max_suspension_travel: wheel.max_suspension_travel,
            radius: wheel.radius,
            suspension_stiffness: wheel.suspension_stiffness,
            damping_compression: wheel.damping_compression,
            damping_relaxation: wheel.damping_relaxation,
            friction_slip: wheel.friction_slip,
            side_friction_stiffness: wheel.side_friction_stiffness,
            rotation: wheel.rotation,
            max_suspension_force: wheel.max_suspension_force,
            forward_impulse: wheel.forward_impulse,
            side_impulse: wheel.side_impulse,
            steering: wheel.steering,
            engine_force: wheel.engine_force,
            brake: wheel.brake,
            wheel_suspension_force: wheel.wheel_suspension_force,
            raycast_info: RayCastInfo {
                contact_normal_ws: contact.contact_normal_ws,
                contact_point_ws: contact.contact_point_ws,
                suspension_length: contact.suspension_length,
                hard_point_ws: contact.hard_point_ws,
                is_in_contact: contact.is_in_contact,
                ground_object: contact.ground_object,
            },
            center: wheel.center(),
            wheel_direction_ws: wheel.suspension(),
            wheel_axle_ws: wheel.axle(),
            delta_rotation: 0.0,
            roll_influence: 0.1,
            clipped_inv_contact_dot_suspension: 0.0,
            suspension_relative_velocity: 0.0,
            skid_info: 0.0,
        }
    }

    fn copy_to_native(&self, wheel: &mut NativeWheel) {
        wheel.chassis_connection_point_cs = self.chassis_connection_point_cs;
        wheel.direction_cs = self.direction_cs;
        wheel.axle_cs = self.axle_cs;
        wheel.suspension_rest_length = self.suspension_rest_length;
        wheel.max_suspension_travel = self.max_suspension_travel;
        wheel.radius = self.radius;
        wheel.suspension_stiffness = self.suspension_stiffness;
        wheel.damping_compression = self.damping_compression;
        wheel.damping_relaxation = self.damping_relaxation;
        wheel.friction_slip = self.friction_slip;
        wheel.side_friction_stiffness = self.side_friction_stiffness;
        wheel.rotation = self.rotation;
        wheel.max_suspension_force = self.max_suspension_force;
        wheel.forward_impulse = self.forward_impulse;
        wheel.side_impulse = self.side_impulse;
        wheel.steering = self.steering;
        wheel.engine_force = self.engine_force;
        wheel.brake = self.brake;
        wheel.wheel_suspension_force = self.wheel_suspension_force;
    }
}

#[derive(Clone, Debug)]
enum Backend {
    Native(DynamicRayCastVehicleController),
    Envelope(ExperimentalTires),
}

/// Shared tuning and output cache. Select a mode when creating the controller.
#[derive(Clone, Debug)]
pub struct WheelController {
    pub chassis: RigidBodyHandle,
    pub index_forward_axis: usize,
    pub index_up_axis: usize,
    pub current_vehicle_speed: f32,
    mode: TireContactMode,
    wheels: Vec<Wheel>,
    backend: Backend,
}

impl WheelController {
    pub fn new(chassis: RigidBodyHandle, mode: TireContactMode) -> Self {
        Self {
            chassis,
            index_forward_axis: 0,
            index_up_axis: 1,
            current_vehicle_speed: 0.0,
            mode,
            wheels: Vec::with_capacity(4),
            backend: match mode {
                TireContactMode::Native => {
                    Backend::Native(DynamicRayCastVehicleController::new(chassis))
                }
                TireContactMode::Envelope => Backend::Envelope(ExperimentalTires::new(chassis)),
            },
        }
    }

    pub fn contact_mode(&self) -> TireContactMode {
        self.mode
    }

    pub fn add_wheel(
        &mut self,
        chassis_connection_cs: Vec3,
        direction_cs: Vec3,
        axle_cs: Vec3,
        suspension_rest_length: f32,
        radius: f32,
        tuning: &WheelTuning,
    ) -> &mut Wheel {
        let wheel = match &mut self.backend {
            Backend::Native(native) => Wheel::from_native(native.add_wheel(
                chassis_connection_cs,
                direction_cs,
                axle_cs,
                suspension_rest_length,
                radius,
                tuning,
            )),
            Backend::Envelope(envelope) => {
                let wheel = Wheel::new(
                    chassis_connection_cs,
                    direction_cs,
                    axle_cs,
                    suspension_rest_length,
                    radius,
                    tuning,
                );
                envelope.add_wheel(wheel);
                wheel
            }
        };
        self.wheels.push(wheel);
        self.wheels.last_mut().unwrap()
    }

    pub fn wheels(&self) -> &[Wheel] {
        &self.wheels
    }

    pub fn wheels_mut(&mut self) -> &mut [Wheel] {
        &mut self.wheels
    }

    pub fn update_vehicle(&mut self, dt: f32, queries: QueryPipelineMut<'_>) {
        match &mut self.backend {
            Backend::Native(native) => {
                native.chassis = self.chassis;
                native.index_forward_axis = self.index_forward_axis;
                native.index_up_axis = self.index_up_axis;
                for (cached, wheel) in self.wheels.iter().zip(native.wheels_mut()) {
                    cached.copy_to_native(wheel);
                }
                native.update_vehicle(dt, queries);
                self.current_vehicle_speed = native.current_vehicle_speed;
                for (cached, wheel) in self.wheels.iter_mut().zip(native.wheels()) {
                    *cached = Wheel::from_native(wheel);
                }
            }
            Backend::Envelope(envelope) => {
                envelope.chassis = self.chassis;
                envelope.index_forward_axis = self.index_forward_axis;
                envelope.index_up_axis = self.index_up_axis;
                envelope.wheels.copy_from_slice(&self.wheels);
                envelope.update_vehicle(dt, queries);
                self.current_vehicle_speed = envelope.current_vehicle_speed;
                self.wheels.copy_from_slice(&envelope.wheels);
            }
        }
    }
}

#[cfg(test)]
mod tests;
