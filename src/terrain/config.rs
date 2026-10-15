use serde::{Deserialize, Serialize};

pub const MAX_VERTICES: usize = 66_049;
pub const MAX_HEIGHT: f32 = 512.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerrainPreset {
    Flat,
    Valley,
    Mountain,
    Trial,
    Legacy,
}

impl TerrainPreset {
    pub const ALL: [Self; 5] = [
        Self::Flat,
        Self::Valley,
        Self::Mountain,
        Self::Trial,
        Self::Legacy,
    ];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainConfig {
    pub preset: TerrainPreset,
    pub seed: u64,
    pub width: f32,
    pub depth: f32,
    /// Maximum metres per cell; actual spacing is dimension / ceil(dimension / resolution).
    pub resolution: f32,
    pub amplitude: f32,
    pub frequency: f32,
    pub octaves: u32,
    pub persistence: f32,
    pub lacunarity: f32,
    pub roughness: f32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self::preset(TerrainPreset::Valley)
    }
}

impl TerrainConfig {
    pub fn preset(preset: TerrainPreset) -> Self {
        let mut config = Self {
            preset,
            seed: 1,
            width: 64.0,
            depth: 64.0,
            resolution: 0.5,
            amplitude: 12.0,
            frequency: 0.035,
            octaves: 5,
            persistence: 0.5,
            lacunarity: 2.0,
            roughness: 0.4,
        };
        match preset {
            TerrainPreset::Flat => {
                config.amplitude = 0.2;
                config.octaves = 3;
                config.roughness = 0.2;
            }
            TerrainPreset::Valley => {}
            TerrainPreset::Mountain => {
                config.amplitude = 40.0;
                config.frequency = 0.045;
                config.octaves = 6;
                config.roughness = 0.8;
            }
            TerrainPreset::Trial => {
                config.amplitude = 3.0;
                config.frequency = 0.1;
                config.octaves = 4;
                config.roughness = 0.55;
            }
            TerrainPreset::Legacy => {
                config.width = 120.0;
                config.depth = 165.0;
                config.resolution = 1.25;
            }
        }
        config
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.preset == TerrainPreset::Legacy
            && (self.width != 120.0 || self.depth != 165.0 || self.resolution != 1.25)
        {
            return Err("The historical valley preserves its 120 x 165 m, 1.25 m grid".into());
        }
        for (name, value, min, max) in [
            ("width", self.width, 8.0, 256.0),
            ("depth", self.depth, 8.0, 256.0),
            ("resolution", self.resolution, 0.25, 4.0),
            ("amplitude", self.amplitude, 0.0, 256.0),
            ("frequency", self.frequency, 0.0001, 4.0),
            ("persistence", self.persistence, 0.0, 1.0),
            ("lacunarity", self.lacunarity, 1.0, 4.0),
            ("roughness", self.roughness, 0.0, 2.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!(
                    "Terrain {name} must be finite and between {min} and {max}"
                ));
            }
        }
        if !(1..=8).contains(&self.octaves) {
            return Err("Terrain octaves must be between 1 and 8".into());
        }
        let (nx, nz) = self.cells();
        let vertices = nx
            .checked_add(1)
            .and_then(|x| nz.checked_add(1).and_then(|z| x.checked_mul(z)));
        match vertices {
            Some(count) if count <= MAX_VERTICES => Ok(()),
            _ => Err(format!(
                "Terrain exceeds the limit of {MAX_VERTICES} vertices; increase resolution"
            )),
        }
    }

    /// Call validate before using these counts to allocate. Invalid inputs are never allocated here.
    pub fn cells(&self) -> (usize, usize) {
        // f64 avoids rounding a non-integral f32 ratio down to an integer before ceil.
        (
            (self.width as f64 / self.resolution as f64).ceil() as usize,
            (self.depth as f64 / self.resolution as f64).ceil() as usize,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_a_bounded_64_metre_valley() {
        let config = TerrainConfig::default();
        assert_eq!(config.preset, TerrainPreset::Valley);
        assert_eq!(
            (config.width, config.depth, config.resolution),
            (64.0, 64.0, 0.5)
        );
        assert_eq!(config.cells(), (128, 128));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn every_preset_is_valid_and_legacy_retains_its_original_grid() {
        for preset in TerrainPreset::ALL {
            assert!(TerrainConfig::preset(preset).validate().is_ok());
        }
        let legacy = TerrainConfig::preset(TerrainPreset::Legacy);
        assert_eq!(
            (legacy.width, legacy.depth, legacy.resolution),
            (120.0, 165.0, 1.25)
        );
        assert_eq!(legacy.cells(), (96, 132));
    }

    #[test]
    fn cell_counts_round_up_without_increasing_effective_spacing() {
        let config = TerrainConfig {
            width: 8.1,
            depth: 9.3,
            resolution: 0.7,
            ..Default::default()
        };
        let (nx, nz) = config.cells();
        assert_eq!((nx, nz), (12, 14));
        assert!(config.width as f64 / nx as f64 <= config.resolution as f64);
        assert!(config.depth as f64 / nz as f64 <= config.resolution as f64);
    }

    #[test]
    fn vertex_budget_accepts_boundary_and_rejects_oversized_grids() {
        let mut config = TerrainConfig {
            width: 64.0,
            depth: 64.0,
            resolution: 0.25,
            ..Default::default()
        };
        assert_eq!((config.cells().0 + 1) * (config.cells().1 + 1), 66_049);
        assert!(config.validate().is_ok());
        config.width = 64.25;
        assert!(config.validate().is_err());
        config.width = 256.0;
        config.depth = 256.0;
        assert!(config.validate().is_err());
        config.resolution = 4.0;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_invalid_dimensions_and_noise_parameters() {
        let setters: [fn(&mut TerrainConfig, f32); 8] = [
            |c, v| c.width = v,
            |c, v| c.depth = v,
            |c, v| c.resolution = v,
            |c, v| c.amplitude = v,
            |c, v| c.frequency = v,
            |c, v| c.persistence = v,
            |c, v| c.lacunarity = v,
            |c, v| c.roughness = v,
        ];
        for set in setters {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
                let mut config = TerrainConfig::default();
                set(&mut config, value);
                assert!(config.validate().is_err());
            }
        }
        for value in [0, 9, u32::MAX] {
            assert!(
                TerrainConfig {
                    octaves: value,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        for config in [
            TerrainConfig {
                width: 7.99,
                ..Default::default()
            },
            TerrainConfig {
                depth: 256.1,
                ..Default::default()
            },
            TerrainConfig {
                resolution: 0.249,
                ..Default::default()
            },
            TerrainConfig {
                resolution: 4.01,
                ..Default::default()
            },
            TerrainConfig {
                amplitude: 257.0,
                ..Default::default()
            },
            TerrainConfig {
                frequency: 0.0,
                ..Default::default()
            },
            TerrainConfig {
                frequency: 4.1,
                ..Default::default()
            },
            TerrainConfig {
                persistence: 1.1,
                ..Default::default()
            },
            TerrainConfig {
                lacunarity: 0.9,
                ..Default::default()
            },
            TerrainConfig {
                roughness: 2.1,
                ..Default::default()
            },
        ] {
            assert!(config.validate().is_err(), "accepted {config:?}");
        }
    }
}
