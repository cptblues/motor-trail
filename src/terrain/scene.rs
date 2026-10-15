//! Version 1 stores every height sample; loading never regenerates edited terrain.
use bevy::math::{Quat, Vec3};
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::config::{TerrainConfig, TerrainPreset};
use super::data::TerrainData;
use super::generation::{GENERATOR_VERSION, generate};
use super::legacy;
use super::obstacles::{Obstacle, ObstacleKind};

pub const FORMAT: &str = "motor-trail-terrain";
pub const FORMAT_VERSION: u32 = 1;
pub const MAX_OBSTACLES: usize = 512;
pub const SPAWN_HALF_WIDTH: f32 = 1.3;
pub const SPAWN_HALF_DEPTH: f32 = 2.4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleSpawn {
    /// Ground reference in world metres. Runtime adds the vehicle's clearance.
    pub position: [f32; 3],
    /// Zero points towards +Z.
    pub yaw_degrees: f32,
}

impl VehicleSpawn {
    pub fn validate(&self, terrain: &TerrainData) -> Result<(), String> {
        if !self
            .position
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 512.0)
            || !self.yaw_degrees.is_finite()
        {
            return Err("Vehicle spawn must be finite and its position within +/-512 m".into());
        }
        if terrain.height(self.position[0], self.position[2]).is_none() {
            return Err("Vehicle spawn lies outside the terrain domain".into());
        }
        let rotation = Quat::from_rotation_y((self.yaw_degrees % 360.0).to_radians());
        for x in [-SPAWN_HALF_WIDTH, SPAWN_HALF_WIDTH] {
            for z in [-SPAWN_HALF_DEPTH, SPAWN_HALF_DEPTH] {
                let corner = Vec3::from_array(self.position) + rotation * Vec3::new(x, 0.0, z);
                if corner.x.abs() > terrain.config.width * 0.5 - 0.05
                    || corner.z.abs() > terrain.config.depth * 0.5 - 0.05
                {
                    return Err(
                        "Vehicle spawn needs clearance for the entire chassis inside the terrain"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainScene {
    pub format: String,
    pub format_version: u32,
    pub generator_version: u32,
    pub name: String,
    pub terrain: TerrainData,
    pub obstacles: Vec<Obstacle>,
    pub spawn: VehicleSpawn,
}

impl TerrainScene {
    pub fn generate(name: &str, config: TerrainConfig) -> Result<Self, String> {
        validate_name(name)?;
        let terrain = generate(&config)?;
        let z = (-20.0_f32).max(-config.depth * 0.5 + SPAWN_HALF_DEPTH + 0.8);
        let y = terrain
            .height(0.0, z)
            .ok_or("Cannot sample generated spawn")?;
        let scene = Self {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            generator_version: GENERATOR_VERSION,
            name: name.into(),
            terrain,
            obstacles: Vec::new(),
            spawn: VehicleSpawn {
                position: [0.0, y, z],
                yaw_degrees: 0.0,
            },
        };
        scene.validate()?;
        Ok(scene)
    }

    pub fn reference(reference: ReferenceScene) -> Result<Self, String> {
        if reference == ReferenceScene::Historical {
            let mut scene = Self::generate(
                reference.name(),
                TerrainConfig::preset(TerrainPreset::Legacy),
            )?;
            let x = legacy::road_x(legacy::START);
            scene.spawn.position = [x, legacy::height(x, legacy::START), legacy::START];
            scene.validate()?;
            return Ok(scene);
        }
        let mut config = TerrainConfig::preset(TerrainPreset::Flat);
        config.width = 64.0;
        config.depth = 64.0;
        config.resolution = if reference == ReferenceScene::Ruts {
            0.25
        } else {
            0.5
        };
        config.amplitude = 0.0;
        config.roughness = 0.0;
        let mut scene = Self::generate(reference.name(), config)?;
        scene.terrain.heights.fill(0.0);
        let terrain = &mut scene.terrain;
        for iz in 0..=terrain.cells_z {
            for ix in 0..=terrain.cells_x {
                let x = (ix as f32 / terrain.cells_x as f32 - 0.5) * terrain.config.width;
                let z = (iz as f32 / terrain.cells_z as f32 - 0.5) * terrain.config.depth;
                let height = match reference {
                    ReferenceScene::Hill => {
                        let lane = ((8.0 - x.abs()) / 4.0).clamp(0.0, 1.0);
                        (1.0 - ((z - 4.0) / 16.0).abs()).max(0.0) * 8.0 * lane
                    }
                    ReferenceScene::SideSlope => {
                        let lane = ((10.0 - x.abs()) / 4.0).clamp(0.0, 1.0);
                        x.clamp(-6.0, 6.0)
                            * 0.25
                            * lane
                            * course_envelope(z, -12.0, -8.0, 12.0, 16.0)
                    }
                    ReferenceScene::Ruts => {
                        let groove = (1.0 - (x.abs() - 1.0).abs() / 0.5).max(0.0);
                        -0.45 * groove * course_envelope(z, -12.0, -8.0, 16.0, 20.0)
                    }
                    _ => 0.0,
                };
                terrain.heights[iz * (terrain.cells_x + 1) + ix] = height;
            }
        }
        match reference {
            ReferenceScene::Articulation => {
                for i in 0..6 {
                    let x = if i % 2 == 0 { -0.95 } else { 0.95 };
                    let z = -8.0 + i as f32 * 3.5;
                    let h = 0.4 + (i / 2) as f32 * 0.15;
                    scene.add_obstacle(
                        ObstacleKind::Ramp,
                        [x, h / 2.0, z],
                        [0.8, h, 2.4],
                        if i % 2 == 0 { 0.0 } else { 180.0 },
                    );
                }
            }
            ReferenceScene::Steps => {
                for (i, height) in [0.2, 0.4, 0.6, 0.8].into_iter().enumerate() {
                    scene.add_obstacle(
                        ObstacleKind::Cube,
                        [0.0, height / 2.0, -6.0 + i as f32 * 4.0],
                        [4.0, height, 4.0],
                        0.0,
                    );
                }
            }
            ReferenceScene::Rocks => {
                for (position, size, yaw) in [
                    ([-0.9, 0.25, -8.0], [1.2, 0.5, 1.4], 15.0),
                    ([0.8, 0.35, -4.0], [1.5, 0.7, 1.8], -20.0),
                    ([-0.7, 0.45, 0.0], [1.4, 0.9, 2.0], 30.0),
                    ([1.0, 0.30, 4.0], [1.8, 0.6, 1.4], -15.0),
                    ([-0.9, 0.50, 9.0], [1.6, 1.0, 1.8], 35.0),
                    ([0.7, 0.40, 13.0], [1.4, 0.8, 1.6], 10.0),
                ] {
                    scene.add_obstacle(ObstacleKind::Rock, position, size, yaw);
                }
            }
            _ => {}
        }
        scene.spawn = VehicleSpawn {
            position: [0.0, 0.0, -20.0],
            yaw_degrees: 0.0,
        };
        scene.validate()?;
        Ok(scene)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT || self.format_version != FORMAT_VERSION {
            return Err(format!(
                "Unsupported terrain format {:?} version {}",
                self.format, self.format_version
            ));
        }
        if self.generator_version != GENERATOR_VERSION {
            return Err(format!(
                "Unsupported terrain generator version {}",
                self.generator_version
            ));
        }
        validate_name(&self.name)?;
        self.terrain.validate()?;
        self.spawn.validate(&self.terrain)?;
        validate_obstacles(&self.obstacles)
    }

    pub fn next_id(&self) -> u64 {
        if let Some(next) = self
            .obstacles
            .iter()
            .map(|o| o.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        {
            return next;
        }
        // At most 512 live objects: a free positive ID always exists, even after u64::MAX.
        (1..=self.obstacles.len() as u64 + 1)
            .find(|id| self.obstacles.iter().all(|o| o.id != *id))
            .unwrap_or(1)
    }

    fn add_obstacle(&mut self, kind: ObstacleKind, position: [f32; 3], size: [f32; 3], yaw: f32) {
        self.obstacles.push(Obstacle {
            id: self.next_id(),
            kind,
            position,
            rotation_degrees: [0.0, yaw, 0.0],
            size,
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceScene {
    Historical,
    Flat,
    Hill,
    SideSlope,
    Articulation,
    Steps,
    Rocks,
    Ruts,
}

impl ReferenceScene {
    pub const ALL: [Self; 8] = [
        Self::Historical,
        Self::Flat,
        Self::Hill,
        Self::SideSlope,
        Self::Articulation,
        Self::Steps,
        Self::Rocks,
        Self::Ruts,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Historical => "Vallee historique",
            Self::Flat => "Terrain plat",
            Self::Hill => "Montee et descente",
            Self::SideSlope => "Devers",
            Self::Articulation => "Croisement de ponts",
            Self::Steps => "Marches successives",
            Self::Rocks => "Rochers",
            Self::Ruts => "Ornieres",
        }
    }
}

pub(super) fn validate_obstacles(obstacles: &[Obstacle]) -> Result<(), String> {
    if obstacles.len() > MAX_OBSTACLES {
        return Err(format!(
            "A terrain scene supports at most {MAX_OBSTACLES} obstacles"
        ));
    }
    let mut ids = HashSet::with_capacity(obstacles.len());
    for obstacle in obstacles {
        obstacle.validate()?;
        if !ids.insert(obstacle.id) {
            return Err(format!("Duplicate obstacle ID {}", obstacle.id));
        }
    }
    Ok(())
}

pub(super) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || name.trim() != name
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_'))
    {
        return Err("Terrain name must contain 1..=64 ASCII letters, digits, spaces, '-' or '_', without leading/trailing spaces".into());
    }
    let upper = name.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) || (upper.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper.as_bytes()[3].is_ascii_digit())
    {
        return Err(format!(
            "Terrain name {name:?} is a reserved Windows filename"
        ));
    }
    Ok(())
}

fn course_envelope(z: f32, start: f32, plateau_start: f32, plateau_end: f32, end: f32) -> f32 {
    ((z - start) / (plateau_start - start))
        .clamp(0.0, 1.0)
        .min(((end - z) / (end - plateau_end)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_equipment_runtime_scene_schema_stays_independent_of_installed_loadout() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        scene.spawn.position = [29.0, 0.0, 0.0];
        scene.validate().unwrap();
        let config = crate::vehicle::config::VehiclePhysicsConfig::baseline();
        let base_bounds = crate::vehicle::collision::body_bounds();
        super::super::runtime::checked_spawn_transform(&scene, &config, base_bounds).unwrap();
        let extended_bounds = (base_bounds.0, base_bounds.1.max(Vec3::new(4.3, 0.0, 0.0)));
        assert!(
            super::super::runtime::checked_spawn_transform(&scene, &config, extended_bounds)
                .is_err()
        );
        let json = serde_json::to_value(&scene).unwrap();
        assert_eq!(json["format_version"], FORMAT_VERSION);
        assert!(json.get("equipment").is_none());
        assert!(json.get("loadout").is_none());
        let restored: TerrainScene = serde_json::from_value(json).unwrap();
        assert_eq!(restored, scene);
        restored.validate().unwrap();
    }

    #[test]
    fn small_generated_scenes_keep_the_whole_spawn_footprint_on_the_map() {
        for preset in [
            TerrainPreset::Flat,
            TerrainPreset::Valley,
            TerrainPreset::Mountain,
            TerrainPreset::Trial,
        ] {
            let config = TerrainConfig {
                width: 8.0,
                depth: 8.0,
                resolution: 1.0,
                ..TerrainConfig::preset(preset)
            };
            let scene = TerrainScene::generate("Small safe scene", config).unwrap();
            assert!(scene.spawn.validate(&scene.terrain).is_ok());
            assert!(scene.spawn.position[2] - SPAWN_HALF_DEPTH > -4.0);
        }
    }

    #[test]
    fn spawn_validation_rejects_rotated_chassis_overhang() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        scene.spawn.position = [30.0, 0.0, 0.0];
        scene.spawn.yaw_degrees = 0.0;
        assert!(scene.validate().is_ok());
        scene.spawn.yaw_degrees = 45.0;
        assert!(scene.validate().is_err());
        scene.spawn.position[0] = 31.0;
        scene.spawn.yaw_degrees = 0.0;
        assert!(scene.validate().is_err());
    }

    #[test]
    fn references_are_deterministic_valid_and_start_on_clear_ground() {
        for reference in ReferenceScene::ALL {
            let scene = TerrainScene::reference(reference).unwrap();
            scene.validate().unwrap();
            assert_eq!(scene, TerrainScene::reference(reference).unwrap());
            if reference != ReferenceScene::Historical {
                assert_eq!(scene.spawn.position, [0.0, 0.0, -20.0]);
                assert_eq!(scene.spawn.yaw_degrees, 0.0);
                assert!(
                    scene
                        .obstacles
                        .iter()
                        .all(|o| o.position[2] - o.size[2] / 2.0 > -15.0)
                );
            }
        }
    }

    #[test]
    fn historical_spawn_matches_original_road_and_height() {
        let scene = TerrainScene::reference(ReferenceScene::Historical).unwrap();
        let x = legacy::road_x(legacy::START);
        assert_eq!(
            scene.spawn.position,
            [x, legacy::height(x, legacy::START), legacy::START]
        );
        assert_eq!(scene.terrain.config.preset, TerrainPreset::Legacy);
    }

    #[test]
    fn courses_have_explicit_hill_slope_steps_and_ruts() {
        let hill = TerrainScene::reference(ReferenceScene::Hill).unwrap();
        assert_eq!(hill.terrain.height(0.0, -12.0), Some(0.0));
        assert_eq!(hill.terrain.height(0.0, 4.0), Some(8.0));
        assert_eq!(hill.terrain.height(0.0, 20.0), Some(0.0));
        let slope = TerrainScene::reference(ReferenceScene::SideSlope).unwrap();
        assert!(slope.terrain.height(2.0, 0.0).unwrap() > slope.terrain.height(-2.0, 0.0).unwrap());
        let steps = TerrainScene::reference(ReferenceScene::Steps).unwrap();
        assert_eq!(
            steps
                .obstacles
                .iter()
                .map(|o| o.size[1])
                .collect::<Vec<_>>(),
            vec![0.2, 0.4, 0.6, 0.8]
        );
        let ruts = TerrainScene::reference(ReferenceScene::Ruts).unwrap();
        assert!(ruts.terrain.height(1.0, 0.0).unwrap() < -0.3);
        assert_eq!(ruts.terrain.height(0.0, 0.0), Some(0.0));
    }

    #[test]
    fn document_rejects_bad_versions_duplicate_ids_spawn_and_unknown_fields() {
        let mut scene = TerrainScene::reference(ReferenceScene::Rocks).unwrap();
        scene.obstacles.push(scene.obstacles[0].clone());
        assert!(scene.validate().is_err());
        scene.obstacles.pop();
        scene.spawn.position[0] = 500.0;
        assert!(scene.validate().is_err());
        scene.spawn.position[0] = 0.0;
        scene.generator_version += 1;
        assert!(scene.validate().is_err());
        scene.generator_version = GENERATOR_VERSION;
        scene.format_version += 1;
        assert!(scene.validate().is_err());
        scene.format_version = FORMAT_VERSION;
        scene.format = "other".into();
        assert!(scene.validate().is_err());
        let mut json =
            serde_json::to_value(TerrainScene::reference(ReferenceScene::Flat).unwrap()).unwrap();
        json["unexpected"] = true.into();
        assert!(serde_json::from_value::<TerrainScene>(json).is_err());
    }

    #[test]
    fn id_allocation_does_not_depend_on_object_order_or_overflow() {
        let mut scene = TerrainScene::reference(ReferenceScene::Rocks).unwrap();
        let expected = scene.next_id();
        scene.obstacles.reverse();
        assert_eq!(scene.next_id(), expected);
        scene.obstacles[0].id = u64::MAX;
        assert!(scene.next_id() > 0);
        assert!(!scene.obstacles.iter().any(|o| o.id == scene.next_id()));
    }

    #[test]
    fn names_are_path_safe_and_windows_portable() {
        for name in [
            "",
            " ",
            "../escape",
            "CON",
            "com1",
            "NUL",
            "a.json",
            "a/b",
            "a\\b",
            " leading",
            "trailing ",
        ] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        assert!(validate_name("Exemple Plat_2-Variante").is_ok());
    }

    #[test]
    fn validates_height_snapshot_spawn_numbers_and_object_budget() {
        let mut scene = TerrainScene::reference(ReferenceScene::Rocks).unwrap();
        let original = scene.clone();
        scene.terrain.heights[0] = f32::NAN;
        assert!(scene.validate().is_err());
        scene = original.clone();
        scene.terrain.cells_x += 1;
        assert!(scene.validate().is_err());
        scene = original.clone();
        scene.spawn.position[1] = f32::INFINITY;
        assert!(scene.validate().is_err());
        scene = original.clone();
        scene.spawn.yaw_degrees = f32::NAN;
        assert!(scene.validate().is_err());
        scene = original;
        let object = scene.obstacles[0].clone();
        scene.obstacles = (1..=MAX_OBSTACLES as u64)
            .map(|id| {
                let mut obstacle = object.clone();
                obstacle.id = id;
                obstacle
            })
            .collect();
        scene.validate().unwrap();
        let mut extra = object;
        extra.id = MAX_OBSTACLES as u64 + 1;
        scene.obstacles.push(extra);
        assert!(scene.validate().is_err());
    }
}
