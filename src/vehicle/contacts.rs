//! Contact queries only: this module never applies impulses or creates colliders.
use bevy::prelude::*;
use bevy_rapier3d::parry::{
    query::{ShapeCastHit, ShapeCastOptions, ShapeCastStatus},
    shape::Cylinder,
};
use bevy_rapier3d::rapier::math::Pose;
use bevy_rapier3d::rapier::{
    geometry::ColliderHandle,
    pipeline::{QueryFilter as RapierQueryFilter, QueryPipeline},
};

#[cfg(test)]
mod allocation_tests;

pub const MODEL_TIRE_HALF_WIDTH: f32 = 0.171;
const MAX_COMPOUND_CHILD_QUERIES: usize = 64;

#[derive(Clone, Copy)]
pub struct WheelCast {
    pub mount: Vec3,
    pub down: Vec3,
    pub axle: Vec3,
    pub radius: f32,
    pub half_width: f32,
    pub rest_length: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct EnvelopeContact {
    pub collider: ColliderHandle,
    pub length: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// Rejected hits on flat compounds are retried per child within a fixed query budget.
/// Nested composites are not traversed. TriMesh/heightfield queries still expose only
/// their first hit, so a rejected feature can mask other support in the same collider.
pub fn cast_envelope(queries: &QueryPipeline<'_>, wheel: WheelCast) -> Option<EnvelopeContact> {
    if !wheel.mount.is_finite()
        || !wheel.down.is_finite()
        || !wheel.axle.is_finite()
        || !wheel.radius.is_finite()
        || wheel.radius <= 0.0
        || !wheel.half_width.is_finite()
        || wheel.half_width <= 0.0
        || !wheel.rest_length.is_finite()
        || wheel.rest_length < 0.0
        || wheel.down.length_squared() < 0.5
        || wheel.axle.length_squared() < 0.5
    {
        return None;
    }
    let down = wheel.down.normalize();
    let axle = wheel.axle.normalize();
    if down.cross(axle).length_squared() < 0.5 {
        return None;
    }
    let offset = wheel.radius;
    let pose = Pose::from_parts(
        wheel.mount - down * offset,
        Quat::from_rotation_arc(Vec3::Y, axle),
    );
    let shape = Cylinder::new(wheel.half_width, wheel.radius);
    // Bound pathological overlapping scenes without heap scratch on each physics step.
    const MAX_REJECTED: usize = 32;
    let mut rejected = [ColliderHandle::invalid(); MAX_REJECTED];
    let mut child_queries_left = MAX_COMPOUND_CHILD_QUERIES;
    let mut best_contact = None;
    let mut options = ShapeCastOptions {
        // Initial overlaps are rejected without EPA's allocating penetration geometry.
        compute_impact_geometry_on_penetration: false,
        ..ShapeCastOptions::with_max_time_of_impact(offset + wheel.rest_length)
    };
    for count in 0..=MAX_REJECTED {
        // A ceiling/side hit must not hide admissible support on another collider.
        let accept = |handle, collider: &bevy_rapier3d::rapier::geometry::Collider| {
            !rejected[..count].contains(&handle)
                && queries
                    .filter
                    .predicate
                    .is_none_or(|predicate| predicate(handle, collider))
        };
        let candidates = QueryPipeline {
            filter: RapierQueryFilter {
                predicate: Some(&accept),
                ..queries.filter
            },
            ..*queries
        };
        let Some((collider, hit)) = candidates.cast_shape(&pose, down, &shape, options) else {
            return best_contact;
        };
        if let Some(contact) = support_contact(wheel, down, axle, collider, hit) {
            return Some(contact);
        }
        // This parent already passed the complete caller filter in candidates.
        let object = &queries.colliders[collider];
        if let Some(compound) = object.shape().as_compound() {
            let children = compound.shapes();
            // Reject an oversized fallback in full, rather than selecting an arbitrary prefix.
            if children.len() <= child_queries_left {
                child_queries_left -= children.len();
                for (child_pose, child_shape) in children {
                    if child_shape.as_composite_shape().is_some() {
                        continue;
                    }
                    let world_pose = object.position() * child_pose;
                    let local_pose = world_pose.inv_mul(&pose);
                    let local_down = world_pose.rotation.inverse() * down;
                    let Ok(Some(child_hit)) = queries.dispatcher.cast_shapes(
                        &local_pose,
                        local_down,
                        child_shape.as_ref(),
                        &shape,
                        options,
                    ) else {
                        continue;
                    };
                    let world_hit = child_hit.transform1_by(&world_pose);
                    if let Some(contact) = support_contact(wheel, down, axle, collider, world_hit) {
                        // Other children/colliders can still provide an earlier admissible hit.
                        options.max_time_of_impact = child_hit.time_of_impact;
                        best_contact = Some(contact);
                    }
                }
            }
        }
        if count == MAX_REJECTED {
            return best_contact;
        }
        rejected[count] = collider;
    }
    best_contact
}

fn support_contact(
    wheel: WheelCast,
    down: Vec3,
    axle: Vec3,
    collider: ColliderHandle,
    hit: ShapeCastHit,
) -> Option<EnvelopeContact> {
    // The lifted query can overlap an overhead object. Such a hit is not tire support.
    if hit.time_of_impact <= 1e-5
        || !hit.time_of_impact.is_finite()
        || matches!(
            hit.status,
            ShapeCastStatus::Failed | ShapeCastStatus::PenetratingOrWithinTargetDist
        )
        || !hit.witness1.is_finite()
        || !hit.normal1.is_finite()
        || hit.normal1.length_squared() < 0.5
    {
        return None;
    }
    let normal = hit.normal1.normalize();
    // Near-vertical side contacts must not turn the suspension's clipped gain into wall thrust.
    if -normal.dot(down) < 0.2 || axle.cross(normal).length_squared() < 0.01 {
        return None;
    }
    // GJK's conservative TOI can stop a few millimetres before contact. Recover
    // the cylinder's exact support distance from its witness plane instead.
    let axial = axle.dot(normal).abs().min(1.0);
    let support = wheel.radius * (1.0 - axial * axial).max(0.0).sqrt() + wheel.half_width * axial;
    let length = ((hit.witness1 - wheel.mount).dot(normal) + support) / down.dot(normal);
    if !length.is_finite() || length < -wheel.radius || length > wheel.rest_length + 1e-4 {
        return None;
    }
    Some(EnvelopeContact {
        collider,
        length,
        point: hit.witness1,
        normal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::time::TimeUpdateStrategy;
    use bevy_rapier3d::prelude::*;
    use std::time::Duration;

    fn query(step: bool, mount: Vec3, axle: Vec3) -> Option<EnvelopeContact> {
        let mut obstacles = vec![(
            Collider::cuboid(20.0, 0.5, 20.0),
            Transform::from_xyz(0.0, -0.5, 0.0),
        )];
        if step {
            obstacles.push((
                Collider::cuboid(2.0, 0.125, 1.0),
                Transform::from_xyz(0.0, 0.125, 1.0),
            ));
        }
        query_cast(
            WheelCast {
                mount,
                down: Vec3::NEG_Y,
                axle,
                radius: 0.53,
                half_width: MODEL_TIRE_HALF_WIDTH,
                rest_length: 0.36,
            },
            obstacles,
        )
    }

    fn query_cast(
        wheel: WheelCast,
        obstacles: Vec<(Collider, Transform)>,
    ) -> Option<EnvelopeContact> {
        measured_query(wheel, obstacles).0
    }

    fn measured_query(
        wheel: WheelCast,
        obstacles: Vec<(Collider, Transform)>,
    ) -> (Option<EnvelopeContact>, usize) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule(),
        ))
        .init_resource::<Assets<Mesh>>()
        .insert_resource(Time::<Fixed>::from_hz(120.0))
        .insert_resource(TimestepMode::Fixed {
            dt: 1.0 / 120.0,
            substeps: 1,
        })
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 120.0,
        )));
        for (collider, transform) in obstacles {
            app.world_mut()
                .spawn((RigidBody::Fixed, collider, transform));
        }
        for _ in 0..4 {
            app.update();
        }
        app.world_mut()
            .run_system_once(move |context: ReadRapierContext| {
                context.single().unwrap().with_query_pipeline(
                    QueryFilter::default().exclude_sensors(),
                    |pipeline| {
                        let result = cast_envelope(&pipeline.query_pipeline, wheel);
                        let count = super::allocation_tests::count_allocations(|| {
                            for _ in 0..32 {
                                std::hint::black_box(cast_envelope(
                                    &pipeline.query_pipeline,
                                    wheel,
                                ));
                            }
                        });
                        (result, count)
                    },
                )
            })
            .unwrap()
    }

    #[test]
    fn envelope_flat_ground_preserves_the_native_ray_length() {
        let hit = query(false, Vec3::new(0.0, 0.85, 0.0), Vec3::X)
            .expect("wheel envelope must find the floor");
        assert!((hit.length - 0.32).abs() < 1e-4, "{hit:?}");
        assert!(hit.normal.distance(Vec3::Y) < 2e-4, "{hit:?}");
        assert!(hit.point.y.abs() < 1e-4, "{hit:?}");
    }

    #[test]
    fn envelope_detects_the_vertical_step_before_the_hub_crosses_the_edge() {
        let hit = query(true, Vec3::new(0.0, 0.85, -0.4), Vec3::X)
            .expect("wheel envelope must find the step edge");
        let expected = 0.85 - (0.25 + (0.53_f32.powi(2) - 0.4_f32.powi(2)).sqrt());
        assert!(
            (hit.length - expected).abs() < 0.004,
            "{hit:?}, expected {expected}"
        );
        assert!(
            (hit.point.y - 0.25).abs() < 0.004 && hit.point.z.abs() < 0.004,
            "{hit:?}"
        );
        assert!(hit.normal.y > 0.6 && hit.normal.z < -0.6, "{hit:?}");
    }

    fn wheel() -> WheelCast {
        WheelCast {
            mount: Vec3::new(0.0, 0.85, -0.4),
            down: Vec3::NEG_Y,
            axle: Vec3::X,
            radius: 0.53,
            half_width: MODEL_TIRE_HALF_WIDTH,
            rest_length: 0.36,
        }
    }

    #[test]
    fn envelope_queries_follow_translated_rotated_obstacles_and_steering() {
        let rotation = Quat::from_rotation_y(0.63);
        let offset = Vec3::new(3.0, 2.0, -4.0);
        let base = query(true, wheel().mount, Vec3::X).unwrap();
        let mut cast = wheel();
        cast.mount = offset + rotation * cast.mount;
        cast.down = rotation * cast.down;
        cast.axle = rotation * cast.axle;
        let hit = query_cast(
            cast,
            vec![(
                Collider::cuboid(2.0, 0.125, 1.0),
                Transform::from_translation(offset + rotation * Vec3::new(0.0, 0.125, 1.0))
                    .with_rotation(rotation),
            )],
        )
        .unwrap();
        assert!((hit.length - base.length).abs() < 0.004, "{hit:?}");
        assert!(
            hit.normal.distance(rotation * base.normal) < 0.01,
            "{hit:?}"
        );
        let steered = query(true, wheel().mount, Quat::from_rotation_y(0.3) * Vec3::X).unwrap();
        assert!(
            steered.length < 0.30 && steered.normal.is_finite(),
            "{steered:?}"
        );
    }

    #[test]
    fn envelope_uses_real_tread_width_without_support_outside_it() {
        let inside = query(true, Vec3::new(2.1, 0.85, -0.3), Vec3::X).unwrap();
        let outside = query(true, Vec3::new(2.2, 0.85, -0.3), Vec3::X).unwrap();
        assert!(inside.length < 0.25, "{inside:?}");
        assert!((outside.length - 0.32).abs() < 0.001, "{outside:?}");
    }

    #[test]
    fn envelope_detects_a_catalog_rock_before_the_center_ray() {
        use crate::terrain::obstacles::{Obstacle, ObstacleKind};
        let rock = Obstacle {
            id: 1,
            kind: ObstacleKind::Rock,
            position: [0.0, 0.25, 0.5],
            size: [1.2, 0.5, 1.0],
            rotation_degrees: [0.0; 3],
        };
        let (_, collider) = rock.build().unwrap();
        let mut cast = wheel();
        cast.mount.z = -0.1;
        let hit = query_cast(cast, vec![(collider, rock.transform())]).unwrap();
        assert!(
            hit.length < 0.32 && hit.normal.y > 0.2 && hit.normal.z < 0.0,
            "{hit:?}"
        );
        assert!(
            hit.point.z >= -0.001,
            "central ray at z=-0.1 misses the rock: {hit:?}"
        );
    }

    #[test]
    fn envelope_rejects_degenerate_inputs_and_initial_overlap() {
        assert!(query_cast(wheel(), vec![]).is_none());
        for bad in [
            WheelCast {
                radius: f32::NAN,
                ..wheel()
            },
            WheelCast {
                down: Vec3::ZERO,
                ..wheel()
            },
            WheelCast {
                axle: Vec3::Y,
                ..wheel()
            },
            WheelCast {
                half_width: 0.0,
                ..wheel()
            },
        ] {
            assert!(query_cast(bad, vec![]).is_none());
        }
        // The raised cast starts inside this overhead block: do not invent support.
        assert!(
            query_cast(
                wheel(),
                vec![(
                    Collider::cuboid(1.0, 0.2, 1.0),
                    Transform::from_xyz(0.0, 1.4, -0.4)
                )]
            )
            .is_none()
        );
    }

    #[test]
    fn envelope_keeps_valid_step_support_below_an_overlapping_overhead_collider() {
        let expected = query(true, wheel().mount, Vec3::X).unwrap();
        let hit = query_cast(
            wheel(),
            vec![
                (
                    Collider::cuboid(1.0, 0.2, 1.0),
                    Transform::from_xyz(0.0, 1.4, -0.4),
                ),
                (
                    Collider::cuboid(20.0, 0.5, 20.0),
                    Transform::from_xyz(0.0, -0.5, 0.0),
                ),
                (
                    Collider::cuboid(2.0, 0.125, 1.0),
                    Transform::from_xyz(0.0, 0.125, 1.0),
                ),
            ],
        )
        .expect("A rejected ceiling must not hide the valid step contact");
        assert!((hit.length - expected.length).abs() < 0.004, "{hit:?}");
        assert!(hit.normal.y > 0.6 && hit.normal.z < -0.6, "{hit:?}");
    }

    #[test]
    fn envelope_compound_ceiling_does_not_mask_step_support() {
        let compound = Collider::compound(vec![
            (
                Vec3::new(0.0, 1.4, -0.4),
                Quat::IDENTITY,
                Collider::cuboid(1.0, 0.2, 1.0),
            ),
            (
                Vec3::new(0.0, 0.125, 1.0),
                Quat::IDENTITY,
                Collider::cuboid(2.0, 0.125, 1.0),
            ),
        ]);
        let (hit, allocations) = measured_query(wheel(), vec![(compound, Transform::IDENTITY)]);
        let hit = hit.expect("An overlapping compound ceiling must not mask its step child");
        let expected = 0.85 - (0.25 + (0.53_f32.powi(2) - 0.4_f32.powi(2)).sqrt());
        assert!((hit.length - expected).abs() < 0.004, "{hit:?}");
        assert!(
            (hit.point.y - 0.25).abs() < 0.004 && hit.point.z.abs() < 0.004,
            "{hit:?}"
        );
        assert!(hit.normal.y > 0.6 && hit.normal.z < -0.6, "{hit:?}");
        assert_eq!(
            allocations, 0,
            "Repeated compound queries allocate heap memory"
        );
    }

    #[test]
    fn envelope_repeated_rejected_ceiling_queries_do_not_allocate() {
        let (hit, allocations) = measured_query(
            wheel(),
            vec![
                (
                    Collider::cuboid(1.0, 0.2, 1.0),
                    Transform::from_xyz(0.0, 1.4, -0.4),
                ),
                (
                    Collider::cuboid(20.0, 0.5, 20.0),
                    Transform::from_xyz(0.0, -0.5, 0.0),
                ),
                (
                    Collider::cuboid(2.0, 0.125, 1.0),
                    Transform::from_xyz(0.0, 0.125, 1.0),
                ),
            ],
        );
        assert!(hit.is_some());
        assert_eq!(
            allocations, 0,
            "Repeated rejected support queries allocate heap memory"
        );
    }
}
