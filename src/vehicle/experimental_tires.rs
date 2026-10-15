//! Experimental opt-in adaptation of rapier3d 0.33.0-alpha's
//! src/control/ray_cast_vehicle_controller.rs, originally ported from Bullet's
//! btRaycastVehicle. Copyright 2020 Sebastien Crozet. Apache-2.0; the complete
//! upstream license is in third_party/rapier-LICENSE.
//!
//! Modified for motor-trail: private Wheel/contact fields prevent injecting a
//! chosen contact into upstream Rapier. This isolated adaptation keeps its 3D
//! transforms and suspension/friction equations, selecting contacts before the
//! velocity/projection calculation. It uses the facade's local wheel/contact
//! snapshots, public get_mut/mass_properties/shape accessors and the exact private
//! reciprocal deadband. Buffers grow only when adding wheels. Upstream serde,
//! profiling, dim2/dim3 annotations and duplicate public API/types are omitted.
//! Maintain this against the pinned source if Rapier is upgraded. Native mode
//! always calls the actual upstream controller through wheel_backend.rs.
use bevy_rapier3d::rapier::{
    dynamics::{RigidBody, RigidBodyHandle, RigidBodySet},
    geometry::{ColliderSet, Ray},
    math::{Real, Vector, VectorExt, rotation_from_angle},
    pipeline::{QueryPipeline, QueryPipelineMut},
    utils::{CrossProduct, DotProduct},
};

use super::contacts::{MODEL_TIRE_HALF_WIDTH, WheelCast, cast_envelope};
use super::wheel_backend::Wheel;

#[derive(Clone, Debug)]
pub(super) struct ExperimentalTires {
    pub(super) wheels: Vec<Wheel>,
    forward_ws: Vec<Vector>,
    axle: Vec<Vector>,
    pub(super) current_vehicle_speed: Real,
    pub(super) chassis: RigidBodyHandle,
    pub(super) index_up_axis: usize,
    pub(super) index_forward_axis: usize,
}

impl ExperimentalTires {
    pub(super) fn new(chassis: RigidBodyHandle) -> Self {
        Self {
            wheels: Vec::with_capacity(4),
            forward_ws: Vec::with_capacity(4),
            axle: Vec::with_capacity(4),
            current_vehicle_speed: 0.0,
            chassis,
            index_up_axis: 1,
            index_forward_axis: 0,
        }
    }

    pub(super) fn add_wheel(&mut self, wheel: Wheel) {
        self.wheels.push(wheel);
        self.forward_ws.push(Vector::ZERO);
        self.axle.push(Vector::ZERO);
    }

    fn update_wheel_transform(&mut self, chassis: &RigidBody, wheel_index: usize) {
        let wheel = &mut self.wheels[wheel_index];
        wheel.raycast_info.is_in_contact = false;
        let chassis_transform = chassis.position();
        wheel.raycast_info.hard_point_ws = chassis_transform * wheel.chassis_connection_point_cs;
        wheel.wheel_direction_ws = chassis_transform.rotation * wheel.direction_cs;
        wheel.wheel_axle_ws = chassis_transform.rotation * wheel.axle_cs;

        let steering_orn = rotation_from_angle(-wheel.wheel_direction_ws * wheel.steering);
        wheel.wheel_axle_ws = steering_orn * (chassis.position().rotation * wheel.axle_cs);
        wheel.center = wheel.raycast_info.hard_point_ws
            + wheel.wheel_direction_ws * wheel.raycast_info.suspension_length;
    }

