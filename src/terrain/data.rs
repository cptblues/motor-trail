use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use super::config::{MAX_HEIGHT, TerrainConfig};

/// Canonical row-major heightmap, with X varying fastest and both boundary rows included.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainData {
    pub config: TerrainConfig,
    pub cells_x: usize,
    pub cells_z: usize,
    pub heights: Vec<f32>,
}

/// Inclusive vertex coordinates for downstream mesh/collider updates or future chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyRegion {
    pub min_x: usize,
    pub min_z: usize,
    pub max_x: usize,
    pub max_z: usize,
}

impl TerrainData {
    pub fn from_heights(config: TerrainConfig, heights: Vec<f32>) -> Result<Self, String> {
        config.validate()?;
        let (cells_x, cells_z) = config.cells();
        let data = Self {
            config,
            cells_x,
            cells_z,
            heights,
        };
        data.validate()?;
        Ok(data)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_layout()?;
        if let Some(index) = self
            .heights
            .iter()
            .position(|h| !h.is_finite() || h.abs() > MAX_HEIGHT)
        {
            return Err(format!(
                "Terrain height {index} must be finite and within +/-{MAX_HEIGHT} metres"
            ));
        }
        Ok(())
    }

    fn validate_layout(&self) -> Result<(), String> {
        self.config.validate()?;
        if (self.cells_x, self.cells_z) != self.config.cells() {
            return Err("Terrain cell counts do not match its configuration".into());
        }
        // Config validation bounds both counts before any additions or multiplications.
        let expected = (self.cells_x + 1) * (self.cells_z + 1);
        if self.heights.len() != expected {
            return Err(format!(
                "Terrain requires {expected} heights, got {}",
                self.heights.len()
            ));
        }
        Ok(())
    }

    pub fn height(&self, x: f32, z: f32) -> Option<f32> {
        let (indices, weights) = self.sample_triangle(x, z)?;
        Some(
            indices
                .iter()
                .zip(weights)
                .map(|(&i, w)| self.heights[i] as f64 * w)
                .sum::<f64>() as f32,
        )
    }

    /// Geometric face normal, including the same diagonal tie-break as height().
    pub fn normal(&self, x: f32, z: f32) -> Option<Vec3> {
        let (indices, _) = self.sample_triangle(x, z)?;
        let [a, b, c] = indices.map(|i| self.vertex_position(i));
        Some((b - a).cross(c - a).normalize())
    }

    /// Requires a validated heightmap and index < vertex_count().
    pub fn vertex_position(&self, index: usize) -> Vec3 {
        let stride = self.cells_x + 1;
        let ix = index % stride;
        let iz = index / stride;
        // Preserve the historical grid's arithmetic for exact legacy samples.
        let x = (ix as f32 / self.cells_x as f32 - 0.5) * self.config.width;
        let z = (iz as f32 / self.cells_z as f32 - 0.5) * self.config.depth;
        Vec3::new(x, self.heights[index], z)
    }

    pub fn points(&self) -> Vec<Vec3> {
        (0..self.vertex_count())
            .map(|index| self.vertex_position(index))
            .collect()
    }

    pub fn triangles(&self) -> Vec<[u32; 3]> {
        let mut triangles = Vec::with_capacity(self.triangle_count());
        let stride = self.cells_x + 1;
        for z in 0..self.cells_z {
            for x in 0..self.cells_x {
                let a = (z * stride + x) as u32;
                let b = a + 1;
                let c = a + stride as u32;
                let d = c + 1;
                triangles.push([a, c, b]);
                triangles.push([b, c, d]);
            }
        }
        triangles
    }

    pub fn vertex_count(&self) -> usize {
        self.heights.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.cells_x * self.cells_z * 2
    }

    /// Approximate resident canonical storage, excluding derived mesh/collider buffers.
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.heights.capacity() * std::mem::size_of::<f32>()
    }

    fn sample_triangle(&self, x: f32, z: f32) -> Option<([usize; 3], [f64; 3])> {
        self.validate_layout().ok()?;
        if !x.is_finite()
            || !z.is_finite()
            || x.abs() > self.config.width * 0.5
            || z.abs() > self.config.depth * 0.5
        {
            return None;
        }
        let (ix, u) = sample_axis(x, self.config.width, self.cells_x);
        let (iz, v) = sample_axis(z, self.config.depth, self.cells_z);
        let a = iz * (self.cells_x + 1) + ix;
        let b = a + 1;
        let c = a + self.cells_x + 1;
        let d = c + 1;
        let (indices, weights) = if u + v <= 1.0 {
            ([a, c, b], [1.0 - u - v, v, u])
        } else {
            ([b, c, d], [1.0 - v, 1.0 - u, u + v - 1.0])
        };
        indices
            .iter()
            .all(|&i| self.heights[i].is_finite() && self.heights[i].abs() <= MAX_HEIGHT)
            .then_some((indices, weights))
    }
}

