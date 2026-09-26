//! Painter controls: the workspace picker, the one-click brush shelf and
//! its gallery, Draw mode's large paint dock, and the project palette of
//! colours already painted with. Presentation only; picking never edits
//! the document.
use super::compact::{Bar, CompactLayout};
use super::*;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
};

/// The built-in workspaces, in the order the picker lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuiltinWorkspace {
    Photo,
    Draw,
    Minimal,
}

impl BuiltinWorkspace {
    pub(crate) const ALL: [Self; 3] = [Self::Photo, Self::Draw, Self::Minimal];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photo",
            Self::Draw => "Draw",
            Self::Minimal => "Minimal",
        }
    }
}
use std::collections::{HashMap, HashSet};

/// Brushes the shelf shows when nothing is pinned.
const SHELF_LEN: usize = 8;
/// Gallery tabs that are not brush sets.
const PINNED: &str = "pinned";
const RECENT: &str = "recent";

#[derive(Default)]
pub(crate) struct DrawUi {
    pub(crate) gallery_open: bool,
    gallery_tab: Option<String>,
    thumbs: HashMap<String, Arc<RenderImage>>,
    pending: HashSet<String>,
    /// Catalog revision the thumbnails were drawn from.
    thumbs_revision: u64,
}

fn rgb_u32([r, g, b]: [u8; 3]) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

impl EditorView {
    /// Remember the foreground colour after it lands on a layer.
    pub(super) fn note_painted_color(&mut self) {
        let fg_paints = self.tool == Tool::Brush
            && matches!(
                self.tools.paint,
                PaintKind::Brush | PaintKind::Bucket | PaintKind::Gradient
            );
        if fg_paints {
            let [r, g, b, _] = self.tools.fg;
            self.editor.doc.note_color([r, g, b]);
        }
    }

    /// The built-in workspace on screen: Minimal whenever the options and
    /// view bars are both hidden, otherwise the mode's own.
    pub(super) fn builtin_workspace(&self) -> BuiltinWorkspace {
        let minimal = !self.compact.bars[Bar::Options as usize].open
            && !self.compact.bars[Bar::View as usize].open;
        if minimal {
            BuiltinWorkspace::Minimal
        } else if self.draw_mode {
            BuiltinWorkspace::Draw
        } else {
            BuiltinWorkspace::Photo
        }
    }

    /// Switch to a built-in workspace with that mode's factory toolbars.
    pub(super) fn apply_builtin_workspace(
        &mut self,
        workspace: BuiltinWorkspace,
        cx: &mut Context<Self>,
    ) {
        if workspace != BuiltinWorkspace::Minimal
            && self.draw_mode != (workspace == BuiltinWorkspace::Draw)
        {
            self.toggle_draw_mode(cx);
        }
        self.compact = CompactLayout::for_mode(self.draw_mode, cx);
        self.sidebar_layout.collapsed = workspace == BuiltinWorkspace::Minimal;
        if workspace == BuiltinWorkspace::Minimal {
            for bar in [Bar::Options, Bar::View, Bar::Color, Bar::Brushes] {
                self.compact.bars[bar as usize].open = false;
            }
        }
        cx.notify();
    }

    /// The header picker's choice: Photo and Draw switch mode and bring back
    /// the toolbars that mode was left with, as the old Photo | Draw switch
    /// did. Choosing the current mode from Minimal restores its toolbars.
    pub(super) fn switch_workspace(&mut self, workspace: BuiltinWorkspace, cx: &mut Context<Self>) {
        let draw = workspace == BuiltinWorkspace::Draw;
        if workspace != BuiltinWorkspace::Minimal && self.draw_mode != draw {
            self.toggle_draw_mode(cx);
        } else if workspace != self.builtin_workspace() {
            self.apply_builtin_workspace(workspace, cx);
        }
    }

