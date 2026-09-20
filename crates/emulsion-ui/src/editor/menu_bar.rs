//! A conventional menu bar — File · Edit · Image · Layer · Select · Filter ·
//! View · Window · Help — so people coming from Photoshop or GIMP find
//! commands where they expect them. Every item runs the same action or
//! editor method its shortcut and chip run; nothing lives only here.
//! Shortcut hints come from the default keymap table.

use super::*;
use crate::actions::{self, DEFAULTS};

#[derive(Default)]
pub struct MenuBarState {
    /// Index into `MENUS` of the open dropdown.
    pub open: Option<usize>,
}

/// What an item does when picked.
#[derive(Clone, Copy)]
enum Cmd {
    /// Dispatch a named action through the window (handled by the workspace).
    Act(&'static str),
    /// Call straight into the editor.
    Call(fn(&mut EditorView, &mut Window, &mut Context<EditorView>)),
    /// Add the n-th catalogue filter to the selected node (made smart first).
    Filter(usize),
    Sep,
}

struct Item {
    label: &'static str,
    cmd: Cmd,
}

const fn item(label: &'static str, cmd: Cmd) -> Item {
    Item { label, cmd }
}

const SEP: Item = Item {
    label: "",
    cmd: Cmd::Sep,
};

fn open_add_menu(e: &mut EditorView, _: &mut Window, cx: &mut Context<EditorView>) {
    e.menu = Some(Menu::Add);
    cx.notify();
}

fn menus() -> Vec<(&'static str, Vec<Item>)> {
    vec![
        (
            "File",
            vec![
                item("New…", Cmd::Act("NewDocument")),
                item("Open…", Cmd::Act("Open")),
                SEP,
                item("Save", Cmd::Act("Save")),
                item("Save As…", Cmd::Act("SaveAs")),
                item("Export…", Cmd::Act("Export")),
                SEP,
                item("Home", Cmd::Act("ShowHome")),
                item("Settings…", Cmd::Act("ShowSettings")),
                SEP,
                item("Quit", Cmd::Act("Quit")),
            ],
        ),
        (
            "Edit",
            vec![
                item("Undo", Cmd::Act("Undo")),
                item("Redo", Cmd::Act("Redo")),
                SEP,
                item("Cut pixels", Cmd::Act("CutPixels")),
                item("Copy pixels", Cmd::Act("CopyPixels")),
                item("Paste pixels", Cmd::Act("PastePixels")),
                item("Clear", Cmd::Act("ClearPixels")),
                SEP,
                item("Fill selection", Cmd::Act("FillSelection")),
                item("Content-aware fill", Cmd::Act("ContentAwareFill")),
                item("AI fill (LaMa)", Cmd::Call(|e, _, cx| e.ai_fill(cx))),
                SEP,
                item("Free transform", Cmd::Act("FreeTransform")),
                item(
                    "Warp…",
                    Cmd::Call(|e, _, cx| {
                        e.set_tool(Tool::Move, cx);
                        e.start_warp(cx);
                    }),
                ),
                SEP,
                item("Preferences…", Cmd::Act("ShowSettings")),
            ],
        ),
        (
            "Image",
            vec![
                item(
                    "Image / canvas size…",
                    Cmd::Call(|e, window, cx| e.toggle_size_panel(window, cx)),
                ),
                item("Crop", Cmd::Call(|e, _, cx| e.set_tool(Tool::Crop, cx))),
                SEP,
                item("Rotate view 90° clockwise", Cmd::Act("RotateCw")),
                item("Rotate view 90° counter-clockwise", Cmd::Act("RotateCcw")),
                item("Reset rotation", Cmd::Act("ResetRotation")),
                SEP,
                item(
                    "Lens correction (auto)",
                    Cmd::Call(|e, _, cx| e.lens_profile_auto(cx)),
                ),
                item(
                    "Remove background (AI)",
                    Cmd::Call(|e, _, cx| e.remove_background(cx)),
                ),
                item(
                    "Restore faces (AI)",
                    Cmd::Call(|e, _, cx| e.restore_faces(cx)),
                ),
                item("Upscale (AI)", Cmd::Call(|e, _, cx| e.ai_upscale(cx))),
            ],
        ),
        (
            "Layer",
            vec![
                item("New adjustment, fill or group…", Cmd::Call(open_add_menu)),
                item("Duplicate layer", Cmd::Act("DuplicateNode")),
                item("Delete layer", Cmd::Act("DeleteNode")),
                SEP,
                item("Group layers", Cmd::Act("GroupNodes")),
                item("Ungroup", Cmd::Act("Ungroup")),
                item("Bring forward", Cmd::Act("MoveNodeUp")),
                item("Send backward", Cmd::Act("MoveNodeDown")),
                item("Show / hide", Cmd::Act("ToggleNodeVisible")),
                SEP,
                item("Add layer mask", Cmd::Call(|e, _, cx| e.add_mask(cx))),
                item(
                    "Edit layer mask",
                    Cmd::Call(|e, _, cx| e.set_tool(Tool::Mask, cx)),
                ),
                item(
                    "Convert to smart layer",
                    Cmd::Call(|e, _, cx| e.convert_smart(cx)),
                ),
            ],
        ),
        (
            "Select",
            vec![
                item("All", Cmd::Act("SelectAll")),
                item("Deselect", Cmd::Act("Deselect")),
                item("Inverse", Cmd::Act("InvertSelection")),
                SEP,
                item(
                    "Grow 1 px",
                    Cmd::Call(|e, _, cx| e.modify_selection(1, 0.0, cx)),
                ),
                item(
                    "Shrink 1 px",
                    Cmd::Call(|e, _, cx| e.modify_selection(-1, 0.0, cx)),
                ),
                item(
                    "Feather 2 px",
                    Cmd::Call(|e, _, cx| e.modify_selection(0, 2.0, cx)),
                ),
                SEP,
                item(
                    "Select subject (AI)",
                    Cmd::Call(|e, _, cx| {
                        e.set_tool(Tool::Select, cx);
                        e.set_select(SelectShape::Quick, cx);
                    }),
                ),
            ],
        ),
        (
            "Filter",
            emulsion_filters::Filter::catalogue()
                .iter()
                .enumerate()
                .map(|(i, f)| Item {
                    label: f.label(),
                    cmd: Cmd::Filter(i),
                })
                .collect(),
        ),
        (
            "View",
            vec![
                item("Zoom in", Cmd::Act("ZoomIn")),
                item("Zoom out", Cmd::Act("ZoomOut")),
                item("Fit on screen", Cmd::Act("ZoomFit")),
                item("Actual pixels", Cmd::Act("Zoom100")),
                SEP,
                item("Rulers", Cmd::Act("ToggleRulers")),
                item(
                    "Snap",
                    Cmd::Call(|e, _, cx| {
                        e.snap = !e.snap;
                        cx.notify();
                    }),
                ),
            ],
        ),
        (
            "Window",
            vec![
                item("Navigator", Cmd::Call(|e, _, cx| e.toggle_navigator(cx))),
                item("Info", Cmd::Call(|e, _, cx| e.toggle_info(cx))),
                item("Brushes", Cmd::Call(|e, _, cx| e.toggle_presets(cx))),
                item("Recipes", Cmd::Call(|e, _, cx| e.toggle_recipes(cx))),
                item("History", Cmd::Call(|e, _, cx| e.open_history(cx))),
                item("Animation", Cmd::Call(|e, _, cx| e.toggle_animation(cx))),
                SEP,
                item("Ask the assistant", Cmd::Act("Ask")),
            ],
        ),
        (
            "Help",
            vec![
                item("Shortcuts and settings…", Cmd::Act("ShowSettings")),
                item(
                    "About Emulsion",
                    Cmd::Call(|e, _, cx| {
                        e.set_status(
                            format!(
                                "Emulsion {} · non-destructive image editor with optional AI",
                                env!("CARGO_PKG_VERSION")
                            ),
                            false,
                            cx,
                        )
                    }),
                ),
            ],
        ),
    ]
}

/// The default shortcut for an action, for the hint column.
fn shortcut_for(action: &str) -> Option<&'static str> {
    DEFAULTS
        .iter()
        .find(|(_, a, _)| *a == action)
        .map(|(_, _, keys)| *keys)
}

