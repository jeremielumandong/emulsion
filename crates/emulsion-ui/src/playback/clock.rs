//! What playback time is measured against: the audio device while sound
//! plays (so pictures follow what is heard), a monotonic clock otherwise,
//! and a hand-set clock for tests.
use emulsion_core::timeline::FrameRate;
use std::time::Instant;
#[cfg(test)]
use std::{cell::Cell, rc::Rc};

/// A clock that playback reads. `start` begins counting from a frame (and
/// starts any sound there); `elapsed` is the time since, in seconds.
pub(crate) trait Timebase {
    fn start(&mut self, frame: u64, rate: FrameRate);
    fn elapsed(&self) -> f64;
    fn stop(&mut self);
}

/// The frame showing `seconds` after `origin`. A frame shows for its whole
/// duration, so this rounds down (`FrameRate::seconds_to_frames` rounds to
/// the nearest frame instead).
pub(crate) fn frame_at(rate: FrameRate, origin: u64, seconds: f64) -> u64 {
    let frames = seconds.max(0.) * f64::from(rate.num) / f64::from(rate.den);
    // A hair of tolerance so exact frame times land on their frame.
    origin + (frames + 1e-9).floor() as u64
}

/// Silent playback: a monotonic clock.
#[derive(Default)]
pub(crate) struct SystemClock {
    started: Option<Instant>,
}

impl Timebase for SystemClock {
    fn start(&mut self, _: u64, _: FrameRate) {
        self.started = Some(Instant::now());
    }
    fn elapsed(&self) -> f64 {
        self.started.map_or(0., |s| s.elapsed().as_secs_f64())
    }
    fn stop(&mut self) {
        self.started = None;
    }
}

/// A clock moved by hand, for tests. Clones share the time.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct FakeClock {
    pub now: Rc<Cell<f64>>,
    started: Rc<Cell<Option<f64>>>,
    /// Frames `start` was called with, in order.
    pub starts: Rc<std::cell::RefCell<Vec<u64>>>,
}

#[cfg(test)]
impl FakeClock {
    pub fn advance(&self, seconds: f64) {
        self.now.set(self.now.get() + seconds);
    }
}

#[cfg(test)]
impl Timebase for FakeClock {
    fn start(&mut self, frame: u64, _: FrameRate) {
        self.started.set(Some(self.now.get()));
        self.starts.borrow_mut().push(frame);
    }
    fn elapsed(&self) -> f64 {
        self.started.get().map_or(0., |s| self.now.get() - s)
    }
    fn stop(&mut self) {
        self.started.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_the_clock_at_whole_and_ntsc_rates() {
        let r24 = FrameRate::whole(24);
        assert_eq!(frame_at(r24, 0, 0.), 0);
        assert_eq!(frame_at(r24, 0, 0.99 / 24.), 0);
        assert_eq!(frame_at(r24, 0, 1. / 24.), 1);
        assert_eq!(frame_at(r24, 10, 1.), 34);
        assert_eq!(frame_at(r24, 0, -1.), 0);
        let ntsc = FrameRate::ntsc(30);
        // 29.97 fps: one minute is 1798.2 frames.
        assert_eq!(frame_at(ntsc, 0, 60.), 1798);
        // Round trip through the shared timing maths.
        for f in [0, 1, 1799, 123_456] {
            assert_eq!(frame_at(ntsc, 0, ntsc.frames_to_seconds(f)), f);
        }
    }

    #[test]
    fn the_fake_clock_counts_from_each_start() {
        let mut clock = FakeClock::default();
        assert_eq!(clock.elapsed(), 0.);
        clock.start(5, FrameRate::whole(24));
        clock.advance(0.5);
        assert_eq!(clock.elapsed(), 0.5);
        clock.start(0, FrameRate::whole(24));
        assert_eq!(clock.elapsed(), 0.);
        assert_eq!(*clock.starts.borrow(), [5, 0]);
        let mut system = SystemClock::default();
        system.start(0, FrameRate::whole(24));
        assert!(system.elapsed() >= 0.);
        system.stop();
        assert_eq!(system.elapsed(), 0.);
    }
}
