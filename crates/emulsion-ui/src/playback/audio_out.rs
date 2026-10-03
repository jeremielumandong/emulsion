//! Native audio output (cpal) and the audio clock playback follows. The
//! device pulls samples from a [`Deck`]; the frames it has taken, less the
//! output latency, say how far playback has got, so pictures follow the
//! sound rather than drifting from it. Everything but [`DeviceOutput`] is
//! plain data, so tests drive a [`Deck`] by hand and never need a device.
use super::clock::Timebase;
use emulsion_core::timeline::FrameRate;
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// A function that mixes `count` stereo sample frames from sample frame
/// `first`, interleaved.
pub(crate) type Render = Box<dyn Fn(u64, usize) -> Result<Vec<f32>, String> + Send>;

/// Blocks mixed ahead of the playhead.
const AHEAD: u64 = 4;

/// Sound for playback: interleaved stereo at `rate`, starting at timeline
/// frame 0, mixed a second at a time by a background thread just ahead of
/// where it is wanted. The audio thread never waits for it: a block that is
/// not ready yet plays as silence while the clock keeps running.
pub(crate) struct Mix {
    pub rate: u32,
    /// Stereo frames of sound.
    pub len: u64,
    /// Frames per block.
    block: u64,
    blocks: Mutex<HashMap<u64, Arc<[f32]>>>,
    want: AtomicU64,
    wake: Condvar,
    sleep: Mutex<()>,
    error: Mutex<Option<String>>,
}

impl Mix {
    /// Sound of `len` frames that `render` mixes on demand.
    pub fn new(rate: u32, len: u64, render: Render) -> Arc<Self> {
        let mix = Arc::new(Self::empty(rate, len));
        let weak = Arc::downgrade(&mix);
        std::thread::Builder::new()
            .name("emulsion-mix".into())
            .spawn(move || feed(&weak, &render))
            .ok();
        mix
    }

    fn empty(rate: u32, len: u64) -> Self {
        Self {
            rate,
            len,
            block: u64::from(rate.max(1)),
            blocks: Mutex::default(),
            want: AtomicU64::new(0),
            wake: Condvar::new(),
            sleep: Mutex::new(()),
            error: Mutex::default(),
        }
    }

    /// Sound already mixed, as one buffer.
    #[cfg(test)]
    pub fn from_samples(rate: u32, samples: &[f32]) -> Arc<Self> {
        let mix = Self::empty(rate, (samples.len() / 2) as u64);
        {
            let mut blocks = mix.blocks.lock();
            for (i, chunk) in samples.chunks(mix.block as usize * 2).enumerate() {
                blocks.insert(i as u64, chunk.into());
            }
        }
        Arc::new(mix)
    }

    /// The sample frame where timeline frame `frame` starts.
    pub fn sample_at(&self, rate: FrameRate, frame: u64) -> u64 {
        let at = emulsion_io::audio::mix::frame_sample(rate, frame);
        (u128::from(at) * u128::from(self.rate) / u128::from(emulsion_io::audio::RATE)) as u64
    }

    /// Mix ahead from sample frame `frame`.
    pub fn want(&self, frame: u64) {
        if self.want.swap(frame, Ordering::Relaxed) / self.block != frame / self.block {
            self.wake.notify_one();
        }
    }

    /// Whether the sound at `frame` is mixed (or there is none there).
    pub fn ready(&self, frame: u64) -> bool {
        frame >= self.len || self.blocks.lock().contains_key(&(frame / self.block))
    }

    /// Why mixing failed, if it did; failed blocks play as silence.
    pub fn error(&self) -> Option<String> {
        self.error.lock().clone()
    }

    /// Block `index`, unless it is not mixed yet or the mixer holds it.
    fn block_at(&self, index: u64) -> Option<Arc<[f32]>> {
        self.blocks.try_lock()?.get(&index).cloned()
    }
}