    fn ray_cast(&mut self, queries: &QueryPipeline<'_>, chassis: &RigidBody, wheel_id: usize) {
        let wheel = &mut self.wheels[wheel_id];
        let raylen = wheel.suspension_rest_length + wheel.radius;
        let rayvector = wheel.wheel_direction_ws * raylen;
        let source = wheel.raycast_info.hard_point_ws;
        wheel.raycast_info.contact_point_ws = source + rayvector;
        let ray = Ray::new(source, rayvector);
        let hit = queries.cast_ray_and_get_normal(&ray, 1.0, true);
        wheel.raycast_info.ground_object = None;

        let mut raw_length = None;
        if let Some((collider_hit, mut hit)) = hit {
            if hit.time_of_impact == 0.0 {
                let collider = &queries.colliders[collider_hit];
                let up_ray = Ray::new(source + rayvector, -rayvector);
                if let Some(hit2) = collider.shape().cast_ray_and_get_normal(
                    collider.position(),
                    &up_ray,
                    1.0,
                    false,
                ) {
                    hit.normal = -hit2.normal;
                }
                if hit.normal == Vector::ZERO {
                    hit.normal = -wheel.wheel_direction_ws;
                }
            }
            wheel.raycast_info.contact_normal_ws = hit.normal;
            wheel.raycast_info.is_in_contact = true;
            wheel.raycast_info.ground_object = Some(collider_hit);
            let hit_distance = hit.time_of_impact * raylen;
            raw_length = Some(hit_distance - wheel.radius);
            wheel.raycast_info.contact_point_ws = ray.point_at(hit.time_of_impact);
        }

        if let Some(envelope) = cast_envelope(
            queries,
            WheelCast {
                mount: source,
                down: wheel.wheel_direction_ws,
                axle: wheel.wheel_axle_ws,
                radius: wheel.radius,
                half_width: MODEL_TIRE_HALF_WIDTH * (wheel.radius / 0.53),
                rest_length: wheel.suspension_rest_length,
            },
        ) {
            // Compare raw requests: clamping first loses both negative-travel
            // ordering and equivalence on bottomed-out flat ground.
            if raw_length.is_none_or(|native| envelope.length < native - 0.001) {
                raw_length = Some(envelope.length);
                wheel.raycast_info.contact_normal_ws = envelope.normal;
                wheel.raycast_info.contact_point_ws = envelope.point;
                wheel.raycast_info.is_in_contact = true;
                wheel.raycast_info.ground_object = Some(envelope.collider);
            }
        }

        if let Some(length) = raw_length {
            let min_suspension_length = wheel.suspension_rest_length - wheel.max_suspension_travel;
            let max_suspension_length = wheel.suspension_rest_length + wheel.max_suspension_travel;
            wheel.raycast_info.suspension_length =
                length.clamp(min_suspension_length, max_suspension_length);
            let denominator = wheel
                .raycast_info
                .contact_normal_ws
                .dot(wheel.wheel_direction_ws);
            let chassis_velocity_at_contact_point =
                chassis.velocity_at_point(wheel.raycast_info.contact_point_ws);
            let proj_vel = wheel
                .raycast_info
                .contact_normal_ws
                .dot(chassis_velocity_at_contact_point);
            if denominator >= -0.1 {
                wheel.suspension_relative_velocity = 0.0;
                wheel.clipped_inv_contact_dot_suspension = 1.0 / 0.1;
            } else {
                let inv = -1.0 / denominator;
                wheel.suspension_relative_velocity = proj_vel * inv;
                wheel.clipped_inv_contact_dot_suspension = inv;
            }
        } else {
            wheel.raycast_info.suspension_length = wheel.suspension_rest_length;
            wheel.suspension_relative_velocity = 0.0;
            wheel.raycast_info.contact_normal_ws = -wheel.wheel_direction_ws;
            wheel.clipped_inv_contact_dot_suspension = 1.0;
        }
    }

