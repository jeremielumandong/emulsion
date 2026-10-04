//! Saved workspace presentation. Never changes image pixels or history.
use super::compact::{Bar, CompactLayout, Edge, MAX_SCALE, MIN_SCALE};
use super::*;
use emulsion_io::settings::{ToolbarPlacement, WorkspaceLayout, WorkspacePreset};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};

use super::menu_bar::{MENUS, menu_name};

impl EditorView {
    pub(super) fn menu_visible(&self, id: &str) -> bool {
        // Keep the layout recovery controls reachable, including old presets
        // that hid Window while the separate header picker still existed.
        id == "window"
            || !self
                .compact
                .hidden_menu_ids
                .iter()
                .any(|hidden| hidden == id)
    }

    pub(crate) fn workspace_snapshot(&self) -> WorkspaceLayout {
        WorkspaceLayout {
            toolbar_placements: Bar::ALL
                .into_iter()
                .map(|id| {
                    let bar = &self.compact.bars[id as usize];
                    ToolbarPlacement {
                        id: id.name().into(),
                        edge: match bar.edge {
                            Edge::Left => "left",
                            Edge::Right => "right",
                            Edge::Top => "top",
                            Edge::Bottom => "bottom",
                            Edge::Floating => "floating",
                        }
                        .into(),
                        visible: bar.open,
                        x: f32::from(bar.position.x),
                        y: f32::from(bar.position.y),
                        scale: bar.scale,
                    }
                })
                .collect(),
            tool_ids: self.compact.tool_ids.clone(),
            hidden_menu_ids: self.compact.hidden_menu_ids.clone(),
            draw_mode: self.draw_mode,
            sidebar_collapsed: self.sidebar_layout.collapsed,
            sidebar_width: self.sidebar_layout.width.unwrap_or(300.),
            sidebar_tab: self.sidebar_tab.key().into(),
            dock_tab: match self.dock_tab {
                DockTab::Layers => "layers",
                DockTab::Channels => "channels",
                DockTab::Paths => "paths",
            }
            .into(),
            sidebar_upper_collapsed: self.sidebar_layout.upper_collapsed,
            sidebar_layers_collapsed: self.sidebar_layout.layers_collapsed,
            sidebar_colors_collapsed: self.sidebar_layout.colors_collapsed,
            sidebar_color_tab: self.sidebar_layout.color_tab,
            sidebar_colors_height: self.sidebar_layout.colors_height,
            toolbars_overlay: Some(self.compact.overlay),
            tool_columns: self.compact.tool_columns,
            storyboard_board: self.editor.storyboard().map(|_| self.board_open()),
            storyboard_timeline: self.editor.storyboard().map(|_| self.timeline_open()),
        }
    }

