//! Storyboard layout presets in Window → Layout: Overview (the Board with
//! the Panel inspector), Drawing (the Stage with Paint's tools and the
//! Layers panel) and Timing (the Stage over the Timeline, with the Panel
//! inspector). They are ordinary workspace layouts, so saved layouts,
//! Reset and Customize work as in Paint and Photo, and storyboards reopen
//! the way they were last arranged.
use super::compact::Bar;
use super::*;
use emulsion_io::settings::WorkspaceLayout;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{PopupMenu, PopupMenuItem},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoryboardLayout {
    Overview,
    Drawing,
    Timing,
}

impl StoryboardLayout {
    pub(crate) const ALL: [Self; 3] = [Self::Overview, Self::Drawing, Self::Timing];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Drawing => "Drawing",
            Self::Timing => "Timing",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Overview => "The Board with the Panel inspector, for arranging and captioning",
            Self::Drawing => "The Stage with Paint's tools and the Layers panel",
            Self::Timing => "The Stage over the Timeline, with the Panel inspector",
        }
    }

    /// The preset as a workspace layout, on Paint's factory toolbars.
    pub(crate) fn layout(self) -> WorkspaceLayout {
        let overview = self == Self::Overview;
        // Overview and Timing give the sidebar to the inspector, Drawing to
        // layers.
        let inspector = self != Self::Drawing;
        WorkspaceLayout {
            draw_mode: true,
            sidebar_tab: "storyboard".into(),
            sidebar_upper_collapsed: !inspector,
            sidebar_layers_collapsed: inspector,
            storyboard_board: Some(overview),
            storyboard_timeline: Some(self == Self::Timing),
            ..WorkspaceLayout::default()
        }
    }
}

impl EditorView {
    /// The preset on screen: Timing while the Timeline is open, Overview
    /// while the Board is.
    pub(crate) fn current_storyboard_layout(&self) -> StoryboardLayout {
        if self.timeline_open() && !self.board_open() {
            StoryboardLayout::Timing
        } else if self.board_open() {
            StoryboardLayout::Overview
        } else {
            StoryboardLayout::Drawing
        }
    }

    pub(crate) fn apply_storyboard_layout(
        &mut self,
        preset: StoryboardLayout,
        cx: &mut Context<Self>,
    ) {
        if self.editor.storyboard().is_none() {
            return;
        }
        self.apply_workspace_layout(&preset.layout(), cx);
        if preset != StoryboardLayout::Drawing {
            // Painting controls have nothing to act on over the Board.
            for bar in [Bar::Options, Bar::Brushes] {
                self.compact.bars[bar as usize].open = false;
            }
        }
        self.remember_storyboard_layout(cx);
        self.set_status(format!("{} layout.", preset.label()), false, cx);
    }

    /// Keep this storyboard's toolbars and panels for the next storyboard
    /// opened. Storyboards always open on the Stage, so the Board state is
    /// left out; saved layouts and the presets keep it.
    pub(crate) fn remember_storyboard_layout(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        let layout = WorkspaceLayout {
            storyboard_board: None,
            storyboard_timeline: None,
            ..self.workspace_snapshot()
        };
        if crate::app_state::settings(cx).storyboard_workspace.as_ref() != Some(&layout) {
            crate::app_state::update_settings(cx, |s| s.storyboard_workspace = Some(layout));
        }
    }

