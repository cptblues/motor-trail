//! Optional body, equipment and wheel overlay; never changes physics or profiles.
use super::controller::{MOUNTS, Truck, VehiclePhysics};
use crate::ui::UiInputCapture;
use bevy::gizmos::config::GizmoConfigGroup;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
mod probe;

#[derive(Resource, Default)]
pub struct WheelDiagnostics {
    pub enabled: bool,
}

pub struct WheelDiagnosticsPlugin;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct WheelGizmos;

#[derive(Resource, Default)]
struct EquipmentOverlay {
    revision: Option<u64>,
    colliders: Vec<(Transform, Collider)>,
    mounts: Vec<Transform>,
}

impl Plugin for WheelDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WheelDiagnostics {
            enabled: std::env::args_os().any(|arg| arg == "--wheel-diagnostics"),
        })
        .init_resource::<EquipmentOverlay>()
        .init_gizmo_group::<WheelGizmos>()
        .add_plugins(probe::WheelContactProbePlugin)
        .add_plugins(RapierDebugRenderPlugin {
            enabled: false,
            default_collider_debug: ColliderDebug::NeverRender,
            mode: DebugRenderMode::COLLIDER_SHAPES,
            ..default()
        })
        .add_systems(
            Update,
            (toggle, configure_colliders, refresh_equipment)
                .chain()
                .before(crate::controls),
        )
        .add_systems(
            PostUpdate,
            draw.after(bevy::transform::TransformSystems::Propagate),
        );
        let mut store = app.world_mut().resource_mut::<GizmoConfigStore>();
        let (config, _) = store.config_mut::<WheelGizmos>();
        config.depth_bias = -1.0;
        config.line.width = 1.5;
    }
}

fn refresh_equipment(
    state: Res<WheelDiagnostics>,
    runtime: Res<super::equipment_runtime::EquipmentRuntime>,
    mut overlay: ResMut<EquipmentOverlay>,
) {
    if !state.enabled || overlay.revision == Some(runtime.revision) {
        return;
    }
    let active = runtime.active();
    let Ok(installed) =
        super::equipment::resolve_loadout(&active.mounts, &active.catalog, &active.loadout)
    else {
        return;
    };
    overlay.colliders.clear();
    overlay.mounts.clear();
    for item in installed {
        if item.mode != super::equipment::EquipmentMode::Rigid {
            continue;
        }
        for part in item.colliders {
            overlay.colliders.push((
                item.local_transform.mul_transform(part.local_transform),
                part.collider,
            ));
        }
    }
    for mount in &active.mounts.mounts {
        if let Ok(pose) = mount.local_pose.transform() {
            overlay.mounts.push(pose);
        }
    }
    overlay.revision = Some(runtime.revision);
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<UiInputCapture>,
    mut state: ResMut<WheelDiagnostics>,
) {
    if !capture.keyboard && keys.just_pressed(KeyCode::KeyV) {
        state.enabled = !state.enabled;
    }
}

fn configure_colliders(
    mut commands: Commands,
    state: Res<WheelDiagnostics>,
    mut render: ResMut<DebugRenderContext>,
    obstacles: Query<
        (Entity, Option<&ColliderDebug>),
        With<crate::terrain::runtime::ObstacleEntity>,
    >,
) {
    render.enabled = state.enabled;
    for (entity, debug) in &obstacles {
        if state.enabled && debug != Some(&ColliderDebug::AlwaysRender) {
            commands.entity(entity).insert(ColliderDebug::AlwaysRender);
        } else if !state.enabled && debug.is_some() {
            commands.entity(entity).remove::<ColliderDebug>();
        }
    }
}

