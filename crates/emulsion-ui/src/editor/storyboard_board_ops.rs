//! Board editing: panels, groups, locks, renumbering, thumbnail sheets, drag
//! and drop and the panel clipboard. Each change is one project Undo step in
//! core; errors go to the status bar.
use super::*;
use emulsion_core::project::PanelClip;
use emulsion_core::storyboard::{RenumberScope, ThumbnailGrid};
use gpui_kit::component::{WindowExt, checkbox::Checkbox};
use std::cell::Cell;

/// Panels copied on a Board. A GPUI global, so any open storyboard can
/// paste them.
pub(super) struct PanelClipboard(Arc<PanelClip>);
impl Global for PanelClipboard {}

/// Where dropped panels land.
#[derive(Clone, Copy, Debug)]
pub(in crate::editor) enum DropAt {
    /// Just before this panel, in its scene.
    Before(PageId),
    /// At the end of this scene.
    SceneEnd(GroupId),
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

impl EditorView {
    /// Whether any selected panel, or its scene, is locked.
    fn board_selection_has_lock(&self) -> bool {
        self.editor
            .storyboard()
            .is_some_and(|b| self.board_selection().iter().any(|id| b.is_locked(*id)))
    }

    fn board_layout(&self) -> Vec<PageId> {
        self.editor.page_list().iter().map(|m| m.id).collect()
    }

    /// Report a storyboard edit: refresh on success, status bar on error.
    fn board_result<T>(
        &mut self,
        result: Result<T, String>,
        done: impl FnOnce(&T) -> Option<String>,
        cx: &mut Context<Self>,
    ) -> Option<T> {
        match result {
            Ok(value) => {
                self.after_change(cx);
                if let Some(message) = done(&value) {
                    self.set_status(message, false, cx);
                }
                Some(value)
            }
            Err(error) => {
                self.set_status(error, true, cx);
                None
            }
        }
    }

    /// Change storyboard data as one Undo step.
    fn board_edit(
        &mut self,
        edit: impl FnOnce(&mut emulsion_core::storyboard::Storyboard) -> Result<(), String>,
        done: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let result = self.editor.edit_storyboard(edit);
        self.board_result(result, |_| done, cx);
    }

    /// Make the last selected panel active, so page commands act after it.
    fn board_focus_last(&mut self, cx: &mut Context<Self>) -> PageId {
        let last = *self.board_selection().last().unwrap();
        self.select_page(last, cx);
        last
    }

    pub(super) fn board_add(&mut self, smart: bool, cx: &mut Context<Self>) {
        let after = self.board_focus_last(cx);
        if smart {
            if !self.prepare_page_action(cx) {
                return;
            }
            let result = self.editor.smart_add_panel(after);
            self.board_result(result, |_| None, cx);
        } else {
            self.add_project_page(false, cx);
        }
        self.set_board_selection(vec![self.editor.active_page()]);
    }

    pub(super) fn board_duplicate(&mut self, cx: &mut Context<Self>) {
        let ids = self.board_selection();
        if let [id] = ids[..] {
            self.select_page(id, cx);
            self.add_project_page(true, cx);
            self.set_board_selection(vec![self.editor.active_page()]);
            return;
        }
        // Several panels duplicate together, as one Undo step.
        if !self.prepare_page_action(cx) {
            return;
        }
        let result = self
            .editor
            .copy_panels(&ids)
            .and_then(|clip| self.editor.paste_panels(ids.last().copied(), &clip));
        if let Some(new) = self.board_result(
            result,
            |new| Some(format!("Duplicated {}.", plural(new.len(), "panel"))),
            cx,
        ) {
            self.set_board_selection(new);
        }
    }

