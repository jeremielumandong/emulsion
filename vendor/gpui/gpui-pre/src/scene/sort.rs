// Modified by Emulsion; adapted from gpui-fast 8111e627725c1868930141bfba3c1663acc8e978.
// SPDX-License-Identifier: Apache-2.0
// Apache-2.0; original attribution is preserved in this package's LICENSE-APACHE.
//! Sort compact cached keys, then permute primitives without cloning them.
//! Already ordered sprites retain their original sequence rather than paying
//! for atlas regrouping whose renderer benefit has not yet been measured.

use super::{MonochromeSprite, PolychromeSprite, Quad, Scene, Shadow, SubpixelSprite, Underline};
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Record<const N: usize> {
    // Lexicographic keys, followed by the original position for stable ties.
    words: [u32; N],
}

#[derive(Default)]
struct Buffers<const N: usize> {
    order: Vec<Record<N>>,
    radix: Vec<Record<N>>,
}

#[derive(Default)]
pub(super) struct SortScratch {
    primitives: Buffers<2>,
    sprites: Buffers<4>,
}

fn sort_by_cached_key<T: Copy, const N: usize>(
    items: &mut [T],
    buffers: &mut Buffers<N>,
    key: impl Fn(&T) -> Record<N>,
) {
    debug_assert!(
        N == 2 || N == 4,
        "supported primitive and sprite record layouts"
    );
    if items.is_sorted_by_key(&key) {
        return;
    }
    // Reverse descending runs directly, restoring each equal-key run so the
    // result retains its original paint order without integer scratch buffers.
    let mut has_equal = false;
    if items.is_sorted_by(|a, b| {
        let a = key(a);
        let b = key(b);
        has_equal |= a == b;
        a >= b
    }) {
        items.reverse();
        if has_equal {
            let mut start = 0;
            while start < items.len() {
                let run_key = key(&items[start]);
                let mut end = start + 1;
                while end < items.len() && key(&items[end]) == run_key {
                    end += 1;
                }
                items[start..end].reverse();
                start = end;
            }
        }
        return;
    }
    if items.len() > u32::MAX as usize {
        items.sort_by_key(key);
        return;
    }
    let Buffers { order, radix } = buffers;
    order.clear();
    order.extend(items.iter().enumerate().map(|(index, item)| {
        let mut record = key(item);
        record.words[N - 1] = index as u32;
        record
    }));
    if order.len() < 128 {
        order.sort_unstable();
    } else {
        // Stable byte-wise counting passes exploit the narrow range of
        // orders/atlas IDs. Constant bytes need no pass. Original positions
        // start ordered, so stability removes the need to sort the position field.
        let mut varying = [0_u32; N];
        for record in order.iter() {
            for (word, bits) in varying[..N - 1].iter_mut().enumerate() {
                *bits |= record.words[word] ^ order[0].words[word];
            }
        }
        let passes: usize = varying[..N - 1]
            .iter()
            .map(|bits| bits.to_le_bytes().iter().filter(|&&byte| byte != 0).count())
            .sum();
        // Several byte passes and their histograms cost more than comparing
        // cached records on small arrays with wide, sparsely distributed IDs.
        if order.len() < 1024 && passes > 3 {
            order.sort_unstable();
        } else {
            radix.resize(order.len(), Record { words: [0; N] });
            // The two record layouts have one or three key words. Keeping the
            // word index constant removes indexed field loads in the hot loops.
            if N == 4 {
                radix_word::<N, 2>(order, radix, varying[2]);
                radix_word::<N, 1>(order, radix, varying[1]);
            }
            radix_word::<N, 0>(order, radix, varying[0]);
        }
    }
    // Each record names the original source of the item for this destination.
    // Resolve permutation cycles in place; setting source == destination marks
    // a visited slot. This holds no primitive or texture in persistent scratch.
    for start in 0..items.len() {
        if order[start].words[N - 1] as usize == start {
            continue;
        }
        let saved = items[start];
        let mut current = start;
        loop {
            let next = order[current].words[N - 1] as usize;
            order[current].words[N - 1] = current as u32;
            if next == start {
                items[current] = saved;
                break;
            }
            items[current] = items[next];
            current = next;
        }
    }
}

// Read the selected 32-bit field directly; no variable double-word shifts.
#[inline]
fn radix_word<const N: usize, const WORD: usize>(
    order: &mut Vec<Record<N>>,
    radix: &mut Vec<Record<N>>,
    varying: u32,
) {
    for shift in (0..32).step_by(8) {
        let mask = (varying >> shift) & 255;
        if mask == 0 {
            continue;
        }
        // Removing shared constant bits preserves the order of the keys.
        let mut offsets = [0_usize; 256];
        for &record in order.iter() {
            offsets[((record.words[WORD] >> shift) & mask) as usize] += 1;
        }
        let mut total = 0;
        for offset in &mut offsets[..=mask as usize] {
            let count = *offset;
            *offset = total;
            total += count;
        }
        for &record in order.iter() {
            let offset = &mut offsets[((record.words[WORD] >> shift) & mask) as usize];
            radix[*offset] = record;
            *offset += 1;
        }
        std::mem::swap(order, radix);
    }
}

impl Scene {
    pub(super) fn sort_in_drawing_order(&mut self) {
        macro_rules! primitive {
            ($field:ident, $ty:ty) => {
                sort_by_cached_key(
                    &mut self.$field,
                    &mut self.sort_scratch.primitives,
                    |item: &$ty| Record {
                        words: [item.order, 0],
                    },
                );
            };
        }
        primitive!(shadows, Shadow);
        primitive!(quads, Quad);
        primitive!(underlines, Underline);
        macro_rules! sprites {
            ($field:ident, $ty:ty) => {
                if !self
                    .$field
                    .is_sorted_by_key(|sprite| (sprite.order, sprite.tile.tile_id))
                {
                    sort_by_cached_key(
                        &mut self.$field,
                        &mut self.sort_scratch.sprites,
                        |sprite: &$ty| Record {
                            words: [
                                sprite.order,
                                sprite.tile.texture_id.index,
                                sprite.tile.tile_id.0,
                                0,
                            ],
                        },
                    );
                }
            };
        }
        sprites!(monochrome_sprites, MonochromeSprite);
        sprites!(subpixel_sprites, SubpixelSprite);
        sprites!(polychrome_sprites, PolychromeSprite);
        self.paths.sort_by_key(|path| path.order);
        self.surfaces.sort_by_key(|surface| surface.order);
    }
}
