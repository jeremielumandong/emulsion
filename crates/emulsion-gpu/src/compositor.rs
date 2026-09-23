//! Hybrid tile compositor: CPU reference sampling, GPU layer/group blending.
//! Every dispatch contains one complete tile program; oversized or malformed documents
//! return `None` so the caller can use the complete CPU reference renderer.

use crate::GpuContext;
use anyhow::Result;
use emulsion_raster::adjust::Prepared;
use emulsion_raster::blend::{BlendMode, BlendSpace};
use emulsion_raster::composite::{self, CompositeNode, CompositeTree, NodeContent};
use emulsion_raster::geom::TileCoord;
use emulsion_raster::tile::{FTile, TILE, TILE_PX, ftile};

const MAX_NODES: usize = 64;
const MAX_DEPTH: usize = 16;
const MAX_COMMANDS: usize = 512;
const MAX_SOURCE_PIXELS: usize = 4 * 1024 * 1024; // 64 MiB of tile/lookup data.
const NONE: u32 = u32::MAX;

fn mode(mode: BlendMode) -> Option<u32> {
    use BlendMode::*;
    Some(match mode {
        Normal | PassThrough => 0,
        Darken => 1,
        Multiply => 2,
        ColorBurn => 3,
        LinearBurn => 4,
        Lighten => 5,
        Screen => 6,
        ColorDodge => 7,
        LinearDodge => 8,
        Overlay => 9,
        SoftLight => 10,
        HardLight => 11,
        VividLight => 12,
        LinearLight => 13,
        PinLight => 14,
        HardMix => 15,
        Difference => 16,
        Exclusion => 17,
        Subtract => 18,
        Divide => 19,
        DarkerColor => 20,
        LighterColor => 21,
        Hue => 22,
        Saturation => 23,
        Color => 24,
        Luminosity => 25,
        Dissolve => 26,
    })
}

fn supported(nodes: &[CompositeNode], depth: usize, count: &mut usize) -> bool {
    *count = count.saturating_add(nodes.len());
    if depth >= MAX_DEPTH || *count > MAX_NODES {
        return false;
    }
    nodes.iter().enumerate().all(|(i, node)| {
        if !node.visible || node.clip_to.is_some_and(|j| j < i && !nodes[j].visible) {
            return true;
        }
        node.blending == Default::default()
            && mode(node.blend).is_some()
            && match &node.content {
                NodeContent::StyledGroup { .. } => false,
                NodeContent::Adjust(op) => supported_adjust(op, 0),
                NodeContent::Group(children) => supported(children, depth + 1, count),
                _ => true,
            }
    })
}

fn supported_adjust(op: &Prepared, depth: usize) -> bool {
    fn visit(op: &Prepared, depth: usize, remaining: &mut usize) -> bool {
        if depth > 16 || *remaining == 0 {
            return false;
        }
        *remaining -= 1;
        match op {
            Prepared::SelectiveColor { .. } => false,
            Prepared::Chain(ops) => {
                ops.len() <= 64 && ops.iter().all(|op| visit(op, depth + 1, remaining))
            }
            Prepared::Lut(t) => t.iter().all(|v| v.len() == 4096),
            Prepared::GradientMap(t) => t.len() == 256,
            Prepared::Cube { cube, .. } => {
                cube.size >= 2 && cube.size <= 64 && cube.data.len() == (cube.size as usize).pow(3)
            }
            _ => true,
        }
    }
    let mut remaining = MAX_COMMANDS;
    visit(op, depth, &mut remaining)
}

struct Program<'a> {
    tree: &'a CompositeTree,
    level: u32,
    tile: TileCoord,
    commands: Vec<[u32; 8]>,
    sources: Vec<[f32; 4]>,
    slots: usize,
}

