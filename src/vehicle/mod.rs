pub mod config;
pub mod contacts;
pub mod controller;
pub mod diagnostics;
pub mod telemetry;
pub mod visuals;

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
            .init_resource::<VehiclePhysics>()
            .init_resource::<VehicleVisualState>()
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
                controller::apply_configuration.before(PhysicsSet::SyncBackend),
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
                (visuals::update_wheel_motion, telemetry::sample_vehicle)
                    .chain()
                    .after(PhysicsSet::Writeback),
            )
            .add_systems(Update, visuals::animate_wheels.after(crate::controls))
            .add_systems(Update, telemetry::sample_frame);
    }
}
