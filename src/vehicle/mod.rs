pub mod body_probe;
pub mod collision;
pub mod config;
pub mod contacts;
pub mod controller;
pub mod diagnostics;
pub mod equipment;
pub mod equipment_runtime;
mod experimental_tires;
pub mod mounting;
pub mod presentation;
pub mod telemetry;
pub mod visuals;
pub mod wheel_backend;
pub mod wheel_settings;

use crate::ui::physics_tuning::UiInputCapture;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use controller::{PHYSICS_DT, PHYSICS_HZ};
pub use controller::{Truck, VehiclePhysics};
pub use visuals::VehicleVisualState;

pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<config::VehiclePhysicsConfig>()
            .init_resource::<equipment_runtime::EquipmentRequest>()
            .init_resource::<equipment_runtime::EquipmentRuntime>()
            .init_resource::<wheel_settings::WheelContactSettings>()
            .init_resource::<VehiclePhysics>()
            .init_resource::<VehicleVisualState>()
            .init_resource::<presentation::VehiclePresentation>()
            .init_resource::<telemetry::VehicleTelemetry>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<UiInputCapture>()
            .insert_resource(Time::<Fixed>::from_hz(PHYSICS_HZ))
            .insert_resource(TimestepMode::Fixed {
                dt: PHYSICS_DT,
                substeps: 1,
            })
            .add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule())
            .add_systems(
                FixedUpdate,
                (equipment_runtime::sync, controller::apply_configuration)
                    .chain()
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(
                FixedUpdate,
                controller::suspension_and_drive
                    .run_if(crate::terrain::runtime::driving_enabled)
                    .after(PhysicsSet::SyncBackend)
                    .before(PhysicsSet::StepSimulation),
            )
            .add_systems(
                FixedUpdate,
                presentation::begin_step
                    .after(controller::apply_configuration)
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(
                FixedUpdate,
                (
                    visuals::update_wheel_motion,
                    presentation::finish_step,
                    telemetry::sample_vehicle,
                )
                    .chain()
                    .after(PhysicsSet::Writeback),
            )
            .add_systems(
                Update,
                (presentation::present, visuals::animate_wheels)
                    .chain()
                    .after(crate::controls)
                    .before(crate::camera_follow),
            )
            .add_systems(Update, telemetry::sample_frame);
        if std::env::args_os().any(|arg| arg == "--experimental-tires") {
            app.insert_resource(wheel_settings::WheelContactSettings { experimental: true });
        }
        if std::env::args_os().any(|arg| arg == "--equipment-demo") {
            app.insert_resource(equipment_runtime::EquipmentRequest::demo());
        }
    }
}
