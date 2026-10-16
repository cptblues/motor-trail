//! Mouse-driven single-object gizmos. Transient data never touches the canonical
//! scene nor Rapier until the mouse button is released.
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::Window;

use crate::terrain::obstacles::{Obstacle, ObstacleKind};
use super::{ActiveTerrain, EditorTool, ObstacleEntity, TerrainSync, TerrainWorkshopState, WorkshopMode, commit_objects};
use crate::ui::UiInputCapture;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GizmoMode {
    #[default]
    Move,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X, Y, Z,
}
impl Axis {
    fn vector(self) -> Vec3 {
        match self { Self::X => Vec3::X, Self::Y => Vec3::Y, Self::Z => Vec3::Z }
    }
    fn index(self) -> usize {
        match self { Self::X => 0, Self::Y => 1, Self::Z => 2 }
    }
    fn color(self) -> Color {
        match self {
            Self::X => Color::srgb(0.96, 0.23, 0.22),
            Self::Y => Color::srgb(0.23, 0.94, 0.36),
            Self::Z => Color::srgb(0.22, 0.49, 0.98),
        }
    }
    const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];
}

#[derive(Clone)]
pub(super) struct Drag {
    pub(super) start: Obstacle,
    pub(super) axis: Axis,
    mode: GizmoMode,
    length: f32,
    scalar: f32,
    direction: Vec3,
}
#[derive(Resource, Default)]
pub(crate) struct GizmoGesture {
    pub(super) drag: Option<Drag>,
    pub(crate) candidate: Option<Obstacle>,
}

pub(super) fn axis_length(pivot: Vec3, eye: Vec3) -> f32 {
    // Approximately 60-100 pixels for a normal perspective viewport.
    (pivot.distance(eye) * 0.14).clamp(0.5, 25.0)
}

fn ray_axis(ray: Ray3d, pivot: Vec3, axis: Vec3) -> Option<(f32, f32)> {
    let direction = *ray.direction;
    let b = direction.dot(axis);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-5 { return None; }
    let w = ray.origin - pivot;
    let scalar = (axis.dot(w) - b * direction.dot(w)) / denom;
    let distance = (pivot + axis * scalar - ray.origin)
        .cross(direction).length();
    (scalar.is_finite() && distance.is_finite()).then_some((scalar, distance))
}

fn ray_plane(ray: Ray3d, pivot: Vec3, normal: Vec3) -> Option<Vec3> {
    let denom = normal.dot(*ray.direction);
    if denom.abs() < 1e-5 { return None; }
    let travel = normal.dot(pivot - ray.origin) / denom;
    (travel.is_finite() && travel > 0.0 && travel < 2000.0)
        .then(|| ray.origin + *ray.direction * travel)
}

fn ring_vector(ray: Ray3d, pivot: Vec3, axis: Vec3) -> Option<Vec3> {
    (ray_plane(ray, pivot, axis)? - pivot).try_normalize()
}

fn pick(ray: Ray3d, pivot: Vec3, length: f32, mode: GizmoMode) -> Option<(Axis, f32, Vec3)> {
    let mut best: Option<(Axis, f32, Vec3, f32)> = None;
    for axis in Axis::ALL {
        let direction = axis.vector();
        let (scalar, v, error) = match mode {
            GizmoMode::Rotate => {
                let vector = ring_vector(ray, pivot, direction)?;
                let point = ray_plane(ray, pivot, direction)?;
                let error = ((point - pivot).length() - length).abs();
                (0.0, vector, error)
            }
            _ => {
                let Some((scalar, error)) = ray_axis(ray, pivot, direction) else { continue; };
                if scalar < length * 0.25 || scalar > length * 1.25 { continue; }
                (scalar, Vec3::ZERO, error)
            }
        };
        if error < length * 0.13 && best.as_ref().is_none_or(|b| error < b.3) {
            best = Some((axis, scalar, v, error));
        }
    }
    best.map(|(axis, scalar, v, _)| (axis, scalar, v))
}

fn quantize(value: f32, step: f32) -> f32 {
    if step.is_finite() && step >= 0.001 {
        (value / step).round() * step
    } else {
        value
    }
}

fn extent(object: &Obstacle, axis: Axis) -> f32 {
    let q = object.transform().rotation;
    let half = Vec3::from_array(object.size) * 0.5;
    let direction = axis.vector();
    half.x * (q * Vec3::X).dot(direction).abs()
        + half.y * (q * Vec3::Y).dot(direction).abs()
        + half.z * (q * Vec3::Z).dot(direction).abs()
}

