use super::*;
use crate::vehicle::mounting::{LocalPose, VehicleMountDefinition};
use bevy::math::{Mat3, Quat, Vec3};
use serde_json::json;

fn mounts() -> VehicleMountDefinition {
    serde_json::from_str(include_str!(
        "../../../configs/vehicles/motor_trail_4x4.mounts.json"
    ))
    .unwrap()
}

fn rigid(id: &str) -> EquipmentDefinition {
    EquipmentDefinition {
        id: id.into(),
        equipment_type: "cargo".into(),
        glb_path: None,
        local_pose: LocalPose::default(),
        mass_kg: 20.0,
        local_center_of_mass: [0.0; 3],
        principal_inertia: [2.0, 3.0, 4.0],
        principal_inertia_local_frame: [0.0, 0.0, 0.0, 1.0],
        compatible_mount_categories: vec![
            "roof".into(),
            "rear".into(),
            "left_side".into(),
            "right_side".into(),
        ],
        mode: EquipmentMode::Rigid,
        colliders: vec![EquipmentCollider {
            local_pose: LocalPose::default(),
            shape: EquipmentShape::Cuboid {
                half_extents: [0.3, 0.2, 0.4],
            },
        }],
    }
}

fn catalog(definitions: Vec<EquipmentDefinition>) -> EquipmentCatalog {
    EquipmentCatalog {
        schema_version: 1,
        definitions,
    }
}

fn loadout(entries: &[(&str, &str, &str)]) -> VehicleEquipmentLoadout {
    VehicleEquipmentLoadout {
        schema_version: 1,
        vehicle_id: "motor_trail_4x4".into(),
        equipment: entries
            .iter()
            .map(|(instance, definition, mount)| EquipmentInstallation {
                instance_id: (*instance).into(),
                equipment_id: (*definition).into(),
                mount_id: (*mount).into(),
            })
            .collect(),
    }
}

fn base() -> MassProperties {
    MassProperties {
        mass: 1000.0,
        local_center_of_mass: Vec3::new(0.0, -0.1, 0.0),
        principal_inertia: Vec3::new(800.0, 1500.0, 1200.0),
        principal_inertia_local_frame: Quat::from_rotation_y(0.2),
    }
}

fn tensor(properties: MassProperties) -> Mat3 {
    let rotation = Mat3::from_quat(properties.principal_inertia_local_frame);
    rotation * Mat3::from_diagonal(properties.principal_inertia) * rotation.transpose()
}

fn parallel_axis(mass: f32, offset: Vec3) -> Mat3 {
    mass * (Mat3::from_diagonal(Vec3::splat(offset.length_squared()))
        - Mat3::from_cols(offset * offset.x, offset * offset.y, offset * offset.z))
}

fn assert_tensor_close(actual: Mat3, expected: Mat3) {
    for (actual, expected) in actual
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
    {
        assert!(
            (actual - expected).abs() < 0.02,
            "tensor entry {actual} != {expected}"
        );
    }
}

#[test]
fn equipment_catalog_and_loadout_json_roundtrip_are_separate_from_base_profiles() {
    let catalog = catalog(vec![rigid("dummy_crate")]);
    let loadout = loadout(&[("cargo_1", "dummy_crate", "roof_main")]);
    assert_eq!(
        EquipmentCatalog::from_json(&serde_json::to_string(&catalog).unwrap()).unwrap(),
        catalog
    );
    assert_eq!(
        VehicleEquipmentLoadout::from_json(&serde_json::to_string(&loadout).unwrap()).unwrap(),
        loadout
    );
    let profile = crate::vehicle::config::VehiclePhysicsConfig::baseline();
    let before = serde_json::to_value(&profile).unwrap();
    effective_mass_properties(
        crate::vehicle::controller::mass_properties(&profile),
        &mounts(),
        &catalog,
        &loadout,
    )
    .unwrap();
    assert_eq!(serde_json::to_value(&profile).unwrap(), before);
}

#[test]
fn equipment_json_rejects_unknown_fields_in_nested_definitions_poses_and_shapes() {
    for level in 0..5 {
        let mut value = serde_json::to_value(catalog(vec![rigid("crate")])).unwrap();
        let object = match level {
            0 => &mut value,
            1 => &mut value["definitions"][0],
            2 => &mut value["definitions"][0]["local_pose"],
            3 => &mut value["definitions"][0]["colliders"][0],
            _ => &mut value["definitions"][0]["colliders"][0]["shape"],
        };
        object["unknown"] = json!(123);
        assert!(EquipmentCatalog::from_json(&value.to_string()).is_err());
    }
    for level in 0..2 {
        let mut value = serde_json::to_value(loadout(&[("one", "crate", "roof_main")])).unwrap();
        let object = if level == 0 {
            &mut value
        } else {
            &mut value["equipment"][0]
        };
        object["unknown"] = json!(true);
        assert!(VehicleEquipmentLoadout::from_json(&value.to_string()).is_err());
    }
}