impl Program<'_> {
    // Reuse exact double-precision placement/mip/mask sampling from the CPU
    // reference. Only tile-sized data is uploaded, never an entire source image.
    fn source(&mut self, node: &CompositeNode, content: NodeContent) -> Option<u32> {
        if self.sources.len() > MAX_SOURCE_PIXELS - TILE_PX {
            return None;
        }
        let sampled = composite::render_tile_cpu(
            &CompositeTree {
                width: self.tree.width,
                height: self.tree.height,
                space: self.tree.space,
                nodes: vec![CompositeNode {
                    id: node.id,
                    visible: true,
                    opacity: 1.0,
                    blend: BlendMode::Normal,
                    blending: Default::default(),
                    mask: node.mask.clone(),
                    clip_to: None,
                    content,
                }],
            },
            self.level,
            self.tile,
        );
        let offset = self.sources.len() as u32;
        self.sources.extend(sampled);
        Some(offset)
    }

    fn adjust(&mut self, op: &Prepared) -> Option<()> {
        if self.commands.len() >= MAX_COMMANDS {
            return None;
        }
        if let Prepared::Chain(ops) = op {
            for op in ops {
                self.adjust(op)?;
            }
            return Some(());
        }
        let offset = self.sources.len() as u32;
        let (kind, payload): (u32, Vec<[f32; 4]>) = match op {
            Prepared::Lut(table) => (
                10,
                (0..4096)
                    .map(|i| [table[0][i], table[1][i], table[2][i], 0.0])
                    .collect(),
            ),
            Prepared::Threshold(level) => (11, vec![[*level, 0.0, 0.0, 0.0]]),
            Prepared::GradientMap(table) => {
                (12, table.iter().map(|v| [v[0], v[1], v[2], 0.0]).collect())
            }
            Prepared::HueSat { hue, sat, light } => (13, vec![[*hue, *sat, *light, 0.0]]),
            Prepared::ColorBalance { s, m, h, preserve } => (
                14,
                vec![
                    [s[0], s[1], s[2], u8::from(*preserve) as f32],
                    [m[0], m[1], m[2], 0.0],
                    [h[0], h[1], h[2], 0.0],
                ],
            ),
            Prepared::Vibrance { vib, sat } => (15, vec![[*vib, *sat, 0.0, 0.0]]),
            Prepared::BlackAndWhite { w, tint } => {
                let (col, k) = tint.unwrap_or(([1.0; 3], 0.0));
                (
                    16,
                    vec![
                        [w[0], w[1], w[2], w[3]],
                        [w[4], w[5], 0.0, 0.0],
                        [col[0], col[1], col[2], k],
                    ],
                )
            }
            Prepared::PhotoFilter {
                color,
                density,
                preserve,
            } => (
                17,
                vec![
                    [color[0], color[1], color[2], *density],
                    [u8::from(*preserve) as f32, 0.0, 0.0, 0.0],
                ],
            ),
            Prepared::Grain { amount, size, mono } => {
                (18, vec![[*amount, *size, u8::from(*mono) as f32, 0.0]])
            }
            Prepared::Vignette {
                amount,
                midpoint,
                feather,
                roundness,
            } => (19, vec![[*amount, *midpoint, *feather, *roundness]]),
            Prepared::Cube { cube, strength } => {
                let mut values = vec![[cube.size as f32, *strength, 0.0, 0.0]];
                values.extend(cube.data.iter().map(|v| {
                    [
                        v[0] as f32 / 65535.0,
                        v[1] as f32 / 65535.0,
                        v[2] as f32 / 65535.0,
                        0.0,
                    ]
                }));
                (20, values)
            }
            Prepared::SelectiveColor { .. } => return None,
            Prepared::Chain(_) => unreachable!(),
        };
        if self.sources.len().checked_add(payload.len())? > MAX_SOURCE_PIXELS {
            return None;
        }
        self.sources.extend(payload);
        self.commands.push([kind, 0, 0, 0, offset, 0, 0, 0]);
        Some(())
    }

    fn list(&mut self, nodes: &[CompositeNode], depth: usize) -> Option<()> {
        if depth >= MAX_DEPTH || self.slots.checked_add(nodes.len())? > MAX_NODES {
            return None;
        }
        let base = self.slots;
        self.slots += nodes.len();
        for (i, node) in nodes.iter().enumerate() {
            if !node.visible {
                continue;
            }
            let clip = match node.clip_to {
                Some(j) if j < i => {
                    if !nodes[j].visible {
                        continue;
                    }
                    (base + j) as u32
                }
                _ => NONE,
            };
            if self.commands.len() >= MAX_COMMANDS - 2 {
                return None;
            }
            let blend = mode(node.blend)?;
            let slot = (base + i) as u32;
            let source = match &node.content {
                NodeContent::Pixels { raster, placement } => self.source(
                    node,
                    NodeContent::Pixels {
                        raster: raster.clone(),
                        placement: *placement,
                    },
                )?,
                NodeContent::Fill(color) => self.source(node, NodeContent::Fill(*color))?,
                NodeContent::Adjust(op) => {
                    let mask = if node.mask.is_some() {
                        self.source(node, NodeContent::Fill([1.0; 4]))?
                    } else {
                        NONE
                    };
                    let opacity = if node.opacity >= 1.0 && clip == NONE && mask == NONE {
                        1.0
                    } else {
                        node.opacity
                    };
                    self.commands.push([5, 0, 0, 0, 0, 0, 0, 0]);
                    self.adjust(op)?;
                    self.commands
                        .push([6, blend, slot, clip, mask, opacity.to_bits(), 0, 0]);
                    continue;
                }
                NodeContent::StyledGroup { .. } => return None,
                NodeContent::Group(children) => {
                    let pass = node.blend == BlendMode::PassThrough;
                    let mask = if node.mask.is_some() {
                        self.source(node, NodeContent::Fill([1.0; 4]))?
                    } else {
                        NONE
                    };
                    let opacity = if node.opacity >= 1.0 && clip == NONE && (!pass || mask == NONE)
                    {
                        1.0
                    } else {
                        node.opacity
                    };
                    self.commands
                        .push([if pass { 2 } else { 1 }, 0, 0, 0, 0, 0, 0, 0]);
                    self.list(children, depth + 1)?;
                    self.commands.push([
                        if pass { 4 } else { 3 },
                        blend,
                        slot,
                        clip,
                        mask,
                        opacity.to_bits(),
                        node.id as u32,
                        (node.id >> 32) as u32,
                    ]);
                    continue;
                }
            };
            let opacity = if node.opacity >= 1.0 && clip == NONE {
                1.0
            } else {
                node.opacity
            };
            self.commands.push([
                0,
                blend,
                slot,
                clip,
                source,
                opacity.to_bits(),
                node.id as u32,
                (node.id >> 32) as u32,
            ]);
        }
        Some(())
    }
}

