//! Storyboard animation: a camera per scene with keyframes timed across its
//! panels and optional shake, layer keyframes per panel (position, scale,
//! rotation, skew, opacity and effect values, about a pivot), layer comps,
//! and keeping keyframes in step when panel durations change. Easing,
//! sampling and applying motion come from the shared `motion` module.
use crate::motion::{self, Curve, Easing, KeyView};
use crate::project::PageId;
use crate::storyboard::{GroupId, Storyboard};
use crate::{Command, Document, NodeId};
use glam::{DAffine2, DMat2, DVec2, dvec2};
use serde::{Deserialize, Serialize};

pub const MAX_KEYS: usize = 2000;
pub const MAX_ANIMATED_LAYERS: usize = 500;
pub const MAX_COMPS: usize = 64;
pub const ZOOM: std::ops::RangeInclusive<f64> = 0.05..=20.;

fn one() -> f64 {
    1.
}

/// The camera at one moment: the centre of the shot in panel pixels, zoom
/// (2 shows half the frame) and rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
    pub rotation: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    /// Frames from the start of the scene.
    pub frame: u64,
    pub x: f64,
    pub y: f64,
    #[serde(default = "one")]
    pub zoom: f64,
    #[serde(default)]
    pub rotation: f64,
    /// How the move to the next key eases.
    #[serde(default)]
    pub easing: Easing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

impl CameraKey {
    pub fn at(frame: u64, state: CameraState) -> Self {
        Self {
            frame,
            x: state.x,
            y: state.y,
            zoom: state.zoom,
            rotation: state.rotation,
            easing: Easing::EaseInOut,
            curve: None,
        }
    }
}

/// Seeded camera shake added on top of the keyframes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shake {
    /// Largest offset, in panel pixels.
    pub amplitude: f64,
    /// Largest tilt, in degrees.
    pub rotation: f64,
    /// Wobbles per second.
    pub frequency: f64,
    pub seed: u64,
}

impl Shake {
    /// Handheld, bumpy and earthquake presets.
    pub const PRESETS: [(&'static str, Shake); 3] = [
        (
            "Handheld",
            Shake {
                amplitude: 4.,
                rotation: 0.4,
                frequency: 1.2,
                seed: 1,
            },
        ),
        (
            "Bumpy ride",
            Shake {
                amplitude: 12.,
                rotation: 1.2,
                frequency: 4.,
                seed: 2,
            },
        ),
        (
            "Earthquake",
            Shake {
                amplitude: 30.,
                rotation: 3.,
                frequency: 9.,
                seed: 3,
            },
        ),
    ];

    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64, max: f64| v.is_finite() && (0. ..=max).contains(&v);
        if !ok(self.amplitude, 10_000.) || !ok(self.rotation, 45.) || !ok(self.frequency, 60.) {
            return Err("Shake is up to 10000 px, 45° and 60 wobbles a second.".into());
        }
        Ok(())
    }

    /// The offset (x, y) and tilt at `seconds`: smooth value noise, the same
    /// every time for one seed.
    pub fn offset(&self, seconds: f64) -> (f64, f64, f64) {
        let t = seconds * self.frequency;
        (
            self.amplitude * noise(self.seed, 0, t),
            self.amplitude * noise(self.seed, 1, t),
            self.rotation * noise(self.seed, 2, t),
        )
    }
}

/// Smooth 1D value noise in -1..1.
fn noise(seed: u64, channel: u64, t: f64) -> f64 {
    let hash = |i: i64| {
        let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ seed.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ channel.wrapping_mul(0x1656_67B1_9E37_79F9);
        x ^= x >> 31;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 29;
        (x >> 11) as f64 / (1u64 << 53) as f64 * 2. - 1.
    };
    let i = t.floor();
    let f = t - i;
    let s = f * f * (3. - 2. * f);
    hash(i as i64) * (1. - s) + hash(i as i64 + 1) * s
}

/// A scene's camera.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneCamera {
    /// In frame order.
    #[serde(default)]
    pub keys: Vec<CameraKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shake: Option<Shake>,
}