#[test]
fn equipment_rejects_versions_duplicate_definitions_and_duplicate_instances() {
    let mut catalog = catalog(vec![rigid("crate")]);
    catalog.schema_version = 2;
    assert!(catalog.validate().is_err());
    catalog.schema_version = 1;
    catalog.definitions.push(catalog.definitions[0].clone());
    assert!(catalog.validate().is_err());
    let mut loadout = loadout(&[("one", "crate", "roof_main")]);
    loadout.schema_version = 2;
    assert!(loadout.validate().is_err());
    loadout.schema_version = 1;
    loadout.equipment.push(loadout.equipment[0].clone());
    assert!(loadout.validate().is_err());
}

#[test]
fn equipment_accepts_new_string_categories_and_types_for_installation() {
    let mut mounts = mounts();
    mounts.mounts[0].category = "expedition:communications".into();
    mounts.mounts[0].allowed_equipment_types = vec!["antenna.v2".into()];
    let mut equipment = rigid("antenna");
    equipment.equipment_type = "antenna.v2".into();
    equipment.compatible_mount_categories = vec!["expedition:communications".into()];
    let installed = resolve_loadout(
        &mounts,
        &catalog(vec![equipment]),
        &loadout(&[("one", "antenna", "roof_main")]),
    )
    .unwrap();
    assert_eq!(installed.len(), 1);
}

#[test]
fn equipment_rejects_unknown_vehicle_mount_and_equipment_references() {
    let catalog = catalog(vec![rigid("crate")]);
    let mut wrong_vehicle = loadout(&[]);
    wrong_vehicle.vehicle_id = "other_vehicle".into();
    assert!(resolve_loadout(&mounts(), &catalog, &wrong_vehicle).is_err());
    for loadout in [
        loadout(&[("one", "crate", "unknown")]),
        loadout(&[("one", "missing", "roof_main")]),
    ] {
        assert!(resolve_loadout(&mounts(), &catalog, &loadout).is_err());
        assert!(effective_mass_properties(base(), &mounts(), &catalog, &loadout).is_err());
    }
}

#[test]
fn equipment_requires_both_mount_type_and_equipment_category_compatibility() {
    let mut equipment = rigid("crate");
    let loadout = loadout(&[("one", "crate", "front_winch")]);
    assert!(resolve_loadout(&mounts(), &catalog(vec![equipment.clone()]), &loadout).is_err());
    equipment.compatible_mount_categories = vec!["front".into()];
    assert!(resolve_loadout(&mounts(), &catalog(vec![equipment.clone()]), &loadout).is_err());
    equipment.equipment_type = "winch".into();
    assert!(resolve_loadout(&mounts(), &catalog(vec![equipment]), &loadout).is_ok());
}

#[test]
fn equipment_limits_mass_per_mount_and_prevents_double_occupation() {
    let catalog = catalog(vec![rigid("crate")]);
    let mut mounts = mounts();
    mounts.mounts[0].max_mass_kg = 19.0;
    assert!(
        resolve_loadout(
            &mounts,
            &catalog,
            &loadout(&[("one", "crate", "roof_main")])
        )
        .is_err()
    );
    mounts.mounts[0].max_mass_kg = 20.0;
    assert!(
        resolve_loadout(
            &mounts,
            &catalog,
            &loadout(&[("one", "crate", "roof_main")])
        )
        .is_ok()
    );
    assert!(
        resolve_loadout(
            &mounts,
            &catalog,
            &loadout(&[("one", "crate", "roof_main"), ("two", "crate", "roof_main")])
        )
        .is_err()
    );
}

#[test]
fn equipment_prevents_shared_zones_and_one_way_zone_conflicts_in_either_order() {
    let catalog = catalog(vec![rigid("crate")]);
    for shared in [true, false] {
        let mut mounts = mounts();
        mounts.mounts[0].occupied_zones = vec!["roof_zone".into()];
        mounts.mounts[2].occupied_zones =
            vec![if shared { "roof_zone" } else { "rear_zone" }.into()];
        if !shared {
            mounts.mounts[0].conflicting_zones = vec!["rear_zone".into()];
        }
        for entries in [
            [
                ("one", "crate", "roof_main"),
                ("two", "crate", "rear_spare_wheel"),
            ],
            [
                ("two", "crate", "rear_spare_wheel"),
                ("one", "crate", "roof_main"),
            ],
        ] {
            assert!(resolve_loadout(&mounts, &catalog, &loadout(&entries)).is_err());
        }
        assert!(
            resolve_loadout(
                &mounts,
                &catalog,
                &loadout(&[("one", "crate", "roof_main")])
            )
            .is_ok()
        );
    }
}