    pub(super) fn update_vehicle(&mut self, dt: Real, queries: QueryPipelineMut<'_>) {
        let num_wheels = self.wheels.len();
        let chassis = &queries.bodies[self.chassis];
        for i in 0..num_wheels {
            self.update_wheel_transform(chassis, i);
        }
        self.current_vehicle_speed = chassis.linvel().length();
        let forward_w = chassis.position().rotation * Vector::ith(self.index_forward_axis, 1.0);
        if forward_w.dot(chassis.linvel()) < 0.0 {
            self.current_vehicle_speed *= -1.0;
        }
        for wheel_id in 0..self.wheels.len() {
            self.ray_cast(&queries.as_ref(), chassis, wheel_id);
        }
        let chassis_mass = chassis.mass();
        self.update_suspension(chassis_mass);
        let chassis = queries.bodies.get_mut(self.chassis).unwrap();
        for wheel in &mut self.wheels {
            if wheel.engine_force > 0.0 {
                chassis.wake_up(true);
            }
            let mut suspension_force = wheel.wheel_suspension_force;
            if suspension_force > wheel.max_suspension_force {
                suspension_force = wheel.max_suspension_force;
            }
            let impulse = wheel.raycast_info.contact_normal_ws * suspension_force * dt;
            chassis.apply_impulse_at_point(impulse, wheel.raycast_info.contact_point_ws, false);
        }
        self.update_friction(queries.bodies, queries.colliders, dt);
        let chassis = queries.bodies.get_mut(self.chassis).unwrap();
        for wheel in &mut self.wheels {
            let vel = chassis.velocity_at_point(wheel.raycast_info.hard_point_ws);
            if wheel.raycast_info.is_in_contact {
                let mut fwd =
                    chassis.position().rotation * Vector::ith(self.index_forward_axis, 1.0);
                let proj = fwd.dot(wheel.raycast_info.contact_normal_ws);
                fwd -= wheel.raycast_info.contact_normal_ws * proj;
                let proj2 = fwd.dot(vel);
                wheel.delta_rotation = (proj2 * dt) / (wheel.radius);
                wheel.rotation += wheel.delta_rotation;
            } else {
                wheel.rotation += wheel.delta_rotation;
            }
            wheel.delta_rotation *= 0.99;
        }
    }

    fn update_suspension(&mut self, chassis_mass: Real) {
        for w_it in 0..self.wheels.len() {
            let wheels = &mut self.wheels[w_it];
            if wheels.raycast_info.is_in_contact {
                let mut force;
                {
                    let rest_length = wheels.suspension_rest_length;
                    let current_length = wheels.raycast_info.suspension_length;
                    let length_diff = rest_length - current_length;
                    force = wheels.suspension_stiffness
                        * length_diff
                        * wheels.clipped_inv_contact_dot_suspension;
                }
                {
                    let projected_rel_vel = wheels.suspension_relative_velocity;
                    {
                        let susp_damping = if projected_rel_vel < 0.0 {
                            wheels.damping_compression
                        } else {
                            wheels.damping_relaxation
                        };
                        force -= susp_damping * projected_rel_vel;
                    }
                }
                wheels.wheel_suspension_force = (force * chassis_mass).max(0.0);
            } else {
                wheels.wheel_suspension_force = 0.0;
            }
        }
    }