impl SceneCamera {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.shake.is_none()
    }
    fn validate(&self) -> Result<(), String> {
        if self.keys.len() > MAX_KEYS {
            return Err(format!("A camera holds at most {MAX_KEYS} keys."));
        }
        if self.keys.windows(2).any(|w| w[0].frame >= w[1].frame) {
            return Err("Camera keys must be in frame order, one per frame.".into());
        }
        for key in &self.keys {
            if ![key.x, key.y, key.rotation]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1e6)
                || !ZOOM.contains(&key.zoom)
            {
                return Err("Camera keys need finite positions and a zoom of 0.05–20.".into());
            }
            if let Some(curve) = key.curve {
                curve.validate()?;
            }
        }
        if let Some(shake) = self.shake {
            shake.validate()?;
        }
        Ok(())
    }
    /// The keyed camera at `frame` (scene time), before shake.
    fn sample(&self, frame: f64, rest: CameraState) -> CameraState {
        let channel = |get: fn(&CameraKey) -> f64, rest: f64| {
            motion::sample_by(&self.keys, frame, |k| KeyView {
                time: k.frame as f64,
                value: get(k),
                easing: k.easing,
                curve: k.curve,
            })
            .unwrap_or(rest)
        };
        CameraState {
            x: channel(|k| k.x, rest.x),
            y: channel(|k| k.y, rest.y),
            zoom: channel(|k| k.zoom, rest.zoom),
            rotation: channel(|k| k.rotation, rest.rotation),
        }
    }
}

/// What a layer keyframe track animates.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerProperty {
    /// Offset in panel pixels.
    X,
    Y,
    ScaleX,
    ScaleY,
    /// Degrees.
    Rotation,
    /// Degrees.
    SkewX,
    SkewY,
    /// Multiplies the layer's opacity, 0–1.
    Opacity,
    /// An effect or adjustment parameter, by its key.
    Effect(String),
}

