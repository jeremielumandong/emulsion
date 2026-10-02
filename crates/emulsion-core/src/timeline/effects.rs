//! Keyframable audio clip effects (T12): a gain envelope and a three-band
//! EQ (low shelf, mid peak, high shelf). Keys sit at frames from the
//! clip's start and ease between each other through the shared
//! [`crate::motion`] sampling, as layer and camera keys do. The DSP that
//! plays them lives in `emulsion-io` (`audio::effects`); this is the model.
use crate::motion::{self, Curve, Easing, KeyView};
use serde::{Deserialize, Serialize};

/// Most keys one effect parameter holds.
pub const MAX_EFFECT_KEYS: usize = 1024;
/// EQ band gains are within ± this many dB.
pub const EQ_RANGE_DB: f32 = 24.;
/// Corner of the low shelf, centre of the mid peak and corner of the high
/// shelf, in Hz.
pub const EQ_LOW_HZ: f32 = 200.;
pub const EQ_MID_HZ: f32 = 1000.;
pub const EQ_HIGH_HZ: f32 = 5000.;
/// Width of the mid peak.
pub const EQ_MID_Q: f32 = 0.7;
/// The latest frame a key may sit on (24 hours at 120 fps).
const MAX_KEY_FRAME: u64 = 24 * 3600 * 120;

/// One key of an effect parameter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectKey {
    /// Frames from the start of the clip.
    pub frame: u64,
    pub db: f32,
    /// How the segment from this key to the next eases.
    #[serde(default)]
    pub easing: Easing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

impl EffectKey {
    pub fn new(frame: u64, db: f32) -> Self {
        Self {
            frame,
            db,
            easing: Easing::Linear,
            curve: None,
        }
    }
}

/// The value of `keys` (in frame order) at `frame` frames into the clip,
/// or `None` without keys.
pub fn sample_keys(keys: &[EffectKey], frame: f64) -> Option<f32> {
    motion::sample_by(keys, frame, |k| KeyView {
        time: k.frame as f64,
        value: f64::from(k.db),
        easing: k.easing,
        curve: k.curve,
    })
    .map(|v| v as f32)
}

/// Put `key` into `keys`, replacing a key on the same frame; keeps them in
/// frame order. Returns its index.
pub fn set_key(keys: &mut Vec<EffectKey>, key: EffectKey) -> usize {
    match keys.binary_search_by_key(&key.frame, |k| k.frame) {
        Ok(at) => {
            keys[at] = key;
            at
        }
        Err(at) => {
            keys.insert(at, key);
            at
        }
    }
}

/// Keys shifted `delta` frames earlier, as when a clip's head is trimmed
/// by `delta` (later when negative). Keys that would land before the clip
/// starts are dropped, and their value at the new start is kept as a key
/// there, so what plays does not change.
pub fn shift_keys(keys: &mut Vec<EffectKey>, delta: i64) {
    if keys.is_empty() || delta == 0 {
        return;
    }
    let at_start = sample_keys(keys, delta as f64);
    let cut = keys.iter().any(|k| (k.frame as i64) < delta);
    let easing = keys
        .iter()
        .rev()
        .find(|k| (k.frame as i64) <= delta)
        .map(|k| (k.easing, k.curve));
    keys.retain(|k| k.frame as i64 >= delta);
    for key in keys.iter_mut() {
        key.frame = (key.frame as i64 - delta) as u64;
    }
    if cut
        && let Some(db) = at_start
        && keys.first().is_none_or(|k| k.frame > 0)
    {
        let (easing, curve) = easing.unwrap_or_default();
        keys.insert(
            0,
            EffectKey {
                frame: 0,
                db,
                easing,
                curve,
            },
        );
    }
}

fn check_keys(keys: &[EffectKey], (lo, hi): (f32, f32), what: &str) -> Result<(), String> {
    if keys.len() > MAX_EFFECT_KEYS {
        return Err(format!("{what} holds at most {MAX_EFFECT_KEYS} keys."));
    }
    if keys.windows(2).any(|w| w[0].frame >= w[1].frame) {
        return Err(format!(
            "{what} keys must be in frame order, one per frame."
        ));
    }
    for key in keys {
        if key.frame > MAX_KEY_FRAME || !key.db.is_finite() || !(lo..=hi).contains(&key.db) {
            return Err(format!("{what} keys are {lo}–{hi} dB."));
        }
        if let Some(curve) = key.curve {
            curve.validate()?;
        }
    }
    Ok(())
}

/// One EQ band: a fixed gain, or keys when it has any.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    #[serde(default)]
    pub db: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<EffectKey>,
}

impl EqBand {
    /// The band's gain `frame` frames into the clip.
    pub fn at(&self, frame: f64) -> f32 {
        sample_keys(&self.keys, frame).unwrap_or(self.db)
    }
    pub fn is_flat(&self) -> bool {
        self.db == 0. && self.keys.is_empty()
    }
}

