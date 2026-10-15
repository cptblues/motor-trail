use super::*;
use crate::vehicle::config::VehiclePhysicsConfig;
use crate::vehicle::contacts::{MODEL_TIRE_HALF_WIDTH, WheelCast, cast_envelope};
use crate::vehicle::controller::{MOUNTS, mass_properties};
use crate::vehicle::wheel_backend::TireContactMode;
use crate::vehicle::wheel_settings::WheelContactSettings;
use bevy::ecs::system::RunSystemOnce;
use bevy::math::Mat3;

fn advance(app: &mut App) {
    for _ in 0..8 {
        app.update();
    }
}

fn setup(request: EquipmentRequest) -> (App, Entity) {
    let (mut app, car) = super::tests::app();
    app.world_mut().entity_mut(car).insert(GravityScale(0.0));
    app.insert_resource(request);
    advance(&mut app);
    (app, car)
}

fn roots(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, With<MountedEquipment>>()
        .iter(world)
        .collect()
}

fn raw_mass(app: &mut App, car: Entity) -> MassProperties {
    let handle = app.world().get::<RapierRigidBodyHandle>(car).unwrap().0;
    app.world_mut()
        .run_system_once(move |context: ReadRapierContext| {
            let context = context.single().unwrap();
            MassProperties::from_rapier(
                context.rigidbody_set.bodies[handle]
                    .mass_properties()
                    .local_mprops,
            )
        })
        .unwrap()
}

fn tensor(properties: MassProperties) -> Mat3 {
    let frame = Mat3::from_quat(properties.principal_inertia_local_frame);
    frame * Mat3::from_diagonal(properties.principal_inertia) * frame.transpose()
}

fn assert_mass(actual: MassProperties, expected: MassProperties) {
    assert!(
        (actual.mass - expected.mass).abs() < 0.01,
        "{actual:?} != {expected:?}"
    );
    assert!(
        actual
            .local_center_of_mass
            .distance(expected.local_center_of_mass)
            < 1.0e-5
    );
    for (actual, expected) in tensor(actual)
        .to_cols_array()
        .into_iter()
        .zip(tensor(expected).to_cols_array())
    {
        assert!(
            (actual - expected).abs() < 0.05,
            "inertia {actual} != {expected}"
        );
    }
}

fn expected_mass(profile: &VehiclePhysicsConfig, request: &EquipmentRequest) -> MassProperties {
    effective_mass_properties(
        mass_properties(profile),
        &request.mounts,
        &request.catalog,
        &request.loadout,
    )
    .unwrap()
}

#[test]
fn equipment_runtime_demo_updates_actual_rapier_mass_com_and_tensor_without_changing_source() {
    let request = EquipmentRequest::demo();
    let source = VehiclePhysicsConfig::baseline();
    let source_json = serde_json::to_string(&source).unwrap();
    let (mut app, car) = setup(request.clone());
    let expected = expected_mass(&source, &request);
    assert!((expected.mass - (source.chassis.mass_kg + 40.0)).abs() < 0.01);
    let expected_com = (Vec3::from_array(source.chassis.center_of_mass) * source.chassis.mass_kg
        + Vec3::new(0.0, 1.74, -0.42) * 40.0)
        / (source.chassis.mass_kg + 40.0);
    assert!(expected.local_center_of_mass.distance(expected_com) < 1.0e-5);
    assert_mass(raw_mass(&mut app, car), expected);
    assert_mass(
        *app.world().get::<ReadMassProperties>(car).unwrap().get(),
        expected,
    );
    let revision = app.world().resource::<EquipmentRuntime>().revision;
    advance(&mut app);
    assert_eq!(
        app.world().resource::<EquipmentRuntime>().revision,
        revision
    );
    assert_mass(raw_mass(&mut app, car), expected);
    assert_eq!(
        serde_json::to_string(app.world().resource::<VehiclePhysicsConfig>()).unwrap(),
        source_json
    );
}

