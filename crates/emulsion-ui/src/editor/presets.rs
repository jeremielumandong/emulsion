//! The brush panel: the built-in library by medium, plus brushes the
//! person saves. Saved brushes live in `<data dir>/brush-presets.json`.

use super::*;
use emulsion_raster::library::{self, BrushPreset, CATEGORIES};
use emulsion_raster::paint::{Brush, BrushBlend, GrainKind};

pub const MAX_PRESETS: usize = 400;

fn file() -> PathBuf {
    emulsion_io::recent::data_dir().join("brush-presets.json")
}

/// Saved brushes at `path`. A missing file reads as none. A file that exists
/// but cannot be parsed is moved aside to `<path>.bak` (returned) so a later
/// save cannot overwrite it; if it cannot be read or moved, this is an error
/// and nothing may be saved over it.
fn load_from(path: &std::path::Path) -> std::io::Result<(Vec<BrushPreset>, Option<PathBuf>)> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), None)),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<Vec<BrushPreset>>(&bytes) {
        Ok(mut v) => {
            v.truncate(MAX_PRESETS);
            for p in &mut v {
                p.brush = p.brush.sanitized();
            }
            Ok((v, None))
        }
        Err(_) => {
            let mut bak = path.as_os_str().to_owned();
            bak.push(".bak");
            let bak = PathBuf::from(bak);
            std::fs::rename(path, &bak)?;
            Ok((Vec::new(), Some(bak)))
        }
    }
}