/// "ctrl-shift-s" → "Ctrl+Shift+S".
fn pretty_keys(keys: &str) -> String {
    keys.split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_string(),
            "shift" => "Shift".to_string(),
            "alt" => "Alt".to_string(),
            "cmd" => "Cmd".to_string(),
            "enter" => "Enter".to_string(),
            "escape" => "Esc".to_string(),
            "backspace" => "Backspace".to_string(),
            "delete" => "Delete".to_string(),
            other => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

impl EditorView {
    fn run_menu_cmd(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.open = None;
        match cmd {
            Cmd::Sep => {}
            Cmd::Act(name) => {
                if let Some(binding) = actions::binding(name, "f24", None) {
                    window.dispatch_action(binding.action().boxed_clone(), cx);
                }
            }
            Cmd::Call(f) => f(self, window, cx),
            Cmd::Filter(i) => {
                let Some(f) = emulsion_filters::Filter::catalogue().into_iter().nth(i) else {
                    return;
                };
                let Some(id) = self.selected else {
                    self.set_status("Select a layer to filter first.", false, cx);
                    return;
                };
                if !matches!(
                    self.editor.doc.node(id).map(|n| &n.kind),
                    Some(NodeKind::Smart { .. })
                ) {
                    self.convert_smart(cx);
                }
                if let Some(id) = self.selected {
                    self.add_filter(id, f, cx);
                }
            }
        }
        cx.notify();
    }

    /// The menu bar row, with the open dropdown beneath its title.
    pub(crate) fn menu_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.menus.open;
        let (accent, accent_fg, ink, panel, line, muted) =
            (p.accent, p.accent_fg, p.ink, p.panel, p.line, p.muted);
        let mut bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .px(px(8.))
            .h(px(26.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.chrome)
            .font_family(MONO_FONT)
            .text_size(px(11.))
            .text_color(p.chrome_fg)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.menus.open.is_some() {
                    this.menus.open = None;
                    cx.notify();
                }
            }));
        for (i, (title, items)) in menus().into_iter().enumerate() {
            let is_open = open == Some(i);
            let dropdown = is_open.then(|| {
                div()
                    .id(("menu-drop", i))
                    .absolute()
                    .top(px(26.))
                    .left_0()
                    .min_w(px(240.))
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(ink)
                    .bg(panel)
                    .text_color(ink)
                    .py(px(4.))
                    .children(items.into_iter().enumerate().map(|(k, it)| {
                        if matches!(it.cmd, Cmd::Sep) {
                            return div()
                                .h(px(1.))
                                .my(px(4.))
                                .mx(px(8.))
                                .bg(line)
                                .into_any_element();
                        }
                        let hint = match it.cmd {
                            Cmd::Act(name) => shortcut_for(name).map(pretty_keys),
                            _ => None,
                        };
                        let cmd = it.cmd;
                        div()
                            .id(("menu-item", i * 100 + k))
                            .flex()
                            .justify_between()
                            .gap(px(24.))
                            .px(px(12.))
                            .py(px(4.))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(accent).text_color(accent_fg))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run_menu_cmd(cmd, window, cx);
                            }))
                            .child(it.label)
                            .children(hint.map(|h| div().text_color(muted).child(h)))
                            .into_any_element()
                    }))
            });
            bar = bar.child(
                div()
                    .id(("menu-title", i))
                    .relative()
                    .px(px(8.))
                    .py(px(3.))
                    .cursor_pointer()
                    .when(is_open, |d| d.bg(accent).text_color(accent_fg))
                    .hover(move |s| s.bg(accent.opacity(0.25)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menus.open = if this.menus.open == Some(i) {
                            None
                        } else {
                            Some(i)
                        };
                        cx.notify();
                    }))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        // Sliding along the bar switches menus, as menus do.
                        if *hovering && this.menus.open.is_some() && this.menus.open != Some(i) {
                            this.menus.open = Some(i);
                            cx.notify();
                        }
                    }))
                    .child(title)
                    .children(dropdown),
            );
        }
        bar.into_any_element()
    }
}