/// Conservative transfer-cost estimate from end-to-end Intel Iris Plus tests.
/// Normal-only and LUT-only compositions lost to the CPU; HSL/vibrance and
/// sufficiently dense sRGB blending won after sampling/upload/readback. This
/// is deliberately a workload heuristic, not a hardware-independent guarantee.
fn worthwhile(tree: &CompositeTree) -> bool {
    if !supported(&tree.nodes, 0, &mut 0) {
        return false;
    }
    fn costly_adjustments(op: &Prepared, depth: usize) -> usize {
        if depth > 16 {
            return 0;
        }
        match op {
            Prepared::HueSat { .. } | Prepared::Vibrance { .. } => 1,
            Prepared::Chain(ops) => ops
                .iter()
                .take(MAX_COMMANDS)
                .map(|op| costly_adjustments(op, depth + 1))
                .sum(),
            _ => 0,
        }
    }
    fn count(nodes: &[CompositeNode], space: BlendSpace, depth: usize) -> (usize, usize) {
        if depth >= MAX_DEPTH {
            return (MAX_NODES, 0);
        }
        let (mut sources, mut costly) = (0, 0);
        for (i, node) in nodes.iter().enumerate() {
            if !node.visible || node.clip_to.is_some_and(|j| j < i && !nodes[j].visible) {
                continue;
            }
            match &node.content {
                NodeContent::StyledGroup { .. } => return (MAX_NODES, 0),
                NodeContent::Pixels { .. } | NodeContent::Fill(_) => sources += 1,
                NodeContent::Group(children) => {
                    let (child_sources, child_costly) = count(children, space, depth + 1);
                    sources += child_sources + usize::from(node.mask.is_some());
                    costly += child_costly;
                }
                NodeContent::Adjust(op) => {
                    sources += usize::from(node.mask.is_some());
                    if node.opacity > 0.0 {
                        costly += costly_adjustments(op, 0);
                    }
                }
            }
            if node.opacity > 0.0
                && space == BlendSpace::Srgb
                && !matches!(
                    node.blend,
                    BlendMode::Normal | BlendMode::PassThrough | BlendMode::Dissolve
                )
            {
                costly += 1;
            }
        }
        (sources, costly)
    }
    let (sources, costly) = count(&tree.nodes, tree.space, 0);
    sources > 0 && costly > 0 && costly.saturating_mul(2) >= sources
}