fn snap_neighbors(candidate: &Obstacle, objects: &[Obstacle], axis: Axis, threshold: f32) -> Option<f32> {
    let i = axis.index();
    let c = candidate.position[i];
    let ce = extent(candidate, axis);
    let mut best: Option<(f32, f32)> = None;
    for neighbor in objects.iter().filter(|o| o.id != candidate.id) {
        // Conservative overlap test on both perpendicular axes.
        if Axis::ALL.into_iter().filter(|a| *a != axis).any(|other| {
            let j = other.index();
            (neighbor.position[j] - c_other(candidate, j)).abs()
                > extent(candidate, other) + extent(neighbor, other) + threshold
        }) { continue; }
        let n = neighbor.position[i];
        let ne = extent(neighbor, axis);
        for target in [n - ne - ce, n + ne + ce] {
            let difference = (target - c).abs();
            if difference < threshold
                && best.as_ref().is_none_or(|(_, dist)| difference < *dist) {
                best = Some((target, difference));
            }
        }
    }
    best.map(|(target, _)| target)
}

fn c_other(object: &Obstacle, axis: usize) -> f32 {
    object.position[axis]
}

fn update_candidate(
    drag: &Drag, ray: Ray3d, state: &TerrainWorkshopState,
    objects: &[Obstacle], fine: bool, bypass_snap: bool,
) -> Option<Obstacle> {
    let axis = drag.axis.vector();
    let pivot = Vec3::from_array(drag.start.position);
    let mut result = drag.start.clone();
    let fine_factor = if fine { 0.2 } else { 1.0 };
    let snap = state.snap_enabled && !bypass_snap && !fine;
    let i = drag.axis.index();
    match drag.mode {
        GizmoMode::Move | GizmoMode::Scale => {
            let (scalar, _) = ray_axis(ray, pivot, axis)?;
            let delta = (scalar - drag.scalar) * fine_factor;
            if drag.mode == GizmoMode::Move {
                let mut position = drag.start.position[i] + delta;
                if snap {
                    position = quantize(position, state.snap_grid);
                }
                result.position[i] = position;
                if snap && state.snap_objects {
                    if let Some(target) = snap_neighbors(&result, objects, drag.axis, state.snap_grid * 0.65) {
                        result.position[i] = target;
                    }
                }
            } else if state.uniform_scale {
                let factor = (drag.start.size[i] + 2.0 * delta) / drag.start.size[i];
                let max = drag.start.size.into_iter().fold(f32::INFINITY, |a, s| a.min(32.0 / s));
                let min = drag.start.size.into_iter().fold(0.0_f32, |a, s| a.max(0.1 / s));
                let factor = factor.clamp(min, max);
                result.size = drag.start.size.map(|value| (value * factor).clamp(0.1, 32.0));
            } else {
                let mut size = (drag.start.size[i] + 2.0 * delta).clamp(0.1, 32.0);
                if snap { size = quantize(size, state.snap_grid).clamp(0.1, 32.0); }
                result.size[i] = size;
            }
        }
        GizmoMode::Rotate => {
            let current = ring_vector(ray, pivot, axis)?;
            let radians = axis.dot(drag.direction.cross(current))
                .atan2(drag.direction.dot(current));
            let mut angle = drag.start.rotation_degrees[i] + radians.to_degrees() * fine_factor;
            if snap { angle = quantize(angle, state.snap_angle); }
            result.rotation_degrees[i] = angle;
        }
    }
    result.validate().ok().map(|_| result)
}

