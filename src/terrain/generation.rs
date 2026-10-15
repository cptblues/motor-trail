use noise::{NoiseFn, Perlin};

use super::config::{MAX_HEIGHT, TerrainConfig, TerrainPreset};
use super::data::TerrainData;

/// Change this when any sample-producing arithmetic, preset, or noise version changes.
pub const GENERATOR_VERSION: u32 = 1;

pub fn generate(config: &TerrainConfig) -> Result<TerrainData, String> {
    config.validate()?;
    let (nx, nz) = config.cells();
    let mut heights = Vec::with_capacity((nx + 1) * (nz + 1));
    if config.preset == TerrainPreset::Legacy {
        for iz in 0..=nz {
            for ix in 0..=nx {
                let x = (ix as f32 / nx as f32 - 0.5) * config.width;
                let z = (iz as f32 / nz as f32 - 0.5) * config.depth;
                heights.push(super::legacy::height(x, z));
            }
        }
    } else {
        let broad = Layer::new(config.seed, 0);
        let detail = Layer::new(config.seed, 1);
        let ridge = Layer::new(config.seed, 2);
        for iz in 0..=nz {
            for ix in 0..=nx {
                let x = (ix as f32 / nx as f32 - 0.5) * config.width;
                let z = (iz as f32 / nz as f32 - 0.5) * config.depth;
                let sx = x as f64 * config.frequency as f64;
                let sz = z as f64 * config.frequency as f64;
                let low = broad.fbm(sx, sz, config);
                let fine = detail.fbm(sx * 2.7, sz * 2.7, config) * config.roughness as f64;
                let ridges = 1.0 - ridge.fbm(sx * 0.7, sz * 0.7, config).abs();
                let shape = match config.preset {
                    TerrainPreset::Flat => low * 0.3 + fine * 0.08,
                    TerrainPreset::Valley => {
                        let bank = (2.0 * x as f64 / config.width as f64).powi(2);
                        bank * 0.85 + low * (0.12 + bank * 0.35) + fine * 0.12
                    }
                    TerrainPreset::Mountain => low * 0.65 + ridges * ridges * 0.85 + fine * 0.4,
                    TerrainPreset::Trial => {
                        let undulations = (sx * 3.0).sin() * (sz * 2.0).cos();
                        low * 0.2 + undulations * 0.35 + fine * 0.45
                    }
                    TerrainPreset::Legacy => unreachable!("legacy is sampled separately"),
                };
                heights.push(
                    (shape * config.amplitude as f64).clamp(-(MAX_HEIGHT as f64), MAX_HEIGHT as f64)
                        as f32,
                );
            }
        }
    }
    TerrainData::from_heights(config.clone(), heights)
}

struct Layer {
    noise: Perlin,
    offset_x: f64,
    offset_z: f64,
}

impl Layer {
    fn new(seed: u64, layer: u64) -> Self {
        // SplitMix64 uses explicitly wrapping integer operations, independent of platform/hashers.
        let key = mix64(seed.wrapping_add(layer.wrapping_mul(0x9e37_79b9_7f4a_7c15)));
        Self {
            noise: Perlin::new((key ^ (key >> 32)) as u32),
            // Independent 53-bit offsets retain information beyond Perlin's u32 seed.
            offset_x: unit53(mix64(key ^ 0xa076_1d64_78bd_642f)) * 1024.0,
            offset_z: unit53(mix64(key ^ 0xe703_7ed1_a0b4_28db)) * 1024.0,
        }
    }

    fn fbm(&self, x: f64, z: f64, config: &TerrainConfig) -> f64 {
        let mut total = 0.0;
        let mut weight = 1.0;
        let mut weight_sum = 0.0;
        let mut frequency = 1.0;
        for _ in 0..config.octaves {
            total += self
                .noise
                .get([x * frequency + self.offset_x, z * frequency + self.offset_z])
                * weight;
            weight_sum += weight;
            weight *= config.persistence as f64;
            frequency *= config.lacunarity as f64;
        }
        total / weight_sum
    }
}

fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn unit53(value: u64) -> f64 {
    (value >> 11) as f64 / ((1_u64 << 53) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_version_one_is_deterministic() {
        assert_eq!(GENERATOR_VERSION, 1);
        let config = TerrainConfig {
            width: 8.0,
            depth: 12.0,
            seed: 0x1234_5678_90ab_cdef,
            ..Default::default()
        };
        assert_eq!(generate(&config).unwrap(), generate(&config).unwrap());
    }

    #[test]
    fn different_seeds_including_every_high_bit_change_the_relief() {
        let mut config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            seed: 17,
            ..Default::default()
        };
        let baseline = generate(&config).unwrap().heights;
        for bit in 0..64 {
            config.seed = 17 ^ (1_u64 << bit);
            assert_ne!(
                generate(&config).unwrap().heights,
                baseline,
                "seed bit {bit} was ignored"
            );
        }
    }

    #[test]
    fn presets_generate_finite_distinct_bounded_relief() {
        let mut previous = Vec::new();
        for preset in TerrainPreset::ALL {
            let config = TerrainConfig::preset(preset);
            let data = generate(&config).unwrap();
            assert!(data.validate().is_ok());
            assert_eq!(
                data.vertex_count(),
                (config.cells().0 + 1) * (config.cells().1 + 1)
            );
            assert!(
                data.heights
                    .iter()
                    .all(|h| h.is_finite() && h.abs() <= 512.0)
            );
            assert!(!previous.contains(&data.heights));
            previous.push(data.heights);
        }
    }

    #[test]
    fn legacy_samples_are_exact_and_ignore_procedural_noise_settings() {
        let mut config = TerrainConfig::preset(TerrainPreset::Legacy);
        let data = generate(&config).unwrap();
        for iz in 0..=132 {
            for ix in 0..=96 {
                let x = (ix as f32 / 96.0 - 0.5) * 120.0;
                let z = (iz as f32 / 132.0 - 0.5) * 165.0;
                assert_eq!(
                    data.heights[iz * 97 + ix],
                    super::super::legacy::height(x, z)
                );
            }
        }
        config.seed = u64::MAX;
        config.amplitude = 0.0;
        config.roughness = 0.0;
        assert_eq!(generate(&config).unwrap().heights, data.heights);
    }

    #[test]
    fn noise_extremes_remain_finite_and_zero_amplitude_is_flat() {
        let mut config = TerrainConfig {
            width: 8.1,
            depth: 9.3,
            resolution: 0.25,
            amplitude: 256.0,
            frequency: 4.0,
            octaves: 8,
            persistence: 1.0,
            lacunarity: 4.0,
            roughness: 2.0,
            seed: u64::MAX,
            ..Default::default()
        };
        assert!(generate(&config).unwrap().validate().is_ok());
        config.amplitude = 0.0;
        assert!(generate(&config).unwrap().heights.iter().all(|&h| h == 0.0));
        config.width = f32::NAN;
        assert!(generate(&config).is_err());
        config.width = 256.0;
        config.depth = 256.0;
        assert!(generate(&config).is_err());
    }

    #[test]
    fn noise_layers_and_roughness_are_effective_parameters() {
        let mut config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            ..Default::default()
        };
        let baseline = generate(&config).unwrap().heights;
        config.roughness = 0.0;
        assert_ne!(generate(&config).unwrap().heights, baseline);
        config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            octaves: 1,
            ..Default::default()
        };
        assert_ne!(generate(&config).unwrap().heights, baseline);
    }
}
