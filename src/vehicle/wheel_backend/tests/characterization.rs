use super::*;
use crate::vehicle::contacts::{EnvelopeContact, MODEL_TIRE_HALF_WIDTH, WheelCast, cast_envelope};

const RADIUS: f32 = 0.53;
const MOUNT_Y: f32 = 0.85;
const STEP_HEIGHT: f32 = 0.25;
const HALF_GAP: f32 = 0.4;
// Release queries measured 0.00004283 m length error and about 0.001012
// normal-vector error. Allow 0.1 mm and 0.002 (about 0.115 degrees);
// identical static queries still have to repeat exactly in repeat_contact.
const FLAT_QUERY_LENGTH_EPSILON: f32 = 1e-4;
const FLAT_QUERY_NORMAL_EPSILON: f32 = 0.002;

fn cast(mount: Vec3) -> WheelCast {
    WheelCast {
        mount,
        down: Vec3::NEG_Y,
        axle: Vec3::NEG_X,
        radius: RADIUS,
        half_width: MODEL_TIRE_HALF_WIDTH,
        rest_length: 0.36,
    }
}

fn wedge(reverse_insertion: bool) -> (Scene, [ColliderHandle; 2]) {
    let mut scene = Scene::new(Vec3::new(0.0, MOUNT_Y, 0.0), None, false);
    let sides = if reverse_insertion {
        [1.0, -1.0]
    } else {
        [-1.0, 1.0]
    };
    // Two separate, mirrored blocks; the center ray goes through their empty gap.
    let obstacles = sides.map(|side| {
        scene.colliders.insert(
            ColliderBuilder::cuboid(3.0, STEP_HEIGHT / 2.0, 1.0).translation(Vec3::new(
                0.0,
                STEP_HEIGHT / 2.0,
                side * (HALF_GAP + 1.0),
            )),
        )
    });
    scene.refresh();
    (scene, obstacles)
}

fn flat_trimesh(position: Vec3, other_diagonal: bool) -> (Scene, ColliderHandle) {
    let mut scene = Scene::new(position, None, false);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for z in [-2.0, 0.0, 2.0] {
        for x in [-2.0, 0.0, 2.0] {
            vertices.push(Vec3::new(x, 0.0, z));
        }
    }
    for z in 0..2 {
        for x in 0..2 {
            let a = z * 3 + x;
            let b = a + 1;
            let c = a + 3;
            let d = c + 1;
            if other_diagonal {
                triangles.extend([[a, c, d], [a, d, b]]);
            } else {
                // Same upward winding and cell diagonal as TerrainData::triangles.
                triangles.extend([[a, c, b], [b, c, d]]);
            }
        }
    }
    let floor = scene
        .colliders
        .insert(ColliderBuilder::trimesh(vertices, triangles).expect("valid flat triangle grid"));
    assert!(scene.colliders[floor].shape().as_trimesh().is_some());
    scene.refresh();
    (scene, floor)
}

fn repeat_contact(
    queries: &QueryPipeline<'_>,
    wheel: WheelCast,
    repetitions: usize,
) -> EnvelopeContact {
    let first = cast_envelope(queries, wheel).expect("fixture must have envelope support");
    for sample in 0..repetitions {
        let hit = cast_envelope(queries, wheel).expect("static support must not disappear");
        assert!(hit.length.is_finite(), "sample {sample}: {hit:?}");
        assert!(hit.point.is_finite() && hit.normal.is_finite(), "{hit:?}");
        assert_scalar(hit.normal.length(), 1.0, 1e-5);
        assert!(hit.length >= -wheel.radius && hit.length <= wheel.rest_length + 1e-4);
        assert_eq!(hit.collider, first.collider, "static sample {sample}");
        assert_eq!(hit.length, first.length, "static sample {sample}");
        assert_eq!(hit.normal, first.normal, "static sample {sample}");
        assert_eq!(hit.point, first.point, "static sample {sample}");
    }
    first
}

fn expected_wedge_length(mount_z: f32) -> f32 {
    let distance_to_nearer_edge = HALF_GAP - mount_z.abs();
    MOUNT_Y - STEP_HEIGHT - (RADIUS.powi(2) - distance_to_nearer_edge.powi(2)).sqrt()
}