/// A three-band EQ. Flat (every band 0 dB, no keys) leaves the sound alone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Equalizer {
    #[serde(default, skip_serializing_if = "EqBand::is_flat")]
    pub low: EqBand,
    #[serde(default, skip_serializing_if = "EqBand::is_flat")]
    pub mid: EqBand,
    #[serde(default, skip_serializing_if = "EqBand::is_flat")]
    pub high: EqBand,
}

impl Equalizer {
    pub fn is_flat(&self) -> bool {
        self.low.is_flat() && self.mid.is_flat() && self.high.is_flat()
    }
    /// Low, mid and high gains in dB `frame` frames into the clip.
    pub fn at(&self, frame: f64) -> [f32; 3] {
        [self.low.at(frame), self.mid.at(frame), self.high.at(frame)]
    }
    pub fn validate(&self) -> Result<(), String> {
        for (band, param) in [
            (&self.low, ClipParam::Low),
            (&self.mid, ClipParam::Mid),
            (&self.high, ClipParam::High),
        ] {
            let (lo, hi) = param.range();
            if !band.db.is_finite() || !(lo..=hi).contains(&band.db) {
                return Err(format!("EQ bands are {lo}–{hi} dB."));
            }
            check_keys(&band.keys, (lo, hi), param.label())?;
        }
        Ok(())
    }
}

/// An effect parameter of a clip that takes keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipParam {
    /// The gain envelope, added to the clip's gain.
    Envelope,
    Low,
    Mid,
    High,
}

impl ClipParam {
    pub const ALL: [Self; 4] = [Self::Envelope, Self::Low, Self::Mid, Self::High];
    pub const EQ: [Self; 3] = [Self::Low, Self::Mid, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Envelope => "Gain envelope",
            Self::Low => "EQ low",
            Self::Mid => "EQ mid",
            Self::High => "EQ high",
        }
    }
    /// The name tools use (`envelope`, `low`, `mid`, `high`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Envelope => "envelope",
            Self::Low => "low",
            Self::Mid => "mid",
            Self::High => "high",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
    /// The dB range of the parameter's values.
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Envelope => (super::audio::MIN_GAIN_DB, super::audio::MAX_GAIN_DB),
            _ => (-EQ_RANGE_DB, EQ_RANGE_DB),
        }
    }
}

impl super::AudioClip {
    /// The keys of `param`.
    pub fn keys(&self, param: ClipParam) -> &[EffectKey] {
        match param {
            ClipParam::Envelope => &self.envelope,
            ClipParam::Low => &self.eq.low.keys,
            ClipParam::Mid => &self.eq.mid.keys,
            ClipParam::High => &self.eq.high.keys,
        }
    }
    pub fn keys_mut(&mut self, param: ClipParam) -> &mut Vec<EffectKey> {
        match param {
            ClipParam::Envelope => &mut self.envelope,
            ClipParam::Low => &mut self.eq.low.keys,
            ClipParam::Mid => &mut self.eq.mid.keys,
            ClipParam::High => &mut self.eq.high.keys,
        }
    }
    /// `param`'s value in dB `frame` frames into the clip: the envelope
    /// is 0 dB without keys, a band its fixed gain.
    pub fn param_at(&self, param: ClipParam, frame: f64) -> f32 {
        match param {
            ClipParam::Envelope => sample_keys(&self.envelope, frame).unwrap_or(0.),
            ClipParam::Low => self.eq.low.at(frame),
            ClipParam::Mid => self.eq.mid.at(frame),
            ClipParam::High => self.eq.high.at(frame),
        }
    }
    /// Set `param` to `db` at `frame` frames into the clip: a key when the
    /// parameter has keys (or `key` is set), otherwise a band's fixed gain.
    /// Values are clamped to the parameter's range.
    pub fn set_param(&mut self, param: ClipParam, frame: u64, db: f32, key: bool) {
        let (lo, hi) = param.range();
        let db = db.clamp(lo, hi);
        let keyed = key || !self.keys(param).is_empty() || param == ClipParam::Envelope;
        if keyed {
            let (easing, curve) = self
                .keys(param)
                .iter()
                .find(|k| k.frame == frame)
                .map_or((Easing::Linear, None), |k| (k.easing, k.curve));
            set_key(
                self.keys_mut(param),
                EffectKey {
                    frame,
                    db,
                    easing,
                    curve,
                },
            );
            return;
        }
        match param {
            ClipParam::Low => self.eq.low.db = db,
            ClipParam::Mid => self.eq.mid.db = db,
            ClipParam::High => self.eq.high.db = db,
            ClipParam::Envelope => {}
        }
    }
    /// Whether any effect (envelope or EQ) is set.
    pub fn has_effects(&self) -> bool {
        !self.envelope.is_empty() || !self.eq.is_flat()
    }
    /// Move every effect key as the head is trimmed by `delta` frames
    /// (see [`shift_keys`]).
    pub fn shift_effect_keys(&mut self, delta: i64) {
        for param in ClipParam::ALL {
            shift_keys(self.keys_mut(param), delta);
        }
    }
    pub(super) fn validate_effects(&self) -> Result<(), String> {
        check_keys(
            &self.envelope,
            ClipParam::Envelope.range(),
            ClipParam::Envelope.label(),
        )?;
        self.eq.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AudioClip, Timeline, audio::AudioAsset};
    use super::*;