#[test]
fn equipment_rejects_invalid_mass_com_inertia_and_principal_frame() {
    for value in [
        f32::NAN,
        f32::INFINITY,
        -1.0,
        0.0,
        f32::from_bits(1),
        100_001.0,
    ] {
        let mut equipment = rigid("crate");
        equipment.mass_kg = value;
        assert!(equipment.validate().is_err());
    }
    for value in [
        f32::NAN,
        f32::INFINITY,
        -1.0,
        0.0,
        f32::from_bits(1),
        1.0e10,
    ] {
        let mut equipment = rigid("crate");
        equipment.principal_inertia[1] = value;
        assert!(equipment.validate().is_err());
    }
    let mut equipment = rigid("crate");
    equipment.principal_inertia = [1.0, 1.0, 3.0];
    assert!(equipment.validate().is_err());
    for value in [f32::NAN, f32::INFINITY, 100.1] {
        let mut equipment = rigid("crate");
        equipment.local_center_of_mass[0] = value;
        assert!(equipment.validate().is_err());
    }
    let mut equipment = rigid("crate");
    equipment.principal_inertia_local_frame = [0.0; 4];
    assert!(equipment.validate().is_err());
    equipment.principal_inertia_local_frame = [0.0, 0.0, 0.0, f32::NAN];
    assert!(equipment.validate().is_err());
}

#[test]
fn equipment_checks_definition_and_collider_transforms_before_installation() {
    for collider_pose in [false, true] {
        let mut equipment = rigid("crate");
        let pose = if collider_pose {
            &mut equipment.colliders[0].local_pose
        } else {
            &mut equipment.local_pose
        };
        pose.scale = [2.0; 3];
        assert!(equipment.validate().is_err());
        assert!(
            resolve_loadout(
                &mounts(),
                &catalog(vec![equipment]),
                &loadout(&[("one", "crate", "roof_main")])
            )
            .is_err()
        );
    }
}

#[test]
fn equipment_shapes_have_bounded_positive_dimensions_and_real_geometry() {
    let shapes = [
        EquipmentShape::Cuboid {
            half_extents: [0.3, 0.2, 0.4],
        },
        EquipmentShape::Cylinder {
            half_height: 0.4,
            radius: 0.2,
        },
        EquipmentShape::Ball { radius: 0.3 },
        EquipmentShape::Capsule {
            half_segment: 0.4,
            radius: 0.2,
        },
    ];
    for shape in shapes {
        let spec = EquipmentCollider {
            local_pose: LocalPose::default(),
            shape,
        };
        spec.validate().unwrap();
        let collider = spec.collider().unwrap();
        let bounds = collider.raw.compute_local_aabb();
        assert!(bounds.half_extents().min_element() > 0.1);
    }
    for value in [0.0, -0.1, f32::NAN, f32::INFINITY, 100.1] {
        for shape in [
            EquipmentShape::Cuboid {
                half_extents: [value, 0.2, 0.3],
            },
            EquipmentShape::Cylinder {
                half_height: value,
                radius: 0.2,
            },
            EquipmentShape::Cylinder {
                half_height: 0.2,
                radius: value,
            },
            EquipmentShape::Ball { radius: value },
            EquipmentShape::Capsule {
                half_segment: 0.2,
                radius: value,
            },
        ] {
            let spec = EquipmentCollider {
                local_pose: LocalPose::default(),
                shape,
            };
            assert!(spec.validate().is_err());
            assert!(spec.collider().is_err());
        }
    }
}

#[test]
fn equipment_rigid_requires_collision_and_decorative_has_no_mass_or_collision() {
    let mut equipment = rigid("crate");
    equipment.colliders.clear();
    assert!(equipment.validate().is_err());
    equipment.mode = EquipmentMode::Decorative;
    assert!(equipment.validate().is_err());
    equipment.mass_kg = 0.0;
    equipment.principal_inertia = [0.0; 3];
    equipment.glb_path = Some("models/marker.glb".into());
    assert!(equipment.validate().is_ok());
    let catalog = catalog(vec![equipment.clone()]);
    let loadout = loadout(&[("one", "crate", "roof_main")]);
    let installed = resolve_loadout(&mounts(), &catalog, &loadout).unwrap();
    assert!(installed[0].colliders.is_empty());
    assert_eq!(installed[0].glb_path.as_deref(), Some("models/marker.glb"));
    assert_eq!(
        effective_mass_properties(base(), &mounts(), &catalog, &loadout).unwrap(),
        base()
    );
    equipment.colliders.push(EquipmentCollider {
        local_pose: LocalPose::default(),
        shape: EquipmentShape::Ball { radius: 0.1 },
    });
    assert!(equipment.validate().is_err());
}

