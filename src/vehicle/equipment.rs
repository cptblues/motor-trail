//! Equipment definitions and pure loadout math; ECS attachment belongs to the caller.
use std::collections::{HashMap, HashSet};

use bevy::math::{DMat3, DVec3, Mat3};
use bevy::prelude::{Quat, Transform, Vec3};
use bevy_rapier3d::prelude::{Collider, MassProperties};
use bevy_rapier3d::rapier::math::Pose;
use serde::{Deserialize, Serialize};

use super::mounting::{
    LocalPose, MAX_EQUIPMENT_MASS_KG, MAX_LOCAL_DISTANCE_M, VehicleMountDefinition,
    validate_identifier, validate_local_position, validate_rotation, validate_unique_identifiers,
};

pub const EQUIPMENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_PRINCIPAL_INERTIA_KG_M2: f32 = 1.0e9;

fn validate_dimension(value: f32, field: &str) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 || value > MAX_LOCAL_DISTANCE_M {
        return Err(format!(
            "{field} must be finite, positive and at most {MAX_LOCAL_DISTANCE_M} metres"
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentMode {
    Decorative,
    Rigid,
    Articulated,
    Detachable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EquipmentShape {
    Cuboid {
        half_extents: [f32; 3],
    },
    /// The cylinder's axis is equipment-local +Y before local_pose rotation.
    Cylinder {
        half_height: f32,
        radius: f32,
    },
    Ball {
        radius: f32,
    },
    /// The capsule's segment is equipment-local Y before local_pose rotation.
    Capsule {
        half_segment: f32,
        radius: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentCollider {
    /// Relative to the equipment node, independent of its GLB scene hierarchy.
    pub local_pose: LocalPose,
    pub shape: EquipmentShape,
}

impl EquipmentCollider {
    pub fn validate(&self) -> Result<(), String> {
        self.local_pose.validate()?;
        match &self.shape {
            EquipmentShape::Cuboid { half_extents } => {
                for &extent in half_extents {
                    validate_dimension(extent, "cuboid half extent")?;
                }
            }
            EquipmentShape::Cylinder {
                half_height,
                radius,
            } => {
                validate_dimension(*half_height, "cylinder half height")?;
                validate_dimension(*radius, "cylinder radius")?;
            }
            EquipmentShape::Ball { radius } => validate_dimension(*radius, "ball radius")?,
            EquipmentShape::Capsule {
                half_segment,
                radius,
            } => {
                if !half_segment.is_finite()
                    || *half_segment < 0.0
                    || *half_segment > MAX_LOCAL_DISTANCE_M
                {
                    return Err(format!(
                        "capsule half segment must be finite and within 0..={MAX_LOCAL_DISTANCE_M} metres"
                    ));
                }
                validate_dimension(*radius, "capsule radius")?;
            }
        }
        Ok(())
    }

    /// Geometry only; the caller must assign zero collider density at runtime.
    pub fn collider(&self) -> Result<Collider, String> {
        self.validate()?;
        Ok(match &self.shape {
            EquipmentShape::Cuboid { half_extents } => {
                Collider::cuboid(half_extents[0], half_extents[1], half_extents[2])
            }
            EquipmentShape::Cylinder {
                half_height,
                radius,
            } => Collider::cylinder(*half_height, *radius),
            EquipmentShape::Ball { radius } => Collider::ball(*radius),
            EquipmentShape::Capsule {
                half_segment,
                radius,
            } => Collider::capsule_y(*half_segment, *radius),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentDefinition {
    pub id: String,
    #[serde(rename = "type")]
    pub equipment_type: String,
    pub glb_path: Option<String>,
    /// Equipment origin relative to the selected mount.
    pub local_pose: LocalPose,
    pub mass_kg: f32,
    pub local_center_of_mass: [f32; 3],
    pub principal_inertia: [f32; 3],
    /// Unit quaternion in XYZW order, relative to equipment coordinates.
    pub principal_inertia_local_frame: [f32; 4],
    pub compatible_mount_categories: Vec<String>,
    pub mode: EquipmentMode,
    pub colliders: Vec<EquipmentCollider>,
}

impl EquipmentDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if matches!(
            self.mode,
            EquipmentMode::Articulated | EquipmentMode::Detachable
        ) {
            return Err(format!(
                "equipment {} uses unsupported mode {:?}",
                self.id, self.mode
            ));
        }
        validate_identifier(&self.id, "equipment id")?;
        validate_identifier(&self.equipment_type, "equipment type")?;
        self.local_pose.validate()?;
        validate_local_position(self.local_center_of_mass, "equipment center of mass")?;
        validate_rotation(
            self.principal_inertia_local_frame,
            "principal inertia frame",
        )?;
        validate_unique_identifiers(
            &self.compatible_mount_categories,
            "compatible mount category",
        )?;
        if self.compatible_mount_categories.is_empty() {
            return Err(format!(
                "equipment {} needs a compatible mount category",
                self.id
            ));
        }
        if let Some(path) = &self.glb_path {
            if path.trim().is_empty()
                || path.len() > 512
                || path.chars().any(char::is_control)
                || !path
                    .split('#')
                    .next()
                    .is_some_and(|asset| asset.to_ascii_lowercase().ends_with(".glb"))
            {
                return Err(format!(
                    "equipment {} has an invalid GLB asset path",
                    self.id
                ));
            }
        }
        match self.mode {
            EquipmentMode::Decorative => {
                if self.mass_kg != 0.0
                    || self.principal_inertia != [0.0; 3]
                    || !self.colliders.is_empty()
                {
                    return Err(format!(
                        "decorative equipment {} must have zero mass/inertia and no collision",
                        self.id
                    ));
                }
            }
            EquipmentMode::Rigid => {
                if !self.mass_kg.is_finite()
                    || self.mass_kg <= 0.0
                    || !self.mass_kg.recip().is_finite()
                    || self.mass_kg > MAX_EQUIPMENT_MASS_KG
                {
                    return Err(format!(
                        "equipment {} mass must be finite, positive and at most {MAX_EQUIPMENT_MASS_KG} kg",
                        self.id
                    ));
                }
                if !self.principal_inertia.iter().all(|value| {
                    value.is_finite()
                        && *value > 0.0
                        && value.recip().is_finite()
                        && *value <= MAX_PRINCIPAL_INERTIA_KG_M2
                }) {
                    return Err(format!(
                        "equipment {} principal inertia must be finite, positive and at most {MAX_PRINCIPAL_INERTIA_KG_M2} kg m^2",
                        self.id
                    ));
                }
                let [x, y, z] = self.principal_inertia.map(f64::from);
                let largest = x.max(y).max(z);
                if 2.0 * largest > (x + y + z) * (1.0 + 1.0e-6) {
                    return Err(format!(
                        "equipment {} principal inertia violates the triangle inequality",
                        self.id
                    ));
                }
                if self.colliders.is_empty() {
                    return Err(format!(
                        "rigid equipment {} needs collision geometry",
                        self.id
                    ));
                }
            }
            EquipmentMode::Articulated | EquipmentMode::Detachable => {
                return Err("unsupported equipment mode".into());
            }
        }
        for (index, collider) in self.colliders.iter().enumerate() {
            collider
                .validate()
                .map_err(|error| format!("equipment {} collider {index}: {error}", self.id))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentCatalog {
    pub schema_version: u32,
    pub definitions: Vec<EquipmentDefinition>,
}

impl Default for EquipmentCatalog {
    fn default() -> Self {
        Self {
            schema_version: EQUIPMENT_SCHEMA_VERSION,
            definitions: Vec::new(),
        }
    }
}

impl EquipmentCatalog {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let catalog: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != EQUIPMENT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported equipment catalog schema version: {}",
                self.schema_version
            ));
        }
        let mut ids = HashSet::new();
        for definition in &self.definitions {
            definition.validate()?;
            if !ids.insert(definition.id.as_str()) {
                return Err(format!(
                    "duplicate equipment definition id: {}",
                    definition.id
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EquipmentInstallation {
    pub instance_id: String,
    pub equipment_id: String,
    pub mount_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleEquipmentLoadout {
    pub schema_version: u32,
    pub vehicle_id: String,
    pub equipment: Vec<EquipmentInstallation>,
}

impl VehicleEquipmentLoadout {
    pub fn empty(vehicle_id: impl Into<String>) -> Self {
        Self {
            schema_version: EQUIPMENT_SCHEMA_VERSION,
            vehicle_id: vehicle_id.into(),
            equipment: Vec::new(),
        }
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let loadout: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        loadout.validate()?;
        Ok(loadout)
    }

    /// Structural validation; cross references are checked by resolve_loadout.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != EQUIPMENT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported equipment loadout schema version: {}",
                self.schema_version
            ));
        }
        validate_identifier(&self.vehicle_id, "loadout vehicle id")?;
        let mut ids = HashSet::new();
        for installation in &self.equipment {
            validate_identifier(&installation.instance_id, "equipment instance id")?;
            validate_identifier(&installation.equipment_id, "equipment definition reference")?;
            validate_identifier(&installation.mount_id, "mount reference")?;
            if !ids.insert(installation.instance_id.as_str()) {
                return Err(format!(
                    "duplicate equipment instance id: {}",
                    installation.instance_id
                ));
            }
        }
        Ok(())
    }
}

pub struct InstalledCollider {
    /// Equipment-local; compose with InstalledEquipment::local_transform.
    pub local_transform: Transform,
    pub collider: Collider,
}

pub struct InstalledEquipment {
    pub instance_id: String,
    pub equipment_id: String,
    pub mount_id: String,
    pub mode: EquipmentMode,
    pub glb_path: Option<String>,
    /// Chassis-local equipment transform; no model-root correction is needed.
    pub local_transform: Transform,
    pub colliders: Vec<InstalledCollider>,
}

struct ValidatedInstallation<'a> {
    entry: &'a EquipmentInstallation,
    definition: &'a EquipmentDefinition,
    local_transform: Transform,
}

fn validated_installations<'a>(
    mounts: &'a VehicleMountDefinition,
    catalog: &'a EquipmentCatalog,
    loadout: &'a VehicleEquipmentLoadout,
) -> Result<Vec<ValidatedInstallation<'a>>, String> {
    mounts.validate()?;
    catalog.validate()?;
    loadout.validate()?;
    if loadout.vehicle_id != mounts.vehicle_id {
        return Err(format!(
            "loadout vehicle {} does not match mount vehicle {}",
            loadout.vehicle_id, mounts.vehicle_id
        ));
    }
    let mount_by_id: HashMap<_, _> = mounts
        .mounts
        .iter()
        .map(|mount| (mount.id.as_str(), mount))
        .collect();
    let equipment_by_id: HashMap<_, _> = catalog
        .definitions
        .iter()
        .map(|definition| (definition.id.as_str(), definition))
        .collect();
    let mut occupied_mounts = HashSet::new();
    let mut occupied_zones = HashSet::new();
    let mut blocked_zones = HashSet::new();
    let mut resolved = Vec::with_capacity(loadout.equipment.len());
    for entry in &loadout.equipment {
        let mount = mount_by_id
            .get(entry.mount_id.as_str())
            .ok_or_else(|| format!("unknown mount: {}", entry.mount_id))?;
        let definition = equipment_by_id
            .get(entry.equipment_id.as_str())
            .ok_or_else(|| format!("unknown equipment: {}", entry.equipment_id))?;
        if !definition
            .compatible_mount_categories
            .contains(&mount.category)
            || !mount
                .allowed_equipment_types
                .contains(&definition.equipment_type)
        {
            return Err(format!(
                "equipment {} is incompatible with mount {}",
                definition.id, mount.id
            ));
        }
        if definition.mass_kg > mount.max_mass_kg {
            return Err(format!(
                "equipment {} exceeds mount {} mass limit",
                definition.id, mount.id
            ));
        }
        if !occupied_mounts.insert(mount.id.as_str()) {
            return Err(format!("mount {} is already occupied", mount.id));
        }
        for zone in &mount.occupied_zones {
            if occupied_zones.contains(zone.as_str()) || blocked_zones.contains(zone.as_str()) {
                return Err(format!(
                    "mount {} occupies a reserved or conflicting zone: {zone}",
                    mount.id
                ));
            }
        }
        for zone in &mount.conflicting_zones {
            if occupied_zones.contains(zone.as_str()) {
                return Err(format!(
                    "mount {} conflicts with occupied zone: {zone}",
                    mount.id
                ));
            }
        }
        occupied_zones.extend(mount.occupied_zones.iter().map(String::as_str));
        blocked_zones.extend(mount.conflicting_zones.iter().map(String::as_str));
        let mount_transform = mount.local_pose.transform()?;
        let asset_transform = definition.local_pose.transform()?;
        resolved.push(ValidatedInstallation {
            entry,
            definition,
            local_transform: Transform {
                translation: mount_transform.translation
                    + mount_transform.rotation * asset_transform.translation,
                rotation: (mount_transform.rotation * asset_transform.rotation).normalize(),
                scale: Vec3::ONE,
            },
        });
    }
    Ok(resolved)
}

/// Validate the entire request before returning any installation geometry.
pub fn resolve_loadout(
    mounts: &VehicleMountDefinition,
    catalog: &EquipmentCatalog,
    loadout: &VehicleEquipmentLoadout,
) -> Result<Vec<InstalledEquipment>, String> {
    validated_installations(mounts, catalog, loadout)?
        .into_iter()
        .map(|installation| {
            let colliders = installation
                .definition
                .colliders
                .iter()
                .map(|spec| {
                    Ok(InstalledCollider {
                        local_transform: spec.local_pose.transform()?,
                        collider: spec.collider()?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(InstalledEquipment {
                instance_id: installation.entry.instance_id.clone(),
                equipment_id: installation.entry.equipment_id.clone(),
                mount_id: installation.entry.mount_id.clone(),
                mode: installation.definition.mode,
                glb_path: installation.definition.glb_path.clone(),
                local_transform: installation.local_transform,
                colliders,
            })
        })
        .collect()
}

fn inertia_tensor(properties: MassProperties) -> DMat3 {
    let rotation = Mat3::from_quat(properties.principal_inertia_local_frame).as_dmat3();
    rotation * DMat3::from_diagonal(properties.principal_inertia.as_dvec3()) * rotation.transpose()
}

fn tensor_matches(properties: MassProperties, expected: DMat3) -> bool {
    if !properties.principal_inertia.is_finite()
        || properties.principal_inertia.min_element() <= 0.0
        || !properties.principal_inertia.recip().is_finite()
        || validate_rotation(
            properties.principal_inertia_local_frame.to_array(),
            "aggregate inertia frame",
        )
        .is_err()
    {
        return false;
    }
    // Float32 principal axes must reproduce the independently accumulated tensor.
    let scale = expected
        .to_cols_array()
        .into_iter()
        .map(f64::abs)
        .fold(0.0, f64::max);
    inertia_tensor(properties)
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
        .all(|(actual, expected)| actual.is_finite() && (actual - expected).abs() <= scale * 2.0e-5)
}

fn checked_aggregate(
    rapier_result: MassProperties,
    contributions: &[MassProperties],
) -> Result<MassProperties, String> {
    let mass: f64 = contributions.iter().map(|part| f64::from(part.mass)).sum();
    let center = contributions.iter().fold(DVec3::ZERO, |sum, part| {
        sum + part.local_center_of_mass.as_dvec3() * f64::from(part.mass)
    }) / mass;
    let tensor = contributions.iter().fold(DMat3::ZERO, |sum, part| {
        let offset = part.local_center_of_mass.as_dvec3() - center;
        let parallel_axis = (DMat3::from_diagonal(DVec3::splat(offset.length_squared()))
            - DMat3::from_cols(offset * offset.x, offset * offset.y, offset * offset.z))
            * f64::from(part.mass);
        sum + inertia_tensor(*part) + parallel_axis
    });
    if !(mass as f32).is_finite()
        || mass <= 0.0
        || !center.as_vec3().is_finite()
        || !tensor.as_mat3().is_finite()
    {
        return Err("equipment mass composition produced invalid mass properties".into());
    }
    let candidate = MassProperties {
        mass: mass as f32,
        local_center_of_mass: center.as_vec3(),
        ..rapier_result
    };
    if tensor_matches(candidate, tensor) {
        return Ok(candidate);
    }

    // A supplied principal frame can diagonalize a symmetric sum without an eigensolver.
    for frame in std::iter::once(Quat::IDENTITY).chain(
        contributions
            .iter()
            .map(|part| part.principal_inertia_local_frame),
    ) {
        let rotation = Mat3::from_quat(frame).as_dmat3();
        let projected = rotation.transpose() * tensor * rotation;
        let candidate = MassProperties {
            principal_inertia: Vec3::new(
                projected.x_axis.x as f32,
                projected.y_axis.y as f32,
                projected.z_axis.z as f32,
            ),
            principal_inertia_local_frame: frame,
            ..candidate
        };
        if tensor_matches(candidate, tensor) {
            return Ok(candidate);
        }
    }

    // Retry from the full tensor so a bad intermediate Add cannot contaminate a later sum.
    let recomposed = MassProperties::from_rapier(
        bevy_rapier3d::rapier::dynamics::MassProperties::with_inertia_matrix(
            candidate.local_center_of_mass,
            candidate.mass,
            tensor.as_mat3(),
        ),
    );
    let recomposed = MassProperties {
        mass: candidate.mass,
        local_center_of_mass: candidate.local_center_of_mass,
        ..recomposed
    };
    if tensor_matches(recomposed, tensor) {
        return Ok(recomposed);
    }
    Err("unsupported aggregate inertia: Parry principal-axis decomposition failed tensor residual validation; no supplied principal frame represents the full tensor accurately".into())
}

/// Base profile data is never modified. Empty loadouts return base exactly.
///
/// Results must reconstruct the full rotated and parallel-axis tensor within
/// 2e-5 of its largest entry. Known principal frames recover symmetric sums
/// when pinned Parry misdiagonalizes repeated moments. Otherwise a failed
/// decomposition is rejected explicitly, including physically valid loadouts
/// whose principal axes the dependency cannot represent accurately.
pub fn effective_mass_properties(
    base: MassProperties,
    mounts: &VehicleMountDefinition,
    catalog: &EquipmentCatalog,
    loadout: &VehicleEquipmentLoadout,
) -> Result<MassProperties, String> {
    let installations = validated_installations(mounts, catalog, loadout)?;
    if !installations
        .iter()
        .any(|installation| installation.definition.mode == EquipmentMode::Rigid)
    {
        return Ok(base);
    }
    if !base.mass.is_finite()
        || base.mass <= 0.0
        || !base.mass.recip().is_finite()
        || !base.local_center_of_mass.is_finite()
        || !base.principal_inertia.is_finite()
        || base.principal_inertia.min_element() <= 0.0
        || !base.principal_inertia.recip().is_finite()
    {
        return Err("base mass properties must have finite positive mass/inertia and a finite center of mass".into());
    }
    validate_rotation(
        base.principal_inertia_local_frame.to_array(),
        "base principal inertia frame",
    )?;
    let base = MassProperties {
        principal_inertia_local_frame: base.principal_inertia_local_frame.normalize(),
        ..base
    };
    let mut contributions = Vec::with_capacity(installations.len() + 1);
    contributions.push(base);
    let mut combined = base.into_rapier();
    for installation in installations {
        let definition = installation.definition;
        if definition.mode == EquipmentMode::Decorative {
            continue;
        }
        let local_mass = MassProperties {
            mass: definition.mass_kg,
            local_center_of_mass: Vec3::from_array(definition.local_center_of_mass),
            principal_inertia: Vec3::from_array(definition.principal_inertia),
            principal_inertia_local_frame: Quat::from_array(
                definition.principal_inertia_local_frame,
            )
            .normalize(),
        };
        let pose = Pose::from_parts(
            installation.local_transform.translation,
            installation.local_transform.rotation,
        );
        // Parry 0.27 names this transform_by; Add applies the parallel-axis theorem.
        let transformed = local_mass.into_rapier().transform_by(&pose);
        contributions.push(MassProperties {
            mass: local_mass.mass,
            principal_inertia: local_mass.principal_inertia,
            local_center_of_mass: transformed.local_com,
            principal_inertia_local_frame: transformed.principal_inertia_local_frame.normalize(),
        });
        combined += transformed;
    }
    checked_aggregate(MassProperties::from_rapier(combined), &contributions)
}

#[cfg(test)]
mod tests;
