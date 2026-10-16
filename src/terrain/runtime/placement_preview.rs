//! Temporary render assets only. Canonical objects still go through commit_objects.
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{ActiveTerrain, EditorTool, TerrainSync, TerrainWorkshopState, WorkshopMode};
use crate::terrain::obstacles::{Obstacle, ObstacleKind};
use crate::terrain::scene::MAX_OBSTACLES;

pub(super) fn kind_index(kind: ObstacleKind) -> usize {
    match kind {
        ObstacleKind::Cube => 0,
        ObstacleKind::Cylinder => 1,
        ObstacleKind::Sphere => 2,
        ObstacleKind::Rock => 3,
        ObstacleKind::Ramp => 4,
        ObstacleKind::Nature(asset) => 5 + asset.index(),
    }
}

struct Candidate {
    object: Obstacle,
    error: Option<String>,
    revision: u64,
    requested_size: [u32; 3],
    requested_yaw: f32,
    requested_offset_y: u32,
    requested_follow_ground: bool,
}

#[derive(Resource, Default)]
pub(super) struct PlacementPreview {
    candidate: Option<Candidate>,
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    geometry: Option<(ObstacleKind, [f32; 3])>,
}

impl PlacementPreview {
    pub(super) fn invalidate(&mut self) {
        self.candidate = None;
    }

    pub(super) fn sample(
        &mut self,
        state: &TerrainWorkshopState,
        active: &ActiveTerrain,
        ray: Ray3d,
        ground: Option<Vec3>,
    ) {
        // A missed ray uses its current intersection with a reference plane, never
        // the previous hit. Looking above the horizon simply hides the preview.
        let point = ground.or_else(|| {
            let height = active.scene.terrain.height(0.0, 0.0).unwrap_or(0.0);
            let distance = (height - ray.origin.y) / ray.direction.y;
            (distance.is_finite() && (0.0..=2000.0).contains(&distance))
                .then(|| ray.origin + *ray.direction * distance)
        });
        let Some(point) = point.filter(|point| point.is_finite()) else {
            return;
        };
        let yaw = state.placement_yaw_degrees(state.object_kind);
        let mut object = Obstacle {
            id: active.scene.next_id(),
            kind: state.object_kind,
            position: point.to_array(),
            rotation_degrees: [0.0, yaw, 0.0],
            size: state.object_size,
        };
        let validation = object.validate().and_then(|_| {
            if ground.is_none() {
                return Err("Placement hors du terrain".into());
            }
            if state.placement_follow_ground {
                object.align_to_ground(&active.scene.terrain)?;
            } else {
                // In free mode keep the model upright, permit overlap with
                // other objects and intentional burial in the heightmap.
                object.position[1] = point.y + object.size[1] * 0.5;
            }
            object.position[1] += state.placement_offset_y;
            object.validate()?;
            active.scene.validate()?;
            if active.scene.obstacles.len() >= MAX_OBSTACLES {
                return Err(format!("Limite de {MAX_OBSTACLES} objets atteinte"));
            }
            Ok(())
        });
        let error = validation.err();
        if error.is_some() {
            // Invalid inputs get finite display geometry, but are never committable.
            object.size = object.size.map(|v| {
                if v.is_finite() {
                    v.clamp(0.1, 32.0)
                } else {
                    1.0
                }
            });
            object.rotation_degrees = [0.0, yaw, 0.0];
            object.position = (point + Vec3::Y * object.size[1] * 0.5).to_array();
        }
        self.candidate = Some(Candidate {
            object,
            error,
            revision: active.revision,
            requested_size: state.object_size.map(f32::to_bits),
            requested_yaw: yaw,
            requested_offset_y: state.placement_offset_y.to_bits(),
            requested_follow_ground: state.placement_follow_ground,
        });
    }

    pub(super) fn object(&self) -> Result<&Obstacle, &str> {
        let candidate = self
            .candidate
            .as_ref()
            .ok_or("Aucun point de placement valide")?;
        match &candidate.error {
            Some(error) => Err(error),
            None => Ok(&candidate.object),
        }
    }