#[test]
fn equipment_runtime_live_profile_change_revert_and_disable_restore_base_properties() {
    let request = EquipmentRequest::demo();
    let source = VehiclePhysicsConfig::baseline();
    let (mut app, car) = setup(request.clone());
    let mut changed = source.clone();
    changed.chassis.mass_kg = 2200.0;
    changed.chassis.center_of_mass = [0.1, -0.4, -0.1];
    changed.validate().unwrap();
    for profile in [changed, source.clone()] {
        app.insert_resource(profile.clone());
        advance(&mut app);
        assert_mass(raw_mass(&mut app, car), expected_mass(&profile, &request));
        assert_eq!(*app.world().resource::<VehiclePhysicsConfig>(), profile);
        assert!(
            app.world()
                .resource::<crate::vehicle::VehiclePhysics>()
                .error
                .is_none()
        );
    }
    app.insert_resource(EquipmentRequest::default());
    advance(&mut app);
    assert!(roots(app.world_mut()).is_empty());
    assert_mass(raw_mass(&mut app, car), mass_properties(&source));
    assert_eq!(
        *app.world().get::<ColliderMassProperties>(car).unwrap(),
        ColliderMassProperties::MassProperties(mass_properties(&source))
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 0);
    assert_eq!(*app.world().resource::<VehiclePhysicsConfig>(), source);
}

#[test]
fn equipment_runtime_rotated_and_inverted_children_follow_the_same_rapier_body() {
    let (mut app, car) = setup(EquipmentRequest::demo());
    let child = roots(app.world_mut())[0];
    let local = *app.world().get::<Transform>(child).unwrap();
    for rotation in [
        Quat::from_rotation_y(0.7) * Quat::from_rotation_z(0.6),
        Quat::from_rotation_y(-0.4) * Quat::from_rotation_z(std::f32::consts::PI),
    ] {
        *app.world_mut().get_mut::<Transform>(car).unwrap() =
            Transform::from_xyz(2.0, 6.0, -3.0).with_rotation(rotation);
        advance(&mut app);
        let global = *app.world().get::<GlobalTransform>(child).unwrap();
        let owner_handle = app.world().get::<RapierRigidBodyHandle>(car).unwrap().0;
        let child_handle = app.world().get::<RapierColliderHandle>(child).unwrap().0;
        app.world_mut()
            .run_system_once(move |context: ReadRapierContext| {
                let context = context.single().unwrap();
                let body = &context.rigidbody_set.bodies[owner_handle];
                let collider = &context.colliders.colliders[child_handle];
                let expected_position = body.position() * local.translation;
                let expected_rotation = body.position().rotation * local.rotation;
                assert_eq!(collider.parent(), Some(owner_handle));
                assert!(collider.position().translation.distance(expected_position) < 1.0e-4);
                assert!(collider.position().rotation.dot(expected_rotation).abs() > 0.99999);
                assert!(global.translation().distance(expected_position) < 1.0e-4);
                assert!(global.rotation().dot(expected_rotation).abs() > 0.99999);
            })
            .unwrap();
        assert!(app.world().get::<RigidBody>(child).is_none());
    }
}

#[test]
fn equipment_runtime_overlapping_geometry_has_no_active_internal_contacts_or_response() {
    let mut request = EquipmentRequest::demo();
    request.catalog.definitions[0].local_pose.translation = [0.0, -1.0, 0.42];
    let (mut app, car) = setup(request);
    let child = roots(app.world_mut())[0];
    let body_collider = app.world().get::<RapierColliderHandle>(car).unwrap().0;
    let child_collider = app.world().get::<RapierColliderHandle>(child).unwrap().0;
    app.world_mut()
        .run_system_once(move |context: ReadRapierContext| {
            let context = context.single().unwrap();
            let chassis = &context.colliders.colliders[body_collider];
            let equipment = &context.colliders.colliders[child_collider];
            assert!(
                bevy_rapier3d::parry::query::intersection_test(
                    chassis.position(),
                    chassis.shape(),
                    equipment.position(),
                    equipment.shape()
                )
                .unwrap()
            );
            assert_eq!(equipment.parent(), chassis.parent());
            // Rapier retains broad-phase pairs even when same-body filtering disables contacts.
            for pair in context.contact_pairs_with(child) {
                assert!(!pair.has_any_active_contact());
                assert_eq!(pair.raw.total_impulse_magnitude(), 0.0);
                assert!(
                    pair.raw
                        .manifolds
                        .iter()
                        .all(|manifold| manifold.data.solver_contacts.is_empty())
                );
            }
            let body = &context.rigidbody_set.bodies[chassis.parent().unwrap()];
            assert!(body.linvel().length() < 1.0e-6);
            assert!(body.angvel().length() < 1.0e-6);
            assert!(
                body.position()
                    .translation
                    .distance(Vec3::new(0.0, 4.0, 0.0))
                    < 1.0e-6
            );
        })
        .unwrap();
}