    pub(crate) fn apply_workspace_layout(
        &mut self,
        layout: &WorkspaceLayout,
        cx: &mut Context<Self>,
    ) {
        if !self.photo_transform_ready(cx) {
            return;
        }
        // Bars a layout does not mention (older saves) take the mode's defaults.
        self.compact = CompactLayout::for_mode(layout.draw_mode, cx);
        for saved in &layout.toolbar_placements {
            let Some(id) = Bar::ALL.into_iter().find(|id| id.name() == saved.id) else {
                continue;
            };
            let bar = &mut self.compact.bars[id as usize];
            bar.edge = match saved.edge.as_str() {
                "left" => Edge::Left,
                "right" => Edge::Right,
                "top" => Edge::Top,
                "bottom" => Edge::Bottom,
                "floating" => Edge::Floating,
                _ => bar.edge,
            };
            bar.open = saved.visible;
            let coordinate = |v: f32| {
                if v.is_finite() {
                    v.clamp(0., 10000.)
                } else {
                    0.
                }
            };
            bar.position = point(px(coordinate(saved.x)), px(coordinate(saved.y)));
            if saved.scale.is_finite() {
                bar.scale = saved.scale.clamp(MIN_SCALE, MAX_SCALE);
            }
        }
        for name in &layout.tool_ids {
            if rail::GROUPS
                .iter()
                .flat_map(|group| group.iter())
                .any(|item| item.name == name)
                && !self.compact.tool_ids.contains(name)
            {
                self.compact.tool_ids.push(name.clone());
            }
        }
        self.compact.tool_columns = layout.tool_columns.clamp(1, 2);
        if let Some(overlay) = layout.toolbars_overlay {
            self.compact.overlay = overlay;
        }
        self.compact.hidden_menu_ids = MENUS
            .iter()
            .filter(|id| layout.hidden_menu_ids.iter().any(|hidden| hidden == *id))
            .map(|id| id.to_string())
            .collect();
        self.draw_mode = layout.draw_mode;
        self.rail.flyout = None;
        // The Panel inspector only exists in storyboards.
        let tab = if layout.sidebar_tab == SidebarTab::Storyboard.key()
            && self.editor.storyboard().is_some()
        {
            SidebarTab::Storyboard
        } else {
            SidebarTab::from_key(&layout.sidebar_tab)
        };
        self.select_sidebar(tab, cx);
        self.sidebar_layout.collapsed = layout.sidebar_collapsed;
        self.sidebar_layout.overlay_open = false;
        self.sidebar_layout.upper_collapsed = layout.sidebar_upper_collapsed;
        self.sidebar_layout.layers_collapsed = layout.sidebar_layers_collapsed;
        self.sidebar_layout.colors_collapsed = layout.sidebar_colors_collapsed;
        self.sidebar_layout.color_tab = layout.sidebar_color_tab;
        self.sidebar_layout.colors_height = if layout.sidebar_colors_height.is_finite() {
            layout.sidebar_colors_height.clamp(48., 240.)
        } else {
            64.
        };
        self.dock_tab = match layout.dock_tab.as_str() {
            "channels" => DockTab::Channels,
            "paths" => DockTab::Paths,
            _ => DockTab::Layers,
        };
        self.sidebar_layout.width = layout
            .sidebar_width
            .is_finite()
            .then(|| layout.sidebar_width.clamp(220., 560.));
        if let Some(open) = layout.storyboard_board
            && self.editor.storyboard().is_some()
        {
            self.pages_ui.board.open = open;
        }
        if let Some(open) = layout.storyboard_timeline
            && self.editor.storyboard().is_some()
        {
            self.timeline_ui.open = open;
        }
        cx.notify();
    }

    /// Restore the factory arrangement of the current mode.
    pub(super) fn reset_workspace(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_some() {
            self.apply_storyboard_layout(super::storyboard_layout::StoryboardLayout::Drawing, cx);
            return;
        }
        let layout = WorkspaceLayout {
            draw_mode: self.draw_mode,
            ..Default::default()
        };
        self.apply_workspace_layout(&layout, cx);
        self.set_status(t!("editor.workspace_layout.restored"), false, cx);
    }