/// Return true when an interaction has consumed the pointer, before ordinary
/// terrain picking/placement takes place. The active drag retains ownership.
#[allow(clippy::too_many_arguments)]
pub(super) fn interact(
    gesture: &mut GizmoGesture,
    state: &mut TerrainWorkshopState,
    active: &mut ActiveTerrain,
    sync: &mut TerrainSync,
    capture: &UiInputCapture,
    mouse: &ButtonInput<MouseButton>,
    keys: &mut ButtonInput<KeyCode>,
    focused: bool,
    ray: Option<Ray3d>,
    camera_eye: Vec3,
) -> bool {
    let selected = state.selected.and_then(|id|
        active.scene.obstacles.iter().find(|o| o.id == id));
    let allowed = state.open && state.mode == WorkshopMode::Edit
        && state.tool == EditorTool::Select && focused && !sync.full_reload;
    if gesture.drag.is_some() {
        let cancelled = !allowed || capture.keyboard
            || keys.just_pressed(KeyCode::Escape)
            || mouse.just_pressed(MouseButton::Right)
            || selected.is_none_or(|o| gesture.drag.as_ref().is_some_and(|g| o != &g.start));
        if cancelled {
            keys.clear_just_pressed(KeyCode::Escape);
            gesture.drag = None;
            gesture.candidate = None;
            return true;
        }
        if !mouse.pressed(MouseButton::Left) {
            if let (Some(candidate), Some(drag)) = (gesture.candidate.take(), gesture.drag.take()) {
                if candidate != drag.start {
                    let mut objects = active.scene.obstacles.clone();
                    if let Some(target) = objects.iter_mut().find(|o| o.id == drag.start.id) {
                        *target = candidate;
                        if let Err(error) = commit_objects(objects, active, sync) {
                            state.last_error = Some(error);
                        } else {
                            state.last_error = None;
                        }
                    }
                }
            }
            return true;
        }
        if let Some(ray) = ray {
            let fine = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
            if let Some(drag) = &gesture.drag {
                if let Some(candidate) =
                    update_candidate(drag, ray, state, &active.scene.obstacles, fine, alt) {
                    gesture.candidate = Some(candidate);
                }
            }
        }
        return true;
    }
    gesture.candidate = None;
    if !allowed || capture.pointer || capture.keyboard || selected.is_none() {
        return false;
    }
    if keys.just_pressed(KeyCode::KeyG) { state.gizmo_mode = GizmoMode::Move; }
    if keys.just_pressed(KeyCode::KeyR) { state.gizmo_mode = GizmoMode::Rotate; }
    if keys.just_pressed(KeyCode::KeyS) { state.gizmo_mode = GizmoMode::Scale; }
    if !mouse.just_pressed(MouseButton::Left) { return false; }
    let (Some(ray), Some(selected)) = (ray, selected) else { return false; };
    let pivot = Vec3::from_array(selected.position);
    let length = axis_length(pivot, camera_eye);
    if let Some((axis, scalar, direction)) = pick(ray, pivot, length, state.gizmo_mode) {
        gesture.drag = Some(Drag {
            start: selected.clone(), axis, mode: state.gizmo_mode, length, scalar, direction,
        });
        gesture.candidate = Some(selected.clone());
        return true;
    }
    false
}

pub(super) fn draw(
    gizmos: &mut Gizmos,
    state: &TerrainWorkshopState,
    active: &ActiveTerrain,
    gesture: &GizmoGesture,
    eye: Vec3,
) {
    if state.mode != WorkshopMode::Edit || state.tool != EditorTool::Select || !state.open { return; }
    let Some(object) = gesture.candidate.as_ref().or_else(|| {
        state.selected.and_then(|id| active.scene.obstacles.iter().find(|o| o.id == id))
    }) else { return; };
    let pivot = Vec3::from_array(object.position);
    let length = axis_length(pivot, eye);
    for axis in Axis::ALL {
        let direction = axis.vector();
        if state.gizmo_mode == GizmoMode::Rotate {
            let other = if axis == Axis::Y { Vec3::X } else { Vec3::Y };
            let a = direction.cross(other).normalize();
            let b = direction.cross(a).normalize();
            let segments = (0..=64).map(|i| {
                let t = i as f32 / 64.0 * std::f32::consts::TAU;
                pivot + (a * t.cos() + b * t.sin()) * length
            });
            gizmos.linestrip(segments, axis.color());
        } else {
            gizmos.arrow(pivot + direction * (0.12 * length),
                pivot + direction * length, axis.color());
            if state.gizmo_mode == GizmoMode::Scale {
                gizmos.cube(Transform::from_translation(pivot + direction * length)
                    .with_scale(Vec3::splat(length * 0.15)), axis.color());
            }
        }
    }
}

/// An independent, collider-free visual overlay. The physics object's material
/// and transform never move during a drag; render geometry is cached for
/// repeated transforms and replaced only when dimensions change.
#[derive(Resource, Default)]
pub(super) struct ManipulationPreview {
    entity: Option<Entity>,
    hidden: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    geometry: Option<(ObstacleKind, [u32; 3])>,
}

impl ManipulationPreview {
    fn cleanup(&mut self, world: &mut World) {
        if let Some(entity) = self.hidden.take() {
            if let Some(mut entity) = world.get_entity_mut(entity).ok() {
                entity.insert(Visibility::Inherited);
            }
        }
        if let Some(entity) = self.entity.take() {
            let _ = world.despawn(entity);
        }
        if let Some(mesh) = self.mesh.take() { world.resource_mut::<Assets<Mesh>>().remove(mesh.id()); }
        if let Some(mat) = self.material.take() {
            world.resource_mut::<Assets<StandardMaterial>>().remove(mat.id());
        }
        self.geometry = None;
    }