fn draw(
    state: Res<WheelDiagnostics>,
    physics: Res<VehiclePhysics>,
    equipment: Res<EquipmentOverlay>,
    cars: Query<(Entity, &Transform, Option<&ReadMassProperties>), With<Truck>>,
    context: ReadRapierContext,
    mut gizmos: Gizmos<WheelGizmos>,
) {
    if !state.enabled {
        return;
    }
    let Ok((entity, car, mass)) = cars.single() else {
        return;
    };
    for part in super::collision::body_parts() {
        let color = match part.region {
            super::collision::BodyRegion::Chassis => Color::srgb(1.0, 0.22, 0.18),
            super::collision::BodyRegion::Body => Color::srgb(0.25, 0.55, 1.0),
            super::collision::BodyRegion::Roof => Color::srgb(1.0, 0.90, 0.12),
            super::collision::BodyRegion::Bumper => Color::srgb(1.0, 0.50, 0.10),
            super::collision::BodyRegion::Accessory => Color::srgb(0.9, 0.4, 1.0),
        };
        wire_shape(
            &mut gizmos,
            part.collider.raw.as_ref(),
            car.mul_transform(part.local_transform),
            color,
        );
    }
    for (local, collider) in &equipment.colliders {
        wire_shape(
            &mut gizmos,
            collider.raw.as_ref(),
            car.mul_transform(*local),
            Color::srgb(1.0, 0.0, 0.85),
        );
    }
    for local in &equipment.mounts {
        let pose = car.mul_transform(*local);
        for (axis, color) in [
            (Vec3::X, Color::srgb(1.0, 0.2, 0.2)),
            (Vec3::Y, Color::srgb(0.2, 1.0, 0.3)),
            (Vec3::Z, Color::srgb(0.2, 0.5, 1.0)),
        ] {
            gizmos.arrow(pose.translation, pose.transform_point(axis * 0.28), color);
        }
    }
    let local_com = mass
        .map(|mass| mass.local_center_of_mass)
        .unwrap_or(Vec3::from_array(
            physics.active_config.chassis.center_of_mass,
        ));
    gizmos.sphere(
        Isometry3d::from_translation(car.transform_point(local_com)),
        0.09,
        Color::srgb(1.0, 0.2, 0.75),
    );
    let radius = physics.active_config.tires.radius_m;
    let half_width =
        super::contacts::MODEL_TIRE_HALF_WIDTH * radius / super::controller::MODEL_WHEEL_RADIUS;
    for (index, contact) in physics.wheels.iter().enumerate() {
        let axle = if index < 2 {
            &physics.active_config.front
        } else {
            &physics.active_config.rear
        };
        let geometry = wheel_geometry(
            car,
            index,
            contact.suspension_length,
            axle.rest_length_m,
            radius,
            contact.steer,
        );
        let rotation = Quat::from_rotation_arc(Vec3::Z, geometry.axle);
        gizmos.cross(geometry.center, 0.065, Color::WHITE);
        for side in [-1.0, 0.0, 1.0] {
            gizmos.circle(
                Isometry3d::new(
                    geometry.center + geometry.axle * half_width * side,
                    rotation,
                ),
                radius,
                Color::srgb(0.05, 0.85, 1.0),
            );
        }
        gizmos.arrow(
            geometry.ray_start,
            geometry.ray_end,
            Color::srgb(1.0, 0.15, 0.15),
        );
        if contact.grounded {
            gizmos.cross(contact.point, 0.08, Color::srgb(1.0, 0.65, 0.03));
            gizmos.arrow(
                contact.point,
                contact.point + contact.normal * 0.35,
                Color::srgb(0.2, 1.0, 0.3),
            );
            gizmos.line(geometry.center, contact.point, Color::srgb(1.0, 0.65, 0.03));
        }
        if let Ok(rapier) = context.single() {
            let envelope = rapier.with_query_pipeline(
                QueryFilter::default()
                    .exclude_rigid_body(entity)
                    .exclude_sensors(),
                |pipeline| {
                    super::contacts::cast_envelope(
                        &pipeline.query_pipeline,
                        super::contacts::WheelCast {
                            mount: geometry.ray_start,
                            down: car.rotation * Vec3::NEG_Y,
                            axle: geometry.axle,
                            radius,
                            half_width,
                            rest_length: axle.rest_length_m,
                        },
                    )
                },
            );
            if let Some(candidate) = envelope.filter(|candidate| {
                rapier
                    .colliders
                    .collider_entity(candidate.collider)
                    .is_some()
            }) {
                gizmos.cross(candidate.point, 0.11, Color::srgb(0.5, 1.0, 0.1));
                gizmos.arrow(
                    candidate.point,
                    candidate.point + candidate.normal * 0.22,
                    Color::srgb(0.5, 1.0, 0.1),
                );
                let center = geometry.ray_start + car.rotation * Vec3::NEG_Y * candidate.length;
                gizmos.cross(center, 0.045, Color::srgb(0.5, 1.0, 0.1));
            }
        }
    }
}