    /// Arrange a newly opened storyboard the way storyboards were last left.
    pub(crate) fn restore_storyboard_layout(&mut self, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            return;
        }
        let Some(mut layout) = crate::app_state::settings(cx).storyboard_workspace.clone() else {
            return;
        };
        // Storyboard panels are always drawn with Paint's tools.
        layout.draw_mode = true;
        self.apply_workspace_layout(&layout, cx);
    }

    /// Window → Layout entries for a storyboard.
    pub(super) fn storyboard_layout_items(
        mut menu: PopupMenu,
        editor: WeakEntity<Self>,
        current: StoryboardLayout,
    ) -> PopupMenu {
        for preset in StoryboardLayout::ALL {
            let editor = editor.clone();
            menu = menu.item(
                PopupMenuItem::new(format!("{} layout", preset.label()))
                    .checked(current == preset)
                    .on_click(move |_, window, cx| {
                        editor
                            .update(cx, |this, cx| {
                                this.apply_storyboard_layout(preset, cx);
                                window.focus(&this.canvas_focus, cx);
                            })
                            .ok();
                    }),
            );
        }
        menu
    }

    /// The customizer's preset buttons for a storyboard.
    pub(super) fn storyboard_layout_buttons(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.current_storyboard_layout();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .children(StoryboardLayout::ALL.map(|preset| {
                let id = preset.label().to_lowercase();
                Button::new(SharedString::from(format!("layout-preset-{id}")))
                    .label(preset.label())
                    .small()
                    .ghost()
                    .tooltip(preset.tooltip())
                    .when(current == preset, |b| b.bg(p.ink).text_color(p.paper))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.apply_storyboard_layout(preset, cx)),
                    )
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};
    use gpui_kit::test::TestWindowExt;

    fn install_storyboard(
        ws: &Entity<crate::workspace::Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<EditorView> {
        let project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(64, 36)).unwrap();
        let editor = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        editor
    }

    #[gpui_kit::test]
    fn storyboard_presets_switch_views_and_their_panels_are_remembered_for_the_next_storyboard(
        cx: &mut TestAppContext,
    ) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let e = install_storyboard(&ws, cx);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.apply_storyboard_layout(StoryboardLayout::Overview, cx);
                assert!(e.board_open());
                assert!(e.draw_mode);
                assert!(e.sidebar_tab == SidebarTab::Storyboard);
                assert!(e.sidebar_layout.layers_collapsed);
                assert!(!e.compact.bars[Bar::Brushes as usize].open);
                assert_eq!(e.current_storyboard_layout(), StoryboardLayout::Overview);
            })
        });
        let saved = cx.update(|_, cx| {
            crate::app_state::settings(cx)
                .storyboard_workspace
                .clone()
                .unwrap()
        });
        assert_eq!(saved.storyboard_board, None);
        assert!(saved.sidebar_layers_collapsed);
        // The next storyboard opens with the same panels, on the Stage.
        let next = install_storyboard(&ws, cx);
        cx.update(|_, cx| {
            let next = next.read(cx);
            assert!(!next.board_open());
            assert!(next.sidebar_tab == SidebarTab::Storyboard);
            assert!(next.sidebar_layout.layers_collapsed);
            assert!(!next.compact.bars[Bar::Brushes as usize].open);
        });
        cx.update(|_, cx| {
            next.update(cx, |e, cx| {
                e.apply_storyboard_layout(StoryboardLayout::Drawing, cx);
                assert!(!e.board_open());
                assert!(!e.sidebar_layout.layers_collapsed);
                assert!(e.sidebar_layout.upper_collapsed);
                assert!(e.compact.bars[Bar::Tools as usize].open);
                // Reset brings a storyboard back to Drawing, not Photo.
                e.apply_storyboard_layout(StoryboardLayout::Overview, cx);
                e.reset_workspace(cx);
                assert!(!e.board_open());
                assert!(e.draw_mode);
            })
        });
    }

    #[gpui_kit::test]
    fn saved_custom_layouts_keep_the_board_view_and_the_layout_menu_lists_presets(
        cx: &mut TestAppContext,
    ) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let e = install_storyboard(&ws, cx);
        let custom = cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.apply_storyboard_layout(StoryboardLayout::Overview, cx);
                e.sidebar_layout.width = Some(420.);
                e.workspace_snapshot()
            })
        });
        assert_eq!(custom.storyboard_board, Some(true));
        assert_eq!(custom.storyboard_timeline, Some(false));
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.apply_storyboard_layout(StoryboardLayout::Drawing, cx);
                e.apply_workspace_layout(&custom, cx);
                assert!(e.board_open());
                assert_eq!(e.sidebar_layout.width, Some(420.));
            })
        });
        // A saved layout remembers the Timeline too.
        let timing = cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.apply_storyboard_layout(StoryboardLayout::Timing, cx);
                e.workspace_snapshot()
            })
        });
        assert_eq!(timing.storyboard_timeline, Some(true));
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.apply_workspace_layout(&custom, cx);
                assert!(!e.timeline_open());
                e.apply_workspace_layout(&timing, cx);
                assert!(e.timeline_open());
                e.apply_workspace_layout(&custom, cx);
            })
        });
        // A layout saved from a painting leaves the storyboard's view alone.
        let painting = WorkspaceLayout {
            draw_mode: true,
            ..WorkspaceLayout::default()
        };
        assert_eq!(painting.storyboard_board, None);
        cx.update(|_, cx| e.update(cx, |e, cx| e.apply_workspace_layout(&painting, cx)));
        assert!(cx.update(|_, cx| e.read(cx).board_open()));
        // Window → Layout → Drawing layout (the second entry).
        cx.update(|window, cx| {
            window.within("title-bar").click("window-menu-button", cx);
            window.within("popup-menu").click(2usize, cx);
            window.press("right", cx);
            window.press("down", cx);
            window.press("enter", cx);
        });
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| e.read(cx).board_open()));
    }
}
