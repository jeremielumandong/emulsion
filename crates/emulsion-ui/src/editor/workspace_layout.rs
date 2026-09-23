//! Saved workspace presentation. Never changes image pixels or history.
use super::compact::{Bar, CompactLayout, Edge};
use super::*;
use emulsion_io::settings::{ToolbarPlacement, WorkspaceLayout, WorkspacePreset};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};

const MENUS: [(&str, &str); 4] = [
    ("image", "Image"),
    ("layer", "Layer"),
    ("filter", "Filter"),
    ("recipes", "Recipes"),
];

impl EditorView {
    pub(super) fn menu_visible(&self, id: &str) -> bool {
        !self
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
                    }
                })
                .collect(),
            tool_ids: self.compact.tool_ids.clone(),
            hidden_menu_ids: self.compact.hidden_menu_ids.clone(),
            draw_mode: self.draw_mode,
            sidebar_collapsed: self.sidebar_layout.collapsed,
            sidebar_width: self.sidebar_layout.width.unwrap_or(320.),
            sidebar_tab: match self.sidebar_tab {
                SidebarTab::Histogram => "histogram",
                SidebarTab::Info => "info",
                SidebarTab::History => "history",
                SidebarTab::Adjustments => "adjustments",
                SidebarTab::Navigator => "navigator",
                SidebarTab::BrushSettings => "brush-settings",
                _ => "properties",
            }
            .into(),
        }
    }

    pub(crate) fn apply_workspace_layout(
        &mut self,
        layout: &WorkspaceLayout,
        cx: &mut Context<Self>,
    ) {
        self.compact = CompactLayout::new(cx);
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
        self.compact.hidden_menu_ids = MENUS
            .iter()
            .filter(|(id, _)| layout.hidden_menu_ids.iter().any(|hidden| hidden == id))
            .map(|(id, _)| id.to_string())
            .collect();
        self.draw_mode = layout.draw_mode;
        self.rail.flyout = None;
        let tab = match layout.sidebar_tab.as_str() {
            "histogram" => SidebarTab::Histogram,
            "info" => SidebarTab::Info,
            "history" => SidebarTab::History,
            "adjustments" => SidebarTab::Adjustments,
            "navigator" => SidebarTab::Navigator,
            "brush-settings" => SidebarTab::BrushSettings,
            _ => SidebarTab::Properties,
        };
        self.select_sidebar(tab, cx);
        self.sidebar_layout.collapsed = layout.sidebar_collapsed;
        self.sidebar_layout.width = layout
            .sidebar_width
            .is_finite()
            .then(|| layout.sidebar_width.clamp(220., 560.));
        cx.notify();
    }

    pub(super) fn reset_workspace(&mut self, cx: &mut Context<Self>) {
        self.apply_workspace_layout(&WorkspaceLayout::default(), cx);
        self.set_status(
            "Factory workspace restored. Save as default to use it for new images.",
            false,
            cx,
        );
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
            self.workspace_customizer =
                Some(cx.new(|cx| {
                    InputState::new(window, cx).placeholder("Preset name, e.g. Painting")
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
                self.set_status("Enter a preset name (1–80 characters).", true, cx);
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
                self.set_status("Remove a workspace preset first (32 maximum).", true, cx);
                return;
            }
        }
        match settings.save() {
            Ok(()) => {
                cx.global_mut::<crate::app_state::AppSettings>().0 = settings;
                cx.refresh_windows();
                self.set_status(
                    if as_default {
                        "Workspace saved as the default for new images."
                    } else {
                        "Workspace preset saved."
                    },
                    false,
                    cx,
                );
            }
            Err(error) => self.set_status(format!("Could not save workspace: {error}"), true, cx),
        }
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
        Some(div().id("compact-layout-menu-content").test_support()
            .track_focus(&self.workspace_customizer_focus)
            .absolute().top_2().right_2().w(rems(32.)).max_w(window.viewport_size().width - px(48.))
            .max_h(window.viewport_size().height - px(150.)).overflow_y_scroll()
            .occlude().bg(p.panel).border_1().border_color(p.line).shadow_md()
            .p_3().flex().flex_col().gap_3()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" { this.workspace_customizer = None; window.focus(&this.canvas_focus, cx); cx.stop_propagation(); cx.notify(); }
            }))
            .child(div().flex().items_center().justify_between().child(label("Customize workspace", p)).child(
                Button::new("workspace-customizer-close").label("Done").small().on_click(cx.listener(|this, _, window, cx| this.toggle_workspace_customizer(window, cx)))))
            .child(mono("Drag toolbar grips to float or dock. Choose your tools below.", 11., p.muted))
            .child(self.workspace_presets(p, cx))
            .child(label("Visible toolbars", p)).child(self.toolbar_toggles(p, cx))
            .child(label("Visible menus", p))
            .child(div().flex().flex_wrap().gap_1().children(MENUS.into_iter().map(|(id, name)| {
                let shown = self.menu_visible(id);
                Button::new(SharedString::from(format!("workspace-menu-{id}"))).label(name).small()
                    .when(shown, |b| b.bg(p.soft_bg).border_1().border_color(p.line))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.menu_visible(id) { this.compact.hidden_menu_ids.push(id.into()); }
                        else { this.compact.hidden_menu_ids.retain(|hidden| hidden != id); }
                        cx.notify();
                    }))
            })))
            .child(label("Save workspace", p)).child(Input::new(&input))
            .child(div().flex().flex_wrap().gap_2()
                .child(Button::new("workspace-save-preset").label("Save preset").small().on_click(cx.listener(|this, _, _, cx| this.save_workspace(false, cx))))
                .child(Button::new("workspace-save-default").label("Save as default").small().on_click(cx.listener(|this, _, _, cx| this.save_workspace(true, cx))))
                .when_some(default, |row, layout| row.child(Button::new("workspace-restore-default").label("Load my default").small().on_click(cx.listener(move |this, _, _, cx| this.apply_workspace_layout(&layout, cx))))))
            .child(mono("Saving an existing name replaces that preset. Reset restores the factory layout.", 10., p.muted))
            .children(saved.into_iter().map(|preset| {
                let name = preset.name.clone();
                div().flex().gap_2().child(Button::new(SharedString::from(format!("workspace-load-{name}"))).label(name.clone()).small().on_click(cx.listener(move |this, _, _, cx| this.apply_workspace_layout(&preset.layout, cx))))
                    .child(Button::new(SharedString::from(format!("workspace-delete-{name}"))).label("Remove").small().ghost().on_click(cx.listener(move |_, _, _, cx| crate::app_state::update_settings(cx, |s| s.workspace_presets.retain(|p| p.name != name)))))
            }))
            .child(toolbox).into_any_element())
    }
}