fn wire_shape(
    gizmos: &mut Gizmos<WheelGizmos>,
    shape: &dyn bevy_rapier3d::parry::shape::Shape,
    pose: Transform,
    color: Color,
) {
    use bevy_rapier3d::parry::shape::TypedShape;
    match shape.as_typed_shape() {
        TypedShape::Cuboid(cuboid) => {
            gizmos.cube(pose.with_scale(cuboid.half_extents * 2.0), color);
        }
        TypedShape::ConvexPolyhedron(polyhedron) => {
            for edge in polyhedron.edges() {
                let [a, b] = edge.vertices;
                gizmos.line(
                    pose.transform_point(polyhedron.points()[a as usize]),
                    pose.transform_point(polyhedron.points()[b as usize]),
                    color,
                );
            }
        }
        TypedShape::Cylinder(cylinder) => {
            let rotation = pose.rotation * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
            for side in [-1.0, 1.0] {
                gizmos.circle(
                    Isometry3d::new(
                        pose.transform_point(Vec3::Y * cylinder.half_height * side),
                        rotation,
                    ),
                    cylinder.radius,
                    color,
                );
            }
            for radial in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                gizmos.line(
                    pose.transform_point(radial * cylinder.radius + Vec3::Y * cylinder.half_height),
                    pose.transform_point(radial * cylinder.radius - Vec3::Y * cylinder.half_height),
                    color,
                );
            }
        }
        TypedShape::Ball(ball) => {
            gizmos.sphere(
                Isometry3d::new(pose.translation, pose.rotation),
                ball.radius,
                color,
            );
        }
        TypedShape::Compound(compound) => {
            for (local, child) in compound.shapes() {
                let local =
                    Transform::from_translation(local.translation).with_rotation(local.rotation);
                wire_shape(gizmos, child.as_ref(), pose.mul_transform(local), color);
            }
        }
        _ => {
            let aabb = shape.compute_local_aabb();
            let local = Transform::from_translation((aabb.mins + aabb.maxs) * 0.5);
            gizmos.cube(
                pose.mul_transform(local).with_scale(aabb.maxs - aabb.mins),
                color,
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WheelGeometry {
    center: Vec3,
    axle: Vec3,
    ray_start: Vec3,
    ray_end: Vec3,
}

fn wheel_geometry(
    car: &Transform,
    index: usize,
    length: f32,
    rest: f32,
    radius: f32,
    steer: f32,
) -> WheelGeometry {
    let ray_start = car.transform_point(MOUNTS[index]);
    let down = car.rotation * Vec3::NEG_Y;
    WheelGeometry {
        center: ray_start + down * length,
        axle: car.rotation * Quat::from_rotation_y(steer) * Vec3::NEG_X,
        ray_start,
        ray_end: ray_start + down * (rest + radius),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    const GLB: &[u8] = include_bytes!("../../assets/vehicles/motor_trail_4x4.glb");

    fn model() -> Value {
        assert_eq!(&GLB[..4], b"glTF");
        let length = u32::from_le_bytes(GLB[12..16].try_into().unwrap()) as usize;
        serde_json::from_slice(&GLB[20..20 + length]).unwrap()
    }

    #[test]
    fn measured_glb_rubber_radius_matches_the_physical_reference() {
        let glb = model();
        let node = glb["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == "WheelSpin_FL")
            .unwrap();
        let mesh = &glb["meshes"][node["mesh"].as_u64().unwrap() as usize];
        let mut rubber = None;
        let mut tread = None;
        for primitive in mesh["primitives"].as_array().unwrap() {
            let name = glb["materials"][primitive["material"].as_u64().unwrap() as usize]["name"]
                .as_str()
                .unwrap();
            let accessor =
                &glb["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
            let min = accessor["min"].as_array().unwrap();
            let max = accessor["max"].as_array().unwrap();
            let radius = min[1]
                .as_f64()
                .unwrap()
                .abs()
                .max(max[1].as_f64().unwrap().abs()) as f32;
            if name == "Tire rubber" {
                rubber = Some(radius);
                assert!(
                    ((max[0].as_f64().unwrap() - min[0].as_f64().unwrap()) - 0.342).abs() < 1e-6
                );
            } else if name == "Tread" {
                tread = Some(radius);
            }
        }
        let radius = rubber.unwrap();
        assert!((radius - super::super::controller::MODEL_WHEEL_RADIUS).abs() < 1e-6);
        assert!((0.02..0.03).contains(&(tread.unwrap() - radius)));
    }

    #[test]
    fn diagnostic_centers_match_glb_pivots_with_the_live_suspension_travel() {
        let glb = model();
        let model_rotation = Quat::from_rotation_y(std::f32::consts::PI);
        let car = Transform::from_xyz(2.0, 4.0, 9.0).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.6,
            0.1,
            -0.15,
        ));
        for (label, index) in [("FL", 1), ("FR", 0), ("RL", 3), ("RR", 2)] {
            let name = format!("Suspension_{label}");
            let node = glb["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["name"] == name)
                .unwrap();
            let t = node["translation"].as_array().unwrap();
            for travel in [0.0, 0.13, 0.36] {
                let visual = Vec3::new(0.0, -1.0, 0.0)
                    + model_rotation
                        * Vec3::new(
                            t[0].as_f64().unwrap() as f32,
                            0.78 - travel,
                            t[2].as_f64().unwrap() as f32,
                        );
                let expected = car.transform_point(visual);
                let geometry = wheel_geometry(&car, index, travel, 0.36, 0.53, 0.2);
                assert!(
                    geometry.center.distance(expected) < 1e-5,
                    "{label}: {:?} != {expected:?}",
                    geometry.center
                );
                assert!((geometry.axle.length() - 1.0).abs() < 1e-5);
                assert!((geometry.ray_start.distance(geometry.ray_end) - 0.89).abs() < 1e-5);
            }
        }
    }
}