fn sample_axis(value: f32, size: f32, cells: usize) -> (usize, f64) {
    let coordinate = |index: usize| (index as f32 / cells as f32 - 0.5) * size;
    let estimate = (value as f64 / size as f64 + 0.5) * cells as f64;
    let mut index = (estimate.floor() as usize).min(cells - 1);
    // The estimate uses ideal spacing; choose the cell and weights from the actual f32 mesh.
    while index > 0 && value < coordinate(index) {
        index -= 1;
    }
    while index + 1 < cells && value >= coordinate(index + 1) {
        index += 1;
    }
    let start = coordinate(index) as f64;
    let end = coordinate(index + 1) as f64;
    (
        index,
        ((value as f64 - start) / (end - start)).clamp(0.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::super::config::TerrainPreset;
    use super::*;

    fn small(heights: Vec<f32>) -> TerrainData {
        TerrainData::from_heights(
            TerrainConfig {
                preset: TerrainPreset::Flat,
                width: 8.0,
                depth: 8.0,
                resolution: 4.0,
                ..Default::default()
            },
            heights,
        )
        .unwrap()
    }

    #[test]
    fn counts_positions_and_winding_share_one_centered_grid() {
        let data = small(vec![0.0; 9]);
        assert_eq!(data.vertex_count(), 9);
        assert_eq!(data.triangle_count(), 8);
        assert_eq!(data.vertex_position(0), Vec3::new(-4.0, 0.0, -4.0));
        assert_eq!(data.vertex_position(8), Vec3::new(4.0, 0.0, 4.0));
        assert_eq!(data.points().len(), 9);
        let triangles = data.triangles();
        assert_eq!(&triangles[..2], &[[0, 3, 1], [1, 3, 4]]);
        assert_eq!(triangles.len(), data.triangle_count());
        for triangle in triangles {
            assert!(triangle.iter().all(|&i| (i as usize) < data.vertex_count()));
            let [a, b, c] = triangle.map(|i| data.vertex_position(i as usize));
            assert!((b - a).cross(c - a).y > 0.0);
        }
        assert_eq!(
            data.memory_bytes(),
            std::mem::size_of::<TerrainData>()
                + data.heights.capacity() * std::mem::size_of::<f32>()
        );
    }

    #[test]
    fn sampling_follows_triangles_instead_of_bilinear_interpolation() {
        let mut heights = vec![0.0; 9];
        heights[4] = 4.0;
        let data = small(heights);
        assert_eq!(data.height(-3.0, -3.0), Some(0.0));
        assert_eq!(data.height(-2.0, -2.0), Some(0.0));
        assert_eq!(data.height(-1.0, -1.0), Some(2.0));
        assert_eq!(data.normal(-3.0, -3.0), Some(Vec3::Y));
        let expected = Vec3::new(-1.0, 1.0, -1.0).normalize();
        assert!((data.normal(-1.0, -1.0).unwrap() - expected).length() < 1e-6);
    }

    #[test]
    fn query_matches_each_triangle_plane_and_normal() {
        let data = small(vec![1.0, -2.0, 7.0, 3.0, 5.0, -1.0, 4.0, 8.0, 0.0]);
        for triangle in data.triangles() {
            let [a, b, c] = triangle.map(|i| data.vertex_position(i as usize));
            let p = a * 0.2 + b * 0.3 + c * 0.5;
            assert!((data.height(p.x, p.z).unwrap() - p.y).abs() < 2e-6);
            let expected = (b - a).cross(c - a).normalize();
            assert!((data.normal(p.x, p.z).unwrap() - expected).length() < 1e-6);
        }
    }

    #[test]
    fn vertices_and_closed_edges_are_queryable_but_outside_is_not() {
        let data = small((0..9).map(|i| i as f32).collect());
        for p in data.points() {
            assert_eq!(data.height(p.x, p.z), Some(p.y));
        }
        for (x, z) in [
            (4.01, 0.0),
            (-4.01, 0.0),
            (0.0, 4.01),
            (0.0, -4.01),
            (f32::NAN, 0.0),
            (0.0, f32::INFINITY),
        ] {
            assert_eq!(data.height(x, z), None);
            assert_eq!(data.normal(x, z), None);
        }
    }

    #[test]
    fn non_integral_grids_sample_the_actual_rounded_mesh_vertices_exactly() {
        let config = TerrainConfig {
            width: 8.1,
            depth: 9.3,
            resolution: 0.7,
            ..Default::default()
        };
        let (nx, nz) = config.cells();
        let heights = (0..(nx + 1) * (nz + 1))
            .map(|i| (i % 11) as f32 - 5.0)
            .collect();
        let data = TerrainData::from_heights(config, heights).unwrap();
        for p in data.points() {
            assert_eq!(data.height(p.x, p.z), Some(p.y), "vertex {p:?}");
        }
        for triangle in data.triangles() {
            let [a, b, c] = triangle.map(|i| data.vertex_position(i as usize));
            let p = a * 0.25 + b * 0.25 + c * 0.5;
            assert!((data.height(p.x, p.z).unwrap() - p.y).abs() < 2e-5);
            assert_eq!(
                data.normal(p.x, p.z).unwrap(),
                (b - a).cross(c - a).normalize()
            );
        }
    }

    #[test]
    fn rejects_bad_lengths_heights_and_deserialized_layouts() {
        let config = small(vec![0.0; 9]).config;
        assert!(TerrainData::from_heights(config.clone(), vec![0.0; 8]).is_err());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 512.01, -512.01] {
            let mut heights = vec![0.0; 9];
            heights[3] = bad;
            assert!(TerrainData::from_heights(config.clone(), heights).is_err());
        }
        let mut data = small(vec![512.0; 9]);
        assert!(data.validate().is_ok());
        data.cells_x += 1;
        assert!(data.validate().is_err());
        assert_eq!(data.height(0.0, 0.0), None);
        assert_eq!(data.normal(0.0, 0.0), None);
        data.cells_x = usize::MAX;
        assert!(data.validate().is_err());
    }

    #[test]
    fn exact_height_snapshot_survives_json_round_trip() {
        let data = small(vec![0.0, 0.125, -512.0, 512.0, 1.5, -8.0, 3.0, 6.0, 1.0]);
        let encoded = serde_json::to_string(&data).unwrap();
        let restored: TerrainData = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored, data);
        assert!(restored.validate().is_ok());
    }
}
