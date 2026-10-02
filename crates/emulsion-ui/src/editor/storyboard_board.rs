//! The storyboard Board: every panel as a thumbnail under its act, sequence
//! and scene, the Stage ⇄ Board switch, and what the Stage shows for locked
//! panels and thumbnail sheets. Board editing lives in `storyboard_board_ops`.
use super::*;
use crate::actions::{CopyPixels, CutPixels, DeleteNode, PastePixels, SelectAll};
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{GroupId, Level, PanelStatus, ThumbnailGrid};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use std::collections::HashSet;

#[path = "storyboard_board_ops.rs"]
mod ops;
pub(super) use ops::DropAt;

#[path = "storyboard_commands.rs"]
mod commands;
pub(crate) use commands::BoardCommand;

#[cfg(test)]
#[path = "storyboard_board_tests.rs"]
mod tests;

/// A panel's tag colour, from the inspector's palette.
fn tag_color(tag: u8) -> Hsla {
    let palette = &super::storyboard_inspector::TAG_PALETTE;
    rgb(palette[usize::from(tag) % palette.len()].1).into()
}

/// Running time as minutes, seconds and tenths, such as `1:04.5`.
pub(crate) fn running_time(seconds: f64) -> String {
    format!("{}:{:04.1}", (seconds / 60.).floor(), seconds % 60.)
}

#[derive(Default)]
pub(crate) struct BoardUi {
    /// The Board replaces the Stage.
    pub(crate) open: bool,
    /// Selected panels; the active panel when empty.
    pub(crate) selection: Vec<PageId>,
    anchor: Option<PageId>,
    focus: Option<FocusHandle>,
}

/// Panels being dragged on the Board, in page order.
#[derive(Clone)]
pub(super) struct DraggedPanels(pub(super) Vec<PageId>);

impl Render for DraggedPanels {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .p_2()
            .bg(p.panel)
            .text_color(p.ink)
            .border_1()
            .border_color(p.accent)
            .rounded(px(4.))
            .child(match self.0.len() {
                1 => "1 panel".to_string(),
                n => format!("{n} panels"),
            })
    }
}

struct Card {
    id: PageId,
    name: String,
    /// `None` for thumbnail sheets, which have no screen time.
    seconds: Option<f64>,
    status: PanelStatus,
    tag: Option<u8>,
    locked: bool,
    sheet: Option<ThumbnailGrid>,
    caption: Option<String>,
}

enum Row {
    Group {
        level: Level,
        id: GroupId,
        name: String,
        panels: usize,
        seconds: f64,
        first: bool,
    },
    Scene {
        id: GroupId,
        name: String,
        locked: bool,
        seconds: f64,
        first: bool,
        cards: Vec<Card>,
    },
}

fn menu_item(
    owner: &WeakEntity<EditorView>,
    label: String,
    run: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        owner.update(cx, |e, cx| run(e, window, cx)).ok();
    })
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Scene => "scene",
        Level::Sequence => "sequence",
        Level::Act => "act",
    }
}

impl EditorView {
    pub(crate) fn board_open(&self) -> bool {
        self.pages_ui.board.open && self.editor.storyboard().is_some()
    }

    pub(super) fn board_focus(&mut self, cx: &mut Context<Self>) -> FocusHandle {
        self.pages_ui
            .board
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
    }