pub(crate) fn render_tile(
    gpu: &GpuContext,
    tree: &CompositeTree,
    level: u32,
    tile: TileCoord,
) -> Result<Option<FTile>> {
    // Explicit force/software modes are used for diagnostics and callers who
    // prefer GPU execution regardless of measured transfer overhead.
    if !crate::gpu_preferred() && !worthwhile(tree) {
        return Ok(None);
    }
    render_tile_gpu(gpu, tree, level, tile)
}

fn render_tile_gpu(
    gpu: &GpuContext,
    tree: &CompositeTree,
    level: u32,
    tile: TileCoord,
) -> Result<Option<FTile>> {
    if level >= 32 {
        return Ok(None);
    }
    let (width, height) = composite::level_size(tree.width, tree.height, level);
    let ox = i64::from(tile.x) * i64::from(TILE);
    let oy = i64::from(tile.y) * i64::from(TILE);
    if tile.x < 0 || tile.y < 0 || ox >= i64::from(width) || oy >= i64::from(height) {
        return Ok(Some(ftile()));
    }
    if !supported(&tree.nodes, 0, &mut 0) {
        return Ok(None);
    }
    let mut program = Program {
        tree,
        level,
        tile,
        commands: Vec::new(),
        sources: Vec::new(),
        slots: 0,
    };
    if program.list(&tree.nodes, 0).is_none() {
        return Ok(None);
    }
    if program.commands.is_empty() {
        return Ok(Some(ftile()));
    }
    // Header and eight-word instructions deliberately avoid implicit Rust/WGSL
    // struct padding. wgpu hosts use little-endian storage-buffer words.
    let mut words = vec![
        program.commands.len() as u32,
        u32::from(tree.space == BlendSpace::Srgb),
        (i64::from(width) - ox).min(i64::from(TILE)) as u32,
        (i64::from(height) - oy).min(i64::from(TILE)) as u32,
    ];
    words.extend_from_slice(&[
        ox as u32,
        oy as u32,
        level,
        tree.width,
        tree.height,
        0,
        0,
        0,
    ]);
    for command in &program.commands {
        words.extend_from_slice(command);
    }
    if program.sources.is_empty() {
        program.sources.push([0.0; 4]);
    }
    let out = gpu.run(
        "composite-tile-v1",
        include_str!("composite.wgsl"),
        &[
            bytemuck::cast_slice(&words),
            bytemuck::cast_slice(&program.sources),
        ],
        TILE_PX * 16,
        (TILE_PX as u32).div_ceil(64),
    )?;
    anyhow::ensure!(
        out.len() == TILE_PX * 16,
        "Unexpected GPU compositor output length"
    );
    // Vec<u8> has no alignment promise; decode instead of casting its allocation.
    let pixels = out
        .as_chunks::<16>()
        .0
        .iter()
        .map(|bytes| {
            std::array::from_fn(|channel| {
                f32::from_le_bytes(bytes[channel * 4..channel * 4 + 4].try_into().unwrap())
            })
        })
        .collect();
    Ok(Some(pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::composite::Placement;
    use emulsion_raster::image::{Mask, Raster};
    use std::sync::Arc;

    fn node(content: NodeContent) -> CompositeNode {
        CompositeNode {
            id: 1,
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            blending: Default::default(),
            mask: None,
            clip_to: None,
            content,
        }
    }

    fn check(gpu: &GpuContext, tree: &CompositeTree, level: u32, tile: TileCoord) {
        let expected = composite::render_tile_cpu(tree, level, tile);
        let actual = render_tile_gpu(gpu, tree, level, tile)
            .unwrap()
            .expect("supported tree");
        for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
            for channel in 0..4 {
                assert!(
                    (expected[channel] - actual[channel]).abs() <= 3e-5,
                    "pixel {index} channel {channel}, level {level}: CPU {expected:?}, GPU {actual:?}"
                );
            }
        }
    }

    #[test]
    fn all_supported_modes_match_reference_in_both_blend_spaces() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        // Include transparent/opaque and near-singularity channels, not just
        // constant middle gray, to exercise burn/dodge/divide and luminosity.
        let raster = Arc::new(Raster::from_srgba8(
            9,
            7,
            &(0..63)
                .flat_map(|i| {
                    [
                        (i * 43 % 256) as u8,
                        (i * 97 % 256) as u8,
                        (i * 19 % 256) as u8,
                        [0, 1, 127, 255][i % 4],
                    ]
                })
                .collect::<Vec<_>>(),
        ));
        for space in [BlendSpace::Linear, BlendSpace::Srgb] {
            for blend in BlendMode::MENU
                .iter()
                .flatten()
                .copied()
                .filter(|b| mode(*b).is_some())
            {
                let mut foreground = node(NodeContent::Pixels {
                    raster: raster.clone(),
                    placement: Placement::at(2.0, 1.0),
                });
                foreground.opacity = 0.71;
                foreground.blend = blend;
                let tree = CompositeTree {
                    width: 15,
                    height: 11,
                    space,
                    nodes: vec![
                        node(NodeContent::Fill([0.13, 0.31, 0.07, 0.63])),
                        foreground,
                    ],
                };
                check(&gpu, &tree, 0, TileCoord::new(0, 0));
            }
        }
    }

    #[test]
    fn masked_placed_pixels_groups_clipping_and_edges_match_reference() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        let mask = Arc::new(Mask::from_fn(37, 29, 0, |x, y| {
            ((x * 13 + y * 7) % 256) as u8
        }));
        let mut placed = node(NodeContent::Pixels {
            raster: Arc::new(Raster::solid(37, 29, [0.32, 0.12, 0.48, 0.6])),
            placement: Placement {
                x: 252.4,
                y: 249.2,
                scale_x: 0.8,
                scale_y: 1.2,
                rotation: 23.0,
                flip_x: true,
                ..Placement::default()
            },
        });
        placed.mask = Some(mask);
        placed.opacity = 0.53;
        let mut clipped = node(NodeContent::Fill([0.5, 0.1, 0.0, 0.7]));
        clipped.clip_to = Some(0);
        clipped.blend = BlendMode::Multiply;
        let mut isolated = node(NodeContent::Group(vec![placed, clipped]));
        isolated.opacity = 0.61;
        isolated.mask = Some(Arc::new(Mask::empty(280, 277, 193)));
        let mut pass = node(NodeContent::Group(vec![
            node(NodeContent::Fill([0.04, 0.11, 0.1, 0.2])),
            isolated,
        ]));
        pass.blend = BlendMode::PassThrough;
        pass.opacity = 0.73;
        pass.mask = Some(Arc::new(Mask::empty(280, 277, 211)));
        let mut hidden = node(NodeContent::Fill([1.0; 4]));
        hidden.visible = false;
        let mut hidden_clip = node(NodeContent::Fill([1.0; 4]));
        hidden_clip.clip_to = Some(2);
        let tree = CompositeTree {
            width: 280,
            height: 277,
            space: BlendSpace::Srgb,
            nodes: vec![
                node(NodeContent::Fill([0.1, 0.2, 0.04, 0.4])),
                pass,
                hidden,
                hidden_clip,
            ],
        };
        for level in [0, 1, 3] {
            check(&gpu, &tree, level, TileCoord::new(0, 0));
            check(&gpu, &tree, level, TileCoord::new(1, 1));
        }
        check(&gpu, &tree, 0, TileCoord::new(-1, 0));
    }

    #[test]
    fn every_adjustment_family_and_chain_matches_reference() {
        use emulsion_raster::adjust::{Adjustment, Cube};
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        let mut ops = Vec::new();
        for mut adjustment in Adjustment::catalogue() {
            for param in adjustment.params() {
                let changed =
                    (param.value + (param.max - param.min) * 0.13).clamp(param.min, param.max);
                adjustment.set_param(param.key, changed);
            }
            ops.push(Arc::new(adjustment.prepare()));
        }
        ops.push(Arc::new(Prepared::Cube {
            cube: Cube {
                name: "parity".into(),
                size: 2,
                data: Arc::new(
                    (0..8)
                        .map(|i| {
                            [
                                if i & 1 == 0 { 2000 } else { 60000 },
                                if i & 2 == 0 { 8000 } else { 50000 },
                                if i & 4 == 0 { 0 } else { 65535 },
                            ]
                        })
                        .collect(),
                ),
            },
            strength: 0.71,
        }));
        ops.push(Arc::new(Prepared::Chain(ops[..3].to_vec())));
        let raster = Arc::new(Raster::from_srgba8(
            17,
            13,
            &(0..221)
                .flat_map(|i| {
                    [
                        (i * 13 % 256) as u8,
                        (i * 47 % 256) as u8,
                        (i * 113 % 256) as u8,
                        [0, 81, 177, 255][i % 4],
                    ]
                })
                .collect::<Vec<_>>(),
        ));
        for (index, op) in ops.into_iter().enumerate() {
            for masked in [false, true] {
                let base = node(NodeContent::Pixels {
                    raster: raster.clone(),
                    placement: Placement::default(),
                });
                let mut adjust = node(NodeContent::Adjust(op.clone()));
                if masked {
                    adjust.opacity = 0.63;
                    adjust.mask = Some(Arc::new(Mask::from_fn(17, 13, 0, |x, y| {
                        ((x * 19 + y * 31) % 256) as u8
                    })));
                    adjust.clip_to = Some(0);
                    adjust.blend = BlendMode::SoftLight;
                }
                let tree = CompositeTree {
                    width: 17,
                    height: 13,
                    space: BlendSpace::Srgb,
                    nodes: vec![base, adjust],
                };
                eprintln!("adjustment parity family {index}, masked={masked}");
                check(&gpu, &tree, 0, TileCoord::new(0, 0));
                if op.positional() {
                    check(&gpu, &tree, 2, TileCoord::new(0, 0));
                }
            }
        }
    }

    #[test]
    fn dissolve_matches_reference_with_high_seed_and_mip_coordinates() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        let mut source = node(NodeContent::Fill([0.21, 0.11, 0.09, 0.37]));
        source.id = 0xf1a3_0764_b572_18d9;
        source.opacity = 0.63;
        source.blend = BlendMode::Dissolve;
        let tree = CompositeTree {
            width: 1031,
            height: 777,
            space: BlendSpace::Linear,
            nodes: vec![node(NodeContent::Fill([0.1, 0.2, 0.3, 0.6])), source],
        };
        for level in [0, 1, 2] {
            check(&gpu, &tree, level, TileCoord::new(1, 0));
        }
    }

    /// End-to-end timings include exact source sampling, uploads and synchronous
    /// readback. Run alone on an otherwise idle hardware GPU in release mode.
    #[test]
    #[ignore = "manual end-to-end compositor performance measurement"]
    fn benchmark_tile_compositing() {
        use emulsion_raster::adjust::Adjustment;
        use std::time::{Duration, Instant};
        let gpu = crate::test_gpu().expect("benchmark requires GPU");
        eprintln!("compositor benchmark device: {}", gpu.name());
        for edge in [256u32, 1024] {
            let bytes: Vec<u8> = (0..edge * edge)
                .flat_map(|i| {
                    let x = i % edge;
                    let y = i / edge;
                    [
                        (x * 13 + y * 5) as u8,
                        (x * 3 + y * 23) as u8,
                        (x * 11 + y * 17) as u8,
                        (127 + (x + y) % 129) as u8,
                    ]
                })
                .collect();
            let raster = Arc::new(Raster::from_srgba8(edge, edge, &bytes));
            let tiles: Vec<_> = (0..edge / 256)
                .flat_map(|y| (0..edge / 256).map(move |x| TileCoord::new(x as i32, y as i32)))
                .collect();
            for scenario in [
                "one-layer",
                "eight-normal",
                "eight-blends",
                "eight-adjustments",
                "two-blends",
                "eight-one-blend",
                "one-lut",
                "one-hsl",
            ] {
                if std::env::var("EMULSION_COMPOSITOR_BENCH_CASES")
                    .is_ok_and(|cases| !cases.split(',').any(|case| case == scenario))
                {
                    continue;
                }
                let layer_count = if matches!(
                    scenario,
                    "one-layer" | "eight-adjustments" | "one-lut" | "one-hsl"
                ) {
                    1
                } else if scenario == "two-blends" {
                    2
                } else {
                    8
                };
                let mut nodes: Vec<_> = (0..layer_count)
                    .map(|i| {
                        let mut layer = node(NodeContent::Pixels {
                            raster: raster.clone(),
                            placement: Placement::at(f64::from(i) * 1.25, f64::from(i) * 0.75),
                        });
                        layer.opacity = if i == 0 { 1.0 } else { 0.53 };
                        if scenario == "eight-blends" {
                            layer.blend = [
                                BlendMode::Normal,
                                BlendMode::Multiply,
                                BlendMode::Overlay,
                                BlendMode::SoftLight,
                                BlendMode::ColorBurn,
                                BlendMode::Hue,
                                BlendMode::Screen,
                                BlendMode::Difference,
                            ][i as usize];
                        }
                        if (scenario == "two-blends" && i == 1)
                            || (scenario == "eight-one-blend" && i == 7)
                        {
                            layer.blend = BlendMode::Multiply;
                        }
                        layer
                    })
                    .collect();
                if matches!(scenario, "eight-adjustments" | "one-lut" | "one-hsl") {
                    for i in 0..if scenario == "eight-adjustments" {
                        8
                    } else {
                        1
                    } {
                        let i = if scenario == "one-hsl" { 2 } else { i };
                        let op = match i % 4 {
                            0 => Adjustment::Exposure {
                                exposure: 0.15,
                                offset: 0.0,
                                gamma: 1.0,
                            }
                            .prepare(),
                            1 => Adjustment::BrightnessContrast {
                                brightness: -3.0,
                                contrast: 7.0,
                            }
                            .prepare(),
                            2 => Adjustment::HueSaturation {
                                hue: 7.0,
                                saturation: 9.0,
                                lightness: 0.0,
                            }
                            .prepare(),
                            _ => Adjustment::Vibrance {
                                vibrance: 8.0,
                                saturation: -5.0,
                            }
                            .prepare(),
                        };
                        nodes.push(node(NodeContent::Adjust(Arc::new(op))));
                    }
                }
                let tree = CompositeTree {
                    width: edge,
                    height: edge,
                    space: BlendSpace::Srgb,
                    nodes,
                };
                // Warm the shader and validate every tile before measuring.
                for tile in &tiles {
                    check(&gpu, &tree, 0, *tile);
                }
                let mut cpu = Vec::new();
                let mut hardware = Vec::new();
                let measure = |hardware: bool| {
                    let start = Instant::now();
                    for tile in &tiles {
                        let pixels = if hardware {
                            render_tile_gpu(&gpu, &tree, 0, *tile).unwrap().unwrap()
                        } else {
                            composite::render_tile_cpu(&tree, 0, *tile)
                        };
                        std::hint::black_box(pixels);
                    }
                    start.elapsed()
                };
                let before = gpu.dispatch_count();
                // Alternate order to reduce temperature/order bias.
                for i in 0..4 {
                    if i % 2 == 0 {
                        cpu.push(measure(false));
                        hardware.push(measure(true));
                    } else {
                        hardware.push(measure(true));
                        cpu.push(measure(false));
                    }
                }
                cpu.sort();
                hardware.sort();
                let median = |v: &[Duration]| (v[1] + v[2]).as_secs_f64() * 500.0;
                let cpu_ms = median(&cpu);
                let gpu_ms = median(&hardware);
                assert_eq!(
                    gpu.dispatch_count() - before,
                    4 * tiles.len() as u64,
                    "all timed GPU tiles must dispatch"
                );
                eprintln!(
                    "COMPOSITOR {scenario} tiles={} CPU_ms={cpu_ms:.3} GPU_ms={gpu_ms:.3} GPU_over_CPU={:.3}",
                    tiles.len(),
                    gpu_ms / cpu_ms
                );
            }
        }
    }

    #[test]
    fn automatic_routing_matches_measured_workloads() {
        use emulsion_raster::adjust::Adjustment;
        let tree = |nodes| CompositeTree {
            width: 256,
            height: 256,
            space: BlendSpace::Srgb,
            nodes,
        };
        let fill = || node(NodeContent::Fill([0.1, 0.2, 0.3, 0.5]));
        assert!(!worthwhile(&tree(vec![fill()])));
        assert!(!worthwhile(&tree((0..8).map(|_| fill()).collect())));
        let lut = Adjustment::Exposure {
            exposure: 0.5,
            offset: 0.0,
            gamma: 1.0,
        }
        .prepare();
        assert!(!worthwhile(&tree(vec![
            fill(),
            node(NodeContent::Adjust(Arc::new(lut)))
        ])));
        let hsl = || {
            node(NodeContent::Adjust(Arc::new(Prepared::HueSat {
                hue: 0.1,
                sat: 0.2,
                light: 0.0,
            })))
        };
        assert!(worthwhile(&tree(vec![fill(), hsl()])));
        assert!(!worthwhile(&tree(vec![hsl()]))); // no backdrop to adjust
        let mut multiply = fill();
        multiply.blend = BlendMode::Multiply;
        let mut pair = tree(vec![fill(), multiply]);
        assert!(worthwhile(&pair));
        pair.space = BlendSpace::Linear;
        assert!(!worthwhile(&pair)); // linear Multiply has no measured gain
        let mut sparse: Vec<_> = (0..8).map(|_| fill()).collect();
        sparse[7].blend = BlendMode::Multiply;
        assert!(!worthwhile(&tree(sparse)));
        let mut dense: Vec<_> = (0..8).map(|_| fill()).collect();
        for item in dense.iter_mut().skip(1) {
            item.blend = BlendMode::Overlay;
        }
        assert!(worthwhile(&tree(dense)));
    }

    #[test]
    fn automatic_routing_ignores_hidden_and_zero_coverage_expensive_nodes() {
        let base = node(NodeContent::Fill([0.2; 4]));
        let mut hidden = node(NodeContent::Adjust(Arc::new(Prepared::Vibrance {
            vib: 0.2,
            sat: 0.1,
        })));
        hidden.visible = false;
        let mut zero = node(NodeContent::Adjust(Arc::new(Prepared::HueSat {
            hue: 0.1,
            sat: 0.2,
            light: 0.0,
        })));
        zero.opacity = 0.0;
        let mut clipped = node(NodeContent::Fill([0.4; 4]));
        clipped.blend = BlendMode::Screen;
        clipped.clip_to = Some(1);
        assert!(!worthwhile(&CompositeTree {
            width: 256,
            height: 256,
            space: BlendSpace::Srgb,
            nodes: vec![base, hidden, zero, clipped]
        }));
    }

    #[test]
    fn selective_color_declines_gpu_including_fused_chains() {
        let op = Arc::new(
            emulsion_raster::adjust::Adjustment::selective_color_saturation_check().prepare(),
        );
        assert!(!supported_adjust(&op, 0));
        assert!(!supported_adjust(&Prepared::Chain(vec![op.clone()]), 0));
        assert!(!supported(&[node(NodeContent::Adjust(op))], 0, &mut 0));
    }

    #[test]
    fn malformed_adjustments_and_resource_limits_decline_without_dispatch() {
        let mut dissolve = node(NodeContent::Fill([0.3; 4]));
        dissolve.blend = BlendMode::Dissolve;
        assert!(supported(&[dissolve], 0, &mut 0));
        assert!(!supported_adjust(
            &Prepared::Lut(Box::new([vec![0.0], vec![0.0], vec![0.0]])),
            0
        ));
        let many: Vec<_> = (0..=MAX_NODES)
            .map(|_| node(NodeContent::Fill([0.0; 4])))
            .collect();
        assert!(!supported(&many, 0, &mut 0));
        let mut nested = vec![node(NodeContent::Fill([0.1; 4]))];
        for _ in 0..MAX_DEPTH {
            nested = vec![node(NodeContent::Group(nested))];
        }
        assert!(!supported(&nested, 0, &mut 0));
    }
}