#[test]
fn equipment_runtime_native_and_envelope_queries_exclude_owned_equipment_and_sensors() {
    for mode in [TireContactMode::Native, TireContactMode::Envelope] {
        let mut request = EquipmentRequest::demo();
        request.catalog.definitions[0].local_pose.translation = [0.0, -2.0, 0.42];
        request.catalog.definitions[0].colliders[0].shape = EquipmentShape::Cuboid {
            half_extents: [1.3, 0.04, 1.9],
        };
        let (mut app, car) = setup(request);
        app.world_mut().entity_mut(car).insert(LockedAxes::all());
        app.world_mut()
            .resource_mut::<WheelContactSettings>()
            .experimental = mode == TireContactMode::Envelope;
        let floor = app
            .world_mut()
            .spawn((
                RigidBody::Fixed,
                Collider::cuboid(8.0, 0.1, 8.0),
                Transform::from_xyz(0.0, 2.9, 0.0),
            ))
            .id();
        app.world_mut().spawn((
            RigidBody::Fixed,
            Sensor,
            Collider::cuboid(8.0, 0.02, 8.0),
            Transform::from_xyz(0.0, 3.55, 0.0),
        ));
        advance(&mut app);
        let child = roots(app.world_mut())[0];
        let owner_handle = app.world().get::<RapierRigidBodyHandle>(car).unwrap().0;
        app.world_mut()
            .run_system_once(move |context: ReadRapierContext| {
                let context = context.single().unwrap();
                let pose = context.rigidbody_set.bodies[owner_handle].position();
                for mount in MOUNTS {
                    let origin = pose * mount;
                    let down = pose.rotation * Vec3::NEG_Y;
                    let axle = pose.rotation * Vec3::NEG_X;
                    let unfiltered = context
                        .cast_ray(
                            origin,
                            down,
                            0.89,
                            true,
                            QueryFilter::default()
                                .exclude_collider(car)
                                .exclude_sensors(),
                        )
                        .unwrap();
                    assert_eq!(
                        unfiltered.0, child,
                        "fixture must expose an equipment self-hit"
                    );
                    let filter = QueryFilter::default()
                        .exclude_rigid_body(car)
                        .exclude_sensors();
                    assert_eq!(
                        context
                            .cast_ray(origin, down, 0.89, true, filter)
                            .unwrap()
                            .0,
                        floor
                    );
                    let hit = context
                        .with_query_pipeline(filter, |pipeline| {
                            cast_envelope(
                                &pipeline.query_pipeline,
                                WheelCast {
                                    mount: origin,
                                    down,
                                    axle,
                                    radius: 0.53,
                                    half_width: MODEL_TIRE_HALF_WIDTH,
                                    rest_length: 0.36,
                                },
                            )
                        })
                        .expect("filtered envelope must reach the external floor");
                    assert_eq!(context.colliders.collider_entity(hit.collider), Some(floor));
                }
            })
            .unwrap();
        let physics = app.world().resource::<crate::vehicle::VehiclePhysics>();
        let controller = physics.controller.as_ref().unwrap();
        assert_eq!(controller.contact_mode(), mode);
        let floor_handle = app.world().get::<RapierColliderHandle>(floor).unwrap().0;
        for wheel in controller.wheels() {
            assert!(wheel.raycast_info().is_in_contact);
            assert_eq!(wheel.raycast_info().ground_object, Some(floor_handle));
        }
    }
}

#[test]
fn equipment_runtime_invalid_replacement_preserves_geometry_assets_and_actual_mass() {
    let valid = EquipmentRequest::demo();
    let (mut app, car) = setup(valid.clone());
    let old_entities = roots(app.world_mut());
    let old_mass = raw_mass(&mut app, car);
    let old_meshes = app.world().resource::<EquipmentRuntime>().meshes.clone();
    let revision = app.world().resource::<EquipmentRuntime>().revision;
    let bounds = app.world().resource::<EquipmentRuntime>().bounds();
    let mut invalid = valid.clone();
    invalid.loadout.equipment[0].mount_id = "unknown_mount".into();
    app.insert_resource(invalid);
    advance(&mut app);
    let runtime = app.world().resource::<EquipmentRuntime>();
    assert!(runtime.error.is_some());
    assert_eq!(runtime.active(), &valid);
    assert_eq!(runtime.revision, revision);
    assert_eq!(runtime.bounds(), bounds);
    assert_eq!(roots(app.world_mut()), old_entities);
    assert_mass(raw_mass(&mut app, car), old_mass);
    for handle in old_meshes {
        assert!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&handle)
                .is_some()
        );
    }
    app.insert_resource(valid);
    advance(&mut app);
    assert!(app.world().resource::<EquipmentRuntime>().error.is_none());
}

