use bevy::math::Vec2;
use serde::{Deserialize, Serialize};

use super::config::MAX_HEIGHT;
use super::data::{DirtyRegion, TerrainData};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushKind {
    Raise,
    Lower,
    Smooth,
    Flatten,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brush {
    pub kind: BrushKind,
    pub radius: f32,
    /// Metres/second for Raise/Lower; exponential response rate/second for Smooth/Flatten.
    pub strength: f32,
    pub falloff: f32,
    pub flatten_height: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            kind: BrushKind::Raise,
            radius: 3.0,
            strength: 2.0,
            falloff: 1.5,
            flatten_height: 0.0,
        }
    }
}

impl Brush {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, min, max) in [
            ("radius", self.radius, 0.25, 32.0),
            ("strength", self.strength, 0.01, 20.0),
            ("falloff", self.falloff, 0.1, 4.0),
            (
                "flatten height",
                self.flatten_height,
                -MAX_HEIGHT,
                MAX_HEIGHT,
            ),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!(
                    "Brush {name} must be finite and between {min} and {max}"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeightChange {
    pub index: usize,
    pub before: f32,
    pub after: f32,
}

pub fn apply_brush(
    data: &mut TerrainData,
    center: Vec2,
    brush: &Brush,
    dt: f32,
) -> Result<Vec<HeightChange>, String> {
    brush.validate()?;
    if !center.is_finite() {
        return Err("Brush centre must be finite".into());
    }
    if !dt.is_finite() || dt < 0.0 {
        return Err("Brush elapsed time must be finite and nonnegative".into());
    }
    data.validate()?;
    if dt == 0.0 {
        return Ok(Vec::new());
    }
    let Some(region) = affected_region(data, center, brush.radius) else {
        return Ok(Vec::new());
    };
    let stride = data.cells_x + 1;
    // Smooth samples a clipped 3x3 neighbourhood from one local snapshot, never partially edited data.
    let halo = DirtyRegion {
        min_x: region.min_x.saturating_sub(1),
        min_z: region.min_z.saturating_sub(1),
        max_x: (region.max_x + 1).min(data.cells_x),
        max_z: (region.max_z + 1).min(data.cells_z),
    };
    let halo_stride = halo.max_x - halo.min_x + 1;
    let source = if brush.kind == BrushKind::Smooth {
        let mut source = Vec::with_capacity(halo_stride * (halo.max_z - halo.min_z + 1));
        for z in halo.min_z..=halo.max_z {
            source.extend_from_slice(
                &data.heights[z * stride + halo.min_x..=z * stride + halo.max_x],
            );
        }
        source
    } else {
        Vec::new()
    };
    let mut changes = Vec::new();
    for z in region.min_z..=region.max_z {
        for x in region.min_x..=region.max_x {
            let index = z * stride + x;
            let p = data.vertex_position(index);
            // Evaluate in f64 so every finite f32 centre and dt, including their maxima, is safe.
            let distance = (p.x as f64 - center.x as f64).hypot(p.z as f64 - center.y as f64);
            if distance >= brush.radius as f64 {
                continue;
            }
            let weight = (1.0 - distance / brush.radius as f64).powf(brush.falloff as f64);
            let exposure = brush.strength as f64 * dt as f64 * weight;
            let before = data.heights[index];
            let after = match brush.kind {
                BrushKind::Raise => before as f64 + exposure,
                BrushKind::Lower => before as f64 - exposure,
                BrushKind::Flatten => {
                    let alpha = -(-exposure).exp_m1();
                    before as f64 + (brush.flatten_height as f64 - before as f64) * alpha
                }
                BrushKind::Smooth => {
                    let mut sum = 0.0_f64;
                    let mut count = 0;
                    for nz in z.saturating_sub(1)..=(z + 1).min(data.cells_z) {
                        for nx in x.saturating_sub(1)..=(x + 1).min(data.cells_x) {
                            sum += source[(nz - halo.min_z) * halo_stride + nx - halo.min_x] as f64;
                            count += 1;
                        }
                    }
                    let alpha = -(-exposure).exp_m1();
                    before as f64 + (sum / count as f64 - before as f64) * alpha
                }
            }
            .clamp(-(MAX_HEIGHT as f64), MAX_HEIGHT as f64) as f32;
            if after != before {
                data.heights[index] = after;
                changes.push(HeightChange {
                    index,
                    before,
                    after,
                });
            }
        }
    }
    Ok(changes)
}

fn affected_region(data: &TerrainData, center: Vec2, radius: f32) -> Option<DirtyRegion> {
    fn bounds(center: f64, radius: f64, size: f64, cells: usize) -> Option<(usize, usize)> {
        if center - radius > size * 0.5 || center + radius < -size * 0.5 {
            return None;
        }
        // Widen by one vertex to cover f32 coordinate rounding at either AABB boundary.
        let first = (((center - radius) / size + 0.5) * cells as f64)
            .floor()
            .max(0.0) as usize;
        let last = (((center + radius) / size + 0.5) * cells as f64)
            .ceil()
            .max(0.0) as usize;
        Some((
            first.saturating_sub(1).min(cells),
            last.saturating_add(1).min(cells),
        ))
    }
    let (min_x, max_x) = bounds(
        center.x as f64,
        radius as f64,
        data.config.width as f64,
        data.cells_x,
    )?;
    let (min_z, max_z) = bounds(
        center.y as f64,
        radius as f64,
        data.config.depth as f64,
        data.cells_z,
    )?;
    Some(DirtyRegion {
        min_x,
        min_z,
        max_x,
        max_z,
    })
}

#[cfg(test)]
mod tests {
    use super::super::config::TerrainConfig;
    use super::*;

    fn flat(height: f32) -> TerrainData {
        let config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            resolution: 1.0,
            ..Default::default()
        };
        TerrainData::from_heights(config, vec![height; 81]).unwrap()
    }

    fn brush(kind: BrushKind) -> Brush {
        Brush {
            kind,
            radius: 2.0,
            strength: 2.0,
            falloff: 1.0,
            flatten_height: 4.0,
        }
    }

    #[test]
    fn raise_and_lower_integrate_metres_per_second_and_return_reversible_changes() {
        for (kind, expected) in [(BrushKind::Raise, 0.5), (BrushKind::Lower, -0.5)] {
            let mut data = flat(0.0);
            let original = data.clone();
            let changes = apply_brush(&mut data, Vec2::ZERO, &brush(kind), 0.25).unwrap();
            assert_eq!(data.heights[40], expected);
            assert!(!changes.is_empty());
            assert!(changes.windows(2).all(|pair| pair[0].index < pair[1].index));
            for change in &changes {
                assert_eq!(change.before, original.heights[change.index]);
                assert_eq!(change.after, data.heights[change.index]);
                data.heights[change.index] = change.before;
            }
            assert_eq!(data, original);
            for change in changes {
                data.heights[change.index] = change.after;
            }
            assert_eq!(data.heights[40], expected);
        }
    }

    #[test]
    fn raise_and_lower_do_not_discard_elapsed_time() {
        for kind in [BrushKind::Raise, BrushKind::Lower] {
            let mut once = flat(0.0);
            let mut split = once.clone();
            apply_brush(&mut once, Vec2::ZERO, &brush(kind), 2.0).unwrap();
            for _ in 0..120 {
                apply_brush(&mut split, Vec2::ZERO, &brush(kind), 1.0 / 60.0).unwrap();
            }
            assert!((once.heights[40].abs() - 4.0).abs() < 1e-6);
            for (a, b) in once.heights.iter().zip(&split.heights) {
                assert!((a - b).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn all_tools_leave_vertices_on_and_outside_the_radius_unchanged() {
        for kind in [
            BrushKind::Raise,
            BrushKind::Lower,
            BrushKind::Smooth,
            BrushKind::Flatten,
        ] {
            let mut data = flat(0.0);
            data.heights[40] = 9.0;
            let original = data.clone();
            let changes =
                apply_brush(&mut data, Vec2::new(0.25, -0.25), &brush(kind), 0.2).unwrap();
            assert!(!changes.is_empty());
            for index in 0..data.vertex_count() {
                let p = data.vertex_position(index);
                if Vec2::new(p.x, p.z).distance(Vec2::new(0.25, -0.25)) >= 2.0 {
                    assert_eq!(data.heights[index], original.heights[index]);
                }
            }
        }
    }

    #[test]
    fn small_brushes_and_exact_radius_boundaries_do_not_leak() {
        let mut data = flat(0.0);
        let tiny = Brush {
            radius: 0.25,
            ..Default::default()
        };
        assert!(
            apply_brush(&mut data, Vec2::new(0.5, 0.5), &tiny, 1.0)
                .unwrap()
                .is_empty()
        );
        let changes = apply_brush(&mut data, Vec2::ZERO, &tiny, 1.0).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].index, 40);
        for kind in [
            BrushKind::Raise,
            BrushKind::Lower,
            BrushKind::Smooth,
            BrushKind::Flatten,
        ] {
            let mut data = flat(0.0);
            data.heights[40] = 9.0;
            let original = data.clone();
            apply_brush(&mut data, Vec2::ZERO, &brush(kind), 0.2).unwrap();
            for index in [22, 38, 42, 58] {
                assert_eq!(data.heights[index], original.heights[index]);
            }
        }
    }

    #[test]
    fn smoothing_uses_one_source_snapshot_including_untouched_halo() {
        let mut data = flat(0.0);
        data.heights[40] = 9.0;
        data.heights[42] = 6.0;
        let original = data.clone();
        let brush = brush(BrushKind::Smooth);
        let dt = 0.5;
        let changes = apply_brush(&mut data, Vec2::ZERO, &brush, dt).unwrap();
        assert!(data.heights[40] < original.heights[40]);
        assert!(data.heights[41] > 0.0);
        for change in changes {
            let x = change.index % 9;
            let z = change.index / 9;
            let mut total = 0.0_f64;
            let mut count = 0;
            for nz in z.saturating_sub(1)..=(z + 1).min(8) {
                for nx in x.saturating_sub(1)..=(x + 1).min(8) {
                    total += original.heights[nz * 9 + nx] as f64;
                    count += 1;
                }
            }
            let p = original.vertex_position(change.index);
            let weight = 1.0 - ((p.x as f64).hypot(p.z as f64) / 2.0);
            let alpha = -(-2.0 * weight * dt as f64).exp_m1();
            let expected =
                change.before as f64 + (total / count as f64 - change.before as f64) * alpha;
            assert!((change.after as f64 - expected).abs() < 1e-6);
        }
        assert_eq!(data.heights[42], original.heights[42]);
    }

    #[test]
    fn smoothing_at_an_edge_keeps_constant_surfaces_constant() {
        let mut data = flat(3.0);
        assert!(
            apply_brush(
                &mut data,
                Vec2::new(-4.0, -4.0),
                &brush(BrushKind::Smooth),
                1.0
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(data, flat(3.0));
    }

    #[test]
    fn flatten_uses_exponential_response_and_composes_across_timesteps() {
        let mut once = flat(-2.0);
        let mut split = once.clone();
        let brush = brush(BrushKind::Flatten);
        apply_brush(&mut once, Vec2::ZERO, &brush, 1.0).unwrap();
        for _ in 0..100 {
            apply_brush(&mut split, Vec2::ZERO, &brush, 0.01).unwrap();
        }
        let expected = -2.0 + 6.0 * (1.0 - (-2.0_f32).exp());
        assert!((once.heights[40] - expected).abs() < 1e-6);
        assert!(once.heights[40] > -2.0 && once.heights[40] < 4.0);
        for (a, b) in once.heights.iter().zip(&split.heights) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn zero_time_and_distant_brushes_are_noops_and_heights_are_bounded() {
        let mut data = flat(511.0);
        let original = data.clone();
        assert!(
            apply_brush(&mut data, Vec2::ZERO, &brush(BrushKind::Raise), 0.0)
                .unwrap()
                .is_empty()
        );
        assert!(
            apply_brush(
                &mut data,
                Vec2::new(f32::MAX, f32::MAX),
                &brush(BrushKind::Raise),
                1.0
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(data, original);
        apply_brush(&mut data, Vec2::ZERO, &brush(BrushKind::Raise), f32::MAX).unwrap();
        assert_eq!(data.heights[40], 512.0);
        data = flat(-511.0);
        apply_brush(&mut data, Vec2::ZERO, &brush(BrushKind::Lower), f32::MAX).unwrap();
        assert_eq!(data.heights[40], -512.0);
    }

    #[test]
    fn invalid_brushes_times_centres_and_data_are_rejected_without_mutation() {
        assert!(Brush::default().validate().is_ok());
        let setters: [fn(&mut Brush, f32); 4] = [
            |b, v| b.radius = v,
            |b, v| b.strength = v,
            |b, v| b.falloff = v,
            |b, v| b.flatten_height = v,
        ];
        for set in setters {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = Brush::default();
                set(&mut invalid, value);
                assert!(invalid.validate().is_err());
                let mut data = flat(0.0);
                assert!(apply_brush(&mut data, Vec2::ZERO, &invalid, 0.1).is_err());
                assert_eq!(data, flat(0.0));
            }
        }
        for invalid in [
            Brush {
                radius: 0.24,
                ..Default::default()
            },
            Brush {
                radius: 32.1,
                ..Default::default()
            },
            Brush {
                strength: 0.0,
                ..Default::default()
            },
            Brush {
                strength: 20.1,
                ..Default::default()
            },
            Brush {
                falloff: 0.09,
                ..Default::default()
            },
            Brush {
                falloff: 4.1,
                ..Default::default()
            },
            Brush {
                flatten_height: 512.1,
                ..Default::default()
            },
        ] {
            assert!(invalid.validate().is_err());
        }
        let mut data = flat(0.0);
        for dt in [-0.1, f32::NAN, f32::INFINITY] {
            assert!(apply_brush(&mut data, Vec2::ZERO, &Brush::default(), dt).is_err());
            assert_eq!(data, flat(0.0));
        }
        assert!(apply_brush(&mut data, Vec2::new(f32::NAN, 0.0), &Brush::default(), 0.1).is_err());
        data.cells_x = usize::MAX;
        assert!(apply_brush(&mut data, Vec2::ZERO, &Brush::default(), 0.1).is_err());
    }
}
