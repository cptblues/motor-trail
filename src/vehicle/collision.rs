//! Simplified static body envelope in Truck coordinates (+Z forward, model Y - 1).
use bevy::prelude::*;
use bevy_rapier3d::prelude::Collider;
use bevy_rapier3d::rapier::math::Pose;
use std::sync::OnceLock;

#[cfg(test)]
mod rollover_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyRegion {
    Chassis,
    Body,
    Roof,
    Bumper,
    Accessory,
}

pub struct CollisionPart {
    pub id: &'static str,
    pub region: BodyRegion,
    pub local_transform: Transform,
    pub collider: Collider,
}

fn box_part(id: &'static str, region: BodyRegion, position: Vec3, half: Vec3) -> CollisionPart {
    CollisionPart {
        id,
        region,
        local_transform: Transform::from_translation(position),
        collider: Collider::cuboid(half.x, half.y, half.z),
    }
}

/// Cached measured geometry; no per-step construction or density-derived mass.
pub fn body_parts() -> &'static [CollisionPart] {
    static PARTS: OnceLock<Vec<CollisionPart>> = OnceLock::new();
    PARTS.get_or_init(|| {
        use BodyRegion::*;
        let mut parts = vec![
            box_part(
                "ladder_frame",
                Chassis,
                Vec3::new(0.0, -0.203, 0.0),
                Vec3::new(0.85, 0.22, 1.955),
            ),
            box_part(
                "body_tub",
                Body,
                Vec3::new(0.0, 0.25, 0.02),
                Vec3::new(0.83, 0.29, 1.815),
            ),
            box_part(
                "roof",
                Roof,
                Vec3::new(0.0, 1.134, -0.42),
                Vec3::new(0.785, 0.030, 0.95),
            ),
            box_part(
                "hood",
                Body,
                Vec3::new(0.0, 0.615, 1.19),
                Vec3::new(0.75, 0.035, 0.65),
            ),
            box_part(
                "front_bumper",
                Bumper,
                Vec3::new(0.0, 0.01, 2.007),
                Vec3::new(1.0125, 0.0925, 0.13),
            ),
            box_part(
                "front_winch",
                Bumper,
                Vec3::new(0.0, -0.087, 2.134),
                Vec3::new(0.2875, 0.125, 0.21),
            ),
            box_part(
                "front_guard",
                Bumper,
                Vec3::new(0.0, 0.36, 2.04),
                Vec3::new(0.81, 0.255, 0.055),
            ),
            box_part(
                "rear_bumper",
                Bumper,
                Vec3::new(0.0, 0.01, -1.96),
                Vec3::new(1.0125, 0.0925, 0.13),
            ),
            box_part(
                "rear_hitch",
                Bumper,
                Vec3::new(0.0, -0.038, -2.095),
                Vec3::new(0.25, 0.04, 0.0575),
            ),
            box_part(
                "rack",
                Accessory,
                Vec3::new(0.0, 1.285, -0.435),
                Vec3::new(0.775, 0.12, 0.951),
            ),
            box_part(
                "roof_case_a",
                Accessory,
                Vec3::new(0.36, 1.345, -0.57),
                Vec3::new(0.24, 0.105, 0.195),
            ),
            box_part(
                "roof_case_b",
                Accessory,
                Vec3::new(0.31, 1.345, -1.0),
                Vec3::new(0.265, 0.105, 0.145),
            ),
            box_part(
                "roof_case_c",
                Accessory,
                Vec3::new(-0.22, 1.345, -0.62),
                Vec3::new(0.21, 0.105, 0.175),
            ),
            box_part(
                "roof_can",
                Accessory,
                Vec3::new(-0.43, 1.365, -1.05),
                Vec3::new(0.14, 0.125, 0.14),
            ),
            box_part(
                "roof_can_cap",
                Accessory,
                Vec3::new(-0.43, 1.505, -1.05),
                Vec3::new(0.07, 0.0175, 0.08),
            ),
            box_part(
                "snorkel",
                Accessory,
                Vec3::new(0.99, 1.14, 0.64),
                Vec3::new(0.07, 0.13, 0.10),
            ),
        ];
        for side in [-1.0, 1.0] {
            parts.push(box_part(
                if side < 0.0 {
                    "left_step"
                } else {
                    "right_step"
                },
                Body,
                Vec3::new(side * 0.883, -0.099, -0.19),
                Vec3::new(0.105, 0.031, 0.905),
            ));
            parts.push(box_part(
                if side < 0.0 {
                    "left_mirror"
                } else {
                    "right_mirror"
                },
                Accessory,
                Vec3::new(side * 1.021, 0.766, 0.47),
                Vec3::new(0.119, 0.09, 0.115),
            ));
        }
        // Cabin prism follows the sloped windshield/rear window instead of a large box.
        let points: Vec<_> = [-0.768, 0.768]
            .into_iter()
            .flat_map(|x| {
                [(0.50, 0.72), (1.125, 0.47), (1.125, -1.32), (0.52, -1.63)]
                    .map(move |(y, z)| Vec3::new(x, y, z))
            })
            .collect();
        parts.push(CollisionPart {
            id: "cabin",
            region: Body,
            local_transform: Transform::IDENTITY,
            collider: Collider::convex_hull(&points).expect("measured cabin prism is convex"),
        });
        parts.push(CollisionPart {
            id: "existing_spare",
            region: Accessory,
            local_transform: Transform::from_xyz(0.0, 0.39, -2.07)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            collider: Collider::cylinder(0.239, 0.555),
        });
        parts
    })
}