#[test]
fn wedged_two_obstacles_static_envelope_queries_are_repeatable() {
    for reverse_insertion in [false, true] {
        let (mut scene, obstacles) = wedge(reverse_insertion);
        let wheel = cast(Vec3::new(0.0, MOUNT_Y, 0.0));
        let hit = repeat_contact(&scene.queries().as_ref(), wheel, 128);
        assert!(obstacles.contains(&hit.collider), "{hit:?}");
        assert_scalar(hit.length, expected_wedge_length(0.0), 0.004);
        assert_scalar(hit.point.y, STEP_HEIGHT, 0.004);
        assert_scalar(hit.point.z.abs(), HALF_GAP, 0.004);
        assert!(hit.normal.y > 0.6 && hit.normal.z.abs() > 0.6, "{hit:?}");
        assert!(
            scene
                .queries()
                .as_ref()
                .cast_ray(
                    &Ray::new(wheel.mount, wheel.down),
                    wheel.rest_length + wheel.radius,
                    true,
                )
                .is_none(),
            "the center ray must miss both wedge obstacles"
        );

        // Independently establish both admissible supports without assuming a tie winner.
        let isolated = obstacles.map(|excluded| {
            let mut queries = scene.queries();
            queries.filter = queries.filter.exclude_collider(excluded);
            let contact = repeat_contact(&queries.as_ref(), wheel, 8);
            assert_ne!(contact.collider, excluded);
            assert_scalar(contact.length, expected_wedge_length(0.0), 0.004);
            contact
        });
        assert!(isolated[0].normal.z * isolated[1].normal.z < -0.36);
        assert_scalar(isolated[0].length, isolated[1].length, 0.004);
        eprintln!(
            "static wedge reverse_insertion={reverse_insertion}, repeated=128, selected={hit:?}, isolated={isolated:?}"
        );
    }
}

#[test]
fn wedged_two_obstacles_perturbations_characterize_length_and_normal_changes() {
    for reverse_insertion in [false, true] {
        let (mut scene, _) = wedge(reverse_insertion);
        for offset in [0.01, 0.001, 0.0001, 0.000001] {
            let pair = [-offset, offset].map(|z| {
                let hit = repeat_contact(
                    &scene.queries().as_ref(),
                    cast(Vec3::new(0.0, MOUNT_Y, z)),
                    32,
                );
                assert_scalar(hit.length, expected_wedge_length(z), 0.004);
                assert_scalar(hit.point.y, STEP_HEIGHT, 0.004);
                assert!(hit.normal.y > 0.6 && hit.normal.z.abs() > 0.6, "{hit:?}");
                hit
            });
            let length_delta = (pair[0].length - pair[1].length).abs();
            let normal_delta = pair[0].normal.distance(pair[1].normal);
            assert!(length_delta < 0.008, "offset={offset}: {pair:?}");
            // Submillimeter GJK ties may retain either candidate. The 1 cm pair
            // separates the supports beyond the existing edge-query tolerance.
            if offset == 0.01 {
                assert!(
                    pair[0].normal.z > 0.6 && pair[1].normal.z < -0.6,
                    "{pair:?}"
                );
                assert!(normal_delta > 1.2, "{pair:?}");
                assert_ne!(pair[0].collider, pair[1].collider);
            }
            eprintln!(
                "perturbed wedge reverse_insertion={reverse_insertion}, offset=+/-{offset:.6} m, length_delta={length_delta:.8} m, normal_delta={normal_delta:.8}, pair={pair:?}"
            );
        }
    }
}

