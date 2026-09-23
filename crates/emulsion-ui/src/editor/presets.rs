//! Shared catalog and the compact library entry point.
use super::*;
use emulsion_io::brush_library::{self as store, Catalog};
use emulsion_raster::library::{BrushPreset, CATEGORIES};
use emulsion_raster::paint::Brush;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

pub(crate) struct LibraryState {
    pub catalog: Catalog,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub pending_import: Option<(Catalog, Vec<String>, usize, Catalog)>,
}
struct SharedLibrary(Entity<LibraryState>);
impl Global for SharedLibrary {}

pub(crate) fn shared_library(cx: &mut App) -> Entity<LibraryState> {
    if let Some(shared) = cx.try_global::<SharedLibrary>() {
        return shared.0.clone();
    }
    let (catalog, error, warnings) = match store::load_with_report() {
        Ok(report) => (report.catalog, None, report.warnings),
        Err(error) => (Catalog::builtin(), Some(error.to_string()), Vec::new()),
    };
    let state = cx.new(|_| LibraryState {
        catalog,
        error,
        warnings,
        pending_import: None,
    });
    cx.set_global(SharedLibrary(state.clone()));
    state
}
impl LibraryState {
    pub fn commit(&mut self, draft: Catalog, cx: &mut Context<Self>) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(format!("Library could not be loaded: {error}"));
        }
        match store::commit(draft.revision, &draft) {
            Ok(catalog) => {
                self.catalog = catalog;
                cx.notify();
                Ok(())
            }
            Err(store::StoreError::Conflict) => {
                match store::load_with_report() {
                    Ok(report) => {
                        self.catalog = report.catalog;
                        self.warnings = report.warnings;
                    }
                    Err(error) => {
                        self.error = Some(error.to_string());
                    }
                }
                cx.notify();
                Err("The library changed elsewhere and has been reloaded. This change was not published. Retry the library action, restart the import, or save your Studio draft as a new brush.".into())
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

#[derive(Default)]
pub(crate) struct PresetState {
    pub open: bool,
    pub category: Option<String>,
    pub current: Option<String>,
    pub current_id: Option<String>,
    pub definition: Option<Brush>,
    pub definitions: HashMap<tools::BrushSlot, Option<Brush>>,
    pub ids: HashMap<tools::BrushSlot, Option<String>>,
    pub library: Option<Entity<LibraryState>>,
    subscription: Option<Subscription>,
}
impl EditorView {
    pub(crate) fn publish_brush_catalog(report: store::LoadReport, cx: &mut App) {
        if let Some(shared) = cx.try_global::<SharedLibrary>() {
            let library = shared.0.clone();
            library.update(cx, |state, cx| {
                // A newer UI transaction may have finished during the reload.
                if report.catalog.revision >= state.catalog.revision {
                    state.catalog = report.catalog;
                    state.warnings = report.warnings;
                    state.error = None;
                    cx.notify();
                }
            });
        } else {
            let state = cx.new(|_| LibraryState {
                catalog: report.catalog,
                warnings: report.warnings,
                error: None,
                pending_import: None,
            });
            cx.set_global(SharedLibrary(state));
        }
    }
    pub(super) fn prepare_presets(&mut self, cx: &mut Context<Self>) {
        if self.presets.library.is_none() {
            let library = shared_library(cx);
            self.presets.subscription = Some(cx.observe(&library, |this, library, cx| {
                if let Some(id) = this.presets.current_id.as_ref() {
                    match library.read(cx).catalog.brush(id) {
                        Some(definition) => {
                            if this.presets.definition != Some(definition.brush) {
                                let mut brush = definition.brush;
                                if let Some(old) = this.presets.definition {
                                    if this.tools.brush.size != old.size {
                                        brush.size = this.tools.brush.size;
                                    }
                                    if this.tools.brush.opacity != old.opacity {
                                        brush.opacity = this.tools.brush.opacity;
                                    }
                                }
                                this.tools.brush = brush;
                                this.presets.definition = Some(definition.brush);
                            }
                            this.presets.current = Some(definition.name.clone());
                        }
                        None => {
                            this.presets.current_id = None;
                            this.presets.definition = None;
                        }
                    }
                }
                cx.notify();
            }));
            self.presets.library = Some(library);
        }
        if self.presets.category.is_none() {
            self.presets.category = Some(CATEGORIES[0].into());
        }
        cx.notify();
    }
    pub fn toggle_presets(&mut self, cx: &mut Context<Self>) {
        let tab = if self.sidebar_tab == SidebarTab::BrushPresets {
            SidebarTab::History
        } else {
            SidebarTab::BrushPresets
        };
        self.select_sidebar(tab, cx);
        self.prepare_presets(cx);
    }
    pub fn brush(&self) -> Brush {
        self.tools.brush
    }
    /// Brush selection preserves the active painting operation.
    pub fn apply_preset(&mut self, preset: &BrushPreset, cx: &mut Context<Self>) {
        self.remember_active_brush(cx);
        self.finish_tool_interaction(cx);
        if !matches!(
            self.tool,
            Tool::Brush | Tool::Heal | Tool::Clone | Tool::Mask
        ) || (self.tool == Tool::Brush
            && !matches!(
                self.tools.paint,
                PaintKind::Brush | PaintKind::Eraser | PaintKind::Smudge
            ))
        {
            self.set_paint(PaintKind::Brush, cx);
        }
        self.tools.brush = preset.brush.sanitized();
        self.presets.current = Some(preset.name.clone());
        self.presets.current_id = None;
        self.presets.definition = None;
        cx.notify();
    }
    pub(crate) fn apply_brush_id(&mut self, id: &str, cx: &mut Context<Self>) {
        self.prepare_presets(cx);
        let library = self.presets.library.as_ref().unwrap().clone();
        let Some(definition) = library.read(cx).catalog.brush(id).cloned() else {
            return;
        };
        self.apply_preset(
            &BrushPreset {
                name: definition.name,
                category: String::new(),
                note: definition.note,
                brush: definition.brush,
            },
            cx,
        );
        self.presets.current_id = Some(id.to_owned());
        self.presets.definition = Some(definition.brush);
        self.restore_active_brush_memory(cx);
        let mut draft = library.read(cx).catalog.clone();
        if draft.recent.first().is_some_and(|recent| recent == id) {
            return;
        }
        let _ = draft.record_use(id);
        if let Err(error) = library.update(cx, |state, cx| state.commit(draft, cx)) {
            self.set_status(
                format!("Brush selected; couldn't save recent brushes: {error}"),
                true,
                cx,
            );
        }
    }
    pub(crate) fn select_brush_category(&mut self, category: &str, cx: &mut Context<Self>) {
        self.presets.category = Some(category.into());
        cx.notify();
    }
    pub fn apply_preset_named(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        self.prepare_presets(cx);
        let found = self.presets.library.as_ref().and_then(|state| {
            state
                .read(cx)
                .catalog
                .brushes
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(name.trim()))
                .map(|b| b.id.clone())
        });
        if let Some(id) = found {
            self.apply_brush_id(&id, cx);
            true
        } else {
            false
        }
    }
    pub(crate) fn open_brush_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_tool_interaction(cx);
        self.prepare_presets(cx);
        let owner = cx.entity().downgrade();
        let library = self.presets.library.as_ref().unwrap().clone();
        let selected = self.presets.current_id.clone();
        let workspace = cx.new(|cx| {
            super::brush_library_ui::BrushWorkspace::new(owner, library, selected, window, cx)
        });
        self.brush_workspace = Some(workspace);
        cx.notify();
    }
    pub fn save_preset(&mut self, cx: &mut Context<Self>) {
        self.prepare_presets(cx);
        let library = self.presets.library.as_ref().unwrap().clone();
        let mut draft = library.read(cx).catalog.clone();
        let result = (|| -> Result<String, String> {
            let library_id = draft.libraries.first().ok_or("No library")?.id.clone();
            let set = match draft.sets.iter().find(|s| s.name == "My brushes") {
                Some(set) => set.id.clone(),
                None => draft
                    .create_set(&library_id, "My brushes")
                    .map_err(|e| e.to_string())?,
            };
            let source = self
                .presets
                .current_id
                .as_deref()
                .and_then(|id| draft.brush(id))
                .cloned();
            let id = draft
                .add_brush(&set, "New brush", self.tools.brush)
                .map_err(|e| e.to_string())?;
            if let Some(mut source) = source {
                source.id = id.clone();
                source.set_id = set;
                source.name = "New brush".into();
                source.builtin = false;
                if source.brush.tip != self.tools.brush.tip {
                    source.shape_asset = None;
                }
                if source.brush.grain_tex != self.tools.brush.grain_tex {
                    source.grain_asset = None;
                }
                source.brush = self.tools.brush.sanitized();
                *draft.brush_mut(&id).unwrap() = source;
            }
            Ok(id)
        })();
        match result {
            Ok(id) => match library.update(cx, |state, cx| state.commit(draft, cx)) {
                Ok(()) => {
                    self.presets.current_id = Some(id);
                    self.presets.definition = Some(self.tools.brush);
                    self.presets.current = Some("New brush".into());
                    self.set_status(
                        "Saved to My brushes. Open Brush Studio to name and edit it.",
                        false,
                        cx,
                    );
                }
                Err(e) => self.set_status(format!("Couldn't save brush: {e}"), true, cx),
            },
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub fn import_brushes(&mut self, cx: &mut Context<Self>) {
        self.prepare_presets(cx);
        let library = self.presets.library.as_ref().unwrap().clone();
        let mut draft = library.read(cx).catalog.clone();
        let original = draft.clone();
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
            let result = cx
                .background_spawn(async move {
                    let parent = draft
                        .libraries
                        .first()
                        .ok_or_else(|| anyhow::anyhow!("No library"))?
                        .id
                        .clone();
                    let set = draft.create_set(&parent, "Imported")?;
                    let report = store::import_paths(&mut draft, &paths, &set)?;
                    Ok::<_, anyhow::Error>((draft, report))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((draft, report)) => {
                    let count = report.added.len();
                    library.update(cx, |state, cx| {
                        state.pending_import = Some((draft, report.warnings, count, original));
                        cx.notify();
                    });
                    this.set_status(
                        format!(
                            "{count} brushes ready. Review the import report in the brush library."
                        ),
                        false,
                        cx,
                    );
                }
                Err(error) => {
                    this.set_status(format!("Couldn't import brushes: {error}"), true, cx)
                }
            })
            .ok();
        })
        .detach();
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
        let mut rows = div().flex().flex_col().gap_1();
        if let Some(library) = &self.presets.library {
            rows = rows.child(import_review(library, cx));
        }
        if let Some(state) = &self.presets.library {
            let catalog = &state.read(cx).catalog;
            for brush in catalog
                .brushes
                .iter()
                .filter(|b| {
                    catalog.sets.iter().any(|s| {
                        s.id == b.set_id
                            && Some(s.name.as_str()) == self.presets.category.as_deref()
                    })
                })
                .take(30)
            {
                let id = brush.id.clone();
                rows = rows.child(
                    Button::new(SharedString::from(format!("brush-{id}")))
                        .ghost()
                        .small()
                        .label(brush.name.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.apply_brush_id(&id, cx);
                            window.focus(&this.canvas_focus, cx);
                        })),
                );
            }
        }
        let mut categories = div().flex().flex_wrap().gap_1();
        for (index, name) in CATEGORIES.iter().enumerate() {
            categories = categories.child(
                chip(
                    ("bcat", index),
                    *name,
                    self.presets.category.as_deref() == Some(name),
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.select_brush_category(name, cx))),
            );
        }
        Some(
            div()
                .id("brush-presets-panel")
                .test_support()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .child(
                    Button::new("open-brush-library")
                        .label("Brush library…")
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open_brush_workspace(window, cx)
                            }),
                        ),
                )
                .child(categories)
                .child(rows)
                .child(
                    Button::new("preset-save")
                        .label("Save current")
                        .on_click(cx.listener(|this, _, _, cx| this.save_preset(cx))),
                )
                .child(
                    Button::new("bcat-import")
                        .label("Import…")
                        .on_click(cx.listener(|this, _, _, cx| this.import_brushes(cx))),
                ),
        )
    }
}