#[test]
fn equipment_represents_but_rejects_articulated_and_detachable_modes() {
    for mode in [EquipmentMode::Articulated, EquipmentMode::Detachable] {
        let mut equipment = rigid("crate");
        equipment.mode = mode;
        let serialized = serde_json::to_value(&equipment).unwrap();
        let restored: EquipmentDefinition = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored.mode, mode);
        let error = restored.validate().unwrap_err();
        assert!(error.contains("unsupported"), "{error}");
        assert!(
            resolve_loadout(
                &mounts(),
                &catalog(vec![restored]),
                &loadout(&[("one", "crate", "roof_main")])
            )
            .is_err()
        );
    }
}

#[test]
fn equipment_installation_composes_mount_and_asset_poses_and_keeps_shapes_equipment_local() {
    let mut mounts = mounts();
    mounts.mounts[0].local_pose.rotation = Quat::from_rotation_y(0.7).to_array();
    let mut equipment = rigid("crate");
    equipment.local_pose.translation = [0.3, 0.2, -0.4];
    equipment.local_pose.rotation = Quat::from_rotation_z(0.6).to_array();
    equipment.colliders[0].local_pose.translation = [0.1, 0.2, 0.3];
    let expected_translation = Vec3::from_array(mounts.mounts[0].local_pose.translation)
        + Quat::from_rotation_y(0.7) * Vec3::from_array(equipment.local_pose.translation);
    let expected_rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_z(0.6);
    let installed = resolve_loadout(
        &mounts,
        &catalog(vec![equipment]),
        &loadout(&[("one", "crate", "roof_main")]),
    )
    .unwrap();
    assert_eq!(installed[0].instance_id, "one");
    assert_eq!(installed[0].mount_id, "roof_main");
    assert_eq!(installed[0].equipment_id, "crate");
    assert_eq!(installed[0].mode, EquipmentMode::Rigid);
    assert!((installed[0].local_transform.translation - expected_translation).length() < 1.0e-6);
    assert!(
        installed[0]
            .local_transform
            .rotation
            .abs_diff_eq(expected_rotation, 1.0e-6)
    );
    assert_eq!(installed[0].local_transform.scale, Vec3::ONE);
    assert_eq!(
        installed[0].colliders[0].local_transform.translation,
        Vec3::new(0.1, 0.2, 0.3)
    );
    assert!(installed[0].colliders[0].collider.raw.as_cuboid().is_some());
}

#[test]
fn equipment_empty_loadout_returns_exact_base_mass_properties() {
    let base = base();
    let result =
        effective_mass_properties(base, &mounts(), &catalog(vec![]), &loadout(&[])).unwrap();
    assert_eq!(result, base);
    assert_eq!(result.mass.to_bits(), base.mass.to_bits());
    assert_eq!(
        result.principal_inertia.to_array().map(f32::to_bits),
        base.principal_inertia.to_array().map(f32::to_bits)
    );
    assert_eq!(
        result
            .principal_inertia_local_frame
            .to_array()
            .map(f32::to_bits),
        base.principal_inertia_local_frame
            .to_array()
            .map(f32::to_bits)
    );
}

#[test]
fn equipment_roof_and_rear_mass_have_the_expected_weighted_center_of_mass() {
    let catalog = catalog(vec![rigid("crate")]);
    let loadout = loadout(&[
        ("roof", "crate", "roof_main"),
        ("rear", "crate", "rear_spare_wheel"),
    ]);
    let result = effective_mass_properties(base(), &mounts(), &catalog, &loadout).unwrap();
    let expected = (base().mass * base().local_center_of_mass
        + 20.0 * Vec3::new(0.0, 1.240, -0.420)
        + 20.0 * Vec3::new(0.0, 0.390, -2.070))
        / 1040.0;
    assert!((result.mass - 1040.0).abs() < 0.001);
    assert!((result.local_center_of_mass - expected).length() < 1.0e-5);
}

