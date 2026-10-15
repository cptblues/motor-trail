use super::config::PhysicsModel;
use super::visuals::VehicleVisualState;
use super::wheel_settings::WheelContactSettings;
use super::{Truck, VehiclePhysics};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// A graphics-only child. Its parent and all colliders remain at the physical pose.
#[derive(Component)]
pub struct VehicleGraphic {
    owner: Entity,
    parent_local: Transform,
    chassis_local: Transform,
}

impl VehicleGraphic {
    pub fn new(owner: Entity, parent_local: Transform, local: Transform) -> Self {
        Self {
            owner,
            parent_local,
            chassis_local: parent_local.mul_transform(local),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct WheelPose {
    pub spins: [f32; 4],
    pub travels: [f32; 4],
    pub steer: f32,
}

impl From<&VehicleVisualState> for WheelPose {
    fn from(visual: &VehicleVisualState) -> Self {
        Self {
            spins: visual.spins,
            travels: visual.travels,
            steer: visual.steer,
        }
    }
}

impl WheelPose {
    fn interpolate(self, next: Self, alpha: f32) -> Self {
        Self {
            spins: std::array::from_fn(|i| {
                let delta = (next.spins[i] - self.spins[i] + PI).rem_euclid(TAU) - PI;
                (self.spins[i] + delta * alpha).rem_euclid(TAU)
            }),
            travels: std::array::from_fn(|i| self.travels[i].lerp(next.travels[i], alpha)),
            steer: self.steer.lerp(next.steer, alpha),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Sample {
    owner: Entity,
    pose: Transform,
    wheels: WheelPose,
    geometry: [f32; 3],
    model: PhysicsModel,
    experimental: bool,
}

impl Sample {
    fn new(
        owner: Entity,
        pose: Transform,
        visual: &VehicleVisualState,
        physics: &VehiclePhysics,
        settings: &WheelContactSettings,
    ) -> Self {
        Self {
            owner,
            pose,
            wheels: visual.into(),
            geometry: [
                physics.active_config.tires.radius_m,
                physics.active_config.front.rest_length_m,
                physics.active_config.rear.rest_length_m,
            ],
            model: physics.active_config.model,
            experimental: settings.experimental,
        }
    }
}

#[derive(Resource, Default)]
pub struct VehiclePresentation {
    start: Option<Sample>,
    previous: Option<Sample>,
    current: Option<Sample>,
    displayed: Option<Sample>,
}

impl VehiclePresentation {
    pub fn pose_for(&self, owner: Entity, physical: Transform) -> Transform {
        self.displayed
            .filter(|sample| sample.owner == owner)
            .map_or(physical, |sample| sample.pose)
    }

    pub(super) fn wheels(&self, fallback: &VehicleVisualState) -> WheelPose {
        self.displayed
            .map_or_else(|| fallback.into(), |sample| sample.wheels)
    }

    fn snap(&mut self, sample: Sample) {
        self.previous = Some(sample);
        self.current = Some(sample);
        self.displayed = Some(sample);
    }
}

pub fn begin_step(
    cars: Query<(Entity, &Transform), With<Truck>>,
    visual: Res<VehicleVisualState>,
    physics: Res<VehiclePhysics>,
    settings: Res<WheelContactSettings>,
    mut history: ResMut<VehiclePresentation>,
) {
    history.start = cars
        .single()
        .ok()
        .map(|(owner, pose)| Sample::new(owner, *pose, &visual, &physics, &settings));
}

pub fn finish_step(
    cars: Query<(Entity, &Transform), With<Truck>>,
    visual: Res<VehicleVisualState>,
    physics: Res<VehiclePhysics>,
    settings: Res<WheelContactSettings>,
    mut history: ResMut<VehiclePresentation>,
) {
    let Ok((owner, pose)) = cars.single() else {
        *history = VehiclePresentation::default();
        return;
    };
    let end = Sample::new(owner, *pose, &visual, &physics, &settings);
    // Respawns, mode/geometry changes and externally edited transforms must never
    // interpolate from a pose belonging to the previous simulation state.
    if history.start.is_some() && history.start == history.current {
        history.previous = history.start;
        history.current = Some(end);
    } else {
        history.snap(end);
    }
    history.start = None;
}

pub fn present(
    fixed: Res<Time<Fixed>>,
    cars: Query<(Entity, &Transform), With<Truck>>,
    visual: Res<VehicleVisualState>,
    physics: Res<VehiclePhysics>,
    settings: Res<WheelContactSettings>,
    mut history: ResMut<VehiclePresentation>,
    mut graphics: Query<(&VehicleGraphic, &mut Transform), Without<Truck>>,
) {
    let Ok((owner, physical)) = cars.single() else {
        *history = VehiclePresentation::default();
        return;
    };
    let actual = Sample::new(owner, *physical, &visual, &physics, &settings);
    if history.current != Some(actual) {
        history.snap(actual);
    }
    let previous = history.previous.unwrap_or(actual);
    let alpha = fixed.overstep_fraction().clamp(0.0, 1.0);
    let displayed = Sample {
        pose: if previous.pose == actual.pose {
            actual.pose
        } else {
            Transform {
                translation: previous
                    .pose
                    .translation
                    .lerp(actual.pose.translation, alpha),
                rotation: previous.pose.rotation.slerp(actual.pose.rotation, alpha),
                scale: previous.pose.scale.lerp(actual.pose.scale, alpha),
            }
        },
        wheels: previous.wheels.interpolate(actual.wheels, alpha),
        ..actual
    };
    history.displayed = Some(displayed);
    for (graphic, mut local) in &mut graphics {
        if graphic.owner != owner {
            continue;
        }
        let parent = GlobalTransform::from(*physical).mul_transform(graphic.parent_local);
        let rendered = GlobalTransform::from(displayed.pose).mul_transform(graphic.chassis_local);
        *local = rendered.reparented_to(&parent);
    }
}

#[cfg(test)]
mod tests;