    pub(super) fn confirmed(&mut self, revision: u64) {
        if let Some(candidate) = &mut self.candidate {
            candidate.revision = revision;
        }
    }

    pub(super) fn reject(&mut self, error: String) {
        if let Some(candidate) = &mut self.candidate {
            candidate.error = Some(error);
        }
    }

    fn clear_assets(&mut self, world: &mut World) {
        if let Some(entity) = self.entity.take() {
            world.despawn(entity);
        }
        if let Some(mesh) = self.mesh.take() {
            world.resource_mut::<Assets<Mesh>>().remove(mesh.id());
        }
        if let Some(material) = self.material.take() {
            world
                .resource_mut::<Assets<StandardMaterial>>()
                .remove(material.id());
        }
        self.geometry = None;
    }

    fn render(&mut self, world: &mut World) {
        let state = world.resource::<TerrainWorkshopState>();
        let active = world.resource::<ActiveTerrain>();
        let applicable = state.mode == WorkshopMode::Edit
            && state.tool == EditorTool::Place
            && !world.resource::<TerrainSync>().full_reload
            && self.candidate.as_ref().is_some_and(|candidate| {
                candidate.revision == active.revision
                    && candidate.object.kind == state.object_kind
                    && candidate.requested_size == state.object_size.map(f32::to_bits)
                    && candidate.requested_yaw == state.placement_yaw_degrees(state.object_kind)
                    && candidate.requested_offset_y == state.placement_offset_y.to_bits()
                    && candidate.requested_follow_ground == state.placement_follow_ground
            });
        if !applicable {
            self.invalidate();
            self.clear_assets(world);
            return;
        }
        let candidate = self.candidate.as_ref().unwrap();
        let geometry = (candidate.object.kind, candidate.object.size);
        let transform = candidate.object.transform();
        let valid = candidate.error.is_none();
        if self.geometry != Some(geometry) {
            // Committed objects use this same geometry-only factory.
            let prototype = Obstacle {
                position: [0.0; 3],
                rotation_degrees: [0.0; 3],
                ..candidate.object.clone()
            };
            let Ok(mut mesh) = prototype.build_mesh() else {
                self.invalidate();
                self.clear_assets(world);
                return;
            };
            mesh.remove_attribute(Mesh::ATTRIBUTE_COLOR);
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            if let Some(handle) = &self.mesh {
                let _ = meshes.insert(handle.id(), mesh);
            } else {
                self.mesh = Some(meshes.add(mesh));
            }
            self.geometry = Some(geometry);
        }
        let color = if valid {
            Color::srgba(0.12, 0.95, 0.34, 0.42)
        } else {
            Color::srgba(1.0, 0.12, 0.10, 0.42)
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        if let Some(handle) = &self.material {
            if let Some(mut material) = materials.get_mut(handle) {
                material.base_color = color;
            }
        } else {
            self.material = Some(materials.add(StandardMaterial {
                base_color: color,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.9,
                double_sided: true,
                cull_mode: None,
                ..default()
            }));
        }
        if let Some(entity) = self.entity {
            world.entity_mut(entity).insert(transform);
        } else {
            self.entity = Some(
                world
                    .spawn((
                        Name::new("Placement preview"),
                        Mesh3d(self.mesh.as_ref().unwrap().clone()),
                        MeshMaterial3d(self.material.as_ref().unwrap().clone()),
                        transform,
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }
    }
}

pub(super) fn render(world: &mut World) {
    world.resource_scope(|world, mut preview: Mut<PlacementPreview>| preview.render(world));
}

pub(super) fn remove(world: &mut World) {
    if world.contains_resource::<PlacementPreview>() {
        world.resource_scope(|world, mut preview: Mut<PlacementPreview>| {
            preview.invalidate();
            preview.clear_assets(world);
        });
    }
}
