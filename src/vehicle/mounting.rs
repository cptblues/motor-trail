//! Serializable mount data in chassis coordinates: +X right, +Y up, +Z forward.
//! IDs, types, categories and zones use [a-z0-9][a-z0-9_.:-]{0,95}.
//! Use from_json or validate before consuming deserialized data.
use std::collections::HashSet;

use bevy::prelude::{Quat, Transform, Vec3};
use serde::{Deserialize, Serialize};

pub const MOUNT_SCHEMA_VERSION: u32 = 1;
pub const MAX_LOCAL_DISTANCE_M: f32 = 100.0;
pub const MAX_EQUIPMENT_MASS_KG: f32 = 100_000.0;
const MAX_IDENTIFIER_BYTES: usize = 96;

pub(super) fn validate_identifier(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value.as_bytes()[0].is_ascii_lowercase() && !value.as_bytes()[0].is_ascii_digit()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_.:-".contains(&byte)
        })
    {
        return Err(format!(
            "{field} must be a stable lowercase identifier of 1..={MAX_IDENTIFIER_BYTES} bytes, starting with a letter or digit"
        ));
    }
    Ok(())
}

pub(super) fn validate_unique_identifiers(values: &[String], field: &str) -> Result<(), String> {
    let mut seen = HashSet::new();
    for value in values {
        validate_identifier(value, field)?;
        if !seen.insert(value.as_str()) {
            return Err(format!("duplicate {field}: {value}"));
        }
    }
    Ok(())
}

pub(super) fn validate_local_position(position: [f32; 3], field: &str) -> Result<(), String> {
    if !position
        .iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_LOCAL_DISTANCE_M)
    {
        return Err(format!(
            "{field} must be finite and within +/-{MAX_LOCAL_DISTANCE_M} metres per axis"
        ));
    }
    Ok(())
}

pub(super) fn validate_rotation(rotation: [f32; 4], field: &str) -> Result<(), String> {
    let norm_squared: f32 = rotation.iter().map(|value| value * value).sum();
    if !rotation.iter().all(|value| value.is_finite())
        || !norm_squared.is_finite()
        || (norm_squared - 1.0).abs() > 1.0e-4
    {
        return Err(format!("{field} must be a finite unit XYZW quaternion"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPose {
    pub translation: [f32; 3],
    /// Unit quaternion in XYZW order.
    pub rotation: [f32; 4],
    /// Equipment dimensions are explicit; scaling must remain exactly ONE.
    pub scale: [f32; 3],
}

impl Default for LocalPose {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        }
    }
}

impl LocalPose {
    pub fn validate(&self) -> Result<(), String> {
        validate_local_position(self.translation, "translation")?;
        validate_rotation(self.rotation, "rotation")?;
        if self.scale != [1.0; 3] {
            return Err("local scale must be exactly [1, 1, 1]".into());
        }
        Ok(())
    }

    pub fn transform(&self) -> Result<Transform, String> {
        self.validate()?;
        Ok(Transform {
            translation: Vec3::from_array(self.translation),
            // Absorb only the rounding tolerance accepted by validate_rotation.
            rotation: Quat::from_array(self.rotation).normalize(),
            scale: Vec3::ONE,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleMountPoint {
    pub id: String,
    pub category: String,
    pub local_pose: LocalPose,
    /// Design policy limit, not a mass inferred from the visual GLB.
    pub max_mass_kg: f32,
    pub allowed_equipment_types: Vec<String>,
    /// Installing on this mount reserves every listed zone.
    pub occupied_zones: Vec<String>,
    /// Zones that must not be occupied by another installed mount.
    pub conflicting_zones: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleMountDefinition {
    pub schema_version: u32,
    pub vehicle_id: String,
    pub mounts: Vec<VehicleMountPoint>,
}

impl VehicleMountDefinition {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let definition: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        definition.validate()?;
        Ok(definition)
    }

    pub fn motor_trail() -> Result<Self, String> {
        Self::from_json(include_str!(
            "../../configs/vehicles/motor_trail_4x4.mounts.json"
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != MOUNT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported mount schema version: {}",
                self.schema_version
            ));
        }
        validate_identifier(&self.vehicle_id, "vehicle id")?;
        let mut mount_ids = HashSet::new();
        let mut zones = HashSet::new();
        for mount in &self.mounts {
            validate_identifier(&mount.id, "mount id")?;
            if !mount_ids.insert(mount.id.as_str()) {
                return Err(format!("duplicate mount id: {}", mount.id));
            }
            validate_identifier(&mount.category, "mount category")?;
            mount
                .local_pose
                .validate()
                .map_err(|error| format!("mount {}: {error}", mount.id))?;
            if !mount.max_mass_kg.is_finite()
                || mount.max_mass_kg <= 0.0
                || mount.max_mass_kg > MAX_EQUIPMENT_MASS_KG
            {
                return Err(format!(
                    "mount {} mass limit must be finite, positive and at most {MAX_EQUIPMENT_MASS_KG} kg",
                    mount.id
                ));
            }
            validate_unique_identifiers(&mount.allowed_equipment_types, "allowed equipment type")?;
            validate_unique_identifiers(&mount.occupied_zones, "occupied zone")?;
            validate_unique_identifiers(&mount.conflicting_zones, "conflicting zone")?;
            zones.extend(mount.occupied_zones.iter().map(String::as_str));
        }
        for mount in &self.mounts {
            for zone in &mount.conflicting_zones {
                if !zones.contains(zone.as_str()) {
                    return Err(format!(
                        "mount {} conflicts with unknown zone: {zone}",
                        mount.id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