#[test]
fn equipment_full_tensor_includes_mount_asset_and_principal_frame_rotation_and_parallel_axes() {
    let base = base();
    let mut mounts = mounts();
    let mount_position = Vec3::new(1.0, 2.0, -1.0);
    let mount_rotation = Quat::from_rotation_y(0.7);
    mounts.mounts[0].local_pose.translation = mount_position.to_array();
    mounts.mounts[0].local_pose.rotation = mount_rotation.to_array();
    let mut equipment = rigid("crate");
    let asset_position = Vec3::new(0.3, 0.2, -0.4);
    let asset_rotation = Quat::from_rotation_z(0.6);
    let principal_frame = Quat::from_rotation_x(0.4);
    equipment.local_pose.translation = asset_position.to_array();
    equipment.local_pose.rotation = asset_rotation.to_array();
    equipment.local_center_of_mass = [0.2, -0.3, 0.4];
    equipment.principal_inertia_local_frame = principal_frame.to_array();
    let equipment_com = mount_position
        + mount_rotation
            * (asset_position + asset_rotation * Vec3::from_array(equipment.local_center_of_mass));
    let expected_com = (base.mass * base.local_center_of_mass + equipment.mass_kg * equipment_com)
        / (base.mass + equipment.mass_kg);
    let rotation = Mat3::from_quat(mount_rotation * asset_rotation * principal_frame);
    let expected_tensor = tensor(base)
        + parallel_axis(base.mass, base.local_center_of_mass - expected_com)
        + rotation
            * Mat3::from_diagonal(Vec3::from_array(equipment.principal_inertia))
            * rotation.transpose()
        + parallel_axis(equipment.mass_kg, equipment_com - expected_com);
    assert!(expected_tensor.x_axis.y.abs() > 0.1);
    let catalog = catalog(vec![equipment]);
    let result = effective_mass_properties(
        base,
        &mounts,
        &catalog,
        &loadout(&[("one", "crate", "roof_main")]),
    )
    .unwrap();
    assert!((result.local_center_of_mass - expected_com).length() < 1.0e-5);
    assert_tensor_close(tensor(result), expected_tensor);
}

#[test]
fn equipment_install_order_preserves_the_combined_mass_com_and_tensor() {
    let catalog = catalog(vec![rigid("crate")]);
    let first = effective_mass_properties(
        base(),
        &mounts(),
        &catalog,
        &loadout(&[
            ("a", "crate", "roof_main"),
            ("b", "crate", "rear_spare_wheel"),
        ]),
    )
    .unwrap();
    let second = effective_mass_properties(
        base(),
        &mounts(),
        &catalog,
        &loadout(&[
            ("b", "crate", "rear_spare_wheel"),
            ("a", "crate", "roof_main"),
        ]),
    )
    .unwrap();
    assert!((first.mass - second.mass).abs() < 1.0e-3);
    assert!((first.local_center_of_mass - second.local_center_of_mass).length() < 1.0e-5);
    assert_tensor_close(tensor(first), tensor(second));
}

fn repeated_eigen_aggregate(frame: Quat) -> MassProperties {
    let mut mounts = mounts();
    mounts.mounts[0].local_pose = LocalPose::default();
    let mut equipment = rigid("symmetric_crate");
    equipment.principal_inertia = [3.0, 2.0, 2.0];
    equipment.principal_inertia_local_frame = frame.to_array();
    let base = MassProperties {
        mass: 20.0,
        local_center_of_mass: Vec3::ZERO,
        principal_inertia: Vec3::new(3.0, 2.0, 2.0),
        principal_inertia_local_frame: frame,
    };
    effective_mass_properties(
        base,
        &mounts,
        &catalog(vec![equipment]),
        &loadout(&[("one", "symmetric_crate", "roof_main")]),
    )
    .unwrap()
}

#[test]
fn equipment_repeated_eigen_identity_frame_preserves_coincident_com_tensor() {
    let result = repeated_eigen_aggregate(Quat::IDENTITY);
    assert!((result.mass - 40.0).abs() < 1.0e-5);
    assert!(result.local_center_of_mass.length() < 1.0e-6);
    assert_tensor_close(
        tensor(result),
        Mat3::from_diagonal(Vec3::new(6.0, 4.0, 4.0)),
    );
}

#[test]
fn equipment_repeated_eigen_rotated_frame_preserves_coincident_com_tensor() {
    let frame = Quat::from_rotation_y(0.4) * Quat::from_rotation_z(0.7);
    let result = repeated_eigen_aggregate(frame);
    let rotation = Mat3::from_quat(frame);
    let expected = rotation * Mat3::from_diagonal(Vec3::new(6.0, 4.0, 4.0)) * rotation.transpose();
    assert_tensor_close(tensor(result), expected);
}
