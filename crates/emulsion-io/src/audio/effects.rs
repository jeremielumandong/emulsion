//! Clip effects DSP: the three-band EQ of
//! [`emulsion_core::timeline::Equalizer`] as biquad filters (low shelf, mid
//! peak, high shelf, after the RBJ Audio EQ Cookbook), with band gains
//! following their keys. The gain envelope needs no state: the mixer reads
//! it through `AudioClip::gain_at`.
use super::RATE;
use emulsion_core::timeline::FrameRate;
use emulsion_core::timeline::effects::{EQ_HIGH_HZ, EQ_LOW_HZ, EQ_MID_HZ, EQ_MID_Q, Equalizer};

/// Samples of the sound before the part being mixed that the EQ runs over
/// first, so its filters have settled: trimmed clips and the edges of
/// blocks mixed separately do not click. 100 ms is many times the slowest
/// band's settling time.
pub const PREROLL: u64 = 4800;
/// Samples between updates of keyed band gains.
const STEP: usize = 32;

/// Normalised biquad coefficients.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    fn normalised(b: [f64; 3], a: [f64; 3]) -> Self {
        Self {
            b0: b[0] / a[0],
            b1: b[1] / a[0],
            b2: b[2] / a[0],
            a1: a[1] / a[0],
            a2: a[2] / a[0],
        }
    }

    /// Shelf parts shared by both shelves (slope 1).
    fn shelf(hz: f32, db: f32, rate: u32) -> (f64, f64, f64) {
        let a = 10f64.powf(f64::from(db) / 40.);
        let w0 = std::f64::consts::TAU * f64::from(hz) / f64::from(rate);
        let alpha = w0.sin() / 2. * std::f64::consts::SQRT_2;
        (a, w0.cos(), 2. * a.sqrt() * alpha)
    }

    pub fn low_shelf(hz: f32, db: f32, rate: u32) -> Self {
        let (a, cos, k) = Self::shelf(hz, db, rate);
        Self::normalised(
            [
                a * ((a + 1.) - (a - 1.) * cos + k),
                2. * a * ((a - 1.) - (a + 1.) * cos),
                a * ((a + 1.) - (a - 1.) * cos - k),
            ],
            [
                (a + 1.) + (a - 1.) * cos + k,
                -2. * ((a - 1.) + (a + 1.) * cos),
                (a + 1.) + (a - 1.) * cos - k,
            ],
        )
    }

    pub fn high_shelf(hz: f32, db: f32, rate: u32) -> Self {
        let (a, cos, k) = Self::shelf(hz, db, rate);
        Self::normalised(
            [
                a * ((a + 1.) + (a - 1.) * cos + k),
                -2. * a * ((a - 1.) + (a + 1.) * cos),
                a * ((a + 1.) + (a - 1.) * cos - k),
            ],
            [
                (a + 1.) - (a - 1.) * cos + k,
                2. * ((a - 1.) - (a + 1.) * cos),
                (a + 1.) - (a - 1.) * cos - k,
            ],
        )
    }

    pub fn peaking(hz: f32, q: f32, db: f32, rate: u32) -> Self {
        let a = 10f64.powf(f64::from(db) / 40.);
        let w0 = std::f64::consts::TAU * f64::from(hz) / f64::from(rate);
        let alpha = w0.sin() / (2. * f64::from(q));
        let cos = w0.cos();
        Self::normalised(
            [1. + alpha * a, -2. * cos, 1. - alpha * a],
            [1. + alpha / a, -2. * cos, 1. - alpha / a],
        )
    }
}

/// One channel's filter memory (direct form I, which tolerates
/// coefficients changing between samples).
#[derive(Clone, Copy, Debug, Default)]
struct State {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl State {
    fn run(&mut self, f: &Biquad, x: f64) -> f64 {
        let y = f.b0 * x + f.b1 * self.x1 + f.b2 * self.x2 - f.a1 * self.y1 - f.a2 * self.y2;
        (self.x2, self.x1) = (self.x1, x);
        (self.y2, self.y1) = (self.y1, y);
        y
    }
}

/// The three bands in series over interleaved stereo.
pub struct ThreeBand {
    gains: [f32; 3],
    filters: [Biquad; 3],
    state: [[State; 3]; 2],
}

impl ThreeBand {
    pub fn new(gains: [f32; 3]) -> Self {
        Self {
            gains,
            filters: Self::filters(gains),
            state: Default::default(),
        }
    }

    fn filters([low, mid, high]: [f32; 3]) -> [Biquad; 3] {
        [
            Biquad::low_shelf(EQ_LOW_HZ, low, RATE),
            Biquad::peaking(EQ_MID_HZ, EQ_MID_Q, mid, RATE),
            Biquad::high_shelf(EQ_HIGH_HZ, high, RATE),
        ]
    }

