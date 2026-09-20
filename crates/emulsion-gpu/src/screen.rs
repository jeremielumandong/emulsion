//! Nearest-pixel viewport sampling. Numerically ambiguous pixels are marked for
//! the caller's double-precision reference path, preserving crisp pixel edges.
use crate::GpuContext;

pub struct Tile<'a> {
    pub x: i32,
    pub y: i32,
    pub before: bool,
    pub bgra: &'a [u8],
}

pub struct View {
    pub size: [u32; 2],
    pub document: [u32; 2],
    pub origin: [f64; 2],
    pub dx: [f64; 2],
    pub dy: [f64; 2],
    pub wipe: Option<u32>,
}

impl GpuContext {
    /// Returns packed BGRA and a nonzero CPU-correction flag per output pixel.
    pub fn sample_screen(&self, view: &View, tiles: &[Tile<'_>]) -> Option<Vec<[u32; 2]>> {
        if !self.available() || tiles.is_empty() || tiles.len() > 128 {
            return None;
        }
        let count = view.size[0].checked_mul(view.size[1])?;
        if count == 0 || count > 8_388_608 {
            return None;
        }
        let matrix = [
            view.origin[0],
            view.origin[1],
            view.dx[0],
            view.dx[1],
            view.dy[0],
            view.dy[1],
        ];
        if matrix
            .iter()
            .any(|n| !n.is_finite() || n.abs() > 1_000_000.0)
        {
            return None;
        }
        let min_x = tiles.iter().map(|tile| tile.x).min()?;
        let min_y = tiles.iter().map(|tile| tile.y).min()?;
        let max_x = tiles.iter().map(|tile| tile.x).max()?;
        let max_y = tiles.iter().map(|tile| tile.y).max()?;
        let width = u32::try_from(i64::from(max_x) - i64::from(min_x) + 1).ok()?;
        let height = u32::try_from(i64::from(max_y) - i64::from(min_y) + 1).ok()?;
        let cells = width.checked_mul(height)? as usize;
        if cells > 65536 {
            return None;
        }
        let mut grid = vec![u32::MAX; cells * 2];
        let mut pixels = Vec::with_capacity(tiles.len() * 256 * 256);
        for tile in tiles {
            if tile.bgra.len() != 256 * 256 * 4 {
                return None;
            }
            let cell = (tile.y as i64 - min_y as i64) as usize * width as usize
                + (tile.x as i64 - min_x as i64) as usize;
            grid[cell + usize::from(tile.before) * cells] = pixels.len() as u32;
            pixels.extend(
                tile.bgra
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| u32::from_le_bytes(*bytes)),
            );
        }
        let params = [
            view.size[0],
            view.size[1],
            view.document[0],
            view.document[1],
            min_x as u32,
            min_y as u32,
            width,
            height,
            view.wipe.unwrap_or(0).min(view.size[0]),
            0,
            0,
            0,
        ];
        let transform = matrix.map(|n| n as f32);
        let bytes = self
            .run(
                "screen-sample",
                include_str!("screen.wgsl"),
                &[
                    bytemuck::cast_slice(&params),
                    bytemuck::cast_slice(&transform),
                    bytemuck::cast_slice(&grid),
                    bytemuck::cast_slice(&pixels),
                ],
                count as usize * 8,
                count.div_ceil(64),
            )
            .ok()?;
        Some(
            bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|bytes| bytemuck::pod_read_unaligned(bytes))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewport_sampling_preserves_bgra_wipe_missing_tiles_and_nearest_edges() {
        let Some(gpu) = crate::test_gpu() else { return };
        let pixels: Vec<u8> = (0u32..65536)
            .flat_map(|i| (0xff000000 | i).to_le_bytes())
            .collect();
        let previous: Vec<u8> = (0u32..65536)
            .flat_map(|i| (0xaa000000 | i).to_le_bytes())
            .collect();
        let tiles = [
            Tile {
                x: 0,
                y: 0,
                before: false,
                bgra: &pixels,
            },
            Tile {
                x: 0,
                y: 0,
                before: true,
                bgra: &previous,
            },
        ];
        for angle in [0.0f64, 0.37, -1.1] {
            let (s, c) = angle.sin_cos();
            let view = View {
                size: [97, 53],
                document: [300, 280],
                origin: [-1.0, 30.125],
                dx: [c * 2.3, s * 2.3],
                dy: [-s * 2.3, c * 2.3],
                wipe: Some(40),
            };
            let sampled = gpu
                .sample_screen(&view, &tiles)
                .expect("GPU screen sampling");
            for (i, &[pixel, fix]) in sampled.iter().enumerate() {
                let x = i as u32 % view.size[0];
                let y = i as u32 / view.size[0];
                let sx =
                    (view.origin[0] + x as f64 * view.dx[0] + y as f64 * view.dy[0]).floor() as i32;
                let sy =
                    (view.origin[1] + x as f64 * view.dx[1] + y as f64 * view.dy[1]).floor() as i32;
                let expected = if (0..256).contains(&sx) && (0..256).contains(&sy) {
                    (if x < 40 { 0xaa000000 } else { 0xff000000 }) | (sy as u32 * 256 + sx as u32)
                } else {
                    0
                };
                if fix == 0 {
                    assert_eq!(pixel, expected, "pixel{x},{y} angle{angle}");
                }
            }
        }
        let view = View {
            size: [1, 1],
            document: [256, 256],
            origin: [128.0, 64.0],
            dx: [0.5, 0.0],
            dy: [0.0, 0.5],
            wipe: None,
        };
        assert_eq!(
            gpu.sample_screen(&view, &tiles).unwrap()[0][1],
            1,
            "integer edges must use precise reference"
        );
    }
}