#[test]
fn equipment_runtime_retries_the_same_request_after_a_rejected_base_profile_is_corrected() {
    let (mut app, car) = setup(EquipmentRequest::default());
    let source = app.world().resource::<VehiclePhysicsConfig>().clone();
    app.world_mut()
        .resource_mut::<VehiclePhysicsConfig>()
        .chassis
        .mass_kg = 0.0;
    app.insert_resource(EquipmentRequest::demo());
    advance(&mut app);
    assert!(app.world().resource::<EquipmentRuntime>().error.is_some());
    assert!(roots(app.world_mut()).is_empty());
    app.insert_resource(source.clone());
    advance(&mut app);
    assert!(
        app.world().resource::<EquipmentRuntime>().error.is_none(),
        "profile correction must invalidate the rejected-request cache"
    );
    assert_eq!(roots(app.world_mut()).len(), 1);
    assert_mass(
        raw_mass(&mut app, car),
        expected_mass(&source, &EquipmentRequest::demo()),
    );
}

#[test]
fn equipment_runtime_correcting_a_profile_clears_errors_without_replacing_valid_children() {
    let request = EquipmentRequest::demo();
    let (mut app, car) = setup(request.clone());
    let source = app.world().resource::<VehiclePhysicsConfig>().clone();
    let children = roots(app.world_mut());
    app.world_mut()
        .resource_mut::<VehiclePhysicsConfig>()
        .chassis
        .mass_kg = 0.0;
    advance(&mut app);
    assert!(app.world().resource::<EquipmentRuntime>().error.is_some());
    assert!(
        app.world()
            .resource::<crate::vehicle::VehiclePhysics>()
            .error
            .is_some()
    );
    app.insert_resource(source.clone());
    advance(&mut app);
    assert!(app.world().resource::<EquipmentRuntime>().error.is_none());
    assert!(
        app.world()
            .resource::<crate::vehicle::VehiclePhysics>()
            .error
            .is_none()
    );
    assert_eq!(roots(app.world_mut()), children);
    assert_mass(raw_mass(&mut app, car), expected_mass(&source, &request));
}

#[test]
fn equipment_runtime_owner_despawn_and_respawn_clean_entities_handles_and_assets() {
    let request = EquipmentRequest::demo();
    let (mut app, car) = setup(request.clone());
    let old_roots = roots(app.world_mut());
    let old_meshes = app.world().resource::<EquipmentRuntime>().meshes.clone();
    let old_materials = app.world().resource::<EquipmentRuntime>().materials.clone();
    let old_collider = app
        .world()
        .get::<RapierColliderHandle>(old_roots[0])
        .unwrap()
        .0;
    assert!(app.world_mut().despawn(car));
    advance(&mut app);
    assert!(roots(app.world_mut()).is_empty());
    assert!(app.world().resource::<EquipmentRuntime>().owner.is_none());
    assert_eq!(
        app.world().resource::<EquipmentRuntime>().bounds(),
        crate::vehicle::collision::body_bounds()
    );
    for entity in old_roots {
        assert!(app.world().get_entity(entity).is_err());
    }
    for handle in old_meshes {
        assert!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&handle)
                .is_none()
        );
    }
    for handle in old_materials {
        assert!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .is_none()
        );
    }
    app.world_mut()
        .run_system_once(move |context: ReadRapierContext| {
            assert!(
                context
                    .single()
                    .unwrap()
                    .colliders
                    .colliders
                    .get(old_collider)
                    .is_none()
            );
        })
        .unwrap();
    let replacement = app
        .world_mut()
        .spawn((
            crate::vehicle::controller::chassis_bundle(&VehiclePhysicsConfig::baseline()),
            Transform::from_xyz(0.0, 4.0, 0.0),
            GravityScale(0.0),
        ))
        .id();
    advance(&mut app);
    let new_roots = roots(app.world_mut());
    assert_eq!(new_roots.len(), 1);
    assert_eq!(
        app.world().get::<ChildOf>(new_roots[0]).unwrap().parent(),
        replacement
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 1);
    assert_mass(
        raw_mass(&mut app, replacement),
        expected_mass(&VehiclePhysicsConfig::baseline(), &request),
    );
}

