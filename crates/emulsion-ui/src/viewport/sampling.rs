//! Reuse tile bytes while adjacent screen pixels sample the same source tile.
use emulsion_raster::TILE;

#[derive(Default)]
pub(super) struct RowSampler<'a> {
    key: Option<(i32, i32, bool)>,
    bytes: Option<&'a [u8]>,
}

impl<'a> RowSampler<'a> {
    #[inline]
    pub(super) fn pixel(
        &mut self,
        x: i64,
        y: i64,
        before: bool,
        lookup: impl FnOnce(i32, i32, bool) -> Option<&'a [u8]>,
    ) -> Option<&'a [u8]> {
        let t = TILE as i64;
        let key = ((x / t) as i32, (y / t) as i32, before);
        if self.key != Some(key) {
            self.bytes = lookup(key.0, key.1, key.2);
            self.key = Some(key);
        }
        let index = (((y % t) * t + x % t) * 4) as usize;
        self.bytes.map(|bytes| &bytes[index..index + 4])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn cached_tiles_missing_tiles_and_compare_wipe_match_direct_samples() {
        let current: Vec<u8> = (0..TILE as usize * TILE as usize * 4)
            .map(|i| (i % 251) as u8)
            .collect();
        let before: Vec<u8> = current.iter().map(|v| 255 - v).collect();
        let calls = Cell::new(0);
        let lookup = |x, y, old| {
            calls.set(calls.get() + 1);
            (x == 0 && y == 0).then_some(if old {
                before.as_slice()
            } else {
                current.as_slice()
            })
        };
        let mut row = RowSampler::default();
        for old in [false, true] {
            for x in 0..256 {
                let actual = row.pixel(x, 7, old, lookup).unwrap();
                let bytes = if old { &before } else { &current };
                let index = (7 * 256 + x as usize) * 4;
                assert_eq!(actual, &bytes[index..index + 4]);
            }
        }
        assert_eq!(calls.get(), 2);
        for x in 256..512 {
            assert!(row.pixel(x, 7, false, lookup).is_none());
        }
        assert_eq!(calls.get(), 3, "missing tiles are reused too");
        assert_eq!(
            row.pixel(0, 7, false, lookup),
            Some(&current[7 * 256 * 4..7 * 256 * 4 + 4])
        );
        assert_eq!(calls.get(), 4);
    }
}