    fn update_friction(&mut self, bodies: &mut RigidBodySet, colliders: &ColliderSet, dt: Real) {
        let num_wheels = self.wheels.len();
        if num_wheels == 0 {
            return;
        }
        let mut num_wheels_on_ground = 0;
        for wheel in &mut self.wheels {
            let ground_object = wheel.raycast_info.ground_object;
            if ground_object.is_some() {
                num_wheels_on_ground += 1;
            }
            wheel.side_impulse = 0.0;
            wheel.forward_impulse = 0.0;
        }
        {
            for i in 0..num_wheels {
                let wheel = &mut self.wheels[i];
                let ground_object = wheel.raycast_info.ground_object;
                if ground_object.is_some() {
                    self.axle[i] = wheel.wheel_axle_ws;
                    let surf_normal_ws = wheel.raycast_info.contact_normal_ws;
                    let proj = self.axle[i].dot(surf_normal_ws);
                    self.axle[i] -= surf_normal_ws * proj;
                    self.axle[i] = self.axle[i].normalize_or_zero();
                    self.forward_ws[i] = surf_normal_ws.cross(self.axle[i]).normalize_or_zero();
                    if let Some(ground_body) = ground_object
                        .and_then(|h| colliders[h].parent())
                        .map(|h| &bodies[h])
                        .filter(|b| b.is_dynamic())
                    {
                        wheel.side_impulse = resolve_single_bilateral(
                            &bodies[self.chassis],
                            wheel.raycast_info.contact_point_ws,
                            ground_body,
                            wheel.raycast_info.contact_point_ws,
                            self.axle[i],
                        );
                    } else {
                        wheel.side_impulse = resolve_single_unilateral(
                            &bodies[self.chassis],
                            wheel.raycast_info.contact_point_ws,
                            self.axle[i],
                        );
                    }
                    wheel.side_impulse *= wheel.side_friction_stiffness;
                }
            }
        }
        let side_factor = 1.0;
        let fwd_factor = 0.5;
        let mut sliding = false;
        {
            for wheel_id in 0..num_wheels {
                let wheel = &mut self.wheels[wheel_id];
                let ground_object = wheel.raycast_info.ground_object;
                let mut rolling_friction = 0.0;
                if ground_object.is_some() {
                    if wheel.engine_force != 0.0 {
                        rolling_friction = wheel.engine_force * dt;
                    } else {
                        let default_rolling_friction_impulse = 0.0;
                        let max_impulse = if wheel.brake != 0.0 {
                            wheel.brake
                        } else {
                            default_rolling_friction_impulse
                        };
                        let contact_pt = WheelContactPoint::new(
                            &bodies[self.chassis],
                            ground_object
                                .and_then(|h| colliders[h].parent())
                                .map(|h| &bodies[h]),
                            wheel.raycast_info.contact_point_ws,
                            self.forward_ws[wheel_id],
                            max_impulse,
                        );
                        assert!(num_wheels_on_ground > 0);
                        rolling_friction = contact_pt.calc_rolling_friction(num_wheels_on_ground);
                    }
                }
                wheel.forward_impulse = 0.0;
                wheel.skid_info = 1.0;
                if ground_object.is_some() {
                    let max_imp = wheel.wheel_suspension_force * dt * wheel.friction_slip;
                    let max_imp_side = max_imp;
                    let max_imp_squared = max_imp * max_imp_side;
                    assert!(max_imp_squared >= 0.0);
                    wheel.forward_impulse = rolling_friction;
                    let x = wheel.forward_impulse * fwd_factor;
                    let y = wheel.side_impulse * side_factor;
                    let impulse_squared = x * x + y * y;
                    if impulse_squared > max_imp_squared {
                        sliding = true;
                        let factor = max_imp * inv(impulse_squared.sqrt());
                        wheel.skid_info *= factor;
                    }
                }
            }
        }
        if sliding {
            for wheel in &mut self.wheels {
                if wheel.side_impulse != 0.0 && wheel.skid_info < 1.0 {
                    wheel.forward_impulse *= wheel.skid_info;
                    wheel.side_impulse *= wheel.skid_info;
                }
            }
        }
        {
            let chassis = bodies.get_mut(self.chassis).unwrap();
            for wheel_id in 0..num_wheels {
                let wheel = &self.wheels[wheel_id];
                let mut impulse_point = wheel.raycast_info.contact_point_ws;
                if wheel.forward_impulse != 0.0 {
                    chassis.apply_impulse_at_point(
                        self.forward_ws[wheel_id] * wheel.forward_impulse,
                        impulse_point,
                        false,
                    );
                }
                if wheel.side_impulse != 0.0 {
                    let side_impulse = self.axle[wheel_id] * wheel.side_impulse;
                    let v_chassis_world_up =
                        chassis.position().rotation * Vector::ith(self.index_up_axis, 1.0);
                    impulse_point -= v_chassis_world_up
                        * (v_chassis_world_up.dot(impulse_point - chassis.center_of_mass())
                            * (1.0 - wheel.roll_influence));
                    chassis.apply_impulse_at_point(side_impulse, impulse_point, false);
                }
            }
        }
    }
}

// Rapier's private utils::inv, including its inclusive +/- 1e-20 deadband.
fn inv(val: Real) -> Real {
    if (-1.0e-20..=1.0e-20).contains(&val) {
        0.0
    } else {
        1.0 / val
    }
}