#[test]
fn equipment_runtime_invalid_pending_replacement_restores_last_valid_loadout_on_a_new_owner() {
    let valid = EquipmentRequest::demo();
    let (mut app, car) = setup(valid.clone());
    let mut invalid = valid.clone();
    invalid.loadout.equipment[0].mount_id = "unknown_mount".into();
    app.insert_resource(invalid);
    advance(&mut app);
    assert!(app.world().resource::<EquipmentRuntime>().error.is_some());
    assert!(app.world_mut().despawn(car));
    let replacement = app
        .world_mut()
        .spawn((
            crate::vehicle::controller::chassis_bundle(&VehiclePhysicsConfig::baseline()),
            Transform::from_xyz(0.0, 4.0, 0.0),
            GravityScale(0.0),
        ))
        .id();
    advance(&mut app);
    let children = roots(app.world_mut());
    assert_eq!(
        children.len(),
        1,
        "the last valid loadout must survive an owner replacement even with an invalid pending request"
    );
    assert_eq!(
        app.world().get::<ChildOf>(children[0]).unwrap().parent(),
        replacement
    );
    assert_eq!(app.world().resource::<EquipmentRuntime>().active(), &valid);
    assert!(app.world().resource::<EquipmentRuntime>().error.is_some());
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 1);
    assert_mass(
        raw_mass(&mut app, replacement),
        expected_mass(&VehiclePhysicsConfig::baseline(), &valid),
    );
}

#[test]
fn equipment_runtime_replacing_a_deleted_attachment_releases_old_proxy_assets() {
    let (mut app, _) = setup(EquipmentRequest::demo());
    for _ in 0..3 {
        let old_root = roots(app.world_mut())[0];
        let old_mesh = app.world().resource::<EquipmentRuntime>().meshes[0].clone();
        assert!(app.world_mut().despawn(old_root));
        advance(&mut app);
        assert_eq!(roots(app.world_mut()).len(), 1);
        assert!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&old_mesh)
                .is_none()
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 1);
    }
}

#[test]
fn equipment_runtime_cleanup_drops_tracked_handles_even_when_asset_resources_are_absent() {
    let (mut app, _) = setup(EquipmentRequest::demo());
    app.world_mut().remove_resource::<Assets<Mesh>>();
    app.world_mut()
        .remove_resource::<Assets<StandardMaterial>>();
    app.insert_resource(EquipmentRequest::default());
    // Rapier's unrelated async-mesh systems require Assets<Mesh>; isolate teardown.
    sync(app.world_mut());
    let runtime = app.world().resource::<EquipmentRuntime>();
    assert!(
        runtime.meshes.is_empty(),
        "removed asset stores must not leave tracked mesh handles behind"
    );
    assert!(runtime.materials.is_empty());
    assert!(roots(app.world_mut()).is_empty());
}

#[test]
fn equipment_runtime_decorative_children_have_no_collider_or_mass_contribution() {
    let mut request = EquipmentRequest::demo();
    let definition = &mut request.catalog.definitions[0];
    definition.mode = EquipmentMode::Decorative;
    definition.mass_kg = 0.0;
    definition.principal_inertia = [0.0; 3];
    definition.colliders.clear();
    let (mut app, car) = setup(request);
    let children = roots(app.world_mut());
    assert_eq!(children.len(), 1);
    let child = children[0];
    assert_eq!(app.world().get::<ChildOf>(child).unwrap().parent(), car);
    assert!(app.world().get::<Collider>(child).is_none());
    assert!(app.world().get::<RapierColliderHandle>(child).is_none());
    assert!(app.world().get::<ColliderMassProperties>(child).is_none());
    assert!(app.world().get::<RigidBody>(child).is_none());
    assert_mass(
        raw_mass(&mut app, car),
        mass_properties(&VehiclePhysicsConfig::baseline()),
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 0);
}
