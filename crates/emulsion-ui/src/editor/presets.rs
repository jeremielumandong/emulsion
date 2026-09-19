//! Brush presets: a few built-in brushes plus ones the person saves.
//!
//! Saved presets live in `<data dir>/brush-presets.json` and apply to the
//! Brush, Eraser, Heal and Clone tools alike.

use super::*;
use emulsion_raster::paint::Brush;
use serde::{Deserialize, Serialize};

pub const MAX_PRESETS: usize = 48;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub size: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub flow: f32,
    pub spacing: f32,
}

impl Preset {
    fn builtin(name: &str, size: f32, hardness: f32, opacity: f32, flow: f32) -> Self {
        Self {
            name: name.into(),
            size,
            hardness,
            opacity,
            flow,
            spacing: 0.12,
        }
    }

    pub fn from_brush(name: String, b: &Brush) -> Self {
        Self {
            name,
            size: b.size,
            hardness: b.hardness,
            opacity: b.opacity,
            flow: b.flow,
            spacing: b.spacing,
        }
    }

    pub fn apply(&self, b: &mut Brush) {
        b.size = self.size.clamp(1.0, 500.0);
        b.hardness = self.hardness.clamp(0.0, 1.0);
        b.opacity = self.opacity.clamp(0.01, 1.0);
        b.flow = self.flow.clamp(0.01, 1.0);
        b.spacing = self.spacing.clamp(0.02, 1.0);
    }

    fn matches(&self, b: &Brush) -> bool {
        (self.size - b.size).abs() < 0.5
            && (self.hardness - b.hardness).abs() < 0.01
            && (self.opacity - b.opacity).abs() < 0.01
            && (self.flow - b.flow).abs() < 0.01
    }
}

pub fn builtin() -> Vec<Preset> {
    vec![
        Preset::builtin("Hard round 12", 12.0, 1.0, 1.0, 1.0),
        Preset::builtin("Round 40", 40.0, 0.8, 1.0, 1.0),
        Preset::builtin("Soft round 80", 80.0, 0.0, 1.0, 1.0),
        Preset::builtin("Airbrush 200", 200.0, 0.0, 1.0, 0.1),
        Preset::builtin("Glaze 120", 120.0, 0.3, 0.3, 1.0),
        Preset::builtin("Detail 3", 3.0, 1.0, 1.0, 1.0),
    ]
}

fn file() -> PathBuf {
    emulsion_io::recent::data_dir().join("brush-presets.json")
}

/// Saved presets; an unreadable file reads as none.
pub fn load() -> Vec<Preset> {
    std::fs::read(file())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Preset>>(&b).ok())
        .map(|mut v| {
            v.truncate(MAX_PRESETS);
            v
        })
        .unwrap_or_default()
}

fn save(presets: &[Preset]) -> std::io::Result<()> {
    std::fs::create_dir_all(emulsion_io::recent::data_dir())?;
    let bytes = serde_json::to_vec_pretty(presets).map_err(std::io::Error::other)?;
    let tmp = file().with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, file())
}

#[derive(Default)]
pub(crate) struct PresetState {
    pub open: bool,
    /// Loaded on first open.
    saved: Option<Vec<Preset>>,
}

impl EditorView {
    pub fn toggle_presets(&mut self, cx: &mut Context<Self>) {
        self.presets.open = !self.presets.open;
        if self.presets.saved.is_none() {
            self.presets.saved = Some(load());
        }
        cx.notify();
    }

    /// The current brush settings.
    pub fn brush(&self) -> Brush {
        self.tools.brush
    }

    pub fn apply_preset(&mut self, p: &Preset, cx: &mut Context<Self>) {
        p.apply(&mut self.tools.brush);
        cx.notify();
    }

    /// Save the current brush under a name describing it.
    pub fn save_preset(&mut self, cx: &mut Context<Self>) {
        let b = self.tools.brush;
        let saved = self.presets.saved.get_or_insert_with(load);
        if saved.iter().chain(&builtin()).any(|p| p.matches(&b)) {
            self.set_status("That brush is already a preset.", false, cx);
            return;
        }
        if saved.len() >= MAX_PRESETS {
            self.set_status("Remove a preset first; 48 is the limit.", true, cx);
            return;
        }
        let name = format!(
            "{} {:.0} · {:.0}%{}",
            if b.hardness >= 0.5 { "Round" } else { "Soft" },
            b.size,
            b.hardness * 100.0,
            if b.flow < 0.99 {
                format!(" · flow {:.0}%", b.flow * 100.0)
            } else {
                String::new()
            }
        );
        saved.push(Preset::from_brush(name, &b));
        let r = save(saved);
        match r {
            Ok(()) => self.set_status("Saved as a preset.", false, cx),
            Err(e) => self.set_status(format!("Could not save the preset: {e}"), true, cx),
        }
    }

    fn delete_preset(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(saved) = &mut self.presets.saved else {
            return;
        };
        if i < saved.len() {
            saved.remove(i);
            if let Err(e) = save(saved) {
                self.set_status(format!("Could not save presets: {e}"), true, cx);
            }
            cx.notify();
        }
    }

    pub(crate) fn presets_view(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let brushy = matches!(self.tool, Tool::Brush | Tool::Heal | Tool::Clone)
            && !(self.tool == Tool::Brush
                && matches!(self.tools.paint, PaintKind::Bucket | PaintKind::Gradient));
        if !self.presets.open || !brushy {
            return None;
        }
        let b = self.tools.brush;
        let saved = self.presets.saved.clone().unwrap_or_default();
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(16.))
            .py(px(8.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
            .child(label("Brush presets", p));
        for (i, preset) in builtin().into_iter().enumerate() {
            let on = preset.matches(&b);
            let text = preset.name.clone();
            row = row.child(
                chip(("preset-b", i), text, on, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.apply_preset(&preset, cx))),
            );
        }
        for (i, preset) in saved.into_iter().enumerate() {
            let on = preset.matches(&b);
            let text = preset.name.clone();
            row = row
                .child(
                    chip(("preset-u", i), text, on, p).on_click(
                        cx.listener(move |this, _, _, cx| this.apply_preset(&preset, cx)),
                    ),
                )
                .child(
                    chip(("preset-del", i), "×", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_preset(i, cx))),
                );
        }
        Some(
            row.child(div().flex_1()).child(
                button("preset-save", "Save current", false, p)
                    .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Brush, builtin};
    use serde_json;
    type Preset = super::Preset;

    #[test]
    fn presets_apply_and_match() {
        let mut b = Brush::default();
        let air = builtin()
            .into_iter()
            .find(|p| p.name.starts_with("Airbrush"))
            .unwrap();
        air.apply(&mut b);
        assert_eq!((b.size, b.hardness, b.flow), (200.0, 0.0, 0.1));
        assert!(air.matches(&b));
        let json = serde_json::to_string(&vec![air.clone()]).unwrap();
        let back: Vec<Preset> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0], air);
    }
}