impl LayerProperty {
    pub const TRANSFORMS: [Self; 8] = [
        Self::X,
        Self::Y,
        Self::ScaleX,
        Self::ScaleY,
        Self::Rotation,
        Self::SkewX,
        Self::SkewY,
        Self::Opacity,
    ];
    pub fn initial(&self) -> f64 {
        match self {
            Self::ScaleX | Self::ScaleY | Self::Opacity => 1.,
            _ => 0.,
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::X => "Horizontal offset · px".into(),
            Self::Y => "Vertical offset · px".into(),
            Self::ScaleX => "Horizontal scale".into(),
            Self::ScaleY => "Vertical scale".into(),
            Self::Rotation => "Rotation · degrees".into(),
            Self::SkewX => "Horizontal skew · degrees".into(),
            Self::SkewY => "Vertical skew · degrees".into(),
            Self::Opacity => "Opacity".into(),
            Self::Effect(key) => format!("Effect · {}", key.replace('_', " ")),
        }
    }
    fn valid(&self, v: f64) -> bool {
        v.is_finite()
            && match self {
                Self::X | Self::Y => v.abs() <= 100_000.,
                Self::ScaleX | Self::ScaleY => (-100. ..=100.).contains(&v),
                Self::Rotation => v.abs() <= 36_000.,
                Self::SkewX | Self::SkewY => v.abs() <= 80.,
                Self::Opacity => (0. ..=1.).contains(&v),
                Self::Effect(key) => !key.is_empty() && key.len() <= 64 && v.abs() <= 1e6,
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionKey {
    /// Frames from the start of the panel.
    pub frame: u64,
    pub value: f64,
    #[serde(default)]
    pub easing: Easing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyTrack {
    pub property: LayerProperty,
    /// In frame order.
    pub keys: Vec<MotionKey>,
}

impl PropertyTrack {
    pub fn sample(&self, frame: f64) -> f64 {
        motion::sample_by(&self.keys, frame, |k| KeyView {
            time: k.frame as f64,
            value: k.value,
            easing: k.easing,
            curve: k.curve,
        })
        .unwrap_or_else(|| self.property.initial())
    }
}

/// A layer's keyframes within one panel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerMotion {
    /// The point the layer turns, scales and skews about, in panel pixels.
    /// Defaults to the centre of the layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot: Option<[f64; 2]>,
    #[serde(default)]
    pub tracks: Vec<PropertyTrack>,
}

impl LayerMotion {
    pub fn track(&self, property: &LayerProperty) -> Option<&PropertyTrack> {
        self.tracks.iter().find(|t| &t.property == property)
    }
    /// The value of `property` at `frame`.
    pub fn value(&self, property: &LayerProperty, frame: f64) -> f64 {
        self.track(property)
            .map_or_else(|| property.initial(), |t| t.sample(frame))
    }
    /// The transform at `frame` about `pivot`, for the shared applier.
    pub fn transform(&self, frame: f64, pivot: DVec2) -> DAffine2 {
        let v = |p: LayerProperty| self.value(&p, frame);
        let (skew_x, skew_y) = (v(LayerProperty::SkewX), v(LayerProperty::SkewY));
        let skew = DMat2::from_cols(
            dvec2(1., skew_y.to_radians().tan()),
            dvec2(skew_x.to_radians().tan(), 1.),
        );
        DAffine2::from_translation(pivot + dvec2(v(LayerProperty::X), v(LayerProperty::Y)))
            * DAffine2::from_angle(v(LayerProperty::Rotation).to_radians())
            * DAffine2::from_mat2(skew)
            * DAffine2::from_scale(dvec2(v(LayerProperty::ScaleX), v(LayerProperty::ScaleY)))
            * DAffine2::from_translation(-pivot)
    }
    fn validate(&self) -> Result<(), String> {
        if let Some([x, y]) = self.pivot
            && (!x.is_finite() || !y.is_finite())
        {
            return Err("A pivot needs a finite position.".into());
        }
        for (i, track) in self.tracks.iter().enumerate() {
            if self.tracks[..i]
                .iter()
                .any(|t| t.property == track.property)
            {
                return Err("A layer animates each property once.".into());
            }
            if track.keys.is_empty() || track.keys.len() > MAX_KEYS {
                return Err(format!("A track holds 1–{MAX_KEYS} keys."));
            }
            if track.keys.windows(2).any(|w| w[0].frame >= w[1].frame) {
                return Err("Keys must be in frame order, one per frame.".into());
            }
            for key in &track.keys {
                if !track.property.valid(key.value) {
                    return Err(format!("{} is out of range.", track.property.label()));
                }
                if let Some(curve) = key.curve {
                    curve.validate()?;
                }
            }
        }
        Ok(())
    }
}

/// A named set of hidden layers, recalled to switch a panel's look.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerComp {
    pub name: String,
    pub hidden: Vec<NodeId>,
}

/// What happens to keyframes when a panel's duration changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyframeSync {
    /// Keyframes stretch with the panel.
    #[default]
    Scale,
    /// Keyframes keep their frames.
    Keep,
}

impl Storyboard {
    /// The camera at rest: the whole frame.
    pub fn rest_camera(&self) -> CameraState {
        CameraState {
            x: f64::from(self.settings.width) / 2.,
            y: f64::from(self.settings.height) / 2.,
            zoom: 1.,
            rotation: 0.,
        }
    }

    /// Where `scene` starts in the animatic and how long it plays.
    pub fn scene_span(&self, layout: &[PageId], scene: GroupId) -> Option<(u64, u64)> {
        let mut span: Option<(u64, u64)> = None;
        for (id, start) in self.panel_starts(layout) {
            if self.panels[&id].scene == scene {
                let end = start + u64::from(self.panels[&id].frames);
                span = Some(span.map_or((start, end), |(s, _)| (s, end)));
            }
        }
        span.map(|(s, e)| (s, e - s))
    }