    pub(super) fn board_delete(&mut self, cx: &mut Context<Self>) {
        let ids = self.board_selection();
        if self.board_selection_has_lock() {
            self.set_status(
                "A selected panel is locked. Unlock it to delete it.",
                true,
                cx,
            );
            return;
        }
        if ids.len() >= self.editor.page_list().len() {
            self.set_status("Keep at least one panel in the storyboard.", true, cx);
            return;
        }
        if let [id] = ids[..] {
            self.delete_project_page(id, cx);
        } else {
            if !self.prepare_page_action(cx) {
                return;
            }
            let result = self.editor.remove_pages(&ids);
            self.board_result(
                result,
                |_| Some(format!("Deleted {}.", plural(ids.len(), "panel"))),
                cx,
            );
        }
        self.set_board_selection(vec![self.editor.active_page()]);
    }

    pub(super) fn board_lock_panels(&mut self, lock: bool, cx: &mut Context<Self>) {
        let ids = self.board_selection();
        let message = format!(
            "{} {}.",
            if lock { "Locked" } else { "Unlocked" },
            plural(ids.len(), "panel")
        );
        self.board_edit(
            |b| {
                for id in &ids {
                    b.panels.get_mut(id).ok_or("Panel does not exist.")?.locked = lock;
                }
                Ok(())
            },
            Some(message),
            cx,
        );
    }

    pub(super) fn board_lock_scene(&mut self, scene: GroupId, lock: bool, cx: &mut Context<Self>) {
        self.board_edit(
            |b| {
                b.scenes
                    .get_mut(&scene)
                    .ok_or("No scene has that ID.")?
                    .locked = lock;
                Ok(())
            },
            Some(
                if lock {
                    "Scene locked."
                } else {
                    "Scene unlocked."
                }
                .into(),
            ),
            cx,
        );
    }

    /// Give this storyboard the naming rules and Smart add layers from the
    /// storyboard preferences, as one Undo step.
    pub(super) fn board_apply_preferences(&mut self, cx: &mut Context<Self>) {
        let preferences = crate::app_state::settings(cx).storyboard.clone();
        self.board_edit(
            |b| {
                b.naming = preferences.naming;
                b.smart_add_layers = preferences.smart_add_layers;
                b.stage = preferences.stage;
                b.palette = preferences.palette;
                Ok(())
            },
            Some(
                "Applied the naming rules, Smart add layers, Stage guides and palette from Settings."
                    .into(),
            ),
            cx,
        );
    }

    /// Unlock a panel and its scene, as one Undo step.
    pub(super) fn unlock_panel_and_scene(&mut self, id: PageId, cx: &mut Context<Self>) {
        self.board_edit(
            |b| {
                let panel = b.panels.get_mut(&id).ok_or("Panel does not exist.")?;
                panel.locked = false;
                let scene = panel.scene;
                if let Some(scene) = b.scenes.get_mut(&scene) {
                    scene.locked = false;
                }
                Ok(())
            },
            Some("Panel unlocked.".into()),
            cx,
        );
    }

    pub(super) fn board_split(&mut self, panel: PageId, level: Level, cx: &mut Context<Self>) {
        let layout = self.board_layout();
        self.board_edit(
            |b| b.split(&layout, panel, level, None).map(|_| ()),
            Some(format!(
                "Started a new {} at this panel.",
                super::level_name(level)
            )),
            cx,
        );
    }

    pub(super) fn board_join(&mut self, group: GroupId, cx: &mut Context<Self>) {
        let layout = self.board_layout();
        self.board_edit(|b| b.join(&layout, group).map(|_| ()), None, cx);
    }

