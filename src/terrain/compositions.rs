//! Non-destructive groups and portable compositions of built-in obstacles.
use std::collections::HashSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::obstacles::Obstacle;
use super::scene::{MAX_OBSTACLES, validate_name, validate_obstacles};

pub const MAX_COMPOSITION_OBJECTS: usize = 64;
const COMPOSITION_FORMAT: &str = "motor-trail-composition";
const COMPOSITION_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectGroup {
    pub id: u64,
    pub name: String,
    pub members: Vec<u64>,
}

pub fn validate_groups(groups: &[ObjectGroup], objects: &[Obstacle]) -> Result<(), String> {
    if groups.len() > MAX_OBSTACLES / 2 { return Err("Trop de groupes".into()); }
    let mut ids = HashSet::new();
    let mut used = HashSet::new();
    let available: HashSet<u64> = objects.iter().map(|o| o.id).collect();
    for group in groups {
        if group.id == 0 || !ids.insert(group.id) {
            return Err("Identifiant de groupe nul ou duplique".into());
        }
        validate_name(&group.name)?;
        if group.members.len() < 2 || group.members.len() > MAX_OBSTACLES {
            return Err("Un groupe doit contenir entre 2 et 512 objets".into());
        }
        for member in &group.members {
            if !available.contains(member) || !used.insert(*member) {
                return Err(format!("Membre de groupe absent ou present dans plusieurs groupes: {member}"));
            }
        }
    }
    Ok(())
}

pub fn expanded_selection(id: u64, groups: &[ObjectGroup]) -> Vec<u64> {
    groups.iter().find(|g| g.members.contains(&id))
        .map_or_else(|| vec![id], |g| g.members.clone())
}

pub fn next_group_id(groups: &[ObjectGroup]) -> u64 {
    let existing: HashSet<u64> = groups.iter().map(|g| g.id).collect();
    (1..=groups.len() as u64 + 1).find(|id| !existing.contains(id)).unwrap_or(1)
}

pub fn prune_groups(groups: &[ObjectGroup], objects: &[Obstacle]) -> Vec<ObjectGroup> {
    let available: HashSet<u64> = objects.iter().map(|o| o.id).collect();
    groups.iter().filter_map(|g| {
        let mut copy = g.clone();
        copy.members.retain(|id| available.contains(id));
        (copy.members.len() >= 2).then_some(copy)
    }).collect()
}

pub fn center(objects: &[Obstacle]) -> Vec3 {
    let sum: Vec3 = objects.iter().map(|o| Vec3::from_array(o.position)).sum();
    if objects.is_empty() { Vec3::ZERO } else { sum / objects.len() as f32 }
}

/// Rotate and scale member centres around a shared pivot. Object Euler angles
/// use the existing YXZ convention; dimensions remain validated in metres.
pub fn transform_members(
    before: &[Obstacle], pivot_before: Vec3, pivot_after: Vec3,
    delta_angle: Quat, scale: Vec3,
) -> Result<Vec<Obstacle>, String> {
    if !pivot_after.is_finite() || !delta_angle.is_finite()
        || !scale.is_finite() || scale.min_element() <= 0.0 {
        return Err("Transformation de groupe invalide".into());
    }
    before.iter().map(|o| {
        let mut updated = o.clone();
        let start = Vec3::from_array(o.position) - pivot_before;
        updated.position = (pivot_after + delta_angle * (start * scale)).to_array();
        updated.size = (Vec3::from_array(o.size) * scale).to_array();
        // Standardized Euler YXZ rotation, same as Obstacle::transform.
        let rotation = delta_angle * o.transform().rotation;
        let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
        updated.rotation_degrees = [pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()];
        updated.validate()?;
        Ok(updated)
    }).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub format: String,
    pub version: u32,
    pub name: String,
    /// Local positions relative to the centroid; IDs are local and replaced on insertion.
    pub objects: Vec<Obstacle>,
}

