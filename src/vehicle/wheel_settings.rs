use super::wheel_backend::TireContactMode;
use bevy::prelude::*;

/// Session-only experiment; intentionally absent from saved vehicle profiles.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq)]
pub struct WheelContactSettings {
    pub experimental: bool,
}

impl WheelContactSettings {
    pub fn mode(&self) -> TireContactMode {
        if self.experimental {
            TireContactMode::Envelope
        } else {
            TireContactMode::Native
        }
    }
}
