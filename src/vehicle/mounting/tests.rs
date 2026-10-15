use super::*;
use bevy::math::{Quat, Vec3};
use serde_json::json;

fn definition() -> VehicleMountDefinition {
    serde_json::from_str(include_str!(
        "../../../configs/vehicles/motor_trail_4x4.mounts.json"
    ))
    .unwrap()
}

#[test]
fn mounting_json_roundtrip_preserves_measured_vehicle_local_mounts() {
    let mounts = VehicleMountDefinition::from_json(include_str!(
        "../../../configs/vehicles/motor_trail_4x4.mounts.json"
    ))
    .unwrap();
    let expected = [
        ("roof_main", "roof", [0.0, 1.240, -0.420]),
        ("front_winch", "front", [0.0, -0.087, 2.143]),
        ("rear_spare_wheel", "rear", [0.0, 0.390, -2.070]),
        ("left_accessory", "left_side", [-0.988, -0.099, -0.190]),
        ("right_accessory", "right_side", [0.988, -0.099, -0.190]),
    ];
    assert_eq!(mounts.vehicle_id, "motor_trail_4x4");
    assert_eq!(mounts.mounts.len(), expected.len());
    for (id, category, translation) in expected {
        let mount = mounts.mounts.iter().find(|mount| mount.id == id).unwrap();
        assert_eq!(mount.category, category);
        assert_eq!(mount.local_pose.translation, translation);
        assert_eq!(mount.local_pose.rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(mount.local_pose.scale, [1.0; 3]);
    }
    let json = serde_json::to_string(&mounts).unwrap();
    assert_eq!(VehicleMountDefinition::from_json(&json).unwrap(), mounts);
}

#[test]
fn mounting_rejects_unknown_fields_at_every_json_level() {
    for level in 0..3 {
        let mut value = serde_json::to_value(definition()).unwrap();
        let object = match level {
            0 => &mut value,
            1 => &mut value["mounts"][0],
            _ => &mut value["mounts"][0]["local_pose"],
        };
        object["typo"] = json!(true);
        assert!(VehicleMountDefinition::from_json(&value.to_string()).is_err());
    }
}

#[test]
fn mounting_rejects_unsupported_versions_and_duplicate_ids() {
    let mut mounts = definition();
    mounts.schema_version = 2;
    assert!(mounts.validate().is_err());
    mounts.schema_version = 1;
    mounts.mounts.push(mounts.mounts[0].clone());
    assert!(mounts.validate().is_err());
}

#[test]
fn mounting_categories_and_equipment_types_are_extensible_strings() {
    let mut mounts = definition();
    mounts.mounts[0].category = "expedition:communications".into();
    mounts.mounts[0].allowed_equipment_types = vec!["antenna.v2".into()];
    assert!(mounts.validate().is_ok());
    assert_eq!(
        VehicleMountDefinition::from_json(&serde_json::to_string(&mounts).unwrap()).unwrap(),
        mounts
    );
}

#[test]
fn mounting_rejects_invalid_ids_categories_and_repeated_zones() {
    for invalid in [
        "",
        " ",
        "roof main",
        "roof/main",
        "../roof",
        "Roof",
        "roof\n",
    ] {
        let mut mounts = definition();
        mounts.mounts[0].id = invalid.into();
        assert!(mounts.validate().is_err(), "accepted id {invalid:?}");
    }
    let mut mounts = definition();
    mounts.mounts[0].category.clear();
    assert!(mounts.validate().is_err());
    let mut mounts = definition();
    mounts.mounts[0].occupied_zones = vec!["roof".into(), "roof".into()];
    assert!(mounts.validate().is_err());
}

#[test]
fn mounting_rejects_nonfinite_or_unbounded_load_limits() {
    for mass in [f32::NAN, f32::INFINITY, -1.0, 0.0, 100_001.0] {
        let mut mounts = definition();
        mounts.mounts[0].max_mass_kg = mass;
        assert!(mounts.validate().is_err(), "accepted limit {mass}");
    }
}

#[test]
fn mounting_local_pose_requires_finite_bounded_translation_unit_quaternion_and_unit_scale() {
    for value in [f32::NAN, f32::INFINITY, -f32::INFINITY, 100.01, -100.01] {
        let mut pose = LocalPose::default();
        pose.translation[1] = value;
        assert!(pose.validate().is_err());
        assert!(pose.transform().is_err());
    }
    for rotation in [[0.0; 4], [0.0, 0.0, 0.0, 2.0], [f32::NAN, 0.0, 0.0, 1.0]] {
        let pose = LocalPose {
            rotation,
            ..Default::default()
        };
        assert!(pose.validate().is_err());
    }
    for scale in [[2.0; 3], [-1.0, 1.0, 1.0], [0.0; 3], [1.0, f32::NAN, 1.0]] {
        let pose = LocalPose {
            scale,
            ..Default::default()
        };
        assert!(pose.validate().is_err());
    }
}

#[test]
fn mounting_local_pose_keeps_xyzw_rotation_without_changing_the_vehicle_axes() {
    let rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let pose = LocalPose {
        translation: [1.0, 2.0, 3.0],
        rotation: rotation.to_array(),
        ..Default::default()
    };
    let transform = pose.transform().unwrap();
    assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(transform.scale, Vec3::ONE);
    assert!((transform.rotation * Vec3::Z - Vec3::X).length() < 1.0e-6);
}

#[test]
fn mounting_rejects_conflicts_with_unknown_zones() {
    let mut mounts = definition();
    mounts.mounts[0]
        .conflicting_zones
        .push("missing_zone".into());
    assert!(mounts.validate().is_err());
}