impl Composition {
    pub fn from_selection(name: &str, objects: &[Obstacle]) -> Result<Self, String> {
        validate_name(name)?;
        if !(2..=MAX_COMPOSITION_OBJECTS).contains(&objects.len()) {
            return Err("Une composition doit contenir entre 2 et 64 objets".into());
        }
        validate_obstacles(objects)?;
        let pivot = center(objects);
        let mut relative = objects.to_vec();
        for o in &mut relative {
            o.position = (Vec3::from_array(o.position) - pivot).to_array();
        }
        let data = Self {
            format: COMPOSITION_FORMAT.into(),
            version: COMPOSITION_VERSION,
            name: name.into(),
            objects: relative,
        };
        data.validate()?;
        Ok(data)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.format != COMPOSITION_FORMAT || self.version != COMPOSITION_VERSION {
            return Err("Version de composition non reconnue".into());
        }
        validate_name(&self.name)?;
        if !(2..=MAX_COMPOSITION_OBJECTS).contains(&self.objects.len()) {
            return Err("Composition vide ou trop grande (max 64)".into());
        }
        validate_obstacles(&self.objects)
    }
    pub fn instantiate(
        &self, destination: Vec3, existing: &[Obstacle],
    ) -> Result<Vec<Obstacle>, String> {
        self.validate()?;
        if existing.len() + self.objects.len() > MAX_OBSTACLES {
            return Err("Limite de 512 objets atteinte".into());
        }
        if !destination.is_finite() {
            return Err("Destination de composition invalide".into());
        }
        let mut used: HashSet<u64> = existing.iter().map(|o| o.id).collect();
        let mut clones = Vec::with_capacity(self.objects.len());
        for template in &self.objects {
            // Same stable-ID allocation semantics as TerrainScene::next_id,
            // but also reserves fresh IDs within the inserted composition.
            let id = used.iter().copied().max().unwrap_or(0).checked_add(1)
                .filter(|id| *id > 0)
                .unwrap_or_else(|| (1u64..).find(|id| !used.contains(id)).unwrap());
            used.insert(id);
            let mut item = template.clone();
            item.id = id;
            item.position = (Vec3::from_array(item.position) + destination).to_array();
            item.validate()?;
            clones.push(item);
        }
        Ok(clones)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::obstacles::ObstacleKind;
    fn parts() -> Vec<Obstacle> {
        (1..=3).map(|id| Obstacle {
            id, kind: ObstacleKind::Cube, position: [id as f32 * 2.0, 1.0, 0.0],
            rotation_degrees: [0.0; 3], size: [2.0; 3],
        }).collect()
    }
    #[test]
    fn groups_reject_duplicated_and_missing_members() {
        let objects = parts();
        let g = ObjectGroup { id: 1, name: "Pont".into(), members: vec![1,2] };
        assert!(validate_groups(&[g.clone()], &objects).is_ok());
        assert!(validate_groups(&[g.clone(), ObjectGroup {
            id: 2, name: "Autre".into(), members: vec![2,3],
        }], &objects).is_err());
        assert!(validate_groups(&[ObjectGroup { members: vec![1,99], ..g }], &objects).is_err());
    }
    #[test]
    fn composition_roundtrip_and_fresh_ids_keep_relative_layout() {
        let items = parts();
        let c = Composition::from_selection("Pont forestier", &items).unwrap();
        let bytes = serde_json::to_vec(&c).unwrap();
        let read: Composition = serde_json::from_slice(&bytes).unwrap();
        let cloned = read.instantiate(Vec3::new(10.0, 5.0, 8.0), &items).unwrap();
        assert_eq!(cloned.len(), 3);
        assert!(cloned.iter().all(|o| o.id > 3));
        assert_eq!(cloned[1].position[0] - cloned[0].position[0], 2.0);
        assert!(cloned[0].position[1] > 0.0);
    }
    #[test]
    fn multiple_transforms_preserve_offsets_and_are_non_destructive() {
        let parts = parts();
        let pivot = center(&parts);
        let after = transform_members(&parts, pivot, pivot + Vec3::Y,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), Vec3::splat(1.5)).unwrap();
        assert_eq!(parts[0].position[1], 1.0);
        assert_eq!(after[0].position[1], 2.0);
        assert_eq!(after[0].size, [3.0; 3]);
        assert!((Vec3::from_array(after[1].position)-Vec3::from_array(after[0].position)).length() - 3.0 < 1e-5);
    }
}