/// The draft stays unpublished until conversion warnings can be reviewed.
pub(super) fn import_review(library: &Entity<LibraryState>, cx: &mut App) -> AnyElement {
    let Some((_, warnings, count, _)) = &library.read(cx).pending_import else {
        return div().into_any_element();
    };
    let mut report = div()
        .id("brush-import-report")
        .max_h_48()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();
    for warning in warnings {
        report = report.child(div().text_sm().child(warning.clone()));
    }
    let view = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(format!("Import ready: {count} brushes"))
        .child(report);
    let accept = library.clone();
    let cancel = library.clone();
    view.child(
        div()
            .flex()
            .gap_2()
            .child(
                Button::new("confirm-brush-import")
                    .label("Import brushes")
                    .on_click(move |_, _, cx| {
                        accept.update(cx, |state, cx| {
                            let Some((draft, _, _, original)) = state.pending_import.clone() else {
                                return;
                            };
                            let result = merge_import(&state.catalog, &original, &draft)
                                .and_then(|draft| state.commit(draft, cx));
                            match result {
                                Ok(()) => {
                                    state.pending_import = None;
                                }
                                Err(error) => {
                                    if let Some((_, warnings, _, _)) = &mut state.pending_import {
                                        warnings.push(format!("Save failed: {error}"));
                                    }
                                }
                            }
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("cancel-brush-import")
                    .label("Cancel import")
                    .on_click(move |_, _, cx| {
                        cancel.update(cx, |state, cx| {
                            state.pending_import = None;
                            cx.notify();
                        });
                    }),
            ),
    )
    .into_any_element()
}

fn merge_import(
    latest: &Catalog,
    original: &Catalog,
    imported: &Catalog,
) -> Result<Catalog, String> {
    let mut merged = latest.clone();
    for library in imported
        .libraries
        .iter()
        .filter(|l| !original.libraries.iter().any(|old| old.id == l.id))
    {
        if merged.libraries.iter().any(|old| old.id == library.id) {
            return Err("Imported library ID already exists; restart the import.".into());
        }
        merged.libraries.push(library.clone());
    }
    for set in imported
        .sets
        .iter()
        .filter(|s| !original.sets.iter().any(|old| old.id == s.id))
    {
        if merged.sets.iter().any(|old| old.id == set.id) {
            return Err("Imported set ID already exists; restart the import.".into());
        }
        merged.sets.push(set.clone());
    }
    for brush in imported
        .brushes
        .iter()
        .filter(|b| original.brush(&b.id).is_none())
    {
        if merged.brush(&brush.id).is_some() {
            return Err("Imported brush ID already exists; restart the import.".into());
        }
        merged.brushes.push(brush.clone());
    }
    merged.validate().map_err(|e| e.to_string())?;
    Ok(merged)
}

#[cfg(test)]
mod import_tests {
    use super::merge_import;
    use emulsion_io::brush_library::{Catalog, USER_SET};
    use emulsion_raster::paint::Brush;
    #[test]
    fn import_review_keeps_concurrent_library_edits_and_memories() {
        let base = Catalog::builtin();
        let mut imported = base.clone();
        let id = imported
            .add_brush(USER_SET, "Imported", Brush::default())
            .unwrap();
        let mut latest = base.clone();
        let changed = latest.brushes[0].id.clone();
        latest
            .rename_brush(&changed, "Renamed while reviewing")
            .unwrap();
        latest
            .remember_tool(
                "paint",
                &changed,
                Brush {
                    size: 87.,
                    ..Brush::default()
                },
            )
            .unwrap();
        latest.revision = 42;
        let merged = merge_import(&latest, &base, &imported).unwrap();
        assert_eq!(merged.revision, 42);
        assert_eq!(merged.brush(&changed), latest.brush(&changed));
        assert_eq!(merged.tool_memories, latest.tool_memories);
        assert!(merged.brush(&id).is_some());
        assert!(merge_import(&merged, &base, &imported).is_err());
    }
}

#[cfg(test)]
mod catalog_publication_tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn mcp_catalog_publication_notifies_shared_state_and_rejects_stale_reload(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let mut catalog = Catalog::builtin();
            catalog.revision = 5;
            EditorView::publish_brush_catalog(
                store::LoadReport {
                    catalog: catalog.clone(),
                    warnings: vec![],
                },
                cx,
            );
            let first = shared_library(cx);
            let second = shared_library(cx);
            assert_eq!(first.entity_id(), second.entity_id());
            let id = catalog.brushes[0].id.clone();
            catalog.rename_brush(&id, "Changed through MCP").unwrap();
            catalog.revision = 6;
            EditorView::publish_brush_catalog(
                store::LoadReport {
                    catalog,
                    warnings: vec!["conversion warning".into()],
                },
                cx,
            );
            assert_eq!(
                first.read(cx).catalog.brush(&id).unwrap().name,
                "Changed through MCP"
            );
            EditorView::publish_brush_catalog(
                store::LoadReport {
                    catalog: Catalog::builtin(),
                    warnings: vec![],
                },
                cx,
            );
            assert_eq!(second.read(cx).catalog.revision, 6);
            assert_eq!(second.read(cx).warnings, ["conversion warning"]);
        });
    }
}
