//! Playing time-based documents: the transport (playhead, play state, play
//! range and loop) shared by the timeline view and the player, the clocks
//! that drive playback, native audio output, microphone recording and the
//! player engine. Neutral: no workspace types; each workspace draws its own
//! frames.

pub(crate) mod audience;
pub(crate) mod audio_out;
pub(crate) mod clock;
pub(crate) mod player;
pub(crate) mod recorder;

/// Where playback is and how it runs. Frames are at the document's frame
/// rate. The timeline view moves the playhead; the player advances it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Transport {
    /// The frame shown.
    pub frame: u64,
    pub playing: bool,
    /// Play only these frames (start inclusive, end exclusive).
    pub range: Option<(u64, u64)>,
    pub looping: bool,
}

impl Transport {
    /// Move the playhead, kept inside `0..total` (and the play range).
    pub fn seek(&mut self, frame: u64, total: u64) {
        let (start, end) = self.range.unwrap_or((0, total));
        let end = end.min(total).max(start + 1);
        self.frame = frame.clamp(start, end - 1);
    }

    /// The frames that play: the play range inside `0..total`, or all of
    /// them. Never empty.
    pub fn bounds(&self, total: u64) -> (u64, u64) {
        let total = total.max(1);
        let (start, end) = self.range.unwrap_or((0, total));
        let start = start.min(total - 1);
        (start, end.clamp(start + 1, total))
    }

    /// Where Play starts: the playhead, or the start of the play range when
    /// the playhead is outside it or on its last frame.
    pub fn start_frame(&self, total: u64) -> u64 {
        let (start, end) = self.bounds(total);
        if self.frame < start || self.frame + 1 >= end {
            start
        } else {
            self.frame
        }
    }

    /// The playhead `delta` frames on, inside `0..total` (stepping may
    /// leave the play range, as in an editor's frame step).
    pub fn stepped(&self, delta: i64, total: u64) -> u64 {
        self.frame
            .saturating_add_signed(delta)
            .min(total.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeking_stays_inside_the_document_and_range() {
        let mut t = Transport::default();
        t.seek(500, 100);
        assert_eq!(t.frame, 99);
        t.range = Some((10, 20));
        t.seek(0, 100);
        assert_eq!(t.frame, 10);
        t.seek(50, 100);
        assert_eq!(t.frame, 19);
    }

    #[test]
    fn bounds_and_steps_stay_inside_the_document() {
        let mut t = Transport::default();
        assert_eq!(t.bounds(100), (0, 100));
        assert_eq!(t.bounds(0), (0, 1));
        t.range = Some((90, 200));
        assert_eq!(t.bounds(100), (90, 100));
        t.range = Some((150, 200));
        assert_eq!(t.bounds(100), (99, 100));
        t.range = Some((10, 20));
        t.frame = 15;
        assert_eq!(t.start_frame(100), 15);
        t.frame = 19;
        assert_eq!(t.start_frame(100), 10, "from the last frame, start over");
        t.frame = 50;
        assert_eq!(t.start_frame(100), 10);
        t.frame = 5;
        assert_eq!(t.stepped(-10, 100), 0);
        assert_eq!(t.stepped(1, 100), 6);
        assert_eq!(t.stepped(500, 100), 99);
    }
}