fn save_to(path: &std::path::Path, presets: &[BrushPreset]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(presets).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

fn save(presets: &[BrushPreset]) -> std::io::Result<()> {
    save_to(&file(), presets)
}

#[cfg(test)]
mod identity_tests {
    use super::matches;
    use emulsion_raster::paint::Brush;

    #[test]
    fn dynamics_and_texture_changes_are_distinct_presets() {
        let original = Brush::default();
        for changed in [
            Brush {
                size_pressure: 0.6,
                ..original
            },
            Brush {
                tilt: 0.8,
                ..original
            },
            Brush {
                spacing: 0.72,
                ..original
            },
            Brush {
                taper_end: 30.0,
                ..original
            },
            Brush {
                tip: 42,
                ..original
            },
            Brush {
                grain_tex: 17,
                ..original
            },
        ] {
            assert!(!matches(&original, &changed));
        }
        assert!(matches(&original, &original));
    }
}

/// Every setting affects a brush; dynamics and textures must survive saving.
pub(crate) fn matches(a: &Brush, b: &Brush) -> bool {
    a.sanitized() == b.sanitized()
}

#[derive(Default)]
pub(crate) struct PresetState {
    pub open: bool,
    /// Category shown; None = the person's saved brushes.
    pub category: Option<String>,
    /// Loaded on first open.
    saved: Option<Vec<BrushPreset>>,
    /// Name of the brush last picked, for the context bar.
    pub current: Option<String>,
}

impl EditorView {
    /// Load the saved brushes on first use. False when the file could not be
    /// read or set aside: the caller must not save over it.
    fn ensure_saved_presets(&mut self, cx: &mut Context<Self>) -> bool {
        if self.presets.saved.is_some() {
            return true;
        }
        match load_from(&file()) {
            Ok((v, bak)) => {
                self.presets.saved = Some(v);
                if let Some(bak) = bak {
                    self.set_status(
                        format!("Saved brushes were unreadable; moved to {}.", bak.display()),
                        true,
                        cx,
                    );
                }
                true
            }
            Err(e) => {
                self.set_status(format!("Could not read saved brushes: {e}"), true, cx);
                false
            }
        }
    }

    pub fn toggle_presets(&mut self, cx: &mut Context<Self>) {
        let tab = if self.sidebar_tab == SidebarTab::BrushPresets {
            SidebarTab::History
        } else {
            SidebarTab::BrushPresets
        };
        self.select_sidebar(tab, cx);
        self.presets.open = tab == SidebarTab::BrushPresets;
        self.prepare_presets(cx);
        cx.notify();
    }

    pub(super) fn prepare_presets(&mut self, cx: &mut Context<Self>) {
        self.ensure_saved_presets(cx);
        if self.presets.category.is_none() && self.presets.saved.as_ref().is_none_or(Vec::is_empty)
        {
            self.presets.category = Some(CATEGORIES[0].into());
        }
        cx.notify();
    }

    /// The current brush settings.
    pub fn brush(&self) -> Brush {
        self.tools.brush
    }

    /// Switch to a brush. Erasers and smudges also switch the paint kind,
    /// so picking "Soft eraser" erases without another click.
    pub fn apply_preset(&mut self, p: &BrushPreset, cx: &mut Context<Self>) {
        if !matches!(self.tool, Tool::Brush | Tool::Heal | Tool::Clone) {
            self.set_tool(Tool::Brush, cx);
        }
        if self.tool == Tool::Brush {
            let kind = match p.category.as_str() {
                "Eraser" => PaintKind::Eraser,
                "Smudge" => PaintKind::Smudge,
                _ => PaintKind::Brush,
            };
            self.set_paint(kind, cx);
        }
        self.tools.brush = p.brush.sanitized();
        self.presets.current = Some(p.name.clone());
        cx.notify();
    }

    /// Selecting a medium also selects a brush, rather than merely filtering
    /// the list while silently leaving the previous medium active.
    pub(crate) fn select_brush_category(&mut self, category: &str, cx: &mut Context<Self>) {
        let presets: Vec<_> = library::library()
            .into_iter()
            .filter(|p| p.category == category)
            .collect();
        let Some(first) = presets.first() else { return };
        self.presets.category = Some(category.into());
        // Reopening the active medium must preserve the user's adjustments.
        let already_active = presets
            .iter()
            .any(|p| self.presets.current.as_deref() == Some(p.name.as_str()));
        if !already_active {
            self.apply_preset(first, cx);
        }
        cx.notify();
    }

    /// Pick a brush by name, built-in or saved.
    pub fn apply_preset_named(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let found = library::find(name).or_else(|| {
            self.ensure_saved_presets(cx);
            let n = name.trim().to_lowercase();
            self.presets
                .saved
                .iter()
                .flatten()
                .find(|p| p.name.to_lowercase() == n)
                .cloned()
        });
        match found {
            Some(p) => {
                self.apply_preset(&p, cx);
                true
            }
            None => false,
        }
    }

    /// Import Procreate `.brushset` / `.brush` files into the saved brushes,
    /// keeping their tip and grain images.
    pub fn import_brushes(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import brushes".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let imported = cx
                .background_spawn(async move {
                    let mut out: Vec<Result<Vec<emulsion_io::brushset::Imported>, String>> =
                        Vec::new();
                    for p in paths {
                        let r = emulsion_io::brushset::import(&p)
                            .and_then(|list| {
                                for brush in &list {
                                    for png in
                                        [&brush.shape_png, &brush.grain_png].into_iter().flatten()
                                    {
                                        emulsion_io::brushset::store_texture(png)?;
                                    }
                                }
                                Ok(list)
                            })
                            .map_err(|e| format!("{}: {e}", p.display()));
                        out.push(r);
                    }
                    out
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.ensure_saved_presets(cx) {
                    return;
                }
                let Some(saved) = this.presets.saved.as_mut() else {
                    return;
                };
                let previous = saved.clone();
                let (mut added, mut errors) = (0usize, Vec::new());
                for r in imported {
                    match r {
                        Ok(list) => {
                            for b in list {
                                if saved.len() >= MAX_PRESETS {
                                    break;
                                }
                                if !saved.iter().any(|q| {
                                    q.name == b.preset.name && q.category == b.preset.category
                                }) {
                                    saved.push(b.preset);
                                    added += 1;
                                }
                            }
                        }
                        Err(e) => errors.push(e),
                    }
                }
                if let Err(error) = save(saved) {
                    *saved = previous;
                    added = 0;
                    errors.push(format!("Could not save imported brushes: {error}"));
                }
                this.presets.category = None;
                if this.sidebar_tab != SidebarTab::BrushSettings {
                    this.presets.open = true;
                    this.select_sidebar(SidebarTab::BrushPresets, cx);
                }
                if errors.is_empty() {
                    this.set_status(
                        format!(
                            "Imported {added} brush{}.",
                            if added == 1 { "" } else { "es" }
                        ),
                        false,
                        cx,
                    );
                } else {
                    this.set_status(format!("Imported {added}; {}", errors.join("; ")), true, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Save the current brush under a name describing it.
    pub fn save_preset(&mut self, cx: &mut Context<Self>) {
        let b = self.tools.brush;
        if !self.ensure_saved_presets(cx) {
            return;
        }
        let Some(saved) = self.presets.saved.as_mut() else {
            return;
        };
        if saved
            .iter()
            .chain(&library::library())
            .any(|p| matches(&p.brush, &b))
        {
            self.set_status("That brush is already in the panel.", false, cx);
            return;
        }
        if saved.len() >= MAX_PRESETS {
            self.set_status(
                format!("Remove a brush first; {MAX_PRESETS} is the limit."),
                true,
                cx,
            );
            return;
        }
        let medium = match (b.grain, b.wetness > 0.3, b.blend) {
            (_, _, BrushBlend::Multiply) => "Marker",
            (_, true, _) => "Wet",
            (GrainKind::Chalk, _, _) => "Chalk",
            (GrainKind::None, _, _) if b.hardness >= 0.5 => "Round",
            (GrainKind::None, _, _) => "Soft",
            _ => "Textured",
        };
        let name = format!(
            "{medium} {:.0} · {:.0}%{}",
            b.size,
            b.hardness * 100.0,
            if b.flow < 0.99 {
                format!(" · flow {:.0}%", b.flow * 100.0)
            } else {
                String::new()
            }
        );
        saved.push(BrushPreset {
            name: name.clone(),
            category: "Mine".into(),
            note: "Saved from the current settings".into(),
            brush: b,
        });
        let r = save(saved);
        self.presets.category = None;
        self.presets.current = Some(name);
        match r {
            Ok(()) => self.set_status("Saved to My brushes.", false, cx),
            Err(e) => self.set_status(format!("Could not save the brush: {e}"), true, cx),
        }
    }

    fn delete_preset(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(saved) = &mut self.presets.saved else {
            return;
        };
        if i < saved.len() {
            saved.remove(i);
            if let Err(e) = save(saved) {
                self.set_status(format!("Could not save brushes: {e}"), true, cx);
            }
            cx.notify();
        }
    }

    pub(crate) fn presets_view(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let embedded = self.sidebar_tab == SidebarTab::BrushSettings
            && self.brush_settings_section == tools::BrushSettingsSection::Presets;
        if !self.presets.open && !embedded {
            return None;
        }
        let b = self.tools.brush;
        let saved = self.presets.saved.clone().unwrap_or_default();
        let cat = self.presets.category.clone();
        let mut tabs = div().flex().flex_wrap().items_center().gap_1();
        for (ci, c) in CATEGORIES.iter().enumerate() {
            let on = cat.as_deref() == Some(c);
            let name: String = (*c).into();
            tabs = tabs.child(
                chip(("bcat", ci), *c, on, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_brush_category(&name, cx);
                        window.focus(&this.canvas_focus, cx);
                    })),
            );
        }
        tabs = tabs.child(
            chip(
                "bcat-mine",
                format!("Mine ({})", saved.len()),
                cat.is_none(),
                p,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.presets.category = None;
                cx.notify();
            })),
        );
        let mut row = div().flex().flex_col().w_full().gap_1();
        let shown: Vec<(usize, BrushPreset, bool)> = match &cat {
            Some(c) => library::library()
                .into_iter()
                .filter(|q| q.category == *c)
                .enumerate()
                .map(|(i, q)| (i, q, false))
                .collect(),
            None => saved
                .into_iter()
                .enumerate()
                .map(|(i, q)| (i, q, true))
                .collect(),
        };
        if shown.is_empty() {
            row = row.child(mono(
                "No saved brushes yet. Adjust one and press Save current.",
                10.5,
                p.muted,
            ));
        }
        let mut note = None;
        for (i, preset, mine) in shown {
            let on = matches(&preset.brush, &b);
            if on {
                note = Some(preset.note.clone());
            }
            let text = preset.name.clone();
            let apply = preset.clone();
            let mut entry = div().flex().items_center().gap_1().child(
                chip((if mine { "preset-u" } else { "preset-b" }, i), text, on, p)
                    .flex_1()
                    .test_support()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply_preset(&apply, cx);
                        window.focus(&this.canvas_focus, cx);
                    })),
            );
            if mine {
                entry =
                    entry
                        .child(chip(("preset-del", i), "×", false, p).on_click(
                            cx.listener(move |this, _, _, cx| this.delete_preset(i, cx)),
                        ));
            }
            row = row.child(entry);
        }
        Some(
            div()
                .id("brush-presets-panel")
                .flex()
                .flex_col()
                .min_w_0()
                .w_full()
                .gap_3()
                .p_3()
                .when(embedded, |view| view.p_0())
                .bg(p.panel)
                .when(!embedded, |view| {
                    view.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(label("Brush presets", p))
                            .child(
                                button("preset-close", "Close", false, p)
                                    .test_support()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_sidebar(SidebarTab::History, cx);
                                        this.presets.open = false;
                                        window.focus(&this.canvas_focus, cx);
                                    })),
                            ),
                    )
                })
                .child(tabs)
                .child(row)
                .children(note.map(|n| mono(n, 10., p.muted)))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(
                            button("preset-save", "Save current", false, p)
                                .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
                        )
                        .child(
                            button("bcat-import", "Import…", false, p)
                                .on_click(cx.listener(|this, _, _, cx| this.import_brushes(cx))),
                        ),
                )
                .test_support(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Brush, library, load_from, matches, save_to};

    #[test]
    fn a_corrupt_presets_file_survives_the_next_save() {
        let dir = std::env::temp_dir().join(format!(
            "emulsion-presets-corrupt-{}-{}",
            std::process::id(),
            emulsion_io::recent::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brush-presets.json");
        let corrupt = b"[{\"name\": \"My favourite\", \"brush\": {truncated";
        std::fs::write(&path, corrupt).unwrap();

        let (saved, bak) = load_from(&path).unwrap();
        assert!(saved.is_empty());
        let bak = bak.expect("the unreadable file is set aside");
        save_to(&path, &library::library()[..1]).unwrap();

        assert_eq!(std::fs::read(&bak).unwrap(), corrupt);
        assert_eq!(load_from(&path).unwrap().0, library::library()[..1]);
        // A missing file is simply empty, not an error.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(load_from(&path).unwrap(), (Vec::new(), None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn library_brushes_round_trip_through_json() {
        let lib = library::library();
        let json = serde_json::to_string(&lib).unwrap();
        let back: Vec<library::BrushPreset> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, lib);
        // Files from before the engine grew still load: old fields, new defaults.
        let old = r#"[{"name":"x","category":"Mine","note":"","brush":{"size":12.0,"hardness":1.0,"opacity":1.0,"flow":1.0,"spacing":0.1}}]"#;
        let v: Vec<library::BrushPreset> = serde_json::from_str(old).unwrap();
        assert!(matches(
            &v[0].brush,
            &Brush {
                size: 12.0,
                hardness: 1.0,
                spacing: 0.1,
                ..Default::default()
            }
        ));
    }
}