/// The mixing thread: keep the blocks from the wanted one on mixed, until
/// the sound is dropped.
fn feed(mix: &std::sync::Weak<Mix>, render: &Render) {
    loop {
        let Some(mix) = mix.upgrade() else {
            return;
        };
        let want = mix.want.load(Ordering::Relaxed) / mix.block;
        let last = mix.len.div_ceil(mix.block);
        let missing = {
            let blocks = mix.blocks.lock();
            (want..(want + AHEAD).min(last)).find(|b| !blocks.contains_key(b))
        };
        let Some(index) = missing else {
            let mut sleep = mix.sleep.lock();
            mix.wake
                .wait_for(&mut sleep, std::time::Duration::from_millis(100));
            continue;
        };
        let first = index * mix.block;
        let count = mix.block.min(mix.len - first) as usize;
        let samples = render(first, count).unwrap_or_else(|error| {
            *mix.error.lock() = Some(error);
            vec![0.; count * 2]
        });
        let mut blocks = mix.blocks.lock();
        let want = mix.want.load(Ordering::Relaxed) / mix.block;
        blocks.retain(|b, _| *b + 2 >= want && *b <= want + AHEAD);
        blocks.insert(index, samples.into());
    }
}

/// Samples faded in and out at each end of a scrub grain, against clicks.
const GRAIN_RAMP: f64 = 96.;

/// What the device plays: a stretch of a mix, and how much of it the device
/// has taken. Plays at the device's rate, stepping through the mix at its
/// own.
#[derive(Default)]
pub(crate) struct Deck {
    mix: Option<Arc<Mix>>,
    /// The block in hand, to read without locking.
    current: Option<(u64, Arc<[f32]>)>,
    /// Mix frames: where the stretch starts, the read position and the end.
    start: f64,
    pos: f64,
    end: f64,
    /// Mix frames per device frame.
    step: f64,
    /// Playback (as opposed to a scrub grain) runs the clock.
    clock: bool,
    /// Device frames delivered since `play`.
    delivered: u64,
    /// Frames in the last request, and when it came.
    chunk: u64,
    at: Option<Instant>,
    /// Seconds from a request to its sound leaving the speakers.
    latency: f64,
    rate: u32,
}

impl Deck {
    #[cfg(test)]
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            ..Self::default()
        }
    }

    /// Play `mix` from mix frame `from` until stopped; the clock restarts.
    pub fn play(&mut self, mix: Arc<Mix>, from: u64) {
        self.end = mix.len as f64;
        self.load(mix, from);
        self.clock = true;
    }

    /// Play `len` mix frames from `from`, faded at both ends.
    pub fn grain(&mut self, mix: Arc<Mix>, from: u64, len: u64) {
        self.end = (from + len).min(mix.len) as f64;
        self.load(mix, from);
        self.clock = false;
    }

    fn load(&mut self, mix: Arc<Mix>, from: u64) {
        mix.want(from);
        self.step = if self.rate == 0 {
            1.
        } else {
            f64::from(mix.rate) / f64::from(self.rate)
        };
        self.mix = Some(mix);
        self.current = None;
        self.start = from as f64;
        self.pos = from as f64;
        self.delivered = 0;
        self.chunk = 0;
        self.at = None;
    }

    pub fn stop(&mut self) {
        self.mix = None;
        self.current = None;
        self.clock = false;
    }

    /// Write the next frames into `out` (interleaved, `channels` wide):
    /// left and right on the first two channels, their average on a mono
    /// device, silence elsewhere and once the stretch ends.
    pub fn fill(&mut self, out: &mut [f32], channels: usize, now: Option<Instant>, latency: f64) {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        for frame in out.chunks_mut(channels) {
            let (l, r) = self.next();
            match frame {
                [mono] => *mono = (l + r) / 2.,
                [a, b, rest @ ..] => {
                    *a = l;
                    *b = r;
                    rest.fill(0.);
                }
                [] => {}
            }
        }
        if self.clock {
            self.delivered += frames as u64;
            self.chunk = frames as u64;
            self.at = now;
            self.latency = latency;
        }
    }

    fn next(&mut self) -> (f32, f32) {
        if self.mix.is_none() {
            return (0., 0.);
        }
        if self.pos >= self.end {
            if !self.clock {
                self.stop();
            }
            return (0., 0.);
        }
        let i = self.pos.floor() as u64;
        let t = (self.pos - i as f64) as f32;
        let (mut l, mut r) = self.frame(i);
        if t > 0. {
            let (l1, r1) = self.frame(i + 1);
            l += (l1 - l) * t;
            r += (r1 - r) * t;
        }
        let gain = if self.clock {
            1.
        } else {
            ((self.pos - self.start).min(self.end - self.pos) / GRAIN_RAMP).min(1.) as f32
        };
        self.pos += self.step;
        (l * gain, r * gain)
    }

    /// Mix frame `i`, or silence while its block is not ready.
    fn frame(&mut self, i: u64) -> (f32, f32) {
        let Some(mix) = &self.mix else {
            return (0., 0.);
        };
        if i >= mix.len {
            return (0., 0.);
        }
        let index = i / mix.block;
        if self.current.as_ref().is_none_or(|(b, _)| *b != index) {
            mix.want(i);
            self.current = mix.block_at(index).map(|block| (index, block));
        }
        match &self.current {
            Some((_, block)) => {
                let at = ((i % mix.block) * 2) as usize;
                (block[at], block[at + 1])
            }
            None => (0., 0.),
        }
    }

    /// Seconds of playback heard: frames delivered less the output latency,
    /// moving smoothly through the last request when `now` is given.
    pub fn elapsed(&self, now: Option<Instant>) -> f64 {
        if self.rate == 0 || !self.clock {
            return 0.;
        }
        let rate = f64::from(self.rate);
        let chunk = self.chunk as f64 / rate;
        let before = (self.delivered - self.chunk) as f64 / rate;
        let into = match (self.at, now) {
            (Some(at), Some(now)) => now.saturating_duration_since(at).as_secs_f64().min(chunk),
            _ => chunk,
        };
        (before + into - self.latency).max(0.)
    }
}

