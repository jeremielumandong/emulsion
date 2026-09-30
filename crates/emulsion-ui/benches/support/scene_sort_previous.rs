// Modified by Emulsion; adapted from gpui-fast 8111e627725c1868930141bfba3c1663acc8e978.
// SPDX-License-Identifier: Apache-2.0
// Apache-2.0; original attribution is preserved in vendor/gpui/gpui-pre/LICENSE-APACHE.
//! Sort compact cached keys, then permute primitives without cloning them.
//! Already ordered sprites retain their original sequence rather than paying
//! for atlas regrouping whose renderer benefit has not yet been measured.

use gpui_kit::{
    MonochromeSprite, PolychromeSprite, Quad, Scene, Shadow, SubpixelSprite, Underline,
};
#[derive(Default)]
pub(crate) struct SortScratch {
    // High 96 bits: draw order, atlas index, tile ID. Low 32: original position.
    order: Vec<u128>,
    radix: Vec<u128>,
}

fn sort_by_cached_key<T>(
    items: &mut [T],
    order: &mut Vec<u128>,
    radix: &mut Vec<u128>,
    key: impl Fn(&T) -> u128,
) {
    if items.is_sorted_by_key(&key) {
        return;
    }
    // Strict reversal is stable: there are no equal keys to exchange.
    if items.is_sorted_by(|a, b| key(a) > key(b)) {
        items.reverse();
        return;
    }
    if items.len() > u32::MAX as usize {
        items.sort_by_key(key);
        return;
    }
    order.clear();
    order.extend(
        items
            .iter()
            .enumerate()
            .map(|(index, item)| key(item) | index as u128),
    );
    if order.len() < 128 {
        order.sort_unstable();
    } else {
        // Stable byte-wise counting passes exploit the narrow range of
        // orders/atlas IDs. Constant bytes need no pass. Original positions
        // start ordered, so stability removes the need to sort the low 32 bits.
        let first = order[0];
        let varying = order.iter().fold(0, |bits, record| bits | (record ^ first));
        radix.resize(order.len(), 0);
        for shift in (32..128).step_by(8) {
            let mask = (varying >> shift) & 255;
            if mask == 0 {
                continue;
            }
            // Constant bits are shared by every key, so removing them keeps
            // order and lets narrow key ranges use fewer prefix-sum buckets.
            let mut offsets = [0_usize; 256];
            for &record in order.iter() {
                offsets[((record >> shift) & mask) as usize] += 1;
            }
            let mut total = 0;
            for offset in &mut offsets[..=mask as usize] {
                let count = *offset;
                *offset = total;
                total += count;
            }
            for &record in order.iter() {
                let offset = &mut offsets[((record >> shift) & mask) as usize];
                radix[*offset] = record;
                *offset += 1;
            }
            std::mem::swap(order, radix);
        }
    }
    // Each record names the original source of the item for this destination.
    // Resolve permutation cycles in place; setting source == destination marks
    // a visited slot. This holds no primitive or texture in persistent scratch.
    for start in 0..items.len() {
        let mut current = start;
        loop {
            let next = order[current] as u32 as usize;
            order[current] = current as u128;
            if next == start {
                break;
            }
            items.swap(current, next);
            current = next;
        }
    }
}

fn key(order: u32, texture: u32, tile: u32) -> u128 {
    ((order as u128) << 96) | ((texture as u128) << 64) | ((tile as u128) << 32)
}

pub(crate) fn sort_in_drawing_order(scene: &mut Scene, scratch: &mut SortScratch) {
    macro_rules! sort {
        ($field:ident, $key:expr) => {
            sort_by_cached_key(
                &mut scene.$field,
                &mut scratch.order,
                &mut scratch.radix,
                $key,
            )
        };
    }
    sort!(shadows, |shadow: &Shadow| key(shadow.order, 0, 0));
    sort!(quads, |quad: &Quad| key(quad.order, 0, 0));
    sort!(underlines, |underline: &Underline| key(
        underline.order,
        0,
        0
    ));
    macro_rules! sprites {
        ($field:ident, $ty:ty) => {
            if !scene
                .$field
                .is_sorted_by_key(|sprite| (sprite.order, sprite.tile.tile_id))
            {
                sort!($field, |sprite: &$ty| key(
                    sprite.order,
                    sprite.tile.texture_id.index,
                    sprite.tile.tile_id.0
                ));
            }
        };
    }
    sprites!(monochrome_sprites, MonochromeSprite);
    sprites!(subpixel_sprites, SubpixelSprite);
    sprites!(polychrome_sprites, PolychromeSprite);
    scene.paths.sort_by_key(|path| path.order);
    scene.surfaces.sort_by_key(|surface| surface.order);
}