struct WheelContactPoint<'a> {
    body0: &'a RigidBody,
    body1: Option<&'a RigidBody>,
    friction_position_world: Vector,
    friction_direction_world: Vector,
    jac_diag_ab_inv: Real,
    max_impulse: Real,
}

impl<'a> WheelContactPoint<'a> {
    fn new(
        body0: &'a RigidBody,
        body1: Option<&'a RigidBody>,
        friction_position_world: Vector,
        friction_direction_world: Vector,
        max_impulse: Real,
    ) -> Self {
        fn impulse_denominator(body: &RigidBody, pos: Vector, n: Vector) -> Real {
            let dpt = pos - body.center_of_mass();
            let gcross = dpt.gcross(n);
            let v = (body.mass_properties().effective_world_inv_inertia * gcross).gcross(dpt);
            body.mass_properties().local_mprops.inv_mass + n.dot(v)
        }
        let denom0 = impulse_denominator(body0, friction_position_world, friction_direction_world);
        let denom1 = body1
            .map(|body1| {
                impulse_denominator(body1, friction_position_world, friction_direction_world)
            })
            .unwrap_or(0.0);
        let relaxation = 1.0;
        let jac_diag_ab_inv = relaxation / (denom0 + denom1);
        Self {
            body0,
            body1,
            friction_position_world,
            friction_direction_world,
            jac_diag_ab_inv,
            max_impulse,
        }
    }

    fn calc_rolling_friction(&self, num_wheels_on_ground: usize) -> Real {
        let contact_pos_world = self.friction_position_world;
        let max_impulse = self.max_impulse;
        let vel1 = self.body0.velocity_at_point(contact_pos_world);
        let vel2 = self
            .body1
            .map(|b| b.velocity_at_point(contact_pos_world))
            .unwrap_or_default();
        let vel = vel1 - vel2;
        let vrel = self.friction_direction_world.dot(vel);
        (-vrel * self.jac_diag_ab_inv / (num_wheels_on_ground as Real))
            .clamp(-max_impulse, max_impulse)
    }
}

fn resolve_single_bilateral(
    body1: &RigidBody,
    pt1: Vector,
    body2: &RigidBody,
    pt2: Vector,
    normal: Vector,
) -> Real {
    let vel1 = body1.velocity_at_point(pt1);
    let vel2 = body2.velocity_at_point(pt2);
    let dvel = vel1 - vel2;
    let dpt1 = pt1 - body1.center_of_mass();
    let dpt2 = pt2 - body2.center_of_mass();
    let aj = dpt1.gcross(normal);
    let bj = dpt2.gcross(-normal);
    let iaj = body1.mass_properties().effective_world_inv_inertia * aj;
    let ibj = body2.mass_properties().effective_world_inv_inertia * bj;
    let im1 = body1.mass_properties().local_mprops.inv_mass;
    let im2 = body2.mass_properties().local_mprops.inv_mass;
    let jac_diag_ab = im1 + im2 + iaj.gdot(iaj) + ibj.gdot(ibj);
    let jac_diag_ab_inv = inv(jac_diag_ab);
    let rel_vel = normal.dot(dvel);
    let contact_damping = 0.2;
    -contact_damping * rel_vel * jac_diag_ab_inv
}

fn resolve_single_unilateral(body1: &RigidBody, pt1: Vector, normal: Vector) -> Real {
    let vel1 = body1.velocity_at_point(pt1);
    let dvel = vel1;
    let dpt1 = pt1 - body1.center_of_mass();
    let aj = dpt1.gcross(normal);
    let iaj = body1.mass_properties().effective_world_inv_inertia * aj;
    let im1 = body1.mass_properties().local_mprops.inv_mass;
    let jac_diag_ab = im1 + iaj.gdot(iaj);
    let jac_diag_ab_inv = inv(jac_diag_ab);
    let rel_vel = normal.dot(dvel);
    let contact_damping = 0.2;
    -contact_damping * rel_vel * jac_diag_ab_inv
}