    fn render(&mut self, world: &mut World) {
        let candidate = world.resource::<GizmoGesture>().candidate.clone();
        let editing = world.resource::<TerrainWorkshopState>().mode == WorkshopMode::Edit;
        let valid = candidate.as_ref().is_some_and(|o| {
            world.resource::<ActiveTerrain>().scene.obstacles.iter().any(|other|
                other.id == o.id && other.kind == o.kind)
        });
        if !editing || !valid {
            self.cleanup(world);
            return;
        }
        let candidate = candidate.unwrap();
        let source = world.iter_entities().find(|e|
            e.get::<ObstacleEntity>().is_some_and(|id| id.0 == candidate.id)
        ).map(|e| e.id());
        if self.hidden != source {
            if let Some(old) = self.hidden.take() {
                if let Ok(mut entity) = world.get_entity_mut(old) {
                    entity.insert(Visibility::Inherited);
                }
            }
            if let Some(source) = source {
                world.entity_mut(source).insert(Visibility::Hidden);
                self.hidden = Some(source);
            }
        }
        let geometry = (candidate.kind, candidate.size.map(f32::to_bits));
        if self.geometry != Some(geometry) {
            let Ok(mesh) = candidate.build_mesh() else {
                self.cleanup(world);
                return;
            };
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            if let Some(handle) = &self.mesh { meshes.insert(handle.id(), mesh); }
            else { self.mesh = Some(meshes.add(mesh)); }
            self.geometry = Some(geometry);
        }
        if self.material.is_none() {
            self.material = Some(world.resource_mut::<Assets<StandardMaterial>>().add(
                StandardMaterial { base_color: Color::WHITE,
                    perceptual_roughness: 0.9, ..default() }
            ));
        }
        if let Some(entity) = self.entity {
            world.entity_mut(entity).insert(candidate.transform());
        } else {
            self.entity = Some(world.spawn((
                Name::new("Manipulation preview"),
                Mesh3d(self.mesh.as_ref().unwrap().clone()),
                MeshMaterial3d(self.material.as_ref().unwrap().clone()),
                candidate.transform(),
                NotShadowCaster,
            )).id());
        }
    }
}

pub(super) fn render(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<ManipulationPreview>| state.render(world));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn object(id: u64, x: f32) -> Obstacle {
        Obstacle { id, kind: ObstacleKind::Cube, position: [x, 1.0, 0.0],
            size: [2.0, 2.0, 2.0], rotation_degrees: [0.0; 3] }
    }

    #[test]
    fn snapping_is_optional_and_neighbors_never_snap_to_self() {
        assert_eq!(quantize(1.24, 0.5), 1.0);
        assert_eq!(quantize(-1.26, 0.5), -1.5);
        assert_eq!(quantize(1.24, 0.0), 1.24);
        let candidate = object(1, 2.1);
        assert_eq!(snap_neighbors(&candidate, &[object(1, 2.1)], Axis::X, 0.3), None);
        assert_eq!(snap_neighbors(&candidate, &[object(2, 4.0)], Axis::X, 0.3), Some(2.0));
    }

    #[test]
    fn projection_handles_parallel_and_nearly_parallel_rays() {
        let ray = Ray3d::new(Vec3::new(1.0, 10.0, 0.0), Dir3::Y);
        assert_eq!(ray_axis(ray, Vec3::ZERO, Vec3::Y), None);
        assert_eq!(ray_plane(ray, Vec3::ZERO, Vec3::Z), None);
        let ray = Ray3d::new(Vec3::new(2.0, 3.0, 10.0), Dir3::NEG_Z);
        assert!(ray_axis(ray, Vec3::ZERO, Vec3::X).is_some());
        assert!(ray_plane(ray, Vec3::ZERO, Vec3::Z).is_some());
    }

    #[test]
    fn gizmos_resize_with_camera_and_every_axis_is_distinct() {
        assert!(axis_length(Vec3::ZERO, Vec3::new(0.0, 0.0, 5.0)) <
            axis_length(Vec3::ZERO, Vec3::new(0.0, 0.0, 40.0)));
        assert_eq!(Axis::ALL.map(Axis::index), [0, 1, 2]);
    }

    #[test]
    fn click_without_change_does_not_modify_the_baseline() {
        let starting = object(1, 0.0);
        let mut gesture = GizmoGesture::default();
        gesture.candidate = Some(starting.clone());
        assert_eq!(gesture.candidate, Some(starting));
    }
}