    /// The camera at animatic `frame`, shake included.
    pub fn camera_at(&self, layout: &[PageId], frame: f64) -> CameraState {
        let rest = self.rest_camera();
        let Some(at) = self.animatic_frame(layout, frame.max(0.) as u64) else {
            return rest;
        };
        let scene = self.panels[&at.panel].scene;
        let Some(camera) = self.cameras.get(&scene) else {
            return rest;
        };
        let start = self.scene_span(layout, scene).map_or(0, |(s, _)| s);
        let local = frame - start as f64;
        let mut state = camera.sample(local, rest);
        if let Some(shake) = camera.shake {
            let (dx, dy, dr) = shake.offset(self.settings.frame_rate.frames_to_seconds(1) * local);
            state.x += dx;
            state.y += dy;
            state.rotation += dr;
        }
        state
    }

    /// Maps a point of the output frame (0–width, 0–height) to the panel
    /// point the camera shows there.
    pub fn camera_matrix(&self, state: CameraState) -> DAffine2 {
        let (w, h) = (
            f64::from(self.settings.width),
            f64::from(self.settings.height),
        );
        DAffine2::from_translation(dvec2(state.x, state.y))
            * DAffine2::from_angle(state.rotation.to_radians())
            * DAffine2::from_scale(DVec2::splat(1. / state.zoom))
            * DAffine2::from_translation(dvec2(-w / 2., -h / 2.))
    }

    /// The camera frame's corners on the panel, for drawing it on the Stage.
    pub fn camera_corners(&self, state: CameraState) -> [(f64, f64); 4] {
        let (w, h) = (
            f64::from(self.settings.width),
            f64::from(self.settings.height),
        );
        let m = self.camera_matrix(state);
        [(0., 0.), (w, 0.), (w, h), (0., h)].map(|(x, y)| {
            let p = m.transform_point2(dvec2(x, y));
            (p.x, p.y)
        })
    }

