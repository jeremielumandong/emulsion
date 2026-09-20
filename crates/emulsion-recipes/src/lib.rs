//! `emulsion-recipes` — film recipes: the fields Fujifilm shooters already
//! write down, read from TOML or pasted text, compiled into an ordinary
//! group of adjustment nodes.
//!
//! A recipe applies to any image. The base looks are Emulsion's own
//! approximations, named descriptively; a recipe can name a `.cube` LUT as
//! its base look instead.

pub mod bundle;
pub mod cameras;
pub mod effects;
pub mod import;
pub mod looks;
pub mod store;
pub mod workflow;
pub use workflow::capture_adjustments;

use emulsion_core::Node;
use emulsion_raster::adjust::{Adjustment, Cube, straight_curve};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Crate name, used in diagnostics.
pub const CRATE: &str = "emulsion-recipes";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strength {
    #[default]
    Off,
    Weak,
    Strong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrainSize {
    #[default]
    Small,
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Grain {
    pub strength: Strength,
    pub size: GrainSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DynamicRange {
    #[default]
    Dr100,
    Dr200,
    Dr400,
}

/// White balance the way a camera menu says it: a preset or kelvin, plus
/// red and blue shifts −9–+9.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhiteBalance {
    pub preset: String,
    pub kelvin: Option<u32>,
    pub red: i32,
    pub blue: i32,
}

impl Default for WhiteBalance {
    fn default() -> Self {
        Self {
            preset: "auto".into(),
            kelvin: None,
            red: 0,
            blue: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    /// Exact captured edits, when present, replace the camera-style recipe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<workflow::Workflow>,
    /// Embedded film LUT; takes precedence over the legacy path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded_lut: Option<Cube>,
    pub name: String,
    pub author: String,
    pub source_url: String,
    pub license: String,
    pub notes: String,
    /// Informational: which sensor generation the recipe was written for.
    pub sensor: Vec<String>,
    pub tags: Vec<String>,
    /// A key from [`looks::LOOKS`], or empty when `lut` is set.
    pub film_simulation: String,
    /// Path to a `.cube` used as the base look instead of a built-in one.
    pub lut: Option<String>,
    pub dynamic_range: DynamicRange,
    pub grain: Grain,
    pub color_chrome_effect: Strength,
    pub color_chrome_fx_blue: Strength,
    pub white_balance: WhiteBalance,
    /// −2–+4 in camera; any float is accepted.
    pub highlight: f32,
    pub shadow: f32,
    /// Saturation, −4–+4.
    pub color: f32,
    pub sharpness: f32,
    pub noise_reduction: f32,
    pub clarity: f32,
    /// Kept as text ("+1/3 to +1"); its numbers set the exposure node.
    pub exposure_compensation: String,
    pub iso: String,

    // ── Camera and film character (beyond what a Fuji body offers) ──
    /// Corner darkening, 0–100.
    pub vignette: f32,
    /// Lifted blacks and softened whites, 0–100 (faded print).
    pub fade: f32,
    /// Colour shift in the shadows and highlights, −100–100 per RGB.
    pub split_shadows: [f32; 3],
    pub split_highlights: [f32; 3],
    /// Show the picture as an orange-masked colour negative.
    pub negative: bool,
    /// Light leak strength 0–100, its colour and edge.
    pub light_leak: f32,
    pub leak_color: [u8; 3],
    pub leak_side: effects::LeakSide,
    /// Dust, hairs and scratches, 0–100.
    pub dust: f32,
    pub frame: effects::Frame,
    /// Burn a compact-camera date into the corner; `date` like "'98 12 24"
    /// (empty = today).
    pub date_stamp: bool,
    pub date: String,
    /// Seed for the procedural effects.
    pub seed: u32,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            workflow: None,
            embedded_lut: None,
            name: "Untitled recipe".into(),
            author: String::new(),
            source_url: String::new(),
            license: String::new(),
            notes: String::new(),
            sensor: Vec::new(),
            tags: Vec::new(),
            film_simulation: "provia".into(),
            lut: None,
            dynamic_range: DynamicRange::Dr100,
            grain: Grain::default(),
            color_chrome_effect: Strength::Off,
            color_chrome_fx_blue: Strength::Off,
            white_balance: WhiteBalance::default(),
            highlight: 0.0,
            shadow: 0.0,
            color: 0.0,
            sharpness: 0.0,
            noise_reduction: 0.0,
            clarity: 0.0,
            vignette: 0.0,
            fade: 0.0,
            split_shadows: [0.0; 3],
            split_highlights: [0.0; 3],
            negative: false,
            light_leak: 0.0,
            leak_color: [255, 140, 60],
            leak_side: effects::LeakSide::Right,
            dust: 0.0,
            frame: effects::Frame::None,
            date_stamp: false,
            date: String::new(),
            seed: 7,
            exposure_compensation: String::new(),
            iso: String::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecipeError {
    #[error("not a recipe file: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("{0}")]
    Invalid(String),
}

impl Recipe {
    /// Settings retained for interchange but not applied by the current renderer.
    pub fn limitations(&self) -> Vec<&'static str> {
        if self.workflow.is_some() {
            return Vec::new();
        }
        let mut notes = Vec::new();
        if self.sharpness != 0.0 {
            notes.push("Recipe sharpness is stored but not applied.");
        }
        if self.noise_reduction != 0.0 {
            notes.push("Recipe noise reduction is stored but not applied.");
        }
        if self.color_chrome_fx_blue != Strength::Off {
            notes.push("Color Chrome FX Blue is stored but not applied.");
        }
        notes
    }

    /// Snapshot external LUT data so the saved recipe can travel independently.
    pub fn portable(&self) -> Result<Self, RecipeError> {
        self.validate()?;
        let mut recipe = self.clone();
        if recipe.workflow.is_none()
            && recipe.embedded_lut.is_none()
            && let Some(path) = &recipe.lut
        {
            recipe.embedded_lut = Some(load_cube(path)?);
        }
        recipe.lut = None;
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn from_toml(text: &str) -> Result<Recipe, RecipeError> {
        let r: Recipe = toml::from_str(text)?;
        r.validate()?;
        Ok(r)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), RecipeError> {
        if self.name.trim().is_empty() || self.name.chars().count() > 120 {
            return Err(RecipeError::Invalid(
                "a recipe needs a name of up to 120 characters".into(),
            ));
        }
        if let Some(w) = &self.workflow {
            return w.validate();
        }
        if let Some(cube) = &self.embedded_lut {
            workflow::validate_cube(cube)?;
        }
        if self.embedded_lut.is_none()
            && self.lut.is_none()
            && looks::find(&self.film_simulation).is_none()
        {
            return Err(RecipeError::Invalid(format!(
                "unknown film simulation {:?}; one of: {}",
                self.film_simulation,
                looks::LOOKS
                    .iter()
                    .map(|l| l.key)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        for v in [
            self.highlight,
            self.shadow,
            self.color,
            self.sharpness,
            self.noise_reduction,
            self.clarity,
        ] {
            if !v.is_finite() || v.abs() > 20.0 {
                return Err(RecipeError::Invalid(
                    "tone and colour values must be within ±20".into(),
                ));
            }
        }
        for value in [self.vignette, self.fade, self.light_leak, self.dust]
            .into_iter()
            .chain(self.split_shadows)
            .chain(self.split_highlights)
        {
            if !value.is_finite() || value.abs() > 100.0 {
                return Err(RecipeError::Invalid(
                    "film effects must be finite and within ±100".into(),
                ));
            }
        }
        if !self.exposure_ev().is_finite() || self.exposure_ev().abs() > 20.0 {
            return Err(RecipeError::Invalid(
                "exposure compensation must be finite and within ±20 EV".into(),
            ));
        }
        if self.white_balance.red.unsigned_abs() > 9 || self.white_balance.blue.unsigned_abs() > 9 {
            return Err(RecipeError::Invalid(
                "white balance shifts are −9 to +9".into(),
            ));
        }
        Ok(())
    }

    /// The exposure the compensation text implies, in EV: the mean of the
    /// numbers in it ("+1/3 to +1" → 0.67), or 0.
    pub fn exposure_ev(&self) -> f32 {
        let t = self.exposure_compensation.replace(['–', '−'], "-");
        let mut values: Vec<f32> = Vec::new();
        let mut last_whole = false;
        for tok in t
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
        {
            let tok = tok.trim_end_matches("EV").trim_end_matches("ev");
            let Some(v) = parse_fraction(tok) else {
                last_whole = false;
                continue;
            };
            let unsigned_fraction = tok.contains('/') && !tok.starts_with(['+', '-']);
            if unsigned_fraction && last_whole {
                // "+1 1/3" is a mixed number.
                if let Some(w) = values.last_mut() {
                    *w += v * w.signum();
                }
                last_whole = false;
            } else {
                values.push(v);
                last_whole = !tok.contains('/');
            }
        }
        if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f32>() / values.len() as f32
        }
    }

    /// Compile into a group node and its children, bottom to top, in the
    /// order the camera applies them. Effects that need the canvas size
    /// (leaks, dust, frames, the date) are left out; see `compile_for`.
    pub fn compile(&self, cube: Option<Cube>) -> (Node, Vec<Node>) {
        self.compile_for(cube, 0, 0)
    }

    /// Whether the recipe adds pixel layers beyond adjustments.
    pub fn has_effects(&self) -> bool {
        self.workflow.is_none()
            && (self.light_leak > 0.0
                || self.dust > 0.0
                || self.frame != effects::Frame::None
                || self.date_stamp)
    }

    /// Compile for a `w × h` document, including its pixel-layer effects.
    pub fn compile_for(&self, cube: Option<Cube>, w: u32, h: u32) -> (Node, Vec<Node>) {
        if let Some(workflow) = &self.workflow {
            return workflow.compile();
        }
        let cube = self.embedded_lut.clone().or(cube);
        let mut stages: Vec<Adjustment> = Vec::new();
        // 1. White balance, in linear light before the look.
        let wb = &self.white_balance;
        let temperature = match wb.kelvin {
            Some(k) => ((k as f32 - 5500.0) / 45.0).clamp(-100.0, 100.0),
            None => match wb.preset.to_lowercase().as_str() {
                "daylight" | "fine" | "sunny" => 6.0,
                "shade" => 22.0,
                "cloudy" => 14.0,
                "fluorescent" | "fluorescent 1" | "fluorescent-1" => -8.0,
                "fluorescent 2" | "fluorescent-2" => -14.0,
                "fluorescent 3" | "fluorescent-3" => -20.0,
                "incandescent" | "tungsten" => -34.0,
                "underwater" => 10.0,
                _ => 0.0,
            },
        };
        if temperature != 0.0 {
            stages.push(Adjustment::WhiteBalance {
                temperature,
                tint: 0.0,
            });
        }
        if wb.red != 0 || wb.blue != 0 {
            // The camera's R and B axes are independent; colour balance
            // across all tones matches them better than a single warmth.
            let shift = [wb.red as f32 * 5.0, 0.0, wb.blue as f32 * 5.0];
            stages.push(Adjustment::ColorBalance {
                shadows: shift,
                midtones: shift,
                highlights: shift,
                preserve_luminosity: true,
            });
        }
        // 2. Exposure compensation.
        let ev = self.exposure_ev();
        if ev != 0.0 {
            stages.push(Adjustment::Exposure {
                exposure: ev,
                offset: 0.0,
                gamma: 1.0,
            });
        }
        // 3. Dynamic range: highlight roll-off before the look.
        match self.dynamic_range {
            DynamicRange::Dr100 => {}
            DynamicRange::Dr200 => stages.push(curve(vec![
                [0.0, 0.0],
                [128.0, 128.0],
                [200.0, 194.0],
                [255.0, 244.0],
            ])),
            DynamicRange::Dr400 => stages.push(curve(vec![
                [0.0, 0.0],
                [110.0, 112.0],
                [190.0, 176.0],
                [255.0, 232.0],
            ])),
        }
        // 4. Base look.
        match (&cube, looks::find(&self.film_simulation)) {
            (Some(c), _) => stages.push(Adjustment::Lut3D {
                cube: c.clone(),
                strength: 100.0,
            }),
            (None, Some(look)) => stages.extend((look.build)()),
            (None, None) => {}
        }
        // 5. Highlight and shadow tone.
        if self.highlight != 0.0 || self.shadow != 0.0 {
            let hl = 208.0 + self.highlight * 7.0;
            let sh = 48.0 - self.shadow * 7.0;
            stages.push(curve(vec![
                [0.0, 0.0],
                [48.0, sh.clamp(8.0, 100.0)],
                [208.0, hl.clamp(150.0, 250.0)],
                [255.0, 255.0],
            ]));
        }
        // 6. Colour.
        if self.color != 0.0 {
            stages.push(Adjustment::HueSaturation {
                hue: 0.0,
                saturation: (self.color * 12.0).clamp(-100.0, 100.0),
                lightness: 0.0,
            });
        }
        // 7. Colour chrome effect: deepen saturated colours.
        let cce = match self.color_chrome_effect {
            Strength::Off => 0.0,
            Strength::Weak => 12.0,
            Strength::Strong => 25.0,
        };
        if cce > 0.0 {
            stages.push(Adjustment::Vibrance {
                vibrance: -cce * 0.6,
                saturation: cce * 0.5,
            });
        }
        // 8. Clarity as gentle midtone contrast (the local-contrast filter
        //    arrives with the filters crate).
        if self.clarity != 0.0 {
            let k = self.clarity * 4.0;
            stages.push(curve(vec![
                [0.0, 0.0],
                [64.0, (64.0 - k).clamp(0.0, 255.0)],
                [192.0, (192.0 + k).clamp(0.0, 255.0)],
                [255.0, 255.0],
            ]));
        }
        // 9. Grain, in document space.
        if self.grain.strength != Strength::Off {
            stages.push(Adjustment::Grain {
                amount: if self.grain.strength == Strength::Strong {
                    26.0
                } else {
                    13.0
                },
                size: if self.grain.size == GrainSize::Large {
                    2.2
                } else {
                    1.2
                },
                monochrome: true,
            });
        }
        // 10. Film and print character.
        if self.negative {
            // Invert, then the orange mask of colour negative stock.
            stages.push(curve(vec![[0.0, 255.0], [255.0, 0.0]]));
            stages.push(Adjustment::ColorBalance {
                shadows: [38.0, 14.0, -30.0],
                midtones: [22.0, 6.0, -18.0],
                highlights: [10.0, 2.0, -8.0],
                preserve_luminosity: false,
            });
        }
        if self.split_shadows != [0.0; 3] || self.split_highlights != [0.0; 3] {
            stages.push(Adjustment::ColorBalance {
                shadows: self.split_shadows.map(|v| v.clamp(-100.0, 100.0)),
                midtones: [0.0; 3],
                highlights: self.split_highlights.map(|v| v.clamp(-100.0, 100.0)),
                preserve_luminosity: true,
            });
        }
        if self.fade > 0.0 {
            let f = self.fade.clamp(0.0, 100.0);
            stages.push(curve(vec![
                [0.0, f * 0.55],
                [96.0, 96.0 + f * 0.25],
                [255.0, 255.0 - f * 0.22],
            ]));
        }
        if self.vignette != 0.0 {
            stages.push(Adjustment::Vignette {
                amount: self.vignette.clamp(-100.0, 100.0),
                midpoint: 42.0,
                feather: 65.0,
                roundness: 25.0,
            });
        }
        let mut group = Node::group(0, format!("Recipe · {}", self.name));
        group.blend = emulsion_raster::BlendMode::PassThrough;
        let mut children: Vec<Node> = stages
            .into_iter()
            .map(|a| {
                let mut n = Node::adjust(0, a);
                n.name = format!("{} · {}", self.name, n.name);
                n
            })
            .collect();
        if w > 0 && h > 0 {
            let placement = emulsion_raster::Placement::default();
            if self.light_leak > 0.0 {
                let r = effects::light_leak(
                    w,
                    h,
                    self.light_leak / 100.0,
                    self.leak_color,
                    self.leak_side,
                    self.seed,
                );
                let mut n = Node::raster(
                    0,
                    format!("{} · Light leak", self.name),
                    Arc::new(r),
                    placement,
                );
                n.blend = emulsion_raster::BlendMode::Screen;
                children.push(n);
            }
            if self.dust > 0.0 {
                let r = effects::dust(w, h, self.dust / 100.0, self.seed);
                let mut n =
                    Node::raster(0, format!("{} · Dust", self.name), Arc::new(r), placement);
                n.blend = emulsion_raster::BlendMode::Screen;
                n.opacity = 0.85;
                children.push(n);
            }
            if let Some(r) = effects::frame(w, h, self.frame, self.seed) {
                children.push(Node::raster(
                    0,
                    format!("{} · Frame", self.name),
                    Arc::new(r),
                    placement,
                ));
            }
            if self.date_stamp {
                let spec = effects::date_stamp(w, h, Some(&self.date));
                let mut n = Node::text(0, format!("{} · Date", self.name), spec, w, h);
                n.styles.push(emulsion_core::styles::LayerStyle::OuterGlow {
                    color: [255, 120, 30],
                    opacity: 0.55,
                    size: (w.min(h) as f32 * 0.006).max(1.5),
                });
                children.push(n);
            }
        }
        (group, children)
    }
}

fn curve(master: Vec<[f32; 2]>) -> Adjustment {
    Adjustment::Curves {
        master,
        red: straight_curve(),
        green: straight_curve(),
        blue: straight_curve(),
    }
}

/// "+1/3", "-2/3", "1", "+0.7", "2 1/3" pieces.
pub(crate) fn parse_fraction(tok: &str) -> Option<f32> {
    let t = tok.trim().trim_end_matches('.');
    if t.is_empty() || t.eq_ignore_ascii_case("to") {
        return None;
    }
    let (sign, body) = match t.chars().next()? {
        '+' => (1.0, &t[1..]),
        '-' => (-1.0, &t[1..]),
        _ => (1.0, t),
    };
    if let Some((n, d)) = body.split_once('/') {
        let (n, d) = (n.parse::<f32>().ok()?, d.parse::<f32>().ok()?);
        if d == 0.0 {
            return None;
        }
        return Some(sign * n / d);
    }
    body.parse::<f32>().ok().map(|v| sign * v)
}

/// The recipes Emulsion ships. Own approximations, described by mood.
pub fn starter_set() -> Vec<Recipe> {
    let base = |name: &str, sim: &str, tags: &[&str], notes: &str| Recipe {
        name: name.into(),
        author: "Emulsion starter set".into(),
        license: "CC0".into(),
        notes: notes.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        film_simulation: sim.into(),
        ..Recipe::default()
    };
    vec![
        Recipe {
            dynamic_range: DynamicRange::Dr400,
            grain: Grain {
                strength: Strength::Weak,
                size: GrainSize::Small,
            },
            color_chrome_effect: Strength::Strong,
            white_balance: WhiteBalance {
                preset: "daylight".into(),
                kelvin: None,
                red: 2,
                blue: -4,
            },
            highlight: -1.0,
            shadow: 1.0,
            color: 2.0,
            sharpness: -2.0,
            noise_reduction: -4.0,
            exposure_compensation: "+1/3 to +2/3".into(),
            ..base(
                "Chrome Street",
                "classic-chrome",
                &["street", "muted", "documentary"],
                "Muted, cool shadows, a touch of grain",
            )
        },
        Recipe {
            dynamic_range: DynamicRange::Dr200,
            grain: Grain {
                strength: Strength::Strong,
                size: GrainSize::Large,
            },
            color_chrome_effect: Strength::Strong,
            color_chrome_fx_blue: Strength::Weak,
            white_balance: WhiteBalance {
                preset: "auto".into(),
                kelvin: None,
                red: 3,
                blue: -5,
            },
            highlight: -2.0,
            shadow: -1.0,
            color: 2.0,
            clarity: -2.0,
            exposure_compensation: "+2/3 to +1 1/3".into(),
            ..base(
                "Negative Summer",
                "classic-negative",
                &["summer", "film", "warm"],
                "Bright, warm, grainy consumer-film feel",
            )
        },
        Recipe {
            color: 3.0,
            highlight: -1.0,
            shadow: 1.0,
            color_chrome_effect: Strength::Weak,
            white_balance: WhiteBalance {
                preset: "daylight".into(),
                kelvin: None,
                red: 1,
                blue: -1,
            },
            ..base(
                "Slide Punch",
                "velvia",
                &["landscape", "saturated", "slide"],
                "Deep saturated slide-film colour for landscapes",
            )
        },
        Recipe {
            highlight: -1.0,
            shadow: -1.0,
            color: 1.0,
            white_balance: WhiteBalance {
                preset: "auto".into(),
                kelvin: None,
                red: 2,
                blue: -2,
            },
            exposure_compensation: "+1/3".into(),
            ..base(
                "Portrait Soft",
                "astia",
                &["portrait", "soft", "skin"],
                "Soft contrast with gentle, warm skin",
            )
        },
        Recipe {
            dynamic_range: DynamicRange::Dr400,
            highlight: -2.0,
            shadow: -2.0,
            color: -2.0,
            grain: Grain {
                strength: Strength::Weak,
                size: GrainSize::Small,
            },
            white_balance: WhiteBalance {
                preset: "auto".into(),
                kelvin: None,
                red: 0,
                blue: 2,
            },
            ..base(
                "Cinema Flat",
                "eterna",
                &["cinematic", "flat", "video"],
                "Low contrast, desaturated, room to grade",
            )
        },
        Recipe {
            highlight: 1.0,
            shadow: 2.0,
            grain: Grain {
                strength: Strength::Strong,
                size: GrainSize::Small,
            },
            white_balance: WhiteBalance {
                preset: "auto".into(),
                kelvin: None,
                red: 1,
                blue: -3,
            },
            ..base(
                "Bleach Street",
                "eterna-bleach-bypass",
                &["gritty", "high contrast", "desaturated"],
                "Silver-retention look: harsh and nearly grey",
            )
        },
        Recipe {
            color: 1.0,
            highlight: -1.0,
            shadow: 0.0,
            grain: Grain {
                strength: Strength::Weak,
                size: GrainSize::Large,
            },
            white_balance: WhiteBalance {
                preset: "auto".into(),
                kelvin: None,
                red: 4,
                blue: -6,
            },
            exposure_compensation: "+1/3 to +2/3".into(),
            ..base(
                "Nostalgic Amber",
                "nostalgic-negative",
                &["warm", "nostalgic", "amber"],
                "Amber highlights and soft shadows like old prints",
            )
        },
        Recipe {
            highlight: 0.0,
            shadow: 1.0,
            grain: Grain {
                strength: Strength::Strong,
                size: GrainSize::Large,
            },
            dynamic_range: DynamicRange::Dr200,
            ..base(
                "Acros Grain",
                "acros-r",
                &["black and white", "grain", "red filter"],
                "Dramatic monochrome with a red filter and heavy grain",
            )
        },
        Recipe {
            highlight: -1.0,
            shadow: 1.0,
            color: 2.0,
            white_balance: WhiteBalance {
                preset: "daylight".into(),
                kelvin: None,
                red: 0,
                blue: 0,
            },
            ..base(
                "Faithful",
                "reala-ace",
                &["natural", "neutral", "everyday"],
                "True-to-life colour with a little contrast",
            )
        },
        Recipe {
            grain: Grain {
                strength: Strength::Weak,
                size: GrainSize::Small,
            },
            ..base(
                "Sepia Memory",
                "sepia",
                &["sepia", "vintage"],
                "Warm brown monochrome",
            )
        },
    ]
}

/// A group of nodes ready to add: the group plus children bottom to top.
pub type Compiled = (Node, Vec<Node>);

/// Compile a recipe, loading its LUT if it names one.
/// Compile with the canvas size, so leaks, dust, frames and the date stamp
/// are included.
pub fn compile_sized(recipe: &Recipe, w: u32, h: u32) -> Result<Compiled, RecipeError> {
    recipe.validate()?;
    if let Some(workflow) = &recipe.workflow {
        return Ok(workflow.compile());
    }
    let cube = match (&recipe.embedded_lut, &recipe.lut) {
        (Some(cube), _) => Some(cube.clone()),
        (None, Some(p)) => Some(load_cube(p)?),
        (None, None) => None,
    };
    Ok(recipe.compile_for(cube, w, h))
}

pub fn compile(recipe: &Recipe) -> Result<Compiled, RecipeError> {
    compile_sized(recipe, 0, 0)
}

fn load_cube(path: &str) -> Result<Cube, RecipeError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| RecipeError::Invalid(format!("cannot read LUT {path}: {e}")))?;
    Cube::parse(&text).map_err(|e| RecipeError::Invalid(format!("{path}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::NodeKind;

    #[test]
    fn toml_round_trip_and_validation() {
        for r in starter_set() {
            let t = r.to_toml();
            let back = Recipe::from_toml(&t).unwrap();
            assert_eq!(back, r, "{}", r.name);
        }
        assert!(Recipe::from_toml("name = \"x\"\nfilm_simulation = \"kodak\"").is_err());
        let r = Recipe::from_toml("name = \"Plain\"").unwrap();
        assert_eq!(r.film_simulation, "provia");
    }

    #[test]
    fn exposure_text_becomes_ev() {
        let mut r = Recipe {
            exposure_compensation: "+1/3 to +2/3".into(),
            ..Recipe::default()
        };
        assert!((r.exposure_ev() - 0.5).abs() < 1e-3);
        r.exposure_compensation = "-2/3".into();
        assert!((r.exposure_ev() + 0.667).abs() < 1e-2);
        r.exposure_compensation = "0 to +1".into();
        assert!((r.exposure_ev() - 0.5).abs() < 1e-3);
        r.exposure_compensation = String::new();
        assert_eq!(r.exposure_ev(), 0.0);
    }

    #[test]
    fn compiles_in_camera_order_with_every_stage() {
        let r = &starter_set()[0]; // Chrome Street
        let (group, kids) = r.compile(None);
        assert!(group.is_group());
        let kinds: Vec<&str> = kids
            .iter()
            .map(|n| match &n.kind {
                NodeKind::Adjust(a) => a.key(),
                _ => "?",
            })
            .collect();
        // WB preset, WB shift, exposure, DR curve, look stages…, tone curve, colour, CCE, grain.
        assert_eq!(
            &kinds[..4],
            &["white_balance", "color_balance", "exposure", "curves"]
        );
        assert_eq!(kinds.last(), Some(&"grain"));
        assert!(kinds.contains(&"vibrance") && kinds.contains(&"hue_saturation"));
        for k in &kids {
            assert!(k.name.starts_with("Chrome Street · "));
        }
        // A monochrome look ends up grey.
        let (_, kids) = starter_set()
            .iter()
            .find(|r| r.name == "Acros Grain")
            .unwrap()
            .compile(None);
        assert!(
            kids.iter()
                .any(|n| matches!(&n.kind, NodeKind::Adjust(Adjustment::BlackAndWhite { .. })))
        );
    }
}