    /// One workspace picker at the header's right, named for the workspace
    /// on screen. Built-ins, reset and customize keep fixed places at the
    /// top; saved presets follow.
    pub(super) fn workspace_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        Button::new("workspace-menu-button")
            .label(self.builtin_workspace().label())
            .dropdown_caret(true)
            .xsmall()
            .outline()
            .tooltip("Workspace: Photo, Draw, Minimal or one you saved (Ctrl+Alt+Shift+D switches Photo and Draw)")
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, cx| {
                let Some(view) = editor.upgrade() else {
                    return menu;
                };
                let current = view.read(cx).builtin_workspace();
                for workspace in BuiltinWorkspace::ALL {
                    let editor = editor.clone();
                    menu = menu.item(
                        PopupMenuItem::new(workspace.label())
                            .checked(current == workspace)
                            .on_click(move |_, window, cx| {
                                editor
                                    .update(cx, |this, cx| {
                                        this.switch_workspace(workspace, cx);
                                        window.focus(&this.canvas_focus, cx);
                                    })
                                    .ok();
                            }),
                    );
                }
                let reset = editor.clone();
                let customize = editor.clone();
                menu = menu
                    .separator()
                    .item(PopupMenuItem::new("Reset Workspace").on_click(move |_, _, cx| {
                        reset.update(cx, |this, cx| this.reset_workspace(cx)).ok();
                    }))
                    .item(PopupMenuItem::new("Customize Workspace…").on_click(
                        move |_, window, cx| {
                            customize
                                .update(cx, |this, cx| {
                                    if this.workspace_customizer.is_none() {
                                        this.toggle_workspace_customizer(window, cx);
                                    }
                                })
                                .ok();
                        },
                    ));
                let saved = crate::app_state::settings(cx).workspace_presets.clone();
                if !saved.is_empty() {
                    menu = menu.separator().label("Saved");
                    for preset in saved {
                        let editor = editor.clone();
                        menu = menu.item(PopupMenuItem::new(preset.name.clone()).on_click(
                            move |_, _, cx| {
                                editor
                                    .update(cx, |this, cx| {
                                        this.apply_workspace_layout(&preset.layout, cx)
                                    })
                                    .ok();
                            },
                        ));
                    }
                }
                menu
            })
            .into_any_element()
    }

    /// Pinned brushes, topped up with recent, current-set and then any
    /// library brushes so the shelf is never empty, as (id, name).
    fn shelf_brushes(&self, cx: &mut Context<Self>) -> Vec<(String, String)> {
        let library = super::presets::shared_library(cx);
        let catalog = &library.read(cx).catalog;
        let set = self
            .presets
            .set_id
            .clone()
            .or_else(|| catalog.sets.first().map(|s| s.id.clone()));
        let in_set = catalog
            .brushes
            .iter()
            .filter(|b| Some(&b.set_id) == set.as_ref())
            .map(|b| &b.id);
        let limit = catalog.pinned.len().max(SHELF_LEN);
        let mut seen = HashSet::new();
        catalog
            .pinned
            .iter()
            .chain(catalog.recent.iter())
            .chain(in_set)
            .chain(catalog.brushes.iter().map(|b| &b.id))
            .filter(|id| seen.insert(id.as_str()))
            .filter_map(|id| catalog.brush(id).map(|b| (id.clone(), b.name.clone())))
            .take(limit)
            .collect()
    }

    fn toggle_pin_current(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.presets.current_id.clone() else {
            self.set_status("Choose a library brush first, then pin it.", true, cx);
            return;
        };
        let library = super::presets::shared_library(cx);
        let mut draft = library.read(cx).catalog.clone();
        let pin = !draft.pinned.contains(&id);
        if let Err(error) = draft.pin(&id, pin) {
            self.set_status(format!("Couldn't pin brush: {error}"), true, cx);
            return;
        }
        match library.update(cx, |state, cx| state.commit(draft, cx)) {
            Ok(()) => self.set_status(
                if pin {
                    "Brush pinned to the shelf."
                } else {
                    "Brush removed from the shelf."
                },
                false,
                cx,
            ),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(super) fn toggle_brush_gallery(&mut self, cx: &mut Context<Self>) {
        self.draw_ui.gallery_open = !self.draw_ui.gallery_open;
        cx.notify();
    }

    /// The Brushes toolbar: one click switches brush; the star pins the
    /// current one; Gallery drops down every brush with a stroke preview.
    pub(super) fn brush_shelf(
        &mut self,
        vertical: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let brushes = self.shelf_brushes(cx);
        let current = self.presets.current_id.clone();
        let pinned = current.as_ref().is_some_and(|id| {
            super::presets::shared_library(cx)
                .read(cx)
                .catalog
                .pinned
                .contains(id)
        });
        let gallery_open = self.draw_ui.gallery_open;
        div()
            .id("brush-shelf")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .when(vertical, |d| d.flex_col().items_stretch())
            .child(
                Button::new("brush-gallery-toggle")
                    .label(if gallery_open {
                        "Brushes ▴"
                    } else {
                        "Brushes ▾"
                    })
                    .small()
                    .when(gallery_open, |b| b.bg(p.ink).text_color(p.paper))
                    .tooltip("Brush gallery: every brush with a stroke preview")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_brush_gallery(cx))),
            )
            .children(brushes.into_iter().enumerate().map(|(index, (id, name))| {
                let on = current.as_ref() == Some(&id);
                Button::new(("shelf-brush", index))
                    .label(name.clone())
                    .small()
                    .when(on, |b| b.bg(p.soft_bg).border_1().border_color(p.accent))
                    .when(!on, |b| b.outline())
                    .tooltip(format!("Paint with {name}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply_brush_id(&id, cx);
                        window.focus(&this.canvas_focus, cx);
                    }))
            }))
            .child(
                Button::new("shelf-pin")
                    .small()
                    .ghost()
                    .accessibility_label(if pinned {
                        "Unpin current brush"
                    } else {
                        "Pin current brush"
                    })
                    .tooltip(if pinned {
                        "Remove the current brush from the shelf"
                    } else {
                        "Pin the current brush to the shelf"
                    })
                    .child(
                        rail::tool_icon(if pinned { "star-fill" } else { "star" })
                            .size(rems(1.))
                            .text_color(if pinned { p.accent } else { p.ink }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_pin_current(cx))),
            )
            .into_any_element()
    }

    /// Draw preview strokes for brushes that have none yet, off the UI thread.
    fn request_brush_thumbs(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let library = super::presets::shared_library(cx);
        let catalog = &library.read(cx).catalog;
        if catalog.revision != self.draw_ui.thumbs_revision {
            self.draw_ui.thumbs.clear();
            self.draw_ui.thumbs_revision = catalog.revision;
        }
        let revision = catalog.revision;
        let jobs: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                !self.draw_ui.thumbs.contains_key(id) && !self.draw_ui.pending.contains(id)
            })
            .take(24usize.saturating_sub(self.draw_ui.pending.len()))
            .filter_map(|id| {
                catalog
                    .brush(&id)
                    .map(|b| (id, b.brush, b.secondary, b.combine_mode))
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        for (id, ..) in &jobs {
            self.draw_ui.pending.insert(id.clone());
        }
        cx.spawn(async move |this, cx| {
            let images = cx
                .background_spawn(async move {
                    jobs.into_iter()
                        .map(|(id, brush, secondary, mode)| {
                            (
                                id,
                                super::brush_library_ui::stroke_preview(brush, secondary, mode),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                for (id, image) in images {
                    this.draw_ui.pending.remove(&id);
                    if revision == this.draw_ui.thumbs_revision {
                        this.draw_ui.thumbs.insert(id, image);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Procreate-style brush gallery over the canvas: sets as tabs, each
    /// brush a card with its stroke. Clicking outside closes it.
    pub(super) fn brush_gallery(
        &mut self,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.draw_ui.gallery_open {
            return None;
        }
        let library = super::presets::shared_library(cx);
        let (tabs, brushes, pinned) = {
            let catalog = &library.read(cx).catalog;
            let tab = self.draw_ui.gallery_tab.clone().unwrap_or_else(|| {
                if !catalog.pinned.is_empty() {
                    PINNED.into()
                } else {
                    self.presets
                        .set_id
                        .clone()
                        .or_else(|| catalog.brushes.first().map(|b| b.set_id.clone()))
                        .unwrap_or_else(|| RECENT.into())
                }
            });
            let mut tabs = vec![
                (PINNED.to_string(), "Pinned".to_string(), tab == PINNED),
                (RECENT.to_string(), "Recent".to_string(), tab == RECENT),
            ];
            tabs.extend(
                catalog
                    .sets
                    .iter()
                    .map(|s| (s.id.clone(), s.name.clone(), s.id == tab)),
            );
            let ids: Vec<&String> = match tab.as_str() {
                PINNED => catalog.pinned.iter().collect(),
                RECENT => catalog.recent.iter().take(24).collect(),
                set => catalog
                    .brushes
                    .iter()
                    .filter(|b| b.set_id == set)
                    .map(|b| &b.id)
                    .collect(),
            };
            let brushes: Vec<(String, String)> = ids
                .into_iter()
                .filter_map(|id| catalog.brush(id).map(|b| (id.clone(), b.name.clone())))
                .collect();
            (tabs, brushes, catalog.pinned.clone())
        };
        let ids: Vec<String> = brushes.iter().map(|(id, _)| id.clone()).collect();
        cx.defer_in(window, move |this, _, cx| {
            this.request_brush_thumbs(ids, cx)
        });
        let current = self.presets.current_id.clone();
        let mut grid = div()
            .id("brush-gallery-grid")
            .test_support()
            .flex()
            .flex_wrap()
            .gap_2()
            .overflow_y_scroll()
            .min_h_0()
            .flex_1();
        if brushes.is_empty() {
            grid = grid.child(mono(
                "Nothing here yet. Pin brushes with the star, or pick a set above.",
                11.,
                p.muted,
            ));
        }
        for (index, (id, name)) in brushes.into_iter().enumerate() {
            let on = current.as_ref() == Some(&id);
            let is_pinned = pinned.contains(&id);
            let preview = match self.draw_ui.thumbs.get(&id) {
                Some(image) => img(ImageSource::Render(image.clone()))
                    .w_full()
                    .h(rems(3.))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                None => div().w_full().h(rems(3.)).bg(p.soft_bg).into_any_element(),
            };
            let pick = id.clone();
            grid = grid.child(
                div()
                    .id(("gallery-brush", index))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(format!("Paint with {name}"))
                    .tab_index(0)
                    .w(rems(11.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_1()
                    .border_2()
                    .border_color(if on { p.accent } else { p.line })
                    .bg(p.paper)
                    .cursor_pointer()
                    .hover(|s| s.border_color(p.ink))
                    .child(preview)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_xs()
                                    .text_color(p.ink)
                                    .child(name),
                            )
                            .when(is_pinned, |d| {
                                d.child(rail::tool_icon("star-fill").size_3().text_color(p.accent))
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.apply_brush_id(&pick, cx);
                    })),
            );
        }
        let mut tab_row = div().flex().flex_wrap().gap_1();
        for (id, label, on) in tabs {
            tab_row = tab_row.child(
                Button::new(SharedString::from(format!("gallery-tab-{id}")))
                    .label(label)
                    .small()
                    .when(on, |b| b.bg(p.ink).text_color(p.paper))
                    .when(!on, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.draw_ui.gallery_tab = Some(id.clone());
                        cx.notify();
                    })),
            );
        }
        let panel = div()
            .id("brush-gallery")
            .test_support()
            .occlude()
            .w(rems(48.))
            .max_w(window.viewport_size().width - px(80.))
            .h(window.viewport_size().height * 0.6)
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("Brush gallery", p))
                    .child(div().flex_1())
                    .child(
                        Button::new("gallery-pin-current")
                            .label("Pin current")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pin_current(cx))),
                    )
                    .child(
                        Button::new("gallery-library")
                            .label("Edit library…")
                            .small()
                            .ghost()
                            .tooltip("Import, organise and author brushes")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.draw_ui.gallery_open = false;
                                this.open_brush_workspace(window, cx);
                            })),
                    )
                    .child(Button::new("gallery-close").label("Done").small().on_click(
                        cx.listener(|this, _, window, cx| {
                            this.draw_ui.gallery_open = false;
                            window.focus(&this.canvas_focus, cx);
                            cx.notify();
                        }),
                    )),
            )
            .child(tab_row)
            .child(grid);
        // A clear backdrop: clicking anywhere else closes the gallery.
        Some(
            div()
                .id("brush-gallery-backdrop")
                .absolute()
                .size_full()
                .occlude()
                .flex()
                .justify_center()
                .pt(rems(3.5))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.draw_ui.gallery_open = false;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    /// Draw mode's big controls: paint, smudge and erase (click the active
    /// one again for the gallery), layers, colour, size and opacity, undo.
    pub(super) fn draw_dock(
        &mut self,
        horizontal: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let big = |id: &'static str, glyph: &'static str, tip: String, on: bool| {
            Button::new(id)
                .ghost()
                .w(rems(2.75))
                .h(rems(2.75))
                .accessibility_label(tip.clone())
                .tooltip(tip)
                .when(on, |b| b.bg(p.soft_bg).border_1().border_color(p.accent))
                .child(rail::tool_icon(glyph).size(rems(1.375)).text_color(p.ink))
        };
        let rule = || {
            div()
                .flex_none()
                .bg(p.line)
                .when(horizontal, |d| d.w(px(1.)).h(rems(2.)))
                .when(!horizontal, |d| d.h(px(1.)).w(rems(2.)))
        };
        let mut dock = div()
            .id("draw-dock")
            .test_support()
            .flex()
            .items_center()
            .gap(rems(0.25))
            .when(!horizontal, |d| d.flex_col());
        for (id, kind, glyph, name, key) in [
            ("dock-paint", PaintKind::Brush, "brush", "Paint", "B"),
            (
                "dock-smudge",
                PaintKind::Smudge,
                "pointer",
                "Smudge",
                "Shift+B",
            ),
            ("dock-erase", PaintKind::Eraser, "eraser", "Erase", "E"),
        ] {
            let on = self.tool == Tool::Brush && self.tools.paint == kind;
            dock = dock.child(
                big(
                    id,
                    glyph,
                    format!("{name} ({key}). Click again for the brush gallery"),
                    on,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.tool == Tool::Brush && this.tools.paint == kind {
                        this.toggle_brush_gallery(cx);
                    } else {
                        this.set_paint(kind, cx);
                        window.focus(&this.canvas_focus, cx);
                    }
                })),
            );
        }
        let [r, g, b, _] = self.tools.fg;
        dock = dock
            .child(
                big(
                    "dock-layers",
                    "layers",
                    "Layers panel".into(),
                    !self.sidebar_layout.collapsed,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar_layout.collapsed = !this.sidebar_layout.collapsed;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .id("dock-color")
                    .test_support()
                    .role(Role::Button)
                    .aria_label("Choose colour")
                    .tab_index(0)
                    .size(rems(2.25))
                    .rounded_full()
                    .border_2()
                    .border_color(if self.tools.picker { p.accent } else { p.ink })
                    .bg(rgb(rgb_u32([r, g, b])))
                    .cursor_pointer()
                    .tooltip(|w, cx| Tooltip::new("Colour: click to pick").build(w, cx))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tools.picker = !this.tools.picker;
                        this.tools.hue = tools::rgb_to_hsv(this.tools.fg).0;
                        cx.notify();
                    })),
            );
        if let Some([size, opacity]) =
            self.brush_vsliders(if horizontal { 96. } else { 140. }, p, cx)
        {
            dock = dock.child(rule()).child(
                div()
                    .flex()
                    .gap(rems(0.5))
                    .when(!horizontal, |d| d.flex_col())
                    .child(size)
                    .child(opacity),
            );
        }
        dock.child(rule())
            .child(
                big("dock-undo", "undo-2", "Undo (Ctrl+Z)".into(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
            )
            .child(
                big("dock-redo", "redo-2", "Redo (Ctrl+Shift+Z)".into(), false)
                    .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
            )
            .into_any_element()
    }

    /// Colours painted with in this project, newest first; click to reuse.
    pub(super) fn project_colors(
        &self,
        vertical: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &self.editor.doc.colors;
        let [fr, fg, fb, _] = self.tools.fg;
        let mut grid = div()
            .id("project-colors")
            .test_support()
            .flex()
            .flex_wrap()
            .gap(rems(0.25))
            .when(vertical, |d| d.w(rems(3.25)))
            .when(!vertical, |d| d.max_w(rems(28.)));
        if colors.is_empty() {
            return grid
                .child(mono(
                    if vertical {
                        ""
                    } else {
                        "Colours you paint with appear here"
                    },
                    9.5,
                    p.muted,
                ))
                .into_any_element();
        }
        let shown = if vertical { 16 } else { colors.len() };
        for (index, &c) in colors.iter().take(shown).enumerate() {
            let hex = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            let current = c == [fr, fg, fb];
            grid = grid.child(
                div()
                    .id(("project-color", index))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(format!("Use colour {hex}"))
                    .tab_index(0)
                    .size(rems(1.5))
                    .rounded_full()
                    .border_2()
                    .border_color(if current { p.accent } else { p.line })
                    .bg(rgb(rgb_u32(c)))
                    .cursor_pointer()
                    .tooltip(move |w, cx| {
                        Tooltip::new(format!("{hex}: used in this project")).build(w, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_fg([c[0], c[1], c[2], 255], cx);
                    })),
            );
        }
        grid.into_any_element()
    }
}
