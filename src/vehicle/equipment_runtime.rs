//! Transactional rigid attachments; base physics profiles remain separate.
#[cfg(test)]
mod integration_tests;

use super::equipment::*;
use super::mounting::{LocalPose, VehicleMountDefinition};
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct EquipmentRequest {
    pub mounts: VehicleMountDefinition,
    pub catalog: EquipmentCatalog,
    pub loadout: VehicleEquipmentLoadout,
}

impl Default for EquipmentRequest {
    fn default() -> Self {
        Self {
            mounts: VehicleMountDefinition::motor_trail().expect("checked-in mount definition"),
            catalog: EquipmentCatalog::default(),
            loadout: VehicleEquipmentLoadout::empty("motor_trail_4x4"),
        }
    }
}

impl EquipmentRequest {
    pub fn demo() -> Self {
        let mut request = Self::default();
        let mass = 40.0;
        let half = Vec3::new(0.3, 0.2, 0.4);
        let inertia = mass / 3.0
            * Vec3::new(
                half.y * half.y + half.z * half.z,
                half.x * half.x + half.z * half.z,
                half.x * half.x + half.y * half.y,
            );
        request.catalog.definitions.push(EquipmentDefinition {
            id: "demo_roof_box".into(),
            equipment_type: "cargo".into(),
            glb_path: None,
            local_pose: LocalPose {
                translation: [0.0, 0.5, 0.0],
                ..default()
            },
            mass_kg: mass,
            local_center_of_mass: [0.0; 3],
            principal_inertia: inertia.to_array(),
            principal_inertia_local_frame: Quat::IDENTITY.to_array(),
            compatible_mount_categories: vec!["roof".into()],
            mode: EquipmentMode::Rigid,
            colliders: vec![EquipmentCollider {
                local_pose: LocalPose::default(),
                shape: EquipmentShape::Cuboid {
                    half_extents: half.to_array(),
                },
            }],
        });
        request.loadout.equipment.push(EquipmentInstallation {
            instance_id: "demo_roof_1".into(),
            equipment_id: "demo_roof_box".into(),
            mount_id: "roof_main".into(),
        });
        request
    }
}

#[derive(Component)]
pub struct MountedEquipment {
    pub instance_id: String,
}

#[derive(Resource, Default)]
pub struct EquipmentRuntime {
    pub error: Option<String>,
    pub revision: u64,
    active: EquipmentRequest,
    owner: Option<Entity>,
    entities: Vec<Entity>,
    last_attempt: Option<EquipmentRequest>,
    last_profile: Option<super::config::VehiclePhysicsConfig>,
    meshes: Vec<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
    bounds: Option<(Vec3, Vec3)>,
}

impl EquipmentRuntime {
    pub fn active(&self) -> &EquipmentRequest {
        &self.active
    }
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.bounds.unwrap_or_else(super::collision::body_bounds)
    }
    pub fn effective_mass(&self, base: MassProperties) -> Result<MassProperties, String> {
        effective_mass_properties(
            base,
            &self.active.mounts,
            &self.active.catalog,
            &self.active.loadout,
        )
    }
}

