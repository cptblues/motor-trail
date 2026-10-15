use super::*;
use crate::vehicle::config::VehiclePhysicsConfig;
use crate::vehicle::{Truck, VehiclePlugin};
use bevy::ecs::system::RunSystemOnce;
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::*;
use std::time::Duration;

fn fixture(rotation: Quat, slope: Quat) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, VehiclePlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 120.0,
        )))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<crate::GameState>();
    let normal = slope * Vec3::Y;
    app.world_mut().spawn((
        RigidBody::Fixed,
        Collider::cuboid(50.0, 0.5, 50.0),
        Transform::from_translation(-normal * 0.5).with_rotation(slope),
    ));
    let car = app
        .world_mut()
        .spawn((
            crate::vehicle::controller::chassis_bundle(&VehiclePhysicsConfig::baseline()),
            Transform::from_translation(normal * 3.5).with_rotation(slope * rotation),
        ))
        .id();
    (app, car)
}

fn advance(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

#[test]
fn rollover_both_flanks_and_roof_have_support_without_automatic_righting() {
    for roll in [
        std::f32::consts::PI,
        std::f32::consts::FRAC_PI_2,
        -std::f32::consts::FRAC_PI_2,
    ] {
        let (mut app, car) = fixture(Quat::from_rotation_z(roll), Quat::IDENTITY);
        let floor = app
            .world_mut()
            .query_filtered::<Entity, (With<Collider>, Without<Truck>)>()
            .single(app.world())
            .unwrap();
        let mut side_contact = false;
        let mut previous = app.world().get::<Transform>(car).unwrap().rotation;
        for _ in 0..1200 {
            app.update();
            let pose = app.world().get::<Transform>(car).unwrap();
            assert!(
                previous.angle_between(pose.rotation) < 0.5,
                "Discontinuous righting rotation"
            );
            previous = pose.rotation;
            let on_side = (pose.rotation * Vec3::Y).y.abs() < 0.6;
            if on_side {
                side_contact |= app
                    .world_mut()
                    .run_system_once(move |context: ReadRapierContext| {
                        context
                            .single()
                            .unwrap()
                            .contact_pair(car, floor)
                            .is_some_and(|pair| pair.has_any_active_contact())
                    })
                    .unwrap();
            }
        }
        let pose = app.world().get::<Transform>(car).unwrap();
        let velocity = app.world().get::<Velocity>(car).unwrap();
        assert!(
            lowest_point(pose) > -0.04,
            "roll {roll}, lowest {}, {pose:?}",
            lowest_point(pose)
        );
        if roll.abs() > 3.0 {
            assert!(
                (pose.rotation * Vec3::Y).y < 0.0,
                "Roof support was lost: {pose:?}"
            );
        } else {
            // The low COM can naturally roll a flank back onto its tires after impact.
            assert!(
                side_contact,
                "The flank never made a real body/floor contact"
            );
        }
        assert!(
            velocity.linear.length() < 0.20 && velocity.angular.length() < 0.20,
            "{velocity:?}"
        );
    }
}

#[test]
fn rollover_body_contacts_an_inclined_and_steep_surface() {
    for angle in [0.22, 0.52] {
        let slope = Quat::from_rotation_x(angle);
        let (mut app, car) = fixture(Quat::from_rotation_z(std::f32::consts::PI), slope);
        advance(&mut app, 600);
        let pose = app.world().get::<Transform>(car).unwrap();
        let relative = Transform::from_translation(slope.inverse() * pose.translation)
            .with_rotation(slope.inverse() * pose.rotation);
        assert!(
            lowest_point(&relative) > -0.05,
            "slope {angle}, penetration {}",
            -lowest_point(&relative)
        );
        assert!(app.world().get::<Velocity>(car).unwrap().linear.is_finite());
    }
}

#[test]
fn rollover_roof_has_real_contact_with_a_rock() {
    let (mut app, car) = fixture(Quat::from_rotation_z(std::f32::consts::PI), Quat::IDENTITY);
    let rock = app
        .world_mut()
        .spawn((
            RigidBody::Fixed,
            Collider::ball(0.8),
            Transform::from_xyz(0.0, 0.2, 0.0),
        ))
        .id();
    let mut contacted = false;
    for _ in 0..700 {
        app.update();
        contacted |= app
            .world_mut()
            .run_system_once(move |context: ReadRapierContext| {
                context
                    .single()
                    .ok()
                    .and_then(|c| {
                        c.contact_pair(car, rock)
                            .map(|p| p.has_any_active_contact())
                    })
                    .unwrap_or(false)
            })
            .unwrap();
    }
    assert!(contacted, "The inverted body never contacted the rock");
    assert!(lowest_point(app.world().get::<Transform>(car).unwrap()) > -0.05);
}

#[test]
fn composed_body_keeps_exact_profile_mass_com_and_inertia() {
    let (mut app, car) = fixture(Quat::IDENTITY, Quat::IDENTITY);
    advance(&mut app, 600);
    let expected = crate::vehicle::controller::mass_properties(&VehiclePhysicsConfig::baseline());
    let actual = app.world().get::<ReadMassProperties>(car).unwrap();
    assert!((actual.mass - expected.mass).abs() < 0.01);
    assert!(
        actual
            .local_center_of_mass
            .distance(expected.local_center_of_mass)
            < 1e-5
    );
    assert!(
        actual
            .principal_inertia
            .distance(expected.principal_inertia)
            < 0.05
    );
    let mut bodies = app.world_mut().query::<&RapierRigidBodySet>();
    assert_eq!(
        bodies
            .single(app.world())
            .unwrap()
            .bodies
            .iter()
            .filter(|(_, b)| b.is_dynamic())
            .count(),
        1
    );
    assert!(
        app.world_mut()
            .query_filtered::<Entity, With<Truck>>()
            .iter(app.world())
            .count()
            == 1
    );
}

#[test]
fn slow_front_and_rear_bumpers_really_contact_obstacles() {
    for direction in [-1.0, 1.0] {
        let (mut app, car) = fixture(Quat::IDENTITY, Quat::IDENTITY);
        advance(&mut app, 500);
        let block = app
            .world_mut()
            .spawn((
                RigidBody::Fixed,
                Collider::cuboid(2.0, 0.6, 0.25),
                Transform::from_xyz(0.0, 0.6, 2.8 * direction),
            ))
            .id();
        app.world_mut().get_mut::<Velocity>(car).unwrap().linear = Vec3::Z * (1.5 * direction);
        let mut contacted = false;
        for _ in 0..500 {
            app.update();
            contacted |= app
                .world_mut()
                .run_system_once(move |context: ReadRapierContext| {
                    context
                        .single()
                        .ok()
                        .and_then(|c| {
                            c.contact_pair(car, block)
                                .map(|p| p.has_any_active_contact())
                        })
                        .unwrap_or(false)
                })
                .unwrap();
        }
        assert!(contacted, "No bumper contact in direction {direction}");
    }
}