    fn clip() -> AudioClip {
        AudioClip {
            asset: 1,
            name: "Line".into(),
            start: 10,
            frames: 40,
            ..AudioClip::default()
        }
    }

    #[test]
    fn the_envelope_shapes_the_clip_gain_with_eased_keys() {
        let mut c = clip();
        assert!(!c.has_effects());
        c.set_param(ClipParam::Envelope, 0, 0., true);
        c.set_param(ClipParam::Envelope, 20, -20., true);
        assert_eq!(c.envelope.len(), 2);
        assert!((c.gain_at(10) - 1.).abs() < 1e-6);
        assert!((c.gain_at(30) - 0.1).abs() < 1e-4, "-20 dB");
        assert!(
            (c.gain_at(45) - 0.1).abs() < 1e-4,
            "held after the last key"
        );
        // Linear in dB halfway: -10 dB.
        assert!((c.gain_at(20) - 10f32.powf(-0.5)).abs() < 1e-4);
        c.envelope[0].easing = Easing::Step;
        assert!((c.gain_at(29) - 1.).abs() < 1e-6, "held until the next key");
        // Replacing a key keeps its easing.
        c.set_param(ClipParam::Envelope, 0, 3., false);
        assert_eq!(c.envelope[0].easing, Easing::Step);
        assert_eq!(c.envelope[0].db, 3.);
        // Clamped to the range.
        c.set_param(ClipParam::Envelope, 5, -500., false);
        assert_eq!(c.envelope[1].db, super::super::audio::MIN_GAIN_DB);
    }

    #[test]
    fn eq_bands_hold_a_gain_or_keys() {
        let mut c = clip();
        c.set_param(ClipParam::Low, 4, 6., false);
        assert!(c.eq.low.keys.is_empty());
        assert_eq!(c.eq.at(0.)[0], 6.);
        c.set_param(ClipParam::High, 0, -6., true);
        c.set_param(ClipParam::High, 10, 6., false);
        assert_eq!(c.eq.high.keys.len(), 2, "a keyed band keeps taking keys");
        assert!(c.param_at(ClipParam::High, 5.).abs() < 1e-6);
        c.set_param(ClipParam::Mid, 0, 99., false);
        assert_eq!(c.eq.mid.db, EQ_RANGE_DB);
        assert!(c.has_effects());
        assert!(c.validate_effects().is_ok());
        c.eq.mid.db = 30.;
        assert!(c.validate_effects().is_err());
        c.eq.mid.db = 0.;
        c.eq.high.keys.swap(0, 1);
        assert!(c.validate_effects().is_err());
        assert_eq!(ClipParam::from_key("mid"), Some(ClipParam::Mid));
    }

    #[test]
    fn trimming_the_head_moves_keys_and_keeps_the_sound() {
        let mut keys = vec![EffectKey::new(0, 0.), EffectKey::new(20, -20.)];
        shift_keys(&mut keys, 10);
        assert_eq!(keys.len(), 2);
        assert_eq!((keys[0].frame, keys[0].db), (0, -10.));
        assert_eq!(keys[1].frame, 10);
        shift_keys(&mut keys, -5);
        assert_eq!(keys[0].frame, 5);
        shift_keys(&mut keys, 30);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].frame, 0);
        assert_eq!(keys[0].db, -20.);
    }

    #[test]
    fn old_clips_load_and_save_unchanged() {
        let json = r#"{"asset":1,"name":"Rain","start":0,"frames":10,"offset_ms":0,"gain_db":0.0,"fade_in":0,"fade_out":0}"#;
        let c: AudioClip = serde_json::from_str(json).unwrap();
        assert!(!c.has_effects());
        assert_eq!(serde_json::to_string(&c).unwrap(), json);
        let mut c = c;
        c.set_param(ClipParam::Envelope, 0, -6., false);
        c.set_param(ClipParam::Low, 0, 3., false);
        let saved = serde_json::to_string(&c).unwrap();
        assert!(saved.contains("envelope") && saved.contains("low") && !saved.contains("high"));
        assert_eq!(serde_json::from_str::<AudioClip>(&saved).unwrap(), c);
        // A timeline refuses bad keys.
        let mut t = Timeline::default();
        let id = t
            .add_asset(AudioAsset {
                name: "Rain".into(),
                format: "wav".into(),
                duration_ms: 1000,
                sample_rate: 48_000,
                channels: 1,
                folder: String::new(),
                source: None,
            })
            .unwrap();
        t.tracks.push(super::super::AudioTrack::new("A"));
        c.asset = id;
        t.place(0, c).unwrap();
        t.validate().unwrap();
        t.tracks[0].clips[0].envelope[0].db = f32::NAN;
        assert!(t.validate().is_err());
    }
}
