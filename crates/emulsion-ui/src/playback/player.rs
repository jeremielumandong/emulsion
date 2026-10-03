//! The player engine: advances the transport by a clock at the document's
//! frame rate. Each tick shows the frame for the current time, skipping
//! (dropping) frames a slow display missed instead of falling behind; the
//! play range bounds playback and loops when looping is on.
use super::Transport;
use super::clock::{Timebase, frame_at};
use emulsion_core::timeline::FrameRate;

/// What a tick asks the display to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tick {
    /// Not playing.
    Idle,
    /// The same frame is still showing.
    Hold,
    /// Show this frame.
    Show(u64),
    /// Playback reached the end of the range and stopped.
    Ended,
}

/// One run of playback.
pub(crate) struct Player {
    rate: FrameRate,
    /// The frame the clock started at.
    origin: u64,
    /// The frame shown last.
    last: u64,
    /// Frames skipped to keep up with the clock.
    pub dropped: u64,
}

impl Player {
    /// Start playing at the playhead (at the start of the play range when
    /// the playhead is outside it or on its last frame).
    pub fn start(
        transport: &mut Transport,
        total: u64,
        rate: FrameRate,
        clock: &mut dyn Timebase,
    ) -> Self {
        transport.frame = transport.start_frame(total);
        transport.playing = true;
        clock.start(transport.frame, rate);
        Self {
            rate,
            origin: transport.frame,
            last: transport.frame,
            dropped: 0,
        }
    }

    /// Move the transport to the frame for the clock's time.
    pub fn tick(
        &mut self,
        transport: &mut Transport,
        total: u64,
        clock: &mut dyn Timebase,
    ) -> Tick {
        if !transport.playing {
            return Tick::Idle;
        }
        // The playhead was moved while playing: carry on from there.
        if transport.frame != self.last {
            self.restart(transport.frame, clock);
            return Tick::Show(transport.frame);
        }
        let (start, end) = transport.bounds(total);
        let target = frame_at(self.rate, self.origin, clock.elapsed());
        if target >= end {
            if transport.looping {
                transport.frame = start;
                self.restart(start, clock);
                return Tick::Show(start);
            }
            transport.frame = end - 1;
            transport.playing = false;
            self.last = transport.frame;
            clock.stop();
            return Tick::Ended;
        }
        if target <= self.last {
            return Tick::Hold;
        }
        self.dropped += target - self.last - 1;
        self.last = target;
        transport.frame = target;
        Tick::Show(target)
    }

    fn restart(&mut self, frame: u64, clock: &mut dyn Timebase) {
        self.origin = frame;
        self.last = frame;
        clock.start(frame, self.rate);
    }

    /// Pause where playback is.
    pub fn pause(transport: &mut Transport, clock: &mut dyn Timebase) {
        transport.playing = false;
        clock.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::clock::FakeClock;

    const FPS: f64 = 24.;

    fn frames(n: f64) -> f64 {
        n / FPS
    }

    #[test]
    fn playback_follows_the_clock_and_drops_frames_it_missed() {
        let mut clock = FakeClock::default();
        let mut t = Transport::default();
        let rate = FrameRate::whole(24);
        let mut player = Player::start(&mut t, 100, rate, &mut clock);
        assert!(t.playing);
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Hold);
        clock.advance(frames(1.));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(1));
        // A slow frame: the display skips to where the clock is.
        clock.advance(frames(4.5));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(5));
        assert_eq!(player.dropped, 3);
        assert_eq!(t.frame, 5);
        // Ten seconds of ticks never drift from the clock.
        for _ in 0..240 {
            clock.advance(frames(1.));
            player.tick(&mut t, 1000, &mut clock);
        }
        assert_eq!(t.frame, frame_at(rate, 0, clock.elapsed()));
    }

    #[test]
    fn playback_stops_at_the_end_of_the_range_or_loops() {
        let mut clock = FakeClock::default();
        let rate = FrameRate::whole(24);
        let mut t = Transport {
            frame: 50,
            range: Some((10, 20)),
            ..Transport::default()
        };
        let mut player = Player::start(&mut t, 100, rate, &mut clock);
        assert_eq!(t.frame, 10, "playing starts inside the range");
        clock.advance(frames(9.));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(19));
        clock.advance(frames(1.));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Ended);
        assert_eq!((t.frame, t.playing), (19, false));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Idle);

        // Playing again from the last frame starts over; looping wraps.
        t.looping = true;
        let mut player = Player::start(&mut t, 100, rate, &mut clock);
        assert_eq!(t.frame, 10);
        clock.advance(frames(12.));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(10));
        assert!(t.playing);
        clock.advance(frames(2.));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(12));
        assert_eq!(*clock.starts.borrow(), [10, 10, 10]);
    }

    #[test]
    fn moving_the_playhead_while_playing_carries_on_from_there() {
        let mut clock = FakeClock::default();
        let rate = FrameRate::whole(24);
        let mut t = Transport::default();
        let mut player = Player::start(&mut t, 100, rate, &mut clock);
        clock.advance(frames(3.));
        player.tick(&mut t, 100, &mut clock);
        t.frame = 60;
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(60));
        clock.advance(frames(2.));
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Show(62));
        Player::pause(&mut t, &mut clock);
        assert_eq!(player.tick(&mut t, 100, &mut clock), Tick::Idle);
        assert_eq!(t.frame, 62);
    }
}