/// Somewhere sound goes.
pub(crate) trait Output {
    fn play(&self, mix: Arc<Mix>, from: u64);
    fn grain(&self, mix: Arc<Mix>, from: u64, len: u64);
    fn stop(&self);
    /// Seconds heard since `play`.
    fn elapsed(&self) -> f64;
}

/// A deck the caller pulls frames from, standing in for a device in tests.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct FakeOutput {
    pub deck: Arc<Mutex<Deck>>,
}

#[cfg(test)]
impl FakeOutput {
    pub fn new(rate: u32) -> Self {
        Self {
            deck: Arc::new(Mutex::new(Deck::new(rate))),
        }
    }
    /// What the deck holds: `Some(true)` playing, `Some(false)` a scrub
    /// grain, `None` nothing.
    pub fn state(&self) -> Option<bool> {
        let deck = self.deck.lock();
        deck.mix.is_some().then_some(deck.clock)
    }
    /// The device takes `frames` stereo frames; returns them.
    pub fn pull(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.; frames * 2];
        self.deck.lock().fill(&mut out, 2, None, 0.);
        out
    }
}

#[cfg(test)]
impl Output for FakeOutput {
    fn play(&self, mix: Arc<Mix>, from: u64) {
        self.deck.lock().play(mix, from);
    }
    fn grain(&self, mix: Arc<Mix>, from: u64, len: u64) {
        self.deck.lock().grain(mix, from, len);
    }
    fn stop(&self) {
        self.deck.lock().stop();
    }
    fn elapsed(&self) -> f64 {
        self.deck.lock().elapsed(None)
    }
}

/// The system's default output device. Its stream lives on its own thread
/// (streams are not `Send` everywhere) until this is dropped.
pub(crate) struct DeviceOutput {
    deck: Arc<Mutex<Deck>>,
    _alive: std::sync::mpsc::Sender<()>,
}