#[test]
fn flat_trimesh_envelope_queries_stay_flat_across_cell_and_triangle_seams() {
    for other_diagonal in [false, true] {
        let (mut scene, floor) = flat_trimesh(Vec3::new(0.0, MOUNT_Y, 0.0), other_diagonal);
        let mut max_length_error = 0.0_f32;
        let mut max_normal_error = 0.0_f32;
        for (x, z) in [
            (0.0, 0.0),
            (-0.001, 0.0),
            (0.001, 0.0),
            (0.0, -0.001),
            (0.0, 0.001),
            (0.5, 0.499),
            (0.5, 0.5),
            (0.5, 0.501),
            (0.5, 1.499),
            (0.5, 1.5),
            (0.5, 1.501),
        ] {
            for steering in [0.0, 0.3, -0.45] {
                let mut wheel = cast(Vec3::new(x, MOUNT_Y, z));
                wheel.axle = Quat::from_rotation_y(steering) * Vec3::NEG_X;
                let hit = repeat_contact(&scene.queries().as_ref(), wheel, 16);
                assert_eq!(hit.collider, floor);
                let length_error = (hit.length - (MOUNT_Y - RADIUS)).abs();
                let normal_error = hit.normal.distance(Vec3::Y);
                assert!(
                    length_error <= FLAT_QUERY_LENGTH_EPSILON
                        && normal_error <= FLAT_QUERY_NORMAL_EPSILON
                        && hit.point.y.abs() <= FLAT_QUERY_LENGTH_EPSILON,
                    "other_diagonal={other_diagonal}, mount={:?}, steering={steering}, length_error={length_error}, normal_error={normal_error}: {hit:?}",
                    wheel.mount
                );
                max_length_error = max_length_error.max(length_error);
                max_normal_error = max_normal_error.max(normal_error);
            }
        }
        eprintln!(
            "flat TriMesh other_diagonal={other_diagonal}, max_length_error={max_length_error:.8} m, max_normal_error={max_normal_error:.8}"
        );
    }
}

#[test]
fn flat_trimesh_envelope_backend_matches_upstream_at_seams_and_negative_travel() {
    for mount_y in [MOUNT_Y, 0.05] {
        for other_diagonal in [false, true] {
            let position = Vec3::new(0.0, mount_y, 0.0);
            let (mut enabled_scene, _) = flat_trimesh(position, other_diagonal);
            let (mut native_scene, _) = flat_trimesh(position, other_diagonal);
            let mut enabled = controller(
                enabled_scene.chassis,
                TireContactMode::Envelope,
                &[Vec3::ZERO],
            );
            let mut native = DynamicRayCastVehicleController::new(native_scene.chassis);
            native.index_forward_axis = 2;
            native.add_wheel(
                Vec3::ZERO,
                Vec3::NEG_Y,
                Vec3::NEG_X,
                0.36,
                RADIUS,
                &tuning(),
            );
            for (x, z, steering, engine, brake) in [
                (0.0, 0.0, 0.0, 0.0, 0.0),
                (-0.001, 0.0, 0.3, 1700.0, 0.0),
                (0.001, 0.0, -0.3, -1300.0, 0.0),
                (0.5, 0.499, 0.0, 0.0, 25.0),
                (0.5, 0.5, 0.3, 1700.0, 0.0),
                (0.5, 0.501, -0.3, 0.0, 25.0),
            ] {
                for scene in [&mut enabled_scene, &mut native_scene] {
                    let body = &mut scene.bodies[scene.chassis];
                    body.set_translation(Vec3::new(x, mount_y, z), true);
                    body.set_linvel(Vec3::ZERO, true);
                    body.set_angvel(Vec3::ZERO, true);
                    scene.refresh();
                    scene.bodies[scene.chassis].set_linvel(Vec3::new(0.1, -0.12, 2.0), true);
                    scene.bodies[scene.chassis].set_angvel(Vec3::new(0.02, 0.03, -0.01), true);
                }
                let wheel = &mut enabled.wheels_mut()[0];
                wheel.steering = steering;
                wheel.engine_force = engine;
                wheel.brake = brake;
                let wheel = &mut native.wheels_mut()[0];
                wheel.steering = steering;
                wheel.engine_force = engine;
                wheel.brake = brake;
                enabled.update_vehicle(DT, enabled_scene.queries());
                native.update_vehicle(DT, native_scene.queries());
                assert!(enabled.wheels()[0].raycast_info().is_in_contact);
                assert_wheel(&enabled.wheels()[0], &native.wheels()[0], 0.0);
                let actual = &enabled_scene.bodies[enabled_scene.chassis];
                let expected = &native_scene.bodies[native_scene.chassis];
                assert!(actual.linvel().is_finite() && actual.angvel().is_finite());
                assert_eq!(actual.linvel(), expected.linvel());
                assert_eq!(actual.angvel(), expected.angvel());
            }
        }
    }
}

