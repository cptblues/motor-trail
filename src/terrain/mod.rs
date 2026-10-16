//! Canonical terrain, editing tools, scene storage and the Bevy/Rapier bridge.
pub mod brush;
pub mod config;
pub mod data;
pub mod generation;
pub mod history;
pub mod legacy;
pub mod mesh;
pub mod nature;
pub mod obstacles;
pub mod persistence;
pub mod runtime;
pub mod scene;

pub use runtime::TerrainPlugin;