    /// `doc` (panel `panel`) as it looks `frame` frames into the panel, with
    /// its layer keyframes applied. The source is unchanged.
    pub fn animate_panel(
        &self,
        panel: PageId,
        doc: &Document,
        frame: f64,
    ) -> Result<Document, String> {
        let mut out = doc.clone();
        let Some(p) = self.panels.get(&panel).filter(|p| !p.motion.is_empty()) else {
            return Ok(out);
        };
        let order = motion::parents_first(
            doc,
            p.motion
                .keys()
                .copied()
                .filter(|id| doc.node(*id).is_some()),
        );
        motion::with_layers_unlocked(&mut out, |out| {
            for id in order {
                let layer = &p.motion[&id];
                for track in &layer.tracks {
                    if let LayerProperty::Effect(key) = &track.property {
                        Command::SetParam {
                            id,
                            key: key.clone(),
                            value: track.sample(frame) as f32,
                        }
                        .apply(out)
                        .map_err(|e| e.to_string())?;
                    }
                }
                let opacity = layer.value(&LayerProperty::Opacity, frame) as f32;
                if let Some(node) = out.node_mut(id) {
                    node.opacity *= opacity;
                }
                let pivot = match layer.pivot {
                    Some([x, y]) => dvec2(x, y),
                    None => {
                        match crate::geometry::node_bounds(out, id).map_err(|e| e.to_string())? {
                            Some(b) => dvec2(
                                f64::from(b.x) + f64::from(b.w) / 2.,
                                f64::from(b.y) + f64::from(b.h) / 2.,
                            ),
                            None => continue,
                        }
                    }
                };
                let m = layer.transform(frame, pivot);
                if m != DAffine2::IDENTITY {
                    crate::transform::transform_nodes(out, &[id], m.to_cols_array())
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// Stretch keyframes with panels whose duration changed since `before`,
    /// when keyframe sync scales. Camera keys map through each panel of
    /// their scene.
    pub fn sync_keyframes(&mut self, before: &Storyboard, layout: &[PageId]) {
        if self.keyframe_sync == KeyframeSync::Keep {
            return;
        }
        for (id, panel) in &mut self.panels {
            let Some(old) = before.panels.get(id) else {
                continue;
            };
            if old.frames == panel.frames || old.frames == 0 {
                continue;
            }
            let k = f64::from(panel.frames) / f64::from(old.frames);
            for layer in panel.motion.values_mut() {
                for track in &mut layer.tracks {
                    rescale(
                        &mut track.keys,
                        |key| &mut key.frame,
                        |f| (f as f64 * k).round() as u64,
                    );
                }
            }
        }
        let scenes: Vec<GroupId> = self.cameras.keys().copied().collect();
        for scene in scenes {
            let spans = |board: &Storyboard| -> Vec<(u64, u64)> {
                let mut at = 0;
                board
                    .playing(layout)
                    .into_iter()
                    .filter(|(id, _)| board.panels[id].scene == scene)
                    .map(|(_, f)| {
                        let span = (at, u64::from(f));
                        at += u64::from(f);
                        span
                    })
                    .collect()
            };
            let (old, new) = (spans(before), spans(self));
            if old == new || old.len() != new.len() {
                continue;
            }
            let map = |f: u64| -> u64 {
                for (i, ((os, ol), (ns, nl))) in old.iter().zip(&new).enumerate() {
                    if f < os + ol || i + 1 == old.len() {
                        let into = f.saturating_sub(*os) as f64 / (*ol).max(1) as f64;
                        return ns + (into * *nl as f64).round() as u64;
                    }
                }
                f
            };
            if let Some(camera) = self.cameras.get_mut(&scene) {
                rescale(&mut camera.keys, |key| &mut key.frame, map);
            }
        }
    }

    pub(crate) fn validate_motion(&self) -> Result<(), String> {
        for (scene, camera) in &self.cameras {
            if !self.scenes.contains_key(scene) {
                return Err("A camera belongs to a missing scene.".into());
            }
            camera.validate()?;
        }
        for panel in self.panels.values() {
            if panel.motion.len() > MAX_ANIMATED_LAYERS {
                return Err(format!(
                    "A panel animates at most {MAX_ANIMATED_LAYERS} layers."
                ));
            }
            for layer in panel.motion.values() {
                layer.validate()?;
            }
            if panel.comps.len() > MAX_COMPS {
                return Err(format!("A panel holds at most {MAX_COMPS} layer comps."));
            }
            for (i, comp) in panel.comps.iter().enumerate() {
                crate::storyboard::check_name(&comp.name, "Layer comp")?;
                if panel.comps[..i].iter().any(|c| c.name == comp.name) {
                    return Err("Layer comp names must be unique within a panel.".into());
                }
            }
        }
        Ok(())
    }
}

/// Move key frames with `map`, keeping them in order and one per frame.
fn rescale<K>(keys: &mut Vec<K>, frame: impl Fn(&mut K) -> &mut u64, map: impl Fn(u64) -> u64) {
    let mut last: Option<u64> = None;
    keys.retain_mut(|key| {
        let f = frame(key);
        *f = map(*f);
        if last.is_some_and(|l| *f <= l) {
            return false;
        }
        last = Some(*f);
        true
    });
}

/// Sample `src` (RGBA8, `sw` × `sh`) through `m`, which maps output pixels
/// to source pixels, into an `ow` × `oh` frame: the camera's view of a
/// panel. Outside the source is `background`.
pub fn camera_view(
    src: &[u8],
    sw: u32,
    sh: u32,
    m: DAffine2,
    ow: u32,
    oh: u32,
    background: [u8; 4],
) -> Vec<u8> {
    let mut out = vec![0u8; ow as usize * oh as usize * 4];
    let px = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= i64::from(sw) || y >= i64::from(sh) {
            return background.map(f32::from);
        }
        let i = ((y as usize) * sw as usize + x as usize) * 4;
        [src[i], src[i + 1], src[i + 2], src[i + 3]].map(f32::from)
    };
    for (index, pixel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (ox, oy) = (
            (index as u32 % ow) as f64 + 0.5,
            (index as u32 / ow) as f64 + 0.5,
        );
        let p = m.transform_point2(dvec2(ox, oy)) - dvec2(0.5, 0.5);
        let (x0, y0) = (p.x.floor(), p.y.floor());
        let (fx, fy) = ((p.x - x0) as f32, (p.y - y0) as f32);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (
            px(x0, y0),
            px(x0 + 1, y0),
            px(x0, y0 + 1),
            px(x0 + 1, y0 + 1),
        );
        for k in 0..4 {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            pixel[k] = (top + (bottom - top) * fy).round().clamp(0., 255.) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard::Settings;
    use crate::{Node, NodeKind};

    fn board() -> (Storyboard, Vec<PageId>) {
        let ids = vec![1, 2, 3];
        let mut b = Storyboard::new(Settings::new(200, 100), &ids);
        for (id, f) in ids.iter().zip([10, 20, 30]) {
            b.panels.get_mut(id).unwrap().frames = f;
        }
        (b, ids)
    }

    #[test]
    fn the_scene_camera_moves_across_its_panels() {
        let (mut b, ids) = board();
        let scene = b.panels[&1].scene;
        let rest = b.rest_camera();
        b.cameras.insert(
            scene,
            SceneCamera {
                keys: vec![
                    CameraKey {
                        easing: Easing::Linear,
                        ..CameraKey::at(0, rest)
                    },
                    CameraKey::at(
                        40,
                        CameraState {
                            x: 150.,
                            zoom: 2.,
                            ..rest
                        },
                    ),
                ],
                shake: None,
            },
        );
        b.validate(&ids).unwrap();
        assert_eq!(b.camera_at(&ids, 0.), rest);
        let mid = b.camera_at(&ids, 20.);
        assert!((mid.x - 125.).abs() < 1e-9 && (mid.zoom - 1.5).abs() < 1e-9);
        assert_eq!(b.camera_at(&ids, 55.).x, 150.);
        // Zoom 2 on the right half: the output's centre shows (150, 50).
        let state = b.camera_at(&ids, 50.);
        let c = b.camera_matrix(state).transform_point2(dvec2(100., 50.));
        assert!((c.x - 150.).abs() < 1e-9 && (c.y - 50.).abs() < 1e-9);
        let corners = b.camera_corners(state);
        assert!((corners[0].0 - 100.).abs() < 1e-9 && (corners[2].0 - 200.).abs() < 1e-9);
    }

    #[test]
    fn shake_is_repeatable_and_bounded() {
        let (mut b, ids) = board();
        let scene = b.panels[&1].scene;
        b.cameras.insert(
            scene,
            SceneCamera {
                keys: Vec::new(),
                shake: Some(Shake::PRESETS[1].1),
            },
        );
        let a = b.camera_at(&ids, 7.);
        assert_eq!(a, b.camera_at(&ids, 7.));
        assert_ne!(a, b.rest_camera());
        for f in 0..60 {
            let s = b.camera_at(&ids, f64::from(f));
            assert!((s.x - 100.).abs() <= 12. && s.rotation.abs() <= 1.2);
        }
    }

    #[test]
    fn layer_keyframes_move_turn_skew_and_fade_about_the_pivot() {
        let (mut b, _) = board();
        let mut doc = Document::new(200, 100);
        let mut hero = Node::new(5, "Hero", NodeKind::Fill { rgba: [9; 4] });
        hero.opacity = 0.8;
        doc.nodes.push(hero);
        let track = |property, a: f64, z: f64| PropertyTrack {
            property,
            keys: vec![
                MotionKey {
                    frame: 0,
                    value: a,
                    easing: Easing::Linear,
                    curve: None,
                },
                MotionKey {
                    frame: 10,
                    value: z,
                    easing: Easing::Linear,
                    curve: None,
                },
            ],
        };
        let layer = LayerMotion {
            pivot: Some([50., 50.]),
            tracks: vec![
                track(LayerProperty::X, 0., 20.),
                track(LayerProperty::Opacity, 1., 0.5),
                track(LayerProperty::SkewX, 0., 10.),
            ],
        };
        // Halfway: 10 px right, opacity 0.75 of the layer's own.
        let m = layer.transform(5., dvec2(50., 50.));
        let p = m.transform_point2(dvec2(50., 50.));
        assert!((p.x - 60.).abs() < 1e-9 && (p.y - 50.).abs() < 1e-9);
        b.panels.get_mut(&1).unwrap().motion.insert(5, layer);
        let out = b.animate_panel(1, &doc, 5.).unwrap();
        assert!((out.node(5).unwrap().opacity - 0.6).abs() < 1e-6);
        assert_eq!(doc.node(5).unwrap().opacity, 0.8, "the source is unchanged");
        assert_eq!(b.animate_panel(2, &doc, 5.).unwrap(), doc);
        b.validate(&[1, 2, 3]).unwrap();
        b.panels
            .get_mut(&1)
            .unwrap()
            .motion
            .get_mut(&5)
            .unwrap()
            .tracks[0]
            .keys[1]
            .frame = 0;
        assert!(b.validate(&[1, 2, 3]).is_err());
    }

    #[test]
    fn keyframes_follow_duration_changes_when_synced() {
        let (mut b, ids) = board();
        let scene = b.panels[&1].scene;
        let key = |frame| MotionKey {
            frame,
            value: 1.,
            easing: Easing::Linear,
            curve: None,
        };
        b.panels.get_mut(&2).unwrap().motion.insert(
            5,
            LayerMotion {
                pivot: None,
                tracks: vec![PropertyTrack {
                    property: LayerProperty::X,
                    keys: vec![key(0), key(10), key(20)],
                }],
            },
        );
        let rest = b.rest_camera();
        b.cameras.insert(
            scene,
            SceneCamera {
                keys: vec![CameraKey::at(20, rest), CameraKey::at(40, rest)],
                shake: None,
            },
        );
        let before = b.clone();
        b.panels.get_mut(&2).unwrap().frames = 40;
        b.sync_keyframes(&before, &ids);
        let frames: Vec<_> = b.panels[&2].motion[&5].tracks[0]
            .keys
            .iter()
            .map(|k| k.frame)
            .collect();
        assert_eq!(frames, [0, 20, 40]);
        // Key 20 sat halfway through panel 2 (10–30); it stays halfway (10–50).
        let camera: Vec<_> = b.cameras[&scene].keys.iter().map(|k| k.frame).collect();
        assert_eq!(camera, [30, 60]);
        let mut kept = before.clone();
        kept.keyframe_sync = KeyframeSync::Keep;
        let snapshot = kept.clone();
        kept.panels.get_mut(&2).unwrap().frames = 40;
        kept.sync_keyframes(&snapshot, &ids);
        assert_eq!(kept.cameras, snapshot.cameras);
    }

    #[test]
    fn the_camera_view_crops_and_scales() {
        // A 4 × 1 source: black, black, white, white.
        let mut src = Vec::new();
        for v in [0u8, 0, 255, 255] {
            src.extend([v, v, v, 255]);
        }
        // Panning two pixels right shows the white half.
        let panned = camera_view(
            &src,
            4,
            1,
            DAffine2::from_translation(dvec2(2., 0.)),
            2,
            1,
            [0; 4],
        );
        assert_eq!((panned[0], panned[4]), (255, 255));
        // Showing the whole source in two pixels: black then white.
        let fitted = camera_view(
            &src,
            4,
            1,
            DAffine2::from_scale(dvec2(2., 1.)),
            2,
            1,
            [0; 4],
        );
        assert_eq!((fitted[0], fitted[4]), (0, 255));
        let outside = camera_view(
            &src,
            4,
            1,
            DAffine2::from_translation(dvec2(10., 0.)),
            1,
            1,
            [9; 4],
        );
        assert_eq!(outside, [9, 9, 9, 9]);
    }
}