pub fn sync(world: &mut World) {
    let owner = world
        .query_filtered::<Entity, With<super::Truck>>()
        .single(world)
        .ok();
    world.resource_scope(|world, state: Mut<EquipmentRuntime>| {
        let mut state = state;
        let Some(owner) = owner else {
            if state.owner.is_some() {
                clear(world, &mut state);
                state.owner = None;
            }
            return;
        };
        let request = world.resource::<EquipmentRequest>();
        let config = world.resource::<super::config::VehiclePhysicsConfig>();
        let owner_changed = state.owner != Some(owner);
        let attachments_intact = state.entities.iter().all(|entity| {
            world
                .get::<ChildOf>(*entity)
                .is_some_and(|parent| parent.parent() == owner)
        });
        if state.owner == Some(owner)
            && state.last_attempt.as_ref() == Some(request)
            && state.last_profile.as_ref() == Some(config)
            && attachments_intact
        {
            return;
        }
        let request = request.clone();
        let config = config.clone();
        state.last_attempt = Some(request.clone());
        state.last_profile = Some(config.clone());
        let (request, installed, error) = match prepare(&request, &config) {
            Ok(installed) => (request, installed, None),
            Err(error) => {
                if !owner_changed && attachments_intact {
                    state.error = Some(error);
                    return;
                }
                // A rejected replacement must not strand the last valid loadout on a dead owner.
                let active = state.active.clone();
                match prepare(&active, &config) {
                    Ok(installed) => (active, installed, Some(error)),
                    Err(fallback_error) => {
                        clear(world, &mut state);
                        state.owner = None;
                        state.error = Some(format!(
                            "{error}; active loadout could not be restored: {fallback_error}"
                        ));
                        return;
                    }
                }
            }
        };
        if !owner_changed && attachments_intact && state.active == request {
            if state.error.is_some() && error.is_none() {
                // Returning to the cached source profile must also retry controller application.
                state.revision = state.revision.wrapping_add(1);
            }
            state.error = error;
            return;
        }
        clear(world, &mut state);
        let (mut minimum, mut maximum) = super::collision::body_bounds();
        for item in installed {
            let root = world
                .spawn((
                    Name::new(format!(
                        "Equipment {} on {}",
                        item.equipment_id, item.mount_id
                    )),
                    MountedEquipment {
                        instance_id: item.instance_id,
                    },
                    item.local_transform,
                    Visibility::default(),
                    ChildOf(owner),
                ))
                .id();
            if item.mode == EquipmentMode::Rigid {
                let shape = Collider::compound(
                    item.colliders
                        .iter()
                        .map(|part| {
                            let local = item.local_transform.mul_transform(part.local_transform);
                            let aabb = part.collider.raw.compute_aabb(
                                &bevy_rapier3d::rapier::math::Pose::from_parts(
                                    local.translation,
                                    local.rotation,
                                ),
                            );
                            minimum = minimum.min(aabb.mins);
                            maximum = maximum.max(aabb.maxs);
                            (
                                part.local_transform.translation,
                                part.local_transform.rotation,
                                part.collider.clone(),
                            )
                        })
                        .collect(),
                );
                world
                    .entity_mut(root)
                    .insert((shape, ColliderMassProperties::Density(0.0)));
            }
            let graphic = world
                .spawn((
                    Name::new("Equipment graphics"),
                    super::presentation::VehicleGraphic::new(
                        owner,
                        item.local_transform,
                        Transform::IDENTITY,
                    ),
                    Transform::IDENTITY,
                    Visibility::default(),
                    ChildOf(root),
                ))
                .id();
            if let (Some(path), Some(server)) =
                (&item.glb_path, world.get_resource::<AssetServer>())
            {
                let scene = server.load(GltfAssetLabel::Scene(0).from_asset(path.clone()));
                world.spawn((WorldAssetRoot(scene), Transform::IDENTITY, ChildOf(graphic)));
            } else if world.contains_resource::<Assets<Mesh>>()
                && world.contains_resource::<Assets<StandardMaterial>>()
            {
                // Proxy geometry validates rigid attachments before detailed assets exist.
                for part in &item.colliders {
                    let aabb = part.collider.raw.compute_local_aabb();
                    let mesh = world
                        .resource_mut::<Assets<Mesh>>()
                        .add(Cuboid::from_size(aabb.maxs - aabb.mins));
                    let material =
                        world
                            .resource_mut::<Assets<StandardMaterial>>()
                            .add(StandardMaterial {
                                base_color: Color::srgb(0.12, 0.68, 0.75),
                                perceptual_roughness: 0.8,
                                ..default()
                            });
                    world.spawn((
                        Name::new("Equipment proxy"),
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        part.local_transform
                            .mul_transform(Transform::from_translation(
                                (aabb.mins + aabb.maxs) * 0.5,
                            )),
                        ChildOf(graphic),
                    ));
                    state.meshes.push(mesh);
                    state.materials.push(material);
                }
            }
            state.entities.push(root);
        }
        state.active = request;
        state.owner = Some(owner);
        state.bounds = Some((minimum, maximum));
        state.revision = state.revision.wrapping_add(1);
        state.error = error;
    });
}

fn prepare(
    request: &EquipmentRequest,
    config: &super::config::VehiclePhysicsConfig,
) -> Result<Vec<InstalledEquipment>, String> {
    config.validate()?;
    let props = effective_mass_properties(
        super::controller::mass_properties(config),
        &request.mounts,
        &request.catalog,
        &request.loadout,
    )?;
    let mut effective = config.clone();
    effective.chassis.mass_kg = props.mass;
    effective.chassis.center_of_mass = props.local_center_of_mass.to_array();
    effective
        .validate()
        .map_err(|error| format!("Charge incompatible avec le profil : {error}"))?;
    resolve_loadout(&request.mounts, &request.catalog, &request.loadout)
}

fn clear(world: &mut World, state: &mut EquipmentRuntime) {
    for entity in state.entities.drain(..) {
        world.despawn(entity);
    }
    if let Some(mut assets) = world.get_resource_mut::<Assets<Mesh>>() {
        for mesh in &state.meshes {
            assets.remove(mesh.id());
        }
    }
    state.meshes.clear();
    if let Some(mut assets) = world.get_resource_mut::<Assets<StandardMaterial>>() {
        for material in &state.materials {
            assets.remove(material.id());
        }
    }
    state.materials.clear();
    state.bounds = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::config::VehiclePhysicsConfig;
    use crate::vehicle::{Truck, VehiclePlugin};
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    pub(super) fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, VehiclePlugin))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<crate::GameState>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 120.0,
            )));
        let car = app
            .world_mut()
            .spawn((
                crate::vehicle::controller::chassis_bundle(&VehiclePhysicsConfig::baseline()),
                Transform::from_xyz(0.0, 4.0, 0.0),
            ))
            .id();
        (app, car)
    }

    #[test]
    fn equipment_runtime_attaches_one_rigid_child_without_another_body() {
        let (mut app, car) = app();
        app.insert_resource(EquipmentRequest::demo());
        for _ in 0..5 {
            app.update();
        }
        let mut q=app.world_mut().query_filtered::<(Entity,&ChildOf,&Transform,&ColliderMassProperties),With<MountedEquipment>>();
        let items: Vec<_> = q.iter(app.world()).collect();
        assert_eq!(items.len(), 1, "the rigid dummy was not installed");
        let (entity, parent, pose, mass) = items[0];
        assert_eq!(parent.parent(), car);
        assert!(pose.translation.distance(Vec3::new(0.0, 1.74, -0.42)) < 1e-5);
        assert_eq!(*mass, ColliderMassProperties::Density(0.0));
        assert!(app.world().get::<RigidBody>(entity).is_none());
        assert!(app.world().get::<RapierColliderHandle>(entity).is_some());
        assert_eq!(
            app.world()
                .get::<Collider>(entity)
                .unwrap()
                .raw
                .compute_local_aabb()
                .half_extents(),
            Vec3::new(0.3, 0.2, 0.4)
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<Truck>>()
                .iter(app.world())
                .count(),
            1
        );
    }
}
