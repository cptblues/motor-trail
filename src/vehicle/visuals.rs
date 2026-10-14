use super::controller::{MODEL_WHEEL_RADIUS, MOUNTS, PHYSICS_DT, Truck, VehiclePhysics};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct VehicleVisualState {
    pub spins: [f32; 4],
    pub steer: f32,
    pub travels: [f32; 4],
    pub previous_hubs: [Option<Vec3>; 4],
}

pub fn update_wheel_motion(
    physics: Res<VehiclePhysics>,
    mut visual: ResMut<VehicleVisualState>,
    cars: Query<&Transform, With<Truck>>,
) {
    let Ok(car) = cars.single() else {
        return;
    };
    for (i, mount) in MOUNTS.into_iter().enumerate() {
        let contact = physics.wheels[i];
        visual.travels[i] = contact.suspension_length;
        let hub = car.translation + car.rotation * (mount - Vec3::Y * contact.suspension_length);
        if contact.grounded {
            if let Some(previous) = visual.previous_hubs[i] {
                let axle = car.rotation * Quat::from_rotation_y(contact.steer) * Vec3::NEG_X;
                let heading = contact.normal.cross(axle).normalize_or_zero();
                let distance = (hub - previous - contact.ground_velocity * PHYSICS_DT).dot(heading);
                if distance.abs() >= 0.025 * PHYSICS_DT {
                    visual.spins[i] = (visual.spins[i]
                        - distance / physics.active_config.tires.radius_m)
                        .rem_euclid(std::f32::consts::TAU);
                }
            }
            visual.previous_hubs[i] = Some(hub);
        } else {
            visual.previous_hubs[i] = None;
        }
    }
}

pub fn animate_wheels(
    physics: Res<VehiclePhysics>,
    visual: Res<VehicleVisualState>,
    mut nodes: Query<(&Name, &mut Transform), Without<Truck>>,
) {
    for (name, mut transform) in &mut nodes {
        let name = name.as_str();
        let index = match name {
            "Suspension_FL" | "Steer_FL" | "WheelSpin_FL" => 1,
            "Suspension_FR" | "Steer_FR" | "WheelSpin_FR" => 0,
            "Suspension_RL" | "WheelSpin_RL" => 3,
            "Suspension_RR" | "WheelSpin_RR" => 2,
            _ => continue,
        };
        if name.starts_with("Suspension_") {
            transform.translation.y = 0.78 - visual.travels[index];
        } else if name.starts_with("Steer_") {
            transform.rotation = Quat::from_rotation_y(visual.steer);
        } else {
            transform.rotation = Quat::from_rotation_x(visual.spins[index]);
            transform.scale =
                Vec3::splat(physics.active_config.tires.radius_m / MODEL_WHEEL_RADIUS);
        }
    }
}