#[test]
fn wedged_two_obstacles_constrained_integration_moves_and_remains_bounded() {
    for reverse_insertion in [false, true] {
        let (mut scene, obstacles) = wedge(reverse_insertion);
        // A single wheel on a Y/Z rail isolates contact feedback. This is not
        // a free chassis, a four-wheel equilibrium, or a visual-jitter assertion.
        // Keep the real terrain floor under the blocks. An unsupported raycast
        // wheel has no rigid collider to catch the chassis after losing a hit.
        let floor = scene.colliders.insert(
            ColliderBuilder::cuboid(20.0, 0.5, 20.0).translation(Vec3::new(0.0, -0.5, 0.0)),
        );
        scene.refresh();
        let body = &mut scene.bodies[scene.chassis];
        body.lock_rotations(true, true);
        body.set_enabled_translations(false, true, true, true);
        let mut enabled = controller(scene.chassis, TireContactMode::Envelope, &[Vec3::ZERO]);
        let mut previous: Option<RayCastInfo> = None;
        let mut contacts = 0;
        let mut switches = 0;
        let mut moving_ticks = 0;
        let mut max_length_step = 0.0_f32;
        let mut max_normal_step = 0.0_f32;
        let mut max_speed = 0.0_f32;
        let mut max_z = 0.0_f32;
        let mut min_y = MOUNT_Y;
        let mut max_y = MOUNT_Y;
        for tick in 0..240 {
            enabled.update_vehicle(DT, scene.queries());
            let wheel = &enabled.wheels()[0];
            let hit = *wheel.raycast_info();
            assert!(hit.suspension_length.is_finite(), "tick {tick}: {hit:?}");
            assert!(hit.contact_normal_ws.is_finite() && hit.contact_point_ws.is_finite());
            assert!(wheel.wheel_suspension_force.is_finite());
            assert!(wheel.forward_impulse.is_finite() && wheel.side_impulse.is_finite());
            assert!(wheel.rotation.is_finite() && wheel.center().is_finite());
            assert!(hit.suspension_length >= -0.1401 && hit.suspension_length <= 0.8601);
            if hit.is_in_contact {
                contacts += 1;
                let ground = hit.ground_object.unwrap();
                assert!(obstacles.contains(&ground) || ground == floor);
                assert_scalar(hit.contact_normal_ws.length(), 1.0, 1e-5);
                if let Some(old) = previous.filter(|old| old.is_in_contact) {
                    switches += usize::from(old.ground_object != hit.ground_object);
                    max_length_step =
                        max_length_step.max((old.suspension_length - hit.suspension_length).abs());
                    max_normal_step =
                        max_normal_step.max(old.contact_normal_ws.distance(hit.contact_normal_ws));
                }
            }
            previous = Some(hit);
            let before_step = scene.bodies[scene.chassis].translation();
            scene.step(Vec3::new(0.0, -9.81, 0.0));
            let body = &scene.bodies[scene.chassis];
            let position = body.translation();
            assert!(position.is_finite() && body.linvel().is_finite() && body.angvel().is_finite());
            moving_ticks += usize::from(position.distance(before_step) > 1e-6);
            if tick == 0 {
                assert!(
                    position.y < before_step.y - 1e-6,
                    "the initially under-supported chassis must fall on the first step: before={before_step:?}, after={position:?}, velocity={:?}",
                    body.linvel()
                );
            }
            max_speed = max_speed.max(body.linvel().length());
            max_z = max_z.max(position.z.abs());
            min_y = min_y.min(position.y);
            max_y = max_y.max(position.y);
            assert!(
                max_speed < 10.0 && max_z < 2.0 && min_y > -1.0 && max_y < 2.0,
                "tick {tick}: position={position:?}, velocity={:?}",
                body.linvel()
            );
        }
        assert!(
            contacts > 0,
            "the integration trace must exercise wedge contact"
        );
        assert!(
            MOUNT_Y - min_y > 0.01,
            "the constrained chassis must integrate its vertical response, y_range=[{min_y}, {max_y}], moving_ticks={moving_ticks}"
        );
        eprintln!(
            "constrained integrated wedge reverse_insertion={reverse_insertion}, ticks=240, dt={DT}, moving_ticks={moving_ticks}, contacts={contacts}, collider_switches={switches}, max_length_step={max_length_step:.8} m, max_normal_step={max_normal_step:.8}, max_speed={max_speed:.8} m/s, max_abs_z={max_z:.8} m, y_range=[{min_y:.8}, {max_y:.8}] m"
        );
    }
}