    pub(crate) fn toggle_storyboard_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() || !self.prepare_page_action(cx) {
            return;
        }
        let open = !self.pages_ui.board.open;
        self.pages_ui.board.open = open;
        if open {
            let focus = self.board_focus(cx);
            window.focus(&focus, cx);
        } else {
            window.focus(&self.canvas_focus, cx);
        }
        cx.notify();
    }

    /// Selected panels in page order; the active panel when none are.
    pub(crate) fn board_selection(&self) -> Vec<PageId> {
        let chosen: HashSet<_> = self.pages_ui.board.selection.iter().copied().collect();
        let ids: Vec<_> = self
            .editor
            .page_list()
            .iter()
            .map(|m| m.id)
            .filter(|id| chosen.contains(id))
            .collect();
        if ids.is_empty() {
            vec![self.editor.active_page()]
        } else {
            ids
        }
    }

    /// Every selected panel carries its own lock, so the command unlocks.
    fn board_selection_locked(&self) -> bool {
        self.editor.storyboard().is_some_and(|b| {
            self.board_selection()
                .iter()
                .all(|id| b.panels.get(id).is_some_and(|p| p.locked))
        })
    }

    pub(super) fn set_board_selection(&mut self, ids: Vec<PageId>) {
        self.pages_ui.board.anchor = ids.first().copied();
        self.pages_ui.board.selection = ids;
    }

    /// Click selects and opens a panel; Shift extends the selection from the
    /// last plain click, Ctrl/Cmd toggles one panel; a double click shows the
    /// panel on the Stage.
    pub(crate) fn board_click(
        &mut self,
        id: PageId,
        modifiers: Modifiers,
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.board_focus(cx);
        window.focus(&focus, cx);
        let order: Vec<_> = self.editor.page_list().iter().map(|m| m.id).collect();
        let board = &mut self.pages_ui.board;
        let mut open = true;
        if clicks >= 2 {
            board.selection = vec![id];
            board.anchor = Some(id);
            board.open = false;
            window.focus(&self.canvas_focus, cx);
        } else if modifiers.shift {
            let anchor = board.anchor.unwrap_or(id);
            let a = order.iter().position(|p| *p == anchor).unwrap_or(0);
            let b = order.iter().position(|p| *p == id).unwrap_or(0);
            board.selection = order[a.min(b)..=a.max(b)].to_vec();
        } else if modifiers.secondary() {
            if board.selection.is_empty() {
                board.selection.push(self.editor.active_page());
            }
            if let Some(at) = board.selection.iter().position(|p| *p == id) {
                board.selection.remove(at);
                open = false;
            } else {
                board.selection.push(id);
            }
            board.anchor = Some(id);
        } else {
            board.selection = vec![id];
            board.anchor = Some(id);
        }
        if open {
            self.select_page(id, cx);
        }
        cx.notify();
    }

    /// Whether the active storyboard panel, or its scene, is locked.
    pub(crate) fn active_panel_locked(&self) -> bool {
        self.editor
            .storyboard()
            .is_some_and(|b| b.is_locked(self.editor.active_page()))
    }

    /// Drawing on a locked panel is refused up front with the reason, rather
    /// than failing when the stroke lands. Viewing tools still work.
    pub(crate) fn refuse_locked_panel(&mut self, cx: &mut Context<Self>) -> bool {
        if matches!(self.tool, Tool::Hand | Tool::Zoom | Tool::Eyedropper)
            || !self.active_panel_locked()
        {
            return false;
        }
        self.set_status(
            "This panel is locked, so drawing and edits are off. Unlock it from the Stage banner or the Board.",
            false,
            cx,
        );
        true
    }

    /// A thumbnail sheet's camera frames, as closed outlines in document
    /// pixels, so the user draws inside them.
    pub(crate) fn thumbnail_sheet_frames(&self) -> Vec<Vec<(f64, f64)>> {
        let Some(grid) = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&self.editor.active_page()))
            .and_then(|p| p.thumbnails)
        else {
            return Vec::new();
        };
        grid.cells(self.editor.doc.width, self.editor.doc.height)
            .into_iter()
            .map(|r| {
                let (x, y, w, h) = (
                    f64::from(r.x),
                    f64::from(r.y),
                    f64::from(r.w),
                    f64::from(r.h),
                );
                vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h), (x, y)]
            })
            .collect()
    }

    /// What fills the Stage area: the canvas, with a lock banner for locked
    /// panels, or the Board in its place.
    pub(super) fn storyboard_stage(
        &mut self,
        canvas: AnyElement,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.editor.storyboard().is_none() {
            return vec![canvas];
        }
        self.playback_sync(cx);
        self.reference_video_sync(cx);
        if self.board_open() {
            let mut out = vec![self.board_view(p, window, cx)];
            out.extend(self.playback_layers(p, cx));
            return out;
        }
        let mut out = vec![canvas];
        if self.active_panel_locked() {
            out.push(
                div()
                    .id("storyboard-lock-banner")
                    .test_support()
                    .absolute()
                    .top_2()
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_1()
                            .rounded(px(6.))
                            .bg(p.panel.opacity(0.95))
                            .border_1()
                            .border_color(p.accent)
                            .text_size(px(12.))
                            .text_color(p.ink)
                            .child("Locked panel · drawing is off")
                            .child(
                                Button::new("storyboard-stage-unlock")
                                    .label("Unlock")
                                    .xsmall()
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let id = this.editor.active_page();
                                        this.unlock_panel_and_scene(id, cx)
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }
        out.extend(self.stage_controls(p, cx));
        out.extend(self.playback_layers(p, cx));
        out
    }

    /// The strip's Stage ⇄ Board switch.
    pub(super) fn storyboard_view_toggle(&self, cx: &Context<Self>) -> AnyElement {
        let open = self.board_open();
        Button::new("storyboard-view-toggle")
            .label(if open { "Stage" } else { "Board" })
            .tooltip(if open {
                "Draw the active panel on the Stage"
            } else {
                "See every panel on the Board, grouped by scene"
            })
            .small()
            .outline()
            .on_click(cx.listener(|this, _, window, cx| this.toggle_storyboard_board(window, cx)))
            .into_any_element()
    }

    /// View menu entries for storyboards.
    pub(super) fn storyboard_view_items(
        menu: PopupMenu,
        editor: &Entity<EditorView>,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        if editor.read(cx).editor.storyboard().is_none() {
            return menu;
        }
        let open = editor.read(cx).board_open();
        let timeline = editor.read(cx).timeline_open();
        let owner = editor.downgrade();
        let timeline_owner = owner.clone();
        menu.item(
            PopupMenuItem::new("Storyboard Board")
                .checked(open)
                .on_click(move |_, window, cx| {
                    owner
                        .update(cx, |e, cx| e.toggle_storyboard_board(window, cx))
                        .ok();
                }),
        )
        .item(
            PopupMenuItem::new("Timeline")
                .checked(timeline)
                .on_click(move |_, _, cx| {
                    timeline_owner
                        .update(cx, |e, cx| e.toggle_storyboard_timeline(cx))
                        .ok();
                }),
        )
        .separator()
    }

    fn board_rows(&self, captions: bool) -> Vec<Row> {
        let Some(board) = self.editor.storyboard() else {
            return Vec::new();
        };
        let names: HashMap<_, _> = self
            .editor
            .page_list()
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();
        let order: Vec<_> = self.editor.page_list().iter().map(|m| m.id).collect();
        let outline = board.outline(&order);
        let fps = board.settings.frame_rate.fps();
        let seconds = |id: &PageId| {
            board
                .panels
                .get(id)
                .filter(|p| p.thumbnails.is_none())
                .map_or(0., |p| f64::from(p.frames) / fps)
        };
        let mut totals: HashMap<GroupId, (usize, f64)> = HashMap::new();
        for scene in &outline {
            let time: f64 = scene.panels.iter().map(seconds).sum();
            for group in [scene.act, scene.sequence, scene.scene] {
                let total = totals.entry(group).or_default();
                total.0 += scene.panels.len();
                total.1 += time;
            }
        }
        // Acts and sequences only earn a header once there is more than one.
        let show_acts = board.acts.len() > 1;
        let show_sequences = board.sequences.len() > 1;
        let mut rows = Vec::new();
        for (index, scene) in outline.iter().enumerate() {
            let previous = index.checked_sub(1).map(|i| &outline[i]);
            let mut group = |level: Level, id: GroupId, name: &str| {
                let (panels, seconds) = totals[&id];
                rows.push(Row::Group {
                    level,
                    id,
                    name: name.to_string(),
                    panels,
                    seconds,
                    first: previous.is_none(),
                });
            };
            if show_acts && previous.is_none_or(|p| p.act != scene.act) {
                group(Level::Act, scene.act, &board.acts[&scene.act].name);
            }
            if show_sequences && previous.is_none_or(|p| p.sequence != scene.sequence) {
                group(
                    Level::Sequence,
                    scene.sequence,
                    &board.sequences[&scene.sequence].name,
                );
            }
            let cards = scene
                .panels
                .iter()
                .map(|id| {
                    let panel = &board.panels[id];
                    let caption = captions
                        .then(|| {
                            board.captions.iter().find_map(|field| {
                                let text = panel.captions.get(&field.id)?.text.trim();
                                text.lines().next().map(str::to_string)
                            })
                        })
                        .flatten()
                        .filter(|line| !line.is_empty());
                    Card {
                        id: *id,
                        name: names.get(id).cloned().unwrap_or_default(),
                        seconds: panel.thumbnails.is_none().then(|| seconds(id)),
                        status: panel.status,
                        tag: panel.tag,
                        locked: board.is_locked(*id),
                        sheet: panel.thumbnails,
                        caption,
                    }
                })
                .collect();
            let info = &board.scenes[&scene.scene];
            rows.push(Row::Scene {
                id: scene.scene,
                name: info.name.clone(),
                locked: info.locked,
                seconds: totals[&scene.scene].1,
                first: previous.is_none(),
                cards,
            });
        }
        rows
    }

    fn board_view(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focus = self.board_focus(cx);
        let prefs = &crate::app_state::settings(cx).storyboard;
        let width = prefs.thumbnail_width.clamp(
            *emulsion_core::storyboard::Preferences::THUMBNAIL_WIDTHS.start(),
            *emulsion_core::storyboard::Preferences::THUMBNAIL_WIDTHS.end(),
        );
        let captions = prefs.show_captions_on_board;
        let (doc_w, doc_h) = self
            .editor
            .storyboard()
            .map_or((16, 9), |b| (b.settings.width, b.settings.height));
        let height = (width as f32 * doc_h as f32 / doc_w.max(1) as f32).round();
        let selection: HashSet<_> = self.board_selection().into_iter().collect();
        let active = self.editor.active_page();
        let accent = p.accent;
        let mut column = div()
            .id("storyboard-board-scroll")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_2()
            .p_3();
        for row in self.board_rows(captions) {
            match row {
                Row::Group {
                    level,
                    id,
                    name,
                    panels,
                    seconds,
                    first,
                } => {
                    column = column.child(
                        div()
                            .id(("board-group", id))
                            .test_support()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pt(px(if level == Level::Act { 8. } else { 4. }))
                            .text_size(px(if level == Level::Act { 15. } else { 13. }))
                            .text_color(p.ink)
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(p.muted)
                                    .child(Self::group_summary(panels, seconds)),
                            )
                            .child(self.group_menu_button(level, id, name, first, false, cx)),
                    );
                }
                Row::Scene {
                    id,
                    name,
                    locked,
                    seconds,
                    first,
                    cards,
                } => {
                    let count = cards.len();
                    let mut grid = div().flex().flex_wrap().gap_2();
                    for card in cards {
                        let chosen = selection.contains(&card.id);
                        grid = grid.child(self.board_card(
                            card,
                            chosen,
                            active,
                            &selection,
                            (width as f32, height),
                            p,
                            cx,
                        ));
                    }
                    column = column.child(
                        div()
                            .id(("board-scene", id))
                            .test_support()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_2()
                            .rounded(px(6.))
                            .bg(p.panel)
                            .border_1()
                            .border_color(if locked { p.accent } else { p.line })
                            .drag_over::<DraggedPanels>(move |s, _, _, _| {
                                s.border_color(accent).bg(accent.opacity(0.06))
                            })
                            .on_drop(cx.listener(move |this, d: &DraggedPanels, _, cx| {
                                this.board_drop(d.0.clone(), DropAt::SceneEnd(id), cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(12.))
                                    .text_color(p.ink)
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(format!("Scene {name}")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(p.muted)
                                            .child(Self::group_summary(count, seconds)),
                                    )
                                    .when(locked, |row| row.child(Self::badge("Locked", p)))
                                    .child(self.group_menu_button(
                                        Level::Scene,
                                        id,
                                        name,
                                        first,
                                        locked,
                                        cx,
                                    )),
                            )
                            .child(grid),
                    );
                }
            }
        }
        div()
            .id("storyboard-board")
            .test_support()
            .track_focus(&focus)
            .key_context("NodePanel")
            .map(|d| Self::playback_actions(d, cx))
            .on_action(cx.listener(|this, _: &CopyPixels, _, cx| {
                this.board_copy(cx);
            }))
            .on_action(cx.listener(|this, _: &CutPixels, _, cx| this.board_cut(cx)))
            .on_action(cx.listener(|this, _: &PastePixels, _, cx| this.board_paste(cx)))
            .on_action(cx.listener(|this, _: &DeleteNode, _, cx| this.board_delete(cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                let all = this.editor.page_list().iter().map(|m| m.id).collect();
                this.set_board_selection(all);
                cx.notify();
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(p.stage)
            .child(self.board_toolbar(p, window, cx))
            .child(column)
            .into_any_element()
    }

    fn group_summary(panels: usize, seconds: f64) -> String {
        format!(
            "{panels} panel{} · {}",
            if panels == 1 { "" } else { "s" },
            running_time(seconds)
        )
    }

    fn badge(text: &str, p: &Palette) -> Div {
        div()
            .px_1()
            .rounded(px(3.))
            .bg(p.ink.opacity(0.8))
            .text_color(p.panel)
            .text_size(px(10.))
            .child(text.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    fn board_card(
        &mut self,
        card: Card,
        chosen: bool,
        active: PageId,
        selection: &HashSet<PageId>,
        (width, height): (f32, f32),
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = card.id;
        let image = self.page_thumbnail(id, width.round() as u32, cx);
        let accent = p.accent;
        // Dragging a selected panel carries the whole selection.
        let dragged = DraggedPanels(if chosen {
            self.board_selection()
        } else {
            vec![id]
        });
        // Rough is the default, so only later stages are marked.
        let status = super::storyboard_inspector::PANEL_STATUSES
            .iter()
            .find(|(status, _)| *status == card.status && *status != PanelStatus::Rough)
            .map(|(status, label)| {
                let color: Hsla = if *status == PanelStatus::Approved {
                    rgb(0x30A46C).into()
                } else {
                    p.accent
                };
                (*label, color)
            });
        let owner = cx.weak_entity();
        let menu_selected = selection.contains(&id);
        div()
            .id(("board-panel", id))
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .w(px(width + 8.))
            .p_1()
            .rounded(px(5.))
            .border_2()
            .border_color(if chosen {
                p.accent
            } else if id == active {
                p.accent.opacity(0.4)
            } else {
                p.line
            })
            .bg(if chosen { p.soft_bg } else { p.panel })
            .cursor_pointer()
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                this.board_click(id, e.modifiers(), e.click_count(), window, cx)
            }))
            .on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
            .drag_over::<DraggedPanels>(move |s, _, _, _| s.border_color(accent))
            .on_drop(cx.listener(move |this, d: &DraggedPanels, _, cx| {
                this.board_drop(d.0.clone(), DropAt::Before(id), cx);
                cx.stop_propagation();
            }))
            // Library items land on this panel (a panel item: after it).
            .drag_over::<super::storyboard_library::LibraryDrag>(move |s, _, _, _| {
                s.border_color(accent)
            })
            .on_drop(cx.listener(
                move |this, d: &super::storyboard_library::LibraryDrag, _, cx| {
                    this.library_place(d.scope, d.id, Some(id), cx);
                    cx.stop_propagation();
                },
            ))
            .context_menu(move |menu, _, cx| {
                // Right-clicking outside the selection acts on that panel.
                if !menu_selected {
                    owner
                        .update(cx, |e, cx| {
                            e.set_board_selection(vec![id]);
                            cx.notify();
                        })
                        .ok();
                }
                Self::board_panel_menu(menu, owner.clone(), id, cx)
            })
            .child(
                div()
                    .relative()
                    .w(px(width))
                    .h(px(height))
                    .bg(gpui_kit::white())
                    .rounded(px(3.))
                    .overflow_hidden()
                    .children(
                        image.map(|image| img(image).size_full().object_fit(ObjectFit::Contain)),
                    )
                    .when(card.locked, |thumb| {
                        thumb.child(
                            div()
                                .absolute()
                                .top_1()
                                .right_1()
                                .child(Self::badge("Locked", p)),
                        )
                    })
                    .children(card.sheet.map(|grid| {
                        div().absolute().top_1().left_1().child(
                            Self::badge(&format!("Sheet {}×{}", grid.columns, grid.rows), p)
                                .bg(p.accent)
                                .text_color(p.accent_fg),
                        )
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .text_color(p.ink)
                    .children(card.tag.map(|tag| {
                        div()
                            .flex_none()
                            .size(px(8.))
                            .rounded_full()
                            .bg(tag_color(tag))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(card.name),
                    )
                    .children(status.map(|(label, color): (&str, Hsla)| {
                        div().text_size(px(10.)).text_color(color).child(label)
                    }))
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child(card.seconds.map_or("sheet".into(), running_time)),
                    ),
            )
            .children(card.caption.map(|line| {
                div()
                    .text_size(px(10.5))
                    .text_color(p.muted)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(line)
            }))
            .into_any_element()
    }

    fn board_toolbar(&mut self, p: &Palette, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selection = self.board_selection();
        let count = selection.len();
        let first = selection[0];
        let locked = self.board_selection_locked();
        let can_paste = cx.has_global::<ops::PanelClipboard>();
        let owner = cx.weak_entity();
        let button = |id: &'static str, label: &'static str, tooltip: &'static str| {
            Button::new(id)
                .label(label)
                .tooltip(tooltip)
                .small()
                .ghost()
        };
        div()
            .id("storyboard-board-toolbar")
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .flex_wrap()
            .gap_1()
            .px_2()
            .py_1()
            .bg(p.panel)
            .border_b_1()
            .border_color(p.line)
            .child(
                Button::new("board-stage")
                    .label("Stage")
                    .tooltip("Draw the active panel")
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_storyboard_board(window, cx)),
                    ),
            )
            .child(
                button("board-add", "Add", "Add a blank panel after the selection")
                    .on_click(cx.listener(|this, _, _, cx| this.board_add(false, cx))),
            )
            .child(
                button(
                    "board-smart-add",
                    "Smart add",
                    "Add a panel carrying the Smart add layers forward",
                )
                .on_click(cx.listener(|this, _, _, cx| this.board_add(true, cx))),
            )
            .child(
                button(
                    "board-duplicate",
                    "Duplicate",
                    "Duplicate the selected panels",
                )
                .on_click(cx.listener(|this, _, _, cx| this.board_duplicate(cx))),
            )
            .child(
                button("board-delete", "Delete", "Delete the selected panels")
                    .on_click(cx.listener(|this, _, _, cx| this.board_delete(cx))),
            )
            .child(
                button(
                    "board-lock",
                    if locked { "Unlock" } else { "Lock" },
                    "Protect the selected panels from edits",
                )
                .on_click(cx.listener(move |this, _, _, cx| this.board_lock_panels(!locked, cx))),
            )
            .child(
                button(
                    "board-copy",
                    "Copy",
                    "Copy panels (whole scenes stay scenes)",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.board_copy(cx);
                })),
            )
            .child(
                button(
                    "board-paste",
                    "Paste",
                    "Paste copied panels after the selection",
                )
                .disabled(!can_paste)
                .on_click(cx.listener(|this, _, _, cx| this.board_paste(cx))),
            )
            .child(
                button(
                    "board-renumber",
                    "Renumber…",
                    "Rename scenes and panels by the naming rules",
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.board_renumber_dialog(None, window, cx)),
                ),
            )
            .child(
                Button::new("board-more")
                    .label("More ▾")
                    .small()
                    .ghost()
                    .dropdown_menu(move |menu, _, cx| {
                        Self::board_panel_menu(menu, owner.clone(), first, cx)
                    }),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(format!(
                        "{count} selected · double-click opens on the Stage"
                    )),
            )
            .into_any_element()
    }

    fn group_menu_button(
        &self,
        level: Level,
        id: GroupId,
        name: String,
        first: bool,
        locked: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let owner = cx.weak_entity();
        let what = level_name(level);
        Button::new(("board-group-menu", id))
            .label("···")
            .tooltip(format!("Change this {what}"))
            .xsmall()
            .ghost()
            .dropdown_menu(move |menu, _, _| {
                let name = name.clone();
                let mut menu = menu
                    .item(menu_item(
                        &owner,
                        format!("Rename {what}…"),
                        move |e, window, cx| e.board_rename_dialog(id, &name, window, cx),
                    ))
                    .item(
                        menu_item(
                            &owner,
                            format!("Join with the previous {what}"),
                            move |e, _, cx| e.board_join(id, cx),
                        )
                        .disabled(first),
                    )
                    .item(menu_item(
                        &owner,
                        format!("Renumber this {what}…"),
                        move |e, window, cx| e.board_renumber_dialog(Some(vec![id]), window, cx),
                    ));
                if level == Level::Scene {
                    menu = menu.separator().item(menu_item(
                        &owner,
                        if locked { "Unlock scene" } else { "Lock scene" }.into(),
                        move |e, _, cx| e.board_lock_scene(id, !locked, cx),
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// Panel commands for the selection, shared by a card's context menu and
    /// the toolbar's More menu. `id` is the panel the menu was opened on.
    fn board_panel_menu(
        menu: PopupMenu,
        owner: WeakEntity<EditorView>,
        id: PageId,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(editor) = owner.upgrade() else {
            return menu;
        };
        let (selection, locked, sheet, can_paste) = {
            let e = editor.read(cx);
            let selection = e.board_selection();
            let board = e.editor.storyboard();
            let locked = e.board_selection_locked();
            let sheet = board
                .and_then(|b| b.panels.get(&id))
                .is_some_and(|p| p.thumbnails.is_some());
            (
                selection,
                locked,
                sheet,
                cx.has_global::<ops::PanelClipboard>(),
            )
        };
        let item =
            |label: &str,
             run: fn(&mut EditorView, PageId, &mut Window, &mut Context<EditorView>)| {
                let owner = owner.clone();
                PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                    owner.update(cx, |e, cx| run(e, id, window, cx)).ok();
                })
            };
        let several = selection.len() > 1;
        menu.item(item("Open on the Stage", |e, id, window, cx| {
            e.set_board_selection(vec![id]);
            e.select_page(id, cx);
            e.toggle_storyboard_board(window, cx);
        }))
        .separator()
        .item(item("Add panel", |e, _, _, cx| e.board_add(false, cx)))
        .item(item("Smart add", |e, _, _, cx| e.board_add(true, cx)))
        .item(item(
            if several {
                "Duplicate panels"
            } else {
                "Duplicate panel"
            },
            |e, _, _, cx| e.board_duplicate(cx),
        ))
        .item(item(
            if several {
                "Delete panels"
            } else {
                "Delete panel"
            },
            |e, _, _, cx| e.board_delete(cx),
        ))
        .separator()
        .item(item("Cut", |e, _, _, cx| e.board_cut(cx)))
        .item(item("Copy", |e, _, _, cx| {
            e.board_copy(cx);
        }))
        .item(item("Paste", |e, _, _, cx| e.board_paste(cx)).disabled(!can_paste))
        .separator()
        .item(item(
            match (locked, several) {
                (true, true) => "Unlock panels",
                (true, false) => "Unlock panel",
                (false, true) => "Lock panels",
                (false, false) => "Lock panel",
            },
            |e, _, _, cx| e.board_lock_panels(!e.board_selection_locked(), cx),
        ))
        .separator()
        .item(item("Start a scene here", |e, id, _, cx| {
            e.board_split(id, Level::Scene, cx)
        }))
        .item(item("Start a sequence here", |e, id, _, cx| {
            e.board_split(id, Level::Sequence, cx)
        }))
        .item(item("Start an act here", |e, id, _, cx| {
            e.board_split(id, Level::Act, cx)
        }))
        .separator()
        .item(item("Make thumbnail sheet…", |e, id, window, cx| {
            e.board_sheet_dialog(id, window, cx)
        }))
        .item(
            item("Convert sheet to panels", |e, id, _, cx| {
                e.board_convert_sheet(id, cx)
            })
            .disabled(!sheet),
        )
        .item(item("Renumber…", |e, _, window, cx| {
            e.board_renumber_dialog(None, window, cx)
        }))
        .item(item("Apply storyboard preferences", |e, _, _, cx| {
            e.board_apply_preferences(cx)
        }))
    }
}