impl DeviceOutput {
    /// Open the default output device, or say why there is none.
    pub fn open() -> Result<Self, String> {
        let deck = Arc::new(Mutex::new(Deck::default()));
        let (opened, result) = std::sync::mpsc::channel();
        let (alive, until) = std::sync::mpsc::channel::<()>();
        let shared = deck.clone();
        std::thread::Builder::new()
            .name("emulsion-audio".into())
            .spawn(move || match start_stream(shared) {
                Ok((stream, rate)) => {
                    opened.send(Ok(rate)).ok();
                    // Keep the stream until the output is dropped.
                    until.recv().ok();
                    drop(stream);
                }
                Err(error) => {
                    opened.send(Err(error)).ok();
                }
            })
            .map_err(|e| e.to_string())?;
        let rate = result
            .recv()
            .map_err(|_| "The audio device stopped while opening.".to_string())??;
        deck.lock().rate = rate;
        Ok(Self {
            deck,
            _alive: alive,
        })
    }
}

impl Output for DeviceOutput {
    fn play(&self, mix: Arc<Mix>, from: u64) {
        self.deck.lock().play(mix, from);
    }
    fn grain(&self, mix: Arc<Mix>, from: u64, len: u64) {
        self.deck.lock().grain(mix, from, len);
    }
    fn stop(&self) {
        self.deck.lock().stop();
    }
    fn elapsed(&self) -> f64 {
        self.deck.lock().elapsed(Some(Instant::now()))
    }
}

fn start_stream(deck: Arc<Mutex<Deck>>) -> Result<(cpal::Stream, u32), String> {
    use cpal::SampleFormat as F;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("No audio output device.")?;
    // The mix's own rate when the device has it, so nothing is resampled.
    let preferred = device
        .supported_output_configs()
        .ok()
        .and_then(|mut configs| {
            configs.find_map(|c| {
                (c.sample_format() == F::F32 && c.channels() >= 2)
                    .then(|| c.try_with_sample_rate(emulsion_io::audio::RATE))
                    .flatten()
            })
        });
    let supported = match preferred {
        Some(config) => config,
        None => device.default_output_config().map_err(|e| e.to_string())?,
    };
    let config = supported.config();
    let stream = match supported.sample_format() {
        F::F32 => build::<f32>(&device, &config, deck),
        F::I16 => build::<i16>(&device, &config, deck),
        F::U16 => build::<u16>(&device, &config, deck),
        F::I32 => build::<i32>(&device, &config, deck),
        F::U8 => build::<u8>(&device, &config, deck),
        F::F64 => build::<f64>(&device, &config, deck),
        other => return Err(format!("Unsupported audio sample format {other}.")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, config.sample_rate))
}

fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    deck: Arc<Mutex<Deck>>,
) -> Result<cpal::Stream, String> {
    use cpal::traits::DeviceTrait;
    let channels = usize::from(config.channels);
    let mut scratch = Vec::<f32>::new();
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let stamp = info.timestamp();
                let latency = stamp.playback.duration_since(stamp.callback).as_secs_f64();
                scratch.resize(data.len(), 0.);
                deck.lock()
                    .fill(&mut scratch, channels, Some(Instant::now()), latency);
                for (out, s) in data.iter_mut().zip(&scratch) {
                    *out = T::from_sample(*s);
                }
            },
            |error| tracing::warn!("audio output: {error}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Playback timed by an output: what it has played is how far playback is.
pub(crate) struct AudioClock {
    pub output: std::rc::Rc<dyn Output>,
    pub mix: Arc<Mix>,
}

impl Timebase for AudioClock {
    fn start(&mut self, frame: u64, rate: FrameRate) {
        let from = self.mix.sample_at(rate, frame);
        self.output.play(self.mix.clone(), from);
    }
    fn elapsed(&self) -> f64 {
        self.output.elapsed()
    }
    fn stop(&mut self) {
        self.output.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize, rate: u32) -> Arc<Mix> {
        let samples: Vec<f32> = (0..frames).flat_map(|i| [i as f32, -(i as f32)]).collect();
        Mix::from_samples(rate, &samples)
    }

    #[test]
    fn the_deck_plays_from_the_cursor_and_counts_what_was_heard() {
        let out = FakeOutput::new(1000);
        let mix = ramp(100, 1000);
        out.play(mix.clone(), 40);
        assert_eq!(out.elapsed(), 0.);
        let got = out.pull(3);
        assert_eq!(got, [40., -40., 41., -41., 42., -42.]);
        assert!((out.elapsed() - 0.003).abs() < 1e-9);
        // Past the end of the sound the clock keeps running, in silence.
        out.pull(100);
        assert!((out.elapsed() - 0.103).abs() < 1e-9);
        assert_eq!(out.pull(1), [0., 0.]);
        out.stop();
        assert_eq!(out.elapsed(), 0.);
    }

    #[test]
    fn a_device_at_another_rate_steps_through_the_mix() {
        let out = FakeOutput::new(2000);
        out.play(ramp(100, 1000), 10);
        assert_eq!(out.pull(3), [10., -10., 10.5, -10.5, 11., -11.]);
        assert!((out.elapsed() - 0.0015).abs() < 1e-9);
    }

    #[test]
    fn grains_fade_and_stop_without_running_the_clock() {
        let out = FakeOutput::new(1000);
        let mix = Mix::from_samples(1000, &[1.; 2000]);
        let ramp = GRAIN_RAMP as usize;
        out.grain(mix, 10, ramp as u64 * 4);
        let got = out.pull(ramp * 5);
        assert_eq!(got[0], 0., "a grain fades in");
        assert_eq!(got[ramp * 2 * 2], 1.);
        assert!(got[(ramp * 4 - 1) * 2] < 0.1, "and out");
        assert!(got[ramp * 4 * 2..].iter().all(|s| *s == 0.));
        assert_eq!(out.elapsed(), 0.);
    }

    #[test]
    fn mono_and_surround_devices_get_the_mix_on_the_right_channels() {
        let mut deck = Deck::new(1000);
        deck.play(ramp(10, 1000), 2);
        let mut mono = [9.; 2];
        deck.fill(&mut mono, 1, None, 0.);
        assert_eq!(mono, [0., 0.]);
        let mut six = [9.; 6];
        deck.fill(&mut six, 6, None, 0.);
        assert_eq!(six, [4., -4., 0., 0., 0., 0.]);
        // Latency holds the clock back by what is still in flight.
        deck.fill(&mut [0.; 20], 2, None, 0.004);
        assert!((deck.elapsed(None) - 0.009).abs() < 1e-9);
    }

    #[test]
    fn the_audio_clock_starts_the_sound_at_the_playhead() {
        let out = FakeOutput::new(48_000);
        let mix = Mix::from_samples(48_000, &vec![0.5; 48_000 * 2 * 3]);
        assert_eq!(mix.sample_at(FrameRate::whole(25), 1), 1920);
        let mut clock = AudioClock {
            output: std::rc::Rc::new(out.clone()),
            mix,
        };
        clock.start(24, FrameRate::whole(24));
        assert_eq!(out.deck.lock().pos, 48_000.);
        out.pull(24_000);
        assert!((clock.elapsed() - 0.5).abs() < 1e-9);
        clock.stop();
        assert_eq!(clock.elapsed(), 0.);
    }

    #[test]
    fn sound_is_mixed_ahead_in_the_background_and_failures_play_silence() {
        let wait = |mix: &Mix, frame: u64| {
            for _ in 0..500 {
                if mix.ready(frame) {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            false
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let mix = Mix::new(
            100,
            1000,
            Box::new(move |first, count| {
                seen.lock().push(first);
                Ok((0..count)
                    .flat_map(|i| [(first as usize + i) as f32; 2])
                    .collect())
            }),
        );
        assert!(wait(&mix, 0) && wait(&mix, 350));
        mix.want(700);
        assert!(wait(&mix, 950));
        let out = FakeOutput::new(100);
        out.play(mix.clone(), 705);
        assert_eq!(out.pull(1), [705., 705.]);
        assert!(mix.ready(5000), "past the end there is nothing to mix");
        assert!(calls.lock().contains(&900));

        let broken = Mix::new(100, 300, Box::new(|_, _| Err("No FFmpeg.".into())));
        assert!(wait(&broken, 0));
        assert_eq!(broken.error().as_deref(), Some("No FFmpeg."));
        let out = FakeOutput::new(100);
        out.play(broken, 0);
        assert_eq!(out.pull(2), [0.; 4]);
    }
}