    /// Change the band gains (cheap when they are unchanged).
    pub fn set(&mut self, gains: [f32; 3]) {
        if gains != self.gains {
            self.gains = gains;
            self.filters = Self::filters(gains);
        }
    }

    /// Filter interleaved stereo in place.
    pub fn process(&mut self, pcm: &mut [f32]) {
        for frame in pcm.as_chunks_mut::<2>().0 {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut v = f64::from(*sample);
                for (state, filter) in self.state[channel].iter_mut().zip(&self.filters) {
                    v = state.run(filter, v);
                }
                *sample = v as f32;
            }
        }
    }
}

/// Run `eq` over interleaved stereo `pcm` whose first sample is timeline
/// sample `first` (48 kHz; negative before the timeline starts), for a clip
/// starting at timeline frame `clip_start`. Keyed bands follow their keys,
/// updated every few samples.
pub fn equalize(pcm: &mut [f32], eq: &Equalizer, rate: FrameRate, clip_start: u64, first: i64) {
    let frames_per_sample = rate.fps() / f64::from(RATE);
    let into = |sample: i64| (sample as f64 * frames_per_sample - clip_start as f64).max(0.);
    let mut bands = ThreeBand::new(eq.at(into(first)));
    for (n, block) in pcm.chunks_mut(STEP * 2).enumerate() {
        bands.set(eq.at(into(first + (n * STEP) as i64)));
        bands.process(block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::timeline::{EffectKey, EqBand};

    /// Gain of `filter` on an `hz` sine: output over input RMS, once settled.
    fn level(filter: Biquad, hz: f64) -> f64 {
        let mut state = State::default();
        let (mut out, mut inp) = (0., 0.);
        for n in 0..48_000 {
            let x = (std::f64::consts::TAU * hz * n as f64 / 48_000.).sin();
            let y = state.run(&filter, x);
            if n >= 24_000 {
                out += y * y;
                inp += x * x;
            }
        }
        (out / inp).sqrt()
    }

    fn db(v: f64) -> f64 {
        20. * v.log10()
    }

    #[test]
    fn bands_boost_and_cut_their_own_frequencies() {
        let low = Biquad::low_shelf(EQ_LOW_HZ, 12., RATE);
        assert!((db(level(low, 40.)) - 12.).abs() < 0.5);
        assert!(db(level(low, 8000.)).abs() < 0.5);
        let high = Biquad::high_shelf(EQ_HIGH_HZ, -12., RATE);
        assert!((db(level(high, 16_000.)) + 12.).abs() < 1.);
        assert!(db(level(high, 100.)).abs() < 0.5);
        let mid = Biquad::peaking(EQ_MID_HZ, EQ_MID_Q, 6., RATE);
        assert!((db(level(mid, 1000.)) - 6.).abs() < 0.2);
        assert!(db(level(mid, 30.)).abs() < 0.5);
        let flat = Biquad::peaking(EQ_MID_HZ, EQ_MID_Q, 0., RATE);
        assert!((db(level(flat, 1000.))).abs() < 1e-6);
    }

    fn sine(hz: f64, first: i64, count: usize) -> Vec<f32> {
        (0..count as i64)
            .flat_map(|n| {
                let v = (std::f64::consts::TAU * hz * (first + n) as f64 / 48_000.).sin() as f32;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn keyed_bands_follow_their_keys_and_preroll_joins_blocks() {
        let rate = FrameRate::whole(24);
        let eq = Equalizer {
            low: EqBand {
                db: 0.,
                keys: vec![EffectKey::new(0, 0.), EffectKey::new(24, -24.)],
            },
            ..Equalizer::default()
        };
        // A 60 Hz tone fades down by the low shelf over the first second.
        let mut pcm = sine(60., 0, 96_000);
        equalize(&mut pcm, &eq, rate, 0, 0);
        let peak = |pcm: &[f32], from: usize| {
            pcm[from * 2..(from + 2400) * 2]
                .iter()
                .fold(0f32, |m, v| m.max(v.abs()))
        };
        assert!(peak(&pcm, 2400) > 0.7);
        assert!(peak(&pcm, 60_000) < 0.1);
        // Mixing a stretch on its own, with pre-roll, matches the whole.
        let eq = Equalizer {
            high: EqBand {
                db: 12.,
                keys: Vec::new(),
            },
            ..Equalizer::default()
        };
        let mut whole = sine(7000., 0, 20_000);
        equalize(&mut whole, &eq, rate, 0, 0);
        let start = 10_000;
        let mut part = sine(7000., start - PREROLL as i64, 5000 + PREROLL as usize);
        equalize(&mut part, &eq, rate, 0, start - PREROLL as i64);
        let part = &part[PREROLL as usize * 2..];
        let worst = part
            .iter()
            .zip(&whole[start as usize * 2..])
            .fold(0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(worst < 1e-4, "{worst}");
    }
}
