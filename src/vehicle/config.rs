use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicsModel {
    #[default]
    Rapier,
    Legacy,
}

impl PhysicsModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rapier => "Rapier",
            Self::Legacy => "Legacy",
        }
    }
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VehiclePhysicsConfig {
    pub model: PhysicsModel,
    pub chassis: ChassisConfig,
    pub front: SuspensionConfig,
    pub rear: SuspensionConfig,
    pub tires: TiresConfig,
    pub drivetrain: DrivetrainConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChassisConfig {
    pub mass_kg: f32,
    pub center_of_mass: [f32; 3],
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Fraction of ideal gravity compensation at rest, between zero and one.
    pub hill_hold_strength: f32,
}

impl Default for ChassisConfig {
    fn default() -> Self {
        Self {
            mass_kg: 1_800.0,
            center_of_mass: [0.0, -0.2, 0.0],
            linear_damping: 0.12,
            angular_damping: 0.55,
            hill_hold_strength: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SuspensionConfig {
    pub stiffness_n_per_m: f32,
    pub compression_damping_ns_per_m: f32,
    pub rebound_damping_ns_per_m: f32,
    pub rest_length_m: f32,
    pub max_travel_m: f32,
    /// Maximum force from one wheel's spring, in N.
    pub max_force_n: f32,
}

impl Default for SuspensionConfig {
    fn default() -> Self {
        Self {
            stiffness_n_per_m: 44_000.0,
            compression_damping_ns_per_m: 5_760.0,
            rebound_damping_ns_per_m: 8_280.0,
            rest_length_m: 0.36,
            max_travel_m: 0.36,
            max_force_n: 14_000.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TiresConfig {
    pub radius_m: f32,
    pub grip_coefficient: f32,
    /// Native solver response multiplier, between zero and one.
    pub lateral_stiffness: f32,
    /// Total constant rolling resistance over all four wheels, in N.
    pub rolling_resistance_n: f32,
    /// Total speed-dependent rolling drag over all four wheels, in N*s/m.
    pub rolling_drag_ns_per_m: f32,
}

impl Default for TiresConfig {
    fn default() -> Self {
        Self {
            radius_m: 0.53,
            grip_coefficient: 1.35,
            lateral_stiffness: 1.0,
            rolling_resistance_n: 140.0,
            rolling_drag_ns_per_m: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DrivetrainConfig {
    /// Total drive force across all four wheels, before the selected ratio.
    pub engine_force_n: f32,
    pub throttle_response_hz: f32,
    pub normal_ratio: f32,
    pub low_ratio: f32,
    pub normal_max_speed_mps: f32,
    pub low_max_speed_mps: f32,
    pub reverse_max_speed_mps: f32,
    /// Total service brake force across all four wheels, in N.
    pub brake_force_n: f32,
    /// Total speed-dependent legacy brake damping, in N*s/m; ignored by Rapier.
    pub legacy_brake_damping_ns_per_m: f32,
    /// Total engine brake force across all four wheels, in N.
    pub engine_brake_force_n: f32,
}

impl Default for DrivetrainConfig {
    fn default() -> Self {
        Self {
            engine_force_n: 7_000.0,
            throttle_response_hz: 8.0,
            normal_ratio: 1.0,
            low_ratio: 10_400.0 / 7_000.0,
            normal_max_speed_mps: 18.0,
            low_max_speed_mps: 7.0,
            reverse_max_speed_mps: 6.0,
            brake_force_n: 18_000.0,
            legacy_brake_damping_ns_per_m: 0.0,
            engine_brake_force_n: 0.0,
        }
    }
}

impl VehiclePhysicsConfig {
    pub fn baseline() -> Self {
        Self::default()
    }

    pub fn original() -> Self {
        Self {
            model: PhysicsModel::Legacy,
            chassis: ChassisConfig {
                mass_kg: 930.0,
                center_of_mass: [0.0; 3],
                angular_damping: 1.25,
                hill_hold_strength: 0.0,
                ..ChassisConfig::default()
            },
            front: SuspensionConfig {
                stiffness_n_per_m: 15_000.0,
                compression_damping_ns_per_m: 1_750.0,
                rebound_damping_ns_per_m: 1_750.0,
                max_force_n: 8_500.0,
                ..SuspensionConfig::default()
            },
            rear: SuspensionConfig {
                stiffness_n_per_m: 15_000.0,
                compression_damping_ns_per_m: 1_750.0,
                rebound_damping_ns_per_m: 1_750.0,
                max_force_n: 8_500.0,
                ..SuspensionConfig::default()
            },
            tires: TiresConfig {
                rolling_resistance_n: 0.0,
                rolling_drag_ns_per_m: 88.0,
                ..TiresConfig::default()
            },
            drivetrain: DrivetrainConfig {
                engine_force_n: 6_600.0,
                throttle_response_hz: 1_000.0,
                low_ratio: 8_400.0 / 6_600.0,
                reverse_max_speed_mps: 18.0,
                brake_force_n: 0.0,
                legacy_brake_damping_ns_per_m: 7_800.0,
                ..DrivetrainConfig::default()
            },
        }
    }

    pub fn preset_names() -> &'static [&'static str] {
        &[
            "Baseline",
            "Confort",
            "Franchissement",
            "Dynamique",
            "Origine",
        ]
    }

    pub fn preset(name: &str) -> Option<Self> {
        let mut config = Self::baseline();
        // These are experimental comparison settings, not handling recommendations.
        match name {
            "Baseline" => {}
            "Origine" => return Some(Self::original()),
            "Confort" => {
                config.chassis.angular_damping = 0.65;
                config.front.stiffness_n_per_m = 36_000.0;
                config.front.compression_damping_ns_per_m = 5_200.0;
                config.front.rebound_damping_ns_per_m = 7_200.0;
                config.rear = config.front.clone();
                config.tires.grip_coefficient = 1.25;
                config.drivetrain.engine_force_n = 6_500.0;
                config.drivetrain.throttle_response_hz = 5.0;
            }
            "Franchissement" => {
                config.chassis.center_of_mass[1] = -0.28;
                config.chassis.hill_hold_strength = 1.0;
                config.front.stiffness_n_per_m = 48_000.0;
                config.front.compression_damping_ns_per_m = 6_200.0;
                config.front.rebound_damping_ns_per_m = 9_000.0;
                config.front.rest_length_m = 0.40;
                config.front.max_travel_m = 0.40;
                config.front.max_force_n = 18_000.0;
                config.rear = config.front.clone();
                config.tires.grip_coefficient = 1.55;
                config.drivetrain.throttle_response_hz = 6.0;
                config.drivetrain.low_ratio = 1.8;
                config.drivetrain.normal_max_speed_mps = 16.0;
                config.drivetrain.low_max_speed_mps = 5.0;
                config.drivetrain.reverse_max_speed_mps = 4.0;
            }
            "Dynamique" => {
                config.chassis.center_of_mass[1] = -0.25;
                config.chassis.linear_damping = 0.10;
                config.chassis.angular_damping = 0.65;
                config.front.stiffness_n_per_m = 56_000.0;
                config.front.compression_damping_ns_per_m = 6_600.0;
                config.front.rebound_damping_ns_per_m = 9_800.0;
                config.front.rest_length_m = 0.32;
                config.front.max_travel_m = 0.32;
                config.front.max_force_n = 16_000.0;
                config.rear = config.front.clone();
                config.tires.grip_coefficient = 1.45;
                config.drivetrain.engine_force_n = 9_000.0;
                config.drivetrain.throttle_response_hz = 12.0;
                config.drivetrain.normal_max_speed_mps = 22.0;
                config.drivetrain.brake_force_n = 21_000.0;
            }
            _ => return None,
        }
        Some(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(
            "chassis.mass_kg",
            self.chassis.mass_kg,
            100.0,
            10_000.0,
            "kg",
        )?;
        for (index, offset) in self.chassis.center_of_mass.iter().enumerate() {
            bounded(
                &format!("chassis.center_of_mass[{index}]"),
                *offset,
                -1.0,
                1.0,
                "m",
            )?;
        }
        bounded(
            "chassis.linear_damping",
            self.chassis.linear_damping,
            0.0,
            20.0,
            "",
        )?;
        bounded(
            "chassis.angular_damping",
            self.chassis.angular_damping,
            0.0,
            20.0,
            "",
        )?;
        bounded(
            "chassis.hill_hold_strength",
            self.chassis.hill_hold_strength,
            0.0,
            1.0,
            "",
        )?;
        self.front.validate_bounds("front")?;
        self.rear.validate_bounds("rear")?;
        bounded("tires.radius_m", self.tires.radius_m, 0.1, 1.0, "m")?;
        bounded(
            "tires.grip_coefficient",
            self.tires.grip_coefficient,
            0.05,
            3.0,
            "",
        )?;
        bounded(
            "tires.lateral_stiffness",
            self.tires.lateral_stiffness,
            0.0,
            1.0,
            "",
        )?;
        bounded(
            "tires.rolling_resistance_n",
            self.tires.rolling_resistance_n,
            0.0,
            10_000.0,
            "N",
        )?;
        bounded(
            "tires.rolling_drag_ns_per_m",
            self.tires.rolling_drag_ns_per_m,
            0.0,
            2_000.0,
            "N*s/m",
        )?;
        bounded(
            "drivetrain.engine_force_n",
            self.drivetrain.engine_force_n,
            0.0,
            100_000.0,
            "N total",
        )?;
        bounded(
            "drivetrain.throttle_response_hz",
            self.drivetrain.throttle_response_hz,
            0.1,
            1_000.0,
            "Hz",
        )?;
        bounded(
            "drivetrain.normal_ratio",
            self.drivetrain.normal_ratio,
            0.1,
            10.0,
            "",
        )?;
        bounded(
            "drivetrain.low_ratio",
            self.drivetrain.low_ratio,
            0.1,
            10.0,
            "",
        )?;
        bounded(
            "drivetrain.normal_max_speed_mps",
            self.drivetrain.normal_max_speed_mps,
            0.1,
            100.0,
            "m/s",
        )?;
        bounded(
            "drivetrain.low_max_speed_mps",
            self.drivetrain.low_max_speed_mps,
            0.1,
            100.0,
            "m/s",
        )?;
        bounded(
            "drivetrain.reverse_max_speed_mps",
            self.drivetrain.reverse_max_speed_mps,
            0.1,
            100.0,
            "m/s",
        )?;
        bounded(
            "drivetrain.brake_force_n",
            self.drivetrain.brake_force_n,
            0.0,
            200_000.0,
            "N total",
        )?;
        bounded(
            "drivetrain.engine_brake_force_n",
            self.drivetrain.engine_brake_force_n,
            0.0,
            100_000.0,
            "N total",
        )?;
        bounded(
            "drivetrain.legacy_brake_damping_ns_per_m",
            self.drivetrain.legacy_brake_damping_ns_per_m,
            0.0,
            50_000.0,
            "N*s/m total",
        )?;

        // Geometry-dependent load transfer and tire/mount clearance belong to simulation.
        // This check uses a level, equally loaded four-wheel chassis at standard gravity.
        let static_wheel_load_n = self.chassis.mass_kg * 9.81 / 4.0;
        for (axle, suspension) in [("front", &self.front), ("rear", &self.rear)] {
            let sag_m = static_wheel_load_n / suspension.stiffness_n_per_m;
            let compression_m = suspension.rest_length_m.min(suspension.max_travel_m);
            if sag_m >= compression_m {
                return Err(format!(
                    "{axle}: static sag {sag_m:.4} m must be below available compression \
                     {compression_m:.4} m (rest_length_m and max_travel_m); increase \
                     stiffness_n_per_m or travel, or reduce chassis.mass_kg"
                ));
            }
            if suspension.max_force_n < static_wheel_load_n {
                return Err(format!(
                    "{axle}.max_force_n: two springs provide {:.1} N, below the static axle \
                     load {:.1} N; each spring must support at least {static_wheel_load_n:.1} N",
                    suspension.max_force_n * 2.0,
                    static_wheel_load_n * 2.0
                ));
            }
        }
        if self.drivetrain.low_ratio < self.drivetrain.normal_ratio {
            return Err("drivetrain.low_ratio must be at least drivetrain.normal_ratio".into());
        }
        if self.drivetrain.low_max_speed_mps > self.drivetrain.normal_max_speed_mps {
            return Err(
                "drivetrain.low_max_speed_mps must not exceed drivetrain.normal_max_speed_mps"
                    .into(),
            );
        }
        Ok(())
    }
}

impl SuspensionConfig {
    fn validate_bounds(&self, axle: &str) -> Result<(), String> {
        for (field, value, minimum, maximum, unit) in [
            (
                "stiffness_n_per_m",
                self.stiffness_n_per_m,
                1_000.0,
                500_000.0,
                "N/m",
            ),
            (
                "compression_damping_ns_per_m",
                self.compression_damping_ns_per_m,
                0.0,
                100_000.0,
                "N*s/m",
            ),
            (
                "rebound_damping_ns_per_m",
                self.rebound_damping_ns_per_m,
                0.0,
                100_000.0,
                "N*s/m",
            ),
            ("rest_length_m", self.rest_length_m, 0.05, 1.2, "m"),
            ("max_travel_m", self.max_travel_m, 0.01, 1.2, "m"),
            (
                "max_force_n",
                self.max_force_n,
                100.0,
                200_000.0,
                "N per wheel",
            ),
        ] {
            bounded(&format!("{axle}.{field}"), value, minimum, maximum, unit)?;
        }
        Ok(())
    }
}

fn bounded(field: &str, value: f32, minimum: f32, maximum: f32, unit: &str) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{field} must be finite; got {value}"));
    }
    if value < minimum || value > maximum {
        return Err(format!(
            "{field} must be in {minimum}..={maximum} {unit}; got {value}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_defaults_match_the_native_baseline() {
        let config: VehiclePhysicsConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, VehiclePhysicsConfig::baseline());
        assert_eq!(config, VehiclePhysicsConfig::default());
        assert_eq!(config.model, PhysicsModel::Rapier);
        assert_eq!(config.chassis.mass_kg, 1_800.0);
        assert_eq!(config.front.stiffness_n_per_m, 44_000.0);
        assert_eq!(config.front.compression_damping_ns_per_m, 5_760.0);
        assert_eq!(config.front.rebound_damping_ns_per_m, 8_280.0);
        assert_eq!(config.front, config.rear);
        assert_eq!(config.drivetrain.engine_force_n, 7_000.0);
        assert_eq!(config.drivetrain.legacy_brake_damping_ns_per_m, 0.0);
        config.validate().unwrap();
    }

    #[test]
    fn partial_nested_objects_keep_field_defaults() {
        let config: VehiclePhysicsConfig = serde_json::from_str(
            r#"{"chassis":{"mass_kg":2000},"front":{"stiffness_n_per_m":50000}}"#,
        )
        .unwrap();
        assert_eq!(config.chassis.mass_kg, 2_000.0);
        assert_eq!(config.chassis.angular_damping, 0.55);
        assert_eq!(config.front.stiffness_n_per_m, 50_000.0);
        assert_eq!(config.front.rest_length_m, 0.36);
        assert_eq!(config.rear, SuspensionConfig::default());
        config.validate().unwrap();
    }

    #[test]
    fn all_presets_validate_and_roundtrip() {
        assert_eq!(
            VehiclePhysicsConfig::preset_names(),
            &[
                "Baseline",
                "Confort",
                "Franchissement",
                "Dynamique",
                "Origine"
            ]
        );
        for name in VehiclePhysicsConfig::preset_names() {
            let config = VehiclePhysicsConfig::preset(name).unwrap();
            config.validate().unwrap();
            let json = serde_json::to_string_pretty(&config).unwrap();
            let restored: VehiclePhysicsConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, config, "{name}");
            if *name != "Baseline" {
                assert_ne!(config, VehiclePhysicsConfig::baseline(), "{name}");
            }
        }
        assert!(VehiclePhysicsConfig::preset("unknown").is_none());
        assert_eq!(PhysicsModel::Rapier.label(), "Rapier");
        assert_eq!(PhysicsModel::Legacy.label(), "Legacy");
    }

    #[test]
    fn original_preserves_legacy_controller_units() {
        let config = VehiclePhysicsConfig::original();
        assert_eq!(config.model, PhysicsModel::Legacy);
        assert_eq!(config.chassis.mass_kg, 930.0);
        assert_eq!(config.chassis.center_of_mass, [0.0; 3]);
        assert_eq!(config.chassis.hill_hold_strength, 0.0);
        assert_eq!(config.chassis.angular_damping, 1.25);
        assert_eq!(config.front.stiffness_n_per_m, 15_000.0);
        assert_eq!(config.front.compression_damping_ns_per_m, 1_750.0);
        assert_eq!(config.front.rebound_damping_ns_per_m, 1_750.0);
        assert_eq!(config.front.max_force_n, 8_500.0);
        assert_eq!(config.tires.rolling_resistance_n, 0.0);
        assert_eq!(config.tires.rolling_drag_ns_per_m, 88.0);
        assert_eq!(config.drivetrain.engine_force_n, 6_600.0);
        assert_eq!(config.drivetrain.low_ratio, 8_400.0 / 6_600.0);
        assert_eq!(config.drivetrain.throttle_response_hz, 1_000.0);
        assert_eq!(config.drivetrain.reverse_max_speed_mps, 18.0);
        assert_eq!(config.drivetrain.brake_force_n, 0.0);
        assert_eq!(config.drivetrain.legacy_brake_damping_ns_per_m, 7_800.0);
        config.validate().unwrap();
    }

    #[test]
    fn original_brake_damping_preserves_speed_dependent_units() {
        let config = VehiclePhysicsConfig::original();
        let total_damping_ns_per_m = config.drivetrain.legacy_brake_damping_ns_per_m;
        assert_eq!(total_damping_ns_per_m / 4.0, 1_950.0);
        assert_eq!(config.drivetrain.brake_force_n, 0.0);
        assert_eq!(total_damping_ns_per_m * 0.0, 0.0);
        assert_eq!(total_damping_ns_per_m * 2.0, 15_600.0);
        let json = serde_json::to_string(&config).unwrap();
        let restored: VehiclePhysicsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, config);
    }

    #[test]
    fn legacy_brake_damping_defaults_for_older_json_and_accepts_its_bounds() {
        let mut json = serde_json::to_value(VehiclePhysicsConfig::baseline()).unwrap();
        json["drivetrain"]
            .as_object_mut()
            .unwrap()
            .remove("legacy_brake_damping_ns_per_m");
        let mut config: VehiclePhysicsConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.drivetrain.legacy_brake_damping_ns_per_m, 0.0);
        for value in [0.0, 50_000.0] {
            config.drivetrain.legacy_brake_damping_ns_per_m = value;
            config.validate().unwrap();
        }
    }

    #[test]
    fn every_numeric_field_rejects_nonfinite_values() {
        let baseline = serde_json::to_value(VehiclePhysicsConfig::baseline()).unwrap();
        for section in ["chassis", "front", "rear", "tires", "drivetrain"] {
            for (field, value) in baseline[section].as_object().unwrap() {
                let count = value.as_array().map_or(1, Vec::len);
                for index in 0..count {
                    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                        let mut config = VehiclePhysicsConfig::baseline();
                        match section {
                            "chassis" => match field.as_str() {
                                "mass_kg" => config.chassis.mass_kg = invalid,
                                "center_of_mass" => config.chassis.center_of_mass[index] = invalid,
                                "linear_damping" => config.chassis.linear_damping = invalid,
                                "angular_damping" => config.chassis.angular_damping = invalid,
                                "hill_hold_strength" => config.chassis.hill_hold_strength = invalid,
                                _ => panic!("uncovered chassis field: {field}"),
                            },
                            "front" | "rear" => {
                                let suspension = if section == "front" {
                                    &mut config.front
                                } else {
                                    &mut config.rear
                                };
                                match field.as_str() {
                                    "stiffness_n_per_m" => suspension.stiffness_n_per_m = invalid,
                                    "compression_damping_ns_per_m" => {
                                        suspension.compression_damping_ns_per_m = invalid
                                    }
                                    "rebound_damping_ns_per_m" => {
                                        suspension.rebound_damping_ns_per_m = invalid
                                    }
                                    "rest_length_m" => suspension.rest_length_m = invalid,
                                    "max_travel_m" => suspension.max_travel_m = invalid,
                                    "max_force_n" => suspension.max_force_n = invalid,
                                    _ => panic!("uncovered suspension field: {field}"),
                                }
                            }
                            "tires" => match field.as_str() {
                                "radius_m" => config.tires.radius_m = invalid,
                                "grip_coefficient" => config.tires.grip_coefficient = invalid,
                                "lateral_stiffness" => config.tires.lateral_stiffness = invalid,
                                "rolling_resistance_n" => {
                                    config.tires.rolling_resistance_n = invalid
                                }
                                "rolling_drag_ns_per_m" => {
                                    config.tires.rolling_drag_ns_per_m = invalid
                                }
                                _ => panic!("uncovered tire field: {field}"),
                            },
                            "drivetrain" => match field.as_str() {
                                "engine_force_n" => config.drivetrain.engine_force_n = invalid,
                                "throttle_response_hz" => {
                                    config.drivetrain.throttle_response_hz = invalid
                                }
                                "normal_ratio" => config.drivetrain.normal_ratio = invalid,
                                "low_ratio" => config.drivetrain.low_ratio = invalid,
                                "normal_max_speed_mps" => {
                                    config.drivetrain.normal_max_speed_mps = invalid
                                }
                                "low_max_speed_mps" => {
                                    config.drivetrain.low_max_speed_mps = invalid
                                }
                                "reverse_max_speed_mps" => {
                                    config.drivetrain.reverse_max_speed_mps = invalid
                                }
                                "brake_force_n" => config.drivetrain.brake_force_n = invalid,
                                "legacy_brake_damping_ns_per_m" => {
                                    config.drivetrain.legacy_brake_damping_ns_per_m = invalid
                                }
                                "engine_brake_force_n" => {
                                    config.drivetrain.engine_brake_force_n = invalid
                                }
                                _ => panic!("uncovered drivetrain field: {field}"),
                            },
                            _ => unreachable!(),
                        }
                        let error = config.validate().unwrap_err();
                        assert!(error.contains(section), "{error}");
                        assert!(error.contains(field), "{error}");
                        assert!(error.contains("finite"), "{error}");
                    }
                }
            }
        }
    }

    #[test]
    fn rejects_unknown_fields_at_every_schema_level() {
        for section in [
            None,
            Some("chassis"),
            Some("front"),
            Some("rear"),
            Some("tires"),
            Some("drivetrain"),
        ] {
            let mut value = serde_json::to_value(VehiclePhysicsConfig::baseline()).unwrap();
            let object = match section {
                Some(section) => value[section].as_object_mut().unwrap(),
                None => value.as_object_mut().unwrap(),
            };
            object.insert("unsupported_option".into(), serde_json::json!(1));
            assert!(serde_json::from_value::<VehiclePhysicsConfig>(value).is_err());
        }
    }

    #[test]
    fn rejects_sag_at_or_beyond_available_compression() {
        for front in [true, false] {
            let mut config = VehiclePhysicsConfig::baseline();
            let suspension = if front {
                &mut config.front
            } else {
                &mut config.rear
            };
            suspension.stiffness_n_per_m = 10_000.0;
            let error = config.validate().unwrap_err();
            assert!(error.contains(if front { "front" } else { "rear" }));
            assert!(error.contains("sag"), "{error}");
        }
        let mut config = VehiclePhysicsConfig::baseline();
        config.front.rest_length_m = 0.05;
        assert!(config.validate().unwrap_err().contains("sag"));
        let mut config = VehiclePhysicsConfig::baseline();
        config.front.max_travel_m =
            config.chassis.mass_kg * 9.81 / 4.0 / config.front.stiffness_n_per_m;
        assert!(config.validate().unwrap_err().contains("sag"));
    }

    #[test]
    fn rejects_suspension_force_below_static_axle_load() {
        let mut config = VehiclePhysicsConfig::baseline();
        config.rear.max_force_n = 4_000.0;
        let error = config.validate().unwrap_err();
        assert!(error.contains("rear.max_force_n"), "{error}");
        assert!(error.contains("static axle load"), "{error}");
    }

    #[test]
    fn tire_solver_multiplier_and_grip_are_bounded() {
        for invalid in [-0.01, 1.01] {
            let mut config = VehiclePhysicsConfig::baseline();
            config.tires.lateral_stiffness = invalid;
            assert!(config.validate().unwrap_err().contains("lateral_stiffness"));
        }
        for invalid in [0.0, 3.01] {
            let mut config = VehiclePhysicsConfig::baseline();
            config.tires.grip_coefficient = invalid;
            assert!(config.validate().unwrap_err().contains("grip_coefficient"));
        }
        for valid in [0.0, 1.0] {
            let mut config = VehiclePhysicsConfig::baseline();
            config.tires.lateral_stiffness = valid;
            config.validate().unwrap();
        }
    }

    #[test]
    fn rejects_finite_but_nonphysical_settings() {
        let baseline = VehiclePhysicsConfig::baseline();
        for (section, field, value) in [
            ("chassis", "mass_kg", 0.0),
            ("chassis", "angular_damping", -1.0),
            ("chassis", "hill_hold_strength", -1.0),
            ("chassis", "hill_hold_strength", 1.01),
            ("chassis", "hill_hold_strength", 1.2),
            ("front", "stiffness_n_per_m", 0.0),
            ("rear", "compression_damping_ns_per_m", -1.0),
            ("rear", "rebound_damping_ns_per_m", -1.0),
            ("tires", "radius_m", 0.0),
            ("tires", "rolling_resistance_n", -1.0),
            ("tires", "rolling_drag_ns_per_m", -1.0),
            ("drivetrain", "engine_force_n", -1.0),
            ("drivetrain", "throttle_response_hz", 0.0),
            ("drivetrain", "normal_ratio", 0.0),
            ("drivetrain", "reverse_max_speed_mps", 0.0),
            ("drivetrain", "brake_force_n", -1.0),
            ("drivetrain", "legacy_brake_damping_ns_per_m", -1.0),
            ("drivetrain", "legacy_brake_damping_ns_per_m", 50_001.0),
            ("drivetrain", "engine_brake_force_n", -1.0),
        ] {
            let mut json = serde_json::to_value(&baseline).unwrap();
            json[section][field] = serde_json::json!(value);
            let config: VehiclePhysicsConfig = serde_json::from_value(json).unwrap();
            let error = config.validate().unwrap_err();
            assert!(error.contains(&format!("{section}.{field}")), "{error}");
        }
        let mut config = baseline.clone();
        config.chassis.center_of_mass[0] = 2.0;
        assert!(config.validate().unwrap_err().contains("center_of_mass[0]"));
        let mut config = baseline.clone();
        config.drivetrain.low_ratio = 0.5;
        assert!(config.validate().unwrap_err().contains("low_ratio"));
        let mut config = baseline;
        config.drivetrain.low_max_speed_mps = 20.0;
        assert!(config.validate().unwrap_err().contains("low_max_speed_mps"));
    }

    #[test]
    fn archived_json_presets_match_their_rust_definitions() {
        let files = [
            (
                "Baseline",
                include_str!("../../configs/vehicles/Baseline.json"),
            ),
            (
                "Confort",
                include_str!("../../configs/vehicles/Confort.json"),
            ),
            (
                "Franchissement",
                include_str!("../../configs/vehicles/Franchissement.json"),
            ),
            (
                "Dynamique",
                include_str!("../../configs/vehicles/Dynamique.json"),
            ),
            (
                "Origine",
                include_str!("../../configs/vehicles/Origine.json"),
            ),
        ];
        for (name, json) in files {
            let archived: VehiclePhysicsConfig = serde_json::from_str(json).unwrap();
            archived.validate().unwrap();
            assert_eq!(Some(archived), VehiclePhysicsConfig::preset(name), "{name}");
        }
    }
}
