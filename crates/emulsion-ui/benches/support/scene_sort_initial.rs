// Modified by Emulsion; adapted from gpui-fast 8111e627725c1868930141bfba3c1663acc8e978.
// SPDX-License-Identifier: Apache-2.0
// Apache-2.0; original attribution is preserved in vendor/gpui/gpui-pre/LICENSE-APACHE.
//! Sort indices before gathering large, copyable primitives into reusable storage.
//! Paths and surfaces keep their stable in-place sort: cloning them would allocate
//! vertex buffers or retain application-owned textures in scratch storage.

use gpui_kit::{
    MonochromeSprite, PolychromeSprite, Quad, Scene, Shadow, SubpixelSprite, Underline,
};
use std::mem;

#[derive(Default)]
pub(crate) struct SortScratch {
    order: Vec<usize>,
    shadows: Vec<Shadow>,
    quads: Vec<Quad>,
    underlines: Vec<Underline>,
    monochrome_sprites: Vec<MonochromeSprite>,
    subpixel_sprites: Vec<SubpixelSprite>,
    polychrome_sprites: Vec<PolychromeSprite>,
}

fn sort_by_gathering<T: Copy, K: Ord>(
    items: &mut Vec<T>,
    order: &mut Vec<usize>,
    gathered: &mut Vec<T>,
    key: impl Fn(&T) -> K,
) {
    // Ordered scenes are common. Detect them before allocating scratch or
    // copying primitives (the original stable sort may allocate even here).
    if items.is_sorted_by_key(&key) {
        return;
    }
    order.clear();
    order.extend(0..items.len());
    // Original position breaks ties, preserving stable paint order.
    order.sort_unstable_by_key(|&index| (key(&items[index]), index));
    gathered.clear();
    gathered.extend(order.iter().map(|&index| items[index]));
    mem::swap(items, gathered);
    gathered.clear();
}

pub(crate) fn sort_in_drawing_order(scene: &mut Scene, scratch: &mut SortScratch) {
    macro_rules! sort {
        ($field:ident, $key:expr) => {
            sort_by_gathering(
                &mut scene.$field,
                &mut scratch.order,
                &mut scratch.$field,
                $key,
            )
        };
    }
    sort!(shadows, |shadow: &Shadow| shadow.order);
    sort!(quads, |quad: &Quad| quad.order);
    sort!(underlines, |underline: &Underline| underline.order);
    sort!(monochrome_sprites, |sprite: &MonochromeSprite| (
        sprite.order,
        sprite.tile.texture_id.index,
        sprite.tile.tile_id
    ));
    sort!(subpixel_sprites, |sprite: &SubpixelSprite| (
        sprite.order,
        sprite.tile.texture_id.index,
        sprite.tile.tile_id
    ));
    sort!(polychrome_sprites, |sprite: &PolychromeSprite| (
        sprite.order,
        sprite.tile.texture_id.index,
        sprite.tile.tile_id
    ));
    scene.paths.sort_by_key(|path| path.order);
    scene.surfaces.sort_by_key(|surface| surface.order);
}
