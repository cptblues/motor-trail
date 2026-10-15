use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Collider;
use std::time::Duration;

fn move_at_fixed_rate(
    mut car: Query<&mut Transform, With<Truck>>,
    mut visual: ResMut<VehicleVisualState>,
) {
    let mut car = car.single_mut().unwrap();
    car.translation.x += 12.0 / 120.0;
    car.rotate_y(0.6 / 120.0);
    car.rotation = car.rotation.normalize();
    for i in 0..4 {
        visual.spins[i] = (visual.spins[i] + 3.0 / 120.0).rem_euclid(TAU);
        visual.travels[i] += 0.03 / 120.0;
    }
    visual.steer += 0.01 / 120.0;
}

fn app(fps: f64, interpolate: bool) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / fps,
        )))
        .insert_resource(Time::<Fixed>::from_hz(120.0))
        .init_resource::<VehiclePhysics>()
        .init_resource::<VehicleVisualState>()
        .init_resource::<WheelContactSettings>()
        .init_resource::<VehiclePresentation>()
        .add_systems(
            FixedUpdate,
            (begin_step, move_at_fixed_rate, finish_step).chain(),
        );
    if interpolate {
        app.add_systems(
            Update,
            (present, super::super::visuals::animate_wheels).chain(),
        );
    }
    let car = app.world_mut().spawn((Truck, Transform::IDENTITY)).id();
    let authored = Transform::from_xyz(0.0, -1.0, 0.0).with_rotation(Quat::from_rotation_y(PI));
    let graphic = app
        .world_mut()
        .spawn((
            VehicleGraphic::new(car, Transform::IDENTITY, authored),
            authored,
            ChildOf(car),
        ))
        .id();
    (app, car, graphic)
}

#[test]
fn graphics_interpolate_at_30_60_144_and_240_fps_without_mutating_physics() {
    for fps in [30.0, 60.0, 144.0, 240.0] {
        let (mut app, car, graphic) = app(fps, true);
        let (mut reference, reference_car, _) = self::app(fps, false);
        let mut previous = None;
        for frame in 0..(fps as usize * 2) {
            app.update();
            reference.update();
            let physical = *app.world().get::<Transform>(car).unwrap();
            assert_eq!(
                physical,
                *reference.world().get::<Transform>(reference_car).unwrap()
            );
            if frame < 10 {
                continue;
            }
            let alpha = app.world().resource::<Time<Fixed>>().overstep_fraction();
            let shown = app
                .world()
                .resource::<VehiclePresentation>()
                .pose_for(car, physical);
            let expected = physical.translation.x - 0.1 * (1.0 - alpha);
            assert!((shown.translation.x - expected).abs() < 2.0e-5);
            let global = app.world().get::<GlobalTransform>(graphic).unwrap();
            let expected = GlobalTransform::from(shown).mul_transform(
                Transform::from_xyz(0.0, -1.0, 0.0).with_rotation(Quat::from_rotation_y(PI)),
            );
            assert!(global.translation().distance(expected.translation()) < 2.0e-5);
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!((global.rotation() * axis).distance(expected.rotation() * axis) < 1.0e-5);
            }
            if let Some(last) = previous {
                let delta: f32 = shown.translation.x - last;
                assert!(
                    (delta - 12.0 / fps as f32).abs() < 3.0e-5,
                    "{fps} fps, delta {delta}"
                );
            }
            previous = Some(shown.translation.x);
        }
    }
}

#[test]
fn wheel_spin_wraps_in_both_directions_and_travel_and_steer_share_the_frame() {
    let a = WheelPose {
        spins: [TAU - 0.1; 4],
        travels: [0.2; 4],
        steer: -0.2,
    };
    let b = WheelPose {
        spins: [0.1; 4],
        travels: [0.4; 4],
        steer: 0.2,
    };
    for (from, to) in [(a, b), (b, a)] {
        let half = from.interpolate(to, 0.5);
        assert!(Quat::from_rotation_x(half.spins[0]).angle_between(Quat::IDENTITY) < 0.001);
        assert!((half.travels[0] - 0.3).abs() < 1.0e-6);
        assert!(half.steer.abs() < 1.0e-6);
    }
}

#[test]
fn teleport_and_geometry_change_snap_even_on_a_frame_without_a_fixed_tick() {
    let (mut app, car, _) = app(240.0, true);
    for _ in 0..10 {
        app.update();
    }
    let reset = Transform::from_xyz(100.0, 7.0, -8.0).with_rotation(Quat::from_rotation_z(PI));
    *app.world_mut().get_mut::<Transform>(car).unwrap() = reset;
    *app.world_mut().resource_mut::<VehicleVisualState>() = VehicleVisualState::default();
    app.world_mut().run_system_once(present).unwrap();
    let history = app.world().resource::<VehiclePresentation>();
    assert_eq!(history.pose_for(car, Transform::IDENTITY), reset);
    assert_eq!(history.displayed.unwrap().wheels.spins, [0.0; 4]);
    for _ in 0..10 {
        app.update();
    }
    app.world_mut()
        .resource_mut::<VehiclePhysics>()
        .active_config
        .tires
        .radius_m = 0.6;
    app.world_mut().run_system_once(present).unwrap();
    assert_eq!(
        app.world()
            .resource::<VehiclePresentation>()
            .displayed
            .unwrap()
            .pose,
        *app.world().get::<Transform>(car).unwrap()
    );
}

#[test]
fn new_vehicle_and_scene_unload_do_not_reuse_old_history() {
    let (mut app, car, _) = app(144.0, true);
    for _ in 0..10 {
        app.update();
    }
    app.world_mut().despawn(car);
    app.world_mut().run_system_once(present).unwrap();
    assert!(
        app.world()
            .resource::<VehiclePresentation>()
            .displayed
            .is_none()
    );
    let pose = Transform::from_xyz(5.0, 6.0, 7.0);
    let new_car = app.world_mut().spawn((Truck, pose)).id();
    app.world_mut().run_system_once(present).unwrap();
    assert_eq!(
        app.world()
            .resource::<VehiclePresentation>()
            .pose_for(new_car, Transform::IDENTITY),
        pose
    );
}

#[test]
fn equipment_graphics_follow_displayed_chassis_but_collider_parent_stays_physical() {
    let (mut app, car, _) = app(144.0, true);
    let mount = Transform::from_xyz(0.3, 1.7, -0.4).with_rotation(Quat::from_rotation_y(0.4));
    let equipment = app
        .world_mut()
        .spawn((mount, Collider::cuboid(0.3, 0.2, 0.4), ChildOf(car)))
        .id();
    let graphic = app
        .world_mut()
        .spawn((
            VehicleGraphic::new(car, mount, Transform::IDENTITY),
            Transform::IDENTITY,
            ChildOf(equipment),
        ))
        .id();
    for _ in 0..100 {
        app.update();
    }
    let physical = *app.world().get::<Transform>(car).unwrap();
    let displayed = app
        .world()
        .resource::<VehiclePresentation>()
        .pose_for(car, physical);
    assert_eq!(*app.world().get::<Transform>(equipment).unwrap(), mount);
    let collider_world = app.world().get::<GlobalTransform>(equipment).unwrap();
    assert!(
        collider_world
            .translation()
            .distance(physical.mul_transform(mount).translation)
            < 1.0e-5
    );
    let graphics_world = app.world().get::<GlobalTransform>(graphic).unwrap();
    assert!(
        graphics_world
            .translation()
            .distance(displayed.mul_transform(mount).translation)
            < 1.0e-5
    );
    assert!(app.world().get::<Collider>(graphic).is_none());
}