pub fn body_collider() -> Collider {
    static BODY: OnceLock<Collider> = OnceLock::new();
    BODY.get_or_init(|| {
        Collider::compound(
            body_parts()
                .iter()
                .map(|part| {
                    (
                        part.local_transform.translation,
                        part.local_transform.rotation,
                        part.collider.clone(),
                    )
                })
                .collect(),
        )
    })
    .clone()
}

pub fn body_bounds() -> (Vec3, Vec3) {
    let aabb = body_collider().raw.compute_local_aabb();
    (aabb.mins, aabb.maxs)
}

pub fn lowest_point(pose: &Transform) -> f32 {
    let car = Pose::from_parts(pose.translation, pose.rotation);
    body_parts()
        .iter()
        .map(|part| {
            let local = Pose::from_parts(
                part.local_transform.translation,
                part.local_transform.rotation,
            );
            part.collider.raw.compute_aabb(&(car * local)).mins.y
        })
        .fold(f32::INFINITY, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_envelope_has_roof_spare_bumpers_and_no_wheel_centers() {
        let (min, max) = body_bounds();
        assert!((max.y - 1.5225).abs() < 1e-5);
        assert!((max.z - 2.344).abs() < 1e-4);
        assert!(min.z < -2.30 && min.y > -0.43);
        let body = body_collider();
        for mount in crate::vehicle::controller::MOUNTS {
            assert!(
                !body
                    .raw
                    .contains_point(&Pose::IDENTITY, mount - Vec3::Y * 0.30)
            );
        }
        let mut ids = std::collections::HashSet::new();
        for part in body_parts() {
            assert!(ids.insert(part.id));
        }
    }

    #[test]
    fn body_roof_bounds_match_actual_glb_without_graphics_entity() {
        let bytes = include_bytes!("../../assets/vehicles/motor_trail_4x4.glb");
        let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let glb: serde_json::Value = serde_json::from_slice(&bytes[20..20 + n]).unwrap();
        let node = glb["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == "Roof_Rack_Equipment")
            .unwrap();
        let mesh = &glb["meshes"][node["mesh"].as_u64().unwrap() as usize];
        let top = mesh["primitives"]
            .as_array()
            .unwrap()
            .iter()
            .map(|primitive| {
                glb["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize]
                        ["max"][1]
                        .as_f64()
                        .unwrap() as f32
                        - 1.0
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((body_bounds().1.y - top).abs() < 1e-5);
    }
}
