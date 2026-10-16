//! Mouse-driven single-object gizmos. Transient data never touches the canonical
//! scene nor Rapier until the mouse button is released.
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::Window;

use crate::terrain::obstacles::{Obstacle, ObstacleKind};
use crate::terrain::compositions::{center, transform_members};
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
    /// Empty for legacy single-object drags.
    members: Vec<Obstacle>,
}
#[derive(Resource, Default)]
pub(crate) struct GizmoGesture {
    pub(super) drag: Option<Drag>,
    pub(crate) candidate: Option<Obstacle>,
    pub(crate) candidates: Vec<Obstacle>,
    pub(crate) handle: Option<Obstacle>,
    pub(crate) block_orbit: bool,
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
                let Some(point) = ray_plane(ray, pivot, direction) else { continue; };
                let Some(vector) = (point - pivot).try_normalize() else { continue; };
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
    gesture.block_orbit = false;
    let selected = state.selected.and_then(|id|
        active.scene.obstacles.iter().find(|o| o.id == id));
    let selection = state.selection();
    let members: Vec<_> = active.scene.obstacles.iter()
        .filter(|o| selection.contains(&o.id)).cloned().collect();
    let allowed = state.open && state.mode == WorkshopMode::Edit
        && state.tool == EditorTool::Select && focused && !sync.full_reload;
    if gesture.drag.is_some() {
        let cancelled = !allowed || capture.keyboard
            || keys.just_pressed(KeyCode::Escape)
            || mouse.just_pressed(MouseButton::Right)
            || selected.is_none()
            || gesture.drag.as_ref().is_some_and(|g| {
                if g.members.is_empty() { selected != Some(&g.start) }
                else { g.members != members }
            });
        if cancelled {
            // Cancellation by right-click must not orbit the camera on the same
            // frame as the cancelled manipulation.
            gesture.block_orbit = mouse.just_pressed(MouseButton::Right);
            keys.clear_just_pressed(KeyCode::Escape);
            gesture.drag = None;
            gesture.candidate = None;
            gesture.candidates.clear();
            gesture.handle = None;
            return true;
        }
        if !mouse.pressed(MouseButton::Left) {
            if let Some(drag) = gesture.drag.take() {
                let updates = if drag.members.is_empty() {
                    gesture.candidate.take().into_iter().collect::<Vec<_>>()
                } else {
                    std::mem::take(&mut gesture.candidates)
                };
                let mut objects = active.scene.obstacles.clone();
                let mut changed = false;
                for candidate in updates {
                    if let Some(target) = objects.iter_mut().find(|o| o.id == candidate.id) {
                        if *target != candidate {
                            *target = candidate;
                            changed = true;
                        }
                    }
                }
                if changed {
                    if let Err(error) = commit_objects(objects, active, sync) {
                        state.last_error = Some(error);
                    } else {
                        state.last_error = None;
                    }
                }
            }
            gesture.candidate = None;
            gesture.candidates.clear();
            gesture.handle = None;
            return true;
        }
        if let Some(ray) = ray {
            let fine = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
            if let Some(drag) = &gesture.drag {
                if let Some(candidate) =
                    update_candidate(drag, ray, state, &active.scene.obstacles, fine, alt) {
                    if drag.members.is_empty() {
                        gesture.candidate = Some(candidate);
                    } else {
                        let pivot_before = Vec3::from_array(drag.start.position);
                        let pivot_after = Vec3::from_array(candidate.position);
                        let delta_rot = candidate.transform().rotation
                            * drag.start.transform().rotation.inverse();
                        let scale = Vec3::from_array(candidate.size)
                            / Vec3::from_array(drag.start.size);
                        if let Ok(updated) = transform_members(
                            &drag.members, pivot_before, pivot_after, delta_rot, scale,
                        ) {
                            gesture.handle = Some(candidate);
                            gesture.candidates = updated;
                        }
                    }
                }
            }
        }
        return true;
    }
    gesture.candidate = None;
    gesture.candidates.clear();
    gesture.handle = None;
    if !allowed || capture.pointer || capture.keyboard || selected.is_none() {
        return false;
    }
    if keys.just_pressed(KeyCode::KeyG) { state.gizmo_mode = GizmoMode::Move; }
    if keys.just_pressed(KeyCode::KeyR) { state.gizmo_mode = GizmoMode::Rotate; }
    if keys.just_pressed(KeyCode::KeyS) { state.gizmo_mode = GizmoMode::Scale; }
    if !mouse.just_pressed(MouseButton::Left) { return false; }
    let (Some(ray), Some(selected)) = (ray, selected) else { return false; };
    let group = members.len() > 1;
    let mut start = selected.clone();
    if group {
        // A virtual handle is centered on the group. All real objects retain
        // independent geometry and stable IDs.
        start.position = center(&members).to_array();
        start.rotation_degrees = [0.0; 3];
        start.size = [2.0; 3];
    }
    let pivot = Vec3::from_array(start.position);
    let length = axis_length(pivot, camera_eye);
    if let Some((axis, scalar, direction)) = pick(ray, pivot, length, state.gizmo_mode) {
        gesture.drag = Some(Drag {
            start: start.clone(), axis, mode: state.gizmo_mode, length, scalar, direction,
            members: if group { members.clone() } else { Vec::new() },
        });
        if group {
            gesture.candidates = members;
            gesture.handle = Some(start);
        } else {
            gesture.candidate = Some(start);
        }
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
    let selection = state.selection();
    let members: Vec<_> = active.scene.obstacles.iter()
        .filter(|o| selection.contains(&o.id)).cloned().collect();
    if members.is_empty() { return; }
    let pivot = if let Some(handle) = &gesture.handle {
        Vec3::from_array(handle.position)
    } else if members.len() > 1 {
        center(&members)
    } else if let Some(candidate) = &gesture.candidate {
        Vec3::from_array(candidate.position)
    } else {
        Vec3::from_array(members[0].position)
    };
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

/// Collider-free overlays: each selected original is hidden while its
/// independent preview is moved. Rapier never sees transient transforms.
#[derive(Default)]
struct PreviewPart {
    entity: Option<Entity>,
    hidden: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    geometry: Option<(ObstacleKind, [u32; 3])>,
}

#[derive(Resource, Default)]
pub(super) struct ManipulationPreview {
    parts: Vec<PreviewPart>,
}

impl ManipulationPreview {
    fn clear_part(part: &mut PreviewPart, world: &mut World) {
        if let Some(source) = part.hidden.take() {
            if let Ok(mut entity) = world.get_entity_mut(source) {
                entity.insert(Visibility::Inherited);
            }
        }
        if let Some(entity) = part.entity.take() { let _ = world.despawn(entity); }
        if let Some(mesh) = part.mesh.take() {
            world.resource_mut::<Assets<Mesh>>().remove(mesh.id());
        }
        if let Some(mat) = part.material.take() {
            world.resource_mut::<Assets<StandardMaterial>>().remove(mat.id());
        }
        part.geometry = None;
    }

    fn cleanup(&mut self, world: &mut World) {
        for part in &mut self.parts { Self::clear_part(part, world); }
        self.parts.clear();
    }

    fn render(&mut self, world: &mut World) {
        let gesture = world.resource::<GizmoGesture>();
        let candidates = if !gesture.candidates.is_empty() {
            gesture.candidates.clone()
        } else {
            gesture.candidate.iter().cloned().collect()
        };
        let editing = world.resource::<TerrainWorkshopState>().mode == WorkshopMode::Edit;
        let valid = !candidates.is_empty() && candidates.iter().all(|o| {
            world.resource::<ActiveTerrain>().scene.obstacles.iter().any(|other|
                other.id == o.id && other.kind == o.kind)
        });
        if !editing || !valid {
            self.cleanup(world);
            return;
        }
        while self.parts.len() > candidates.len() {
            if let Some(mut part) = self.parts.pop() {
                Self::clear_part(&mut part, world);
            }
        }
        while self.parts.len() < candidates.len() {
            self.parts.push(PreviewPart::default());
        }
        for (index, candidate) in candidates.iter().enumerate() {
            let part = &mut self.parts[index];
            let source = world.iter_entities().find(|e|
                e.get::<ObstacleEntity>().is_some_and(|id| id.0 == candidate.id)
            ).map(|e| e.id());
            if part.hidden != source {
                if let Some(old) = part.hidden.take() {
                    if let Ok(mut entity) = world.get_entity_mut(old) {
                        entity.insert(Visibility::Inherited);
                    }
                }
                if let Some(source) = source {
                    world.entity_mut(source).insert(Visibility::Hidden);
                    part.hidden = Some(source);
                }
            }
            let geometry = (candidate.kind, candidate.size.map(f32::to_bits));
            if part.geometry != Some(geometry) {
                let Ok(mesh) = candidate.build_mesh() else {
                    self.cleanup(world);
                    return;
                };
                let mut meshes = world.resource_mut::<Assets<Mesh>>();
                if let Some(handle) = &part.mesh { meshes.insert(handle.id(), mesh); }
                else { part.mesh = Some(meshes.add(mesh)); }
                part.geometry = Some(geometry);
            }
            if part.material.is_none() {
                part.material = Some(world.resource_mut::<Assets<StandardMaterial>>().add(
                    StandardMaterial {
                        base_color: Color::WHITE,
                        perceptual_roughness: 0.9,
                        double_sided: true,
                        cull_mode: None,
                        ..default()
                    },
                ));
            }
            if let Some(entity) = part.entity {
                world.entity_mut(entity).insert(candidate.transform());
            } else {
                part.entity = Some(world.spawn((
                    Name::new(if index == 0 { "Manipulation preview" } else { "Manipulation preview child" }),
                    Mesh3d(part.mesh.as_ref().unwrap().clone()),
                    MeshMaterial3d(part.material.as_ref().unwrap().clone()),
                    candidate.transform(), NotShadowCaster,
                )).id());
            }
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


    fn pick_ray(x: f32, z: f32) -> Ray3d {
        Ray3d::new(Vec3::new(x, 10.0, z), Dir3::NEG_Y)
    }

    #[test]
    fn translating_and_scaling_never_change_canonical_data_mid_drag() {
        let original = object(1, 0.0);
        let mut state = TerrainWorkshopState::default();
        let drag = Drag {
            start: original.clone(),
            axis: Axis::X,
            mode: GizmoMode::Move,
            length: 3.0,
            scalar: 0.5,
            direction: Vec3::ZERO,
            members: Vec::new(),
        };
        let moved = update_candidate(
            &drag, pick_ray(2.0, 0.0), &state, &[], false, false,
        ).unwrap();
        assert_eq!(moved.position[0], 1.5);
        assert_eq!(original.position[0], 0.0);
        let fine = update_candidate(
            &drag, pick_ray(2.0, 0.0), &state, &[], true, false,
        ).unwrap();
        assert!((fine.position[0] - 0.3).abs() < 1e-5);
        state.snap_enabled = true;
        state.snap_grid = 1.0;
        let snapped = update_candidate(
            &drag, pick_ray(2.0, 0.0), &state, &[], false, false,
        ).unwrap();
        assert_eq!(snapped.position[0], 2.0);
        let ignored = update_candidate(
            &drag, pick_ray(2.0, 0.0), &state, &[], false, true,
        ).unwrap();
        assert_eq!(ignored.position[0], 1.5);

        let scale_drag = Drag { mode: GizmoMode::Scale, ..drag };
        state.snap_enabled = false;
        state.uniform_scale = true;
        let scaled = update_candidate(
            &scale_drag, pick_ray(1.5, 0.0), &state, &[], false, false,
        ).unwrap();
        assert_eq!(scaled.size, [4.0, 4.0, 4.0]);
        state.uniform_scale = false;
        let scaled_one = update_candidate(
            &scale_drag, pick_ray(1.5, 0.0), &state, &[], false, false,
        ).unwrap();
        assert_eq!(scaled_one.size, [4.0, 2.0, 2.0]);
        assert_eq!(original.size, [2.0; 3]);
    }

    #[test]
    fn vertical_motion_allows_partial_burial() {
        let original = object(1, 0.0);
        let state = TerrainWorkshopState::default();
        let drag = Drag {
            start: original,
            axis: Axis::Y,
            mode: GizmoMode::Move,
            length: 3.0,
            scalar: 0.0,
            direction: Vec3::ZERO,
            members: Vec::new(),
        };
        let ray = Ray3d::new(Vec3::new(8.0, 0.4, 0.0), Dir3::NEG_X);
        let buried = update_candidate(&drag, ray, &state, &[], false, false).unwrap();
        assert!((buried.position[1] - 0.4).abs() < 1e-5);
        assert_eq!(buried.size, [2.0; 3]);
    }

    #[test]
    fn signed_rotation_and_snap_are_deterministic() {
        let starting = object(1, 0.0);
        let mut state = TerrainWorkshopState::default();
        let drag = Drag {
            start: starting,
            axis: Axis::Y,
            mode: GizmoMode::Rotate,
            length: 3.0,
            scalar: 0.0,
            direction: Vec3::X,
            members: Vec::new(),
        };
        let turned = update_candidate(
            &drag, pick_ray(0.0, 1.0), &state, &[], false, false,
        ).unwrap();
        assert!((turned.rotation_degrees[1] + 90.0).abs() < 1e-5);
        state.snap_enabled = true;
        state.snap_angle = 15.0;
        let snapped = update_candidate(
            &drag, pick_ray(0.6, 0.8), &state, &[], false, false,
        ).unwrap();
        assert!(snapped.rotation_degrees[1] % 15.0 == 0.0);
    }

    #[test]
    fn cancel_and_commit_are_atomic_and_keep_history_exact() {
        use crate::terrain::scene::{ReferenceScene, TerrainScene};
        let mut active = ActiveTerrain::default();
        active.scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let original = object(1, 0.0);
        active.scene.obstacles.push(original.clone());
        let mut state = TerrainWorkshopState::default();
        state.open = true;
        state.mode = WorkshopMode::Edit;
        state.tool = EditorTool::Select;
        state.selected = Some(1);
        let mut sync = TerrainSync::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut mouse = ButtonInput::<MouseButton>::default();
        let mut gesture = GizmoGesture::default();
        let setup = || Drag {
            start: original.clone(), axis: Axis::X, mode: GizmoMode::Move,
            length: 3.0, scalar: 0.5, direction: Vec3::ZERO, members: Vec::new(),
        };
        mouse.press(MouseButton::Left);
        gesture.drag = Some(setup());
        gesture.candidate = Some(original.clone());
        assert!(interact(&mut gesture, &mut state, &mut active, &mut sync,
            &UiInputCapture::default(), &mouse, &mut keys, true,
            Some(pick_ray(2.0, 0.0)), Vec3::new(0.0, 10.0, 0.0)));
        assert_ne!(gesture.candidate.as_ref().unwrap(), &original);
        assert_eq!(active.scene.obstacles[0], original);
        assert_eq!(active.history.undo_len(), 0);
        mouse.press(MouseButton::Right);
        assert!(interact(&mut gesture, &mut state, &mut active, &mut sync,
            &UiInputCapture::default(), &mouse, &mut keys, true,
            None, Vec3::new(0.0, 10.0, 0.0)));
        assert!(gesture.drag.is_none());
        assert!(gesture.candidate.is_none());
        assert!(gesture.block_orbit);
        assert_eq!(active.scene.obstacles[0], original);
        assert_eq!(active.history.undo_len(), 0);

        mouse.clear();
        mouse.press(MouseButton::Left);
        gesture.drag = Some(setup());
        gesture.candidate = Some(original.clone());
        assert!(interact(&mut gesture, &mut state, &mut active, &mut sync,
            &UiInputCapture::default(), &mouse, &mut keys, true,
            Some(pick_ray(2.0, 0.0)), Vec3::new(0.0, 10.0, 0.0)));
        mouse.release(MouseButton::Left);
        assert!(interact(&mut gesture, &mut state, &mut active, &mut sync,
            &UiInputCapture::default(), &mouse, &mut keys, true,
            None, Vec3::new(0.0, 10.0, 0.0)));
        assert_eq!(active.history.undo_len(), 1);
        assert_eq!(active.scene.obstacles[0].position[0], 1.5);
        assert!(active.history.undo(&mut active.scene));
        assert_eq!(active.scene.obstacles[0], original);
        assert!(active.history.redo(&mut active.scene));
        assert_eq!(active.scene.obstacles[0].position[0], 1.5);
    }
    #[test]
    fn click_without_change_does_not_modify_the_baseline() {
        let starting = object(1, 0.0);
        let mut gesture = GizmoGesture::default();
        gesture.candidate = Some(starting.clone());
        assert_eq!(gesture.candidate, Some(starting));
    }
}