    pub(super) fn toggle_workspace_customizer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_customizer.is_some() {
            self.workspace_customizer = None;
            window.focus(&self.canvas_focus, cx);
        } else {
            self.workspace_customizer = Some(cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("editor.workspace_layout.preset_placeholder"))
            }));
            window.focus(&self.workspace_customizer_focus, cx);
        }
        cx.notify();
    }

    fn save_workspace(&mut self, as_default: bool, cx: &mut Context<Self>) {
        let layout = self.workspace_snapshot();
        let mut settings = crate::app_state::settings(cx).clone();
        if as_default {
            settings.workspace_default = Some(layout);
        } else {
            let name = self
                .workspace_customizer
                .as_ref()
                .map(|input| input.read(cx).value().trim().to_string())
                .unwrap_or_default();
            if name.is_empty() || name.chars().count() > 80 {
                self.set_status(t!("editor.workspace_layout.name_invalid"), true, cx);
                return;
            }
            if let Some(saved) = settings
                .workspace_presets
                .iter_mut()
                .find(|p| p.name == name)
            {
                saved.layout = layout;
            } else if settings.workspace_presets.len() < 32 {
                settings
                    .workspace_presets
                    .push(WorkspacePreset { name, layout });
            } else {
                self.set_status(t!("editor.workspace_layout.too_many"), true, cx);
                return;
            }
        }
        // Use the same ordered writer as mode switches and other preferences.
        // Apply the draft now; completion must never replace newer UI settings.
        cx.global_mut::<crate::app_state::AppSettings>().0 = settings.clone();
        let save = crate::settings_writer::save(settings, cx);
        cx.refresh_windows();
        self.set_status(t!("editor.workspace_layout.saving"), false, cx);
        cx.spawn(async move |this, cx| {
            let result = save.await;
            this.update(cx, |this, cx| match result {
                Ok(()) => this.set_status(
                    if as_default {
                        t!("editor.workspace_layout.saved_default")
                    } else {
                        t!("editor.workspace_layout.saved_preset")
                    },
                    false,
                    cx,
                ),
                Err(error) => this.set_status(
                    t!(
                        "editor.workspace_layout.save_failed",
                        error = error.to_string()
                    ),
                    true,
                    cx,
                ),
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn workspace_customizer(
        &mut self,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let input = self.workspace_customizer.clone()?;
        let saved = crate::app_state::settings(cx).workspace_presets.clone();
        let default = crate::app_state::settings(cx).workspace_default.clone();
        let toolbox = self.toolbox_customizer(p, cx);
        Some(
            div()
                .id("compact-layout-menu-content")
                .test_support()
                .track_focus(&self.workspace_customizer_focus)
                .absolute()
                .top_2()
                .right_2()
                .w(rems(32.))
                .max_w(window.viewport_size().width - px(48.))
                .max_h(window.viewport_size().height - px(150.))
                .overflow_y_scroll()
                .occlude()
                .bg(p.panel)
                .border_1()
                .border_color(p.line)
                .shadow_md()
                .p_3()
                .flex()
                .flex_col()
                .gap_3()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.workspace_customizer = None;
                        window.focus(&this.canvas_focus, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label(t!("editor.workspace_layout.title"), p))
                        .child(
                            Button::new("workspace-customizer-close")
                                .label(t!("editor.workspace_layout.done"))
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_workspace_customizer(window, cx)
                                })),
                        ),
                )
                .child(mono(t!("editor.workspace_layout.intro"), 11., p.muted))
                .child(self.workspace_presets(p, cx))
                .child(label(t!("window.toolbars"), p))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(mono(t!("editor.workspace_layout.docked"), 10., p.muted))
                        .children(
                            [
                                ("beside", t!("editor.workspace_layout.beside"), false),
                                ("over", t!("editor.workspace_layout.over"), true),
                            ]
                            .map(|(id, name, over)| {
                                let on = self.compact.overlay == over;
                                Button::new(SharedString::from(format!("toolbar-placement-{id}")))
                                    .label(name)
                                    .small()
                                    .when(on, |b| b.bg(p.ink).text_color(p.paper))
                                    .when(!on, |b| b.ghost())
                                    .tooltip(if over {
                                        t!("editor.workspace_layout.over_tip")
                                    } else {
                                        t!("editor.workspace_layout.beside_tip")
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.compact.overlay = over;
                                        cx.notify();
                                    }))
                            }),
                        ),
                )
                .child(self.toolbar_toggles(p, cx))
                .child(label(t!("editor.workspace_layout.visible_menus"), p))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .children(MENUS.into_iter().map(|id| {
                            let shown = self.menu_visible(id);
                            Button::new(SharedString::from(format!("workspace-menu-{id}")))
                                .label(menu_name(id))
                                .small()
                                .disabled(id == "window")
                                .when(id == "window", |b| {
                                    b.tooltip(t!("editor.workspace_layout.window_tip"))
                                })
                                .when(shown, |b| b.bg(p.soft_bg).border_1().border_color(p.line))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.menu_visible(id) {
                                        this.compact.hidden_menu_ids.push(id.into());
                                    } else {
                                        this.compact.hidden_menu_ids.retain(|hidden| hidden != id);
                                    }
                                    cx.notify();
                                }))
                        })),
                )
                .child(label(t!("editor.workspace_layout.save_workspace"), p))
                .child(Input::new(&input))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("workspace-save-preset")
                                .label(t!("editor.workspace_layout.save_preset"))
                                .small()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_workspace(false, cx)),
                                ),
                        )
                        .child(
                            Button::new("workspace-save-default")
                                .label(t!("editor.workspace_layout.save_default"))
                                .small()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_workspace(true, cx)),
                                ),
                        )
                        .when_some(default, |row, layout| {
                            row.child(
                                Button::new("workspace-restore-default")
                                    .label(t!("editor.workspace_layout.load_default"))
                                    .small()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.apply_workspace_layout(&layout, cx)
                                    })),
                            )
                        }),
                )
                .child(mono(t!("editor.workspace_layout.save_hint"), 10., p.muted))
                .children(saved.into_iter().map(|preset| {
                    let name = preset.name.clone();
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("workspace-load-{name}")))
                                .label(name.clone())
                                .small()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.apply_workspace_layout(&preset.layout, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("workspace-delete-{name}")))
                                .label(t!("editor.workspace_layout.remove"))
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    crate::app_state::update_settings(cx, |s| {
                                        s.workspace_presets.retain(|p| p.name != name)
                                    })
                                })),
                        )
                }))
                .child(toolbox)
                .into_any_element(),
        )
    }
}
