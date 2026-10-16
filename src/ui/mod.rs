//! Realtime physics controls; the application installs `EguiPlugin::default()`.

pub mod input_capture;
pub mod physics_tuning;
pub mod terrain_workshop;
mod terrain_catalogue;

pub use physics_tuning::{PhysicsTuningPlugin, UiInputCapture};