    pub(super) fn board_rename_dialog(
        &mut self,
        group: GroupId,
        current: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(current.to_string()));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let input = name.clone();
            let owner = owner.clone();
            dialog
                .title("Rename")
                .width(px(360.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Name")
                        .child(Input::new(&name)),
                )
                .footer(crate::widgets::form_dialog_footer("Rename"))
                .on_ok(move |_, _, cx| {
                    let name = input.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| {
                            let ok = this.editor.edit_storyboard(|b| b.rename(group, &name));
                            this.board_result(ok, |_| None, cx).is_some()
                        })
                        .unwrap_or(false)
                })
        });
    }

    /// Renumber scenes and/or panels. `groups` preselects a group; otherwise
    /// the dialog offers the selected panels' scenes or the whole board.
    pub(super) fn board_renumber_dialog(
        &mut self,
        groups: Option<Vec<GroupId>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let narrowed = groups.is_some();
        let groups = groups.unwrap_or_else(|| {
            let mut scenes: Vec<_> = self
                .board_selection()
                .iter()
                .filter_map(|id| board.panels.get(id).map(|p| p.scene))
                .collect();
            scenes.dedup();
            scenes
        });
        let only = Rc::new(Cell::new(narrowed));
        let scenes = Rc::new(Cell::new(true));
        let panels = Rc::new(Cell::new(true));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (o, s, p) = (only.clone(), scenes.clone(), panels.clone());
            let (only, scenes, panels) = (only.clone(), scenes.clone(), panels.clone());
            let groups = groups.clone();
            let owner = owner.clone();
            dialog
                .title("Renumber")
                .width(px(380.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Names follow the naming rules in Settings.")
                        .child(
                            Checkbox::new("board-renumber-only")
                                .label(if narrowed {
                                    "Only this group (otherwise the whole board)"
                                } else {
                                    "Only the selected panels' scenes (otherwise the whole board)"
                                })
                                .checked(o.get())
                                .on_click(move |value, _, _| o.set(*value)),
                        )
                        .child(
                            Checkbox::new("board-renumber-scenes")
                                .label("Scenes")
                                .checked(s.get())
                                .on_click(move |value, _, _| s.set(*value)),
                        )
                        .child(
                            Checkbox::new("board-renumber-panels")
                                .label("Panels")
                                .checked(p.get())
                                .on_click(move |value, _, _| p.set(*value)),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer("Renumber"))
                .on_ok(move |_, _, cx| {
                    let scope = if only.get() {
                        RenumberScope::Groups(groups.clone())
                    } else {
                        RenumberScope::All
                    };
                    let (scenes, panels) = (scenes.get(), panels.get());
                    owner
                        .update(cx, |this, cx| {
                            this.board_renumber(&scope, scenes, panels, cx)
                        })
                        .unwrap_or(false)
                })
        });
    }

    pub(super) fn board_renumber(
        &mut self,
        scope: &RenumberScope,
        scenes: bool,
        panels: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.prepare_page_action(cx) {
            return false;
        }
        let result = self.editor.renumber(scope, scenes, panels);
        self.board_result(
            result,
            |n| {
                Some(match n {
                    0 => "Names already follow the naming rules.".into(),
                    n => format!("Renamed {}.", plural(*n, "scene or panel")),
                })
            },
            cx,
        )
        .is_some()
    }

    pub(super) fn board_sheet_dialog(
        &mut self,
        id: PageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&id))
            .and_then(|p| p.thumbnails)
            .unwrap_or(ThumbnailGrid::new(3, 3));
        let field = |value: u8, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()))
        };
        let columns = field(current.columns, window, cx);
        let rows = field(current.rows, window, cx);
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let (c, r) = (columns.clone(), rows.clone());
            let owner = owner.clone();
            dialog
                .title("Thumbnail sheet")
                .width(px(360.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Draw rough frames in the grid, then convert them to panels.")
                        .child("Columns")
                        .child(Input::new(&columns))
                        .child("Rows")
                        .child(Input::new(&rows)),
                )
                .footer(crate::widgets::form_dialog_footer("Make sheet"))
                .on_ok(move |_, _, cx| {
                    let parse = |input: &Entity<InputState>, cx: &App| {
                        input.read(cx).value().trim().parse::<u8>().unwrap_or(0)
                    };
                    let grid = ThumbnailGrid {
                        columns: parse(&c, cx),
                        rows: parse(&r, cx),
                        ..current
                    };
                    owner
                        .update(cx, |this, cx| this.board_make_sheet(id, grid, cx))
                        .unwrap_or(false)
                })
        });
    }

    pub(super) fn board_make_sheet(
        &mut self,
        id: PageId,
        grid: ThumbnailGrid,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.prepare_page_action(cx) {
            return false;
        }
        let result = self.editor.edit_storyboard(|b| {
            grid.validate(b.settings.width, b.settings.height)?;
            b.panels
                .get_mut(&id)
                .ok_or("Panel does not exist.")?
                .thumbnails = Some(grid);
            Ok(())
        });
        self.board_result(
            result,
            |_| {
                Some(format!(
                    "Thumbnail sheet: draw inside the {} frames, then Convert sheet to panels.",
                    grid.columns as usize * grid.rows as usize
                ))
            },
            cx,
        )
        .is_some()
    }

    pub(super) fn board_convert_sheet(&mut self, id: PageId, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let result = self.editor.convert_thumbnails(id);
        if let Some(new) = self.board_result(
            result,
            |new| {
                Some(format!(
                    "Converted the sheet into {}.",
                    plural(new.len(), "panel")
                ))
            },
            cx,
        ) {
            self.set_board_selection(new);
        }
    }

    /// Move dragged panels. Dropping on a panel or a scene joins that scene;
    /// when that would regroup other panels, it is a plain reorder.
    pub(super) fn board_drop(&mut self, ids: Vec<PageId>, at: DropAt, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let layout = self.board_layout();
        let moving: HashSet<_> = ids.iter().copied().collect();
        let ids: Vec<_> = layout
            .iter()
            .copied()
            .filter(|id| moving.contains(id))
            .collect();
        let rest: Vec<_> = layout
            .iter()
            .copied()
            .filter(|id| !moving.contains(id))
            .collect();
        let scene_of = |id: &PageId| board.panels.get(id).map(|p| p.scene);
        let (to, scene) = match at {
            DropAt::Before(target) => {
                let Some(to) = rest.iter().position(|id| *id == target) else {
                    return;
                };
                (to, scene_of(&target))
            }
            DropAt::SceneEnd(scene) => {
                match rest.iter().rposition(|id| scene_of(id) == Some(scene)) {
                    Some(last) => (last + 1, Some(scene)),
                    // The whole scene is moving: nowhere else to go.
                    None => return,
                }
            }
        };
        if ids.is_empty() || !self.prepare_page_action(cx) {
            return;
        }
        let result = self
            .editor
            .move_panels(&ids, to, scene)
            .or_else(|_| self.editor.move_panels(&ids, to, None));
        if self.board_result(result, |_| None, cx).is_some() {
            self.set_board_selection(ids);
        }
    }

    /// Copy the selection to the panel clipboard. Whole scenes paste back as
    /// scenes.
    pub(super) fn board_copy(&mut self, cx: &mut Context<Self>) -> bool {
        let ids = self.board_selection();
        match self.editor.copy_panels(&ids) {
            Ok(clip) => {
                let message = if clip.whole_scenes {
                    format!("Copied {}.", plural(clip.scenes.len(), "scene"))
                } else {
                    format!("Copied {}.", plural(clip.panels.len(), "panel"))
                };
                cx.set_global(PanelClipboard(Arc::new(clip)));
                self.set_status(message, false, cx);
                true
            }
            Err(error) => {
                self.set_status(error, true, cx);
                false
            }
        }
    }

    pub(super) fn board_cut(&mut self, cx: &mut Context<Self>) {
        if self.board_selection_has_lock() {
            self.set_status("A selected panel is locked. Unlock it to cut it.", true, cx);
            return;
        }
        if self.board_copy(cx) {
            self.board_delete(cx);
        }
    }

    /// Paste the panel clipboard after the selection, from this or another
    /// storyboard.
    pub(super) fn board_paste(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = cx.try_global::<PanelClipboard>().map(|c| c.0.clone()) else {
            self.set_status("Copy panels on a Board first.", false, cx);
            return;
        };
        if !self.prepare_page_action(cx) {
            return;
        }
        let after = self.board_selection().last().copied();
        let result = self.editor.paste_panels(after, &clip);
        if let Some(new) = self.board_result(
            result,
            |new| Some(format!("Pasted {}.", plural(new.len(), "panel"))),
            cx,
        ) {
            self.set_board_selection(new);
        }
    }
}
