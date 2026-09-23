//! Photoshop's menu bar order: File · Edit · Image · Layer · Select ·
//! Filter · View · Window. Items dispatch the same actions as their
//! shortcuts, so the menus show and share the person's key bindings.
use super::compact::Bar;
use super::*;
use crate::actions::*;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};

/// Menus a workspace may hide, in menu-bar order.
pub(super) const MENUS: [(&str, &str); 9] = [
    ("file", "File"),
    ("edit", "Edit"),
    ("image", "Image"),
    ("layer", "Layer"),
    ("select", "Select"),
    ("filter", "Filter"),
    ("view", "View"),
    ("window", "Window"),
    ("recipes", "Recipes"),
];

impl EditorView {
    fn menu_button(
        &self,
        id: &'static str,
        name: &'static str,
        p: &Palette,
        cx: &Context<Self>,
        build: fn(PopupMenu, &Entity<EditorView>, &mut App) -> PopupMenu,
    ) -> AnyElement {
        let editor = cx.entity().downgrade();
        div()
            .id(SharedString::from(format!("{id}-menu")))
            .when(!self.menu_visible(id), |d| d.hidden())
            .test_support()
            .child(
                Button::new(SharedString::from(format!("{id}-menu-button")))
                    .label(name)
                    .small()
                    .ghost()
                    .text_color(p.ink)
                    .dropdown_menu(move |menu, _, cx| {
                        let Some(editor) = editor.upgrade() else {
                            return menu;
                        };
                        let focus = editor.read(cx).canvas_focus.clone();
                        build(menu.action_context(focus), &editor, cx)
                    }),
            )
            .into_any_element()
    }

    pub(super) fn file_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("file", "File", p, cx, |menu, _, _| {
            menu.menu("New…", Box::new(NewDocument))
                .menu("Open…", Box::new(Open))
                .separator()
                .menu("Close", Box::new(CloseTab))
                .menu("Save", Box::new(Save))
                .menu("Save As…", Box::new(SaveAs))
                .separator()
                .menu("Export…", Box::new(Export))
                .separator()
                .menu("Quit", Box::new(Quit))
        })
    }

    pub(super) fn edit_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("edit", "Edit", p, cx, |menu, _, _| {
            menu.menu("Undo", Box::new(Undo))
                .menu("Redo", Box::new(Redo))
                .separator()
                .menu("Cut", Box::new(CutPixels))
                .menu("Copy", Box::new(CopyPixels))
                .menu("Paste", Box::new(PastePixels))
                .menu("Clear", Box::new(ClearPixels))
                .separator()
                .menu("Fill", Box::new(FillSelection))
                .menu("Content-Aware Fill", Box::new(ContentAwareFill))
                .separator()
                .menu("Free Transform", Box::new(FreeTransform))
                .menu("Scale", Box::new(TransformScale))
                .menu("Rotate", Box::new(TransformRotate))
                .menu("Distort", Box::new(TransformDistort))
                .menu("Warp", Box::new(TransformWarp))
                .separator()
                .menu(
                    "Keyboard Shortcuts and Preferences…",
                    Box::new(ShowSettings),
                )
        })
    }

    pub(super) fn select_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("select", "Select", p, cx, |menu, _, _| {
            menu.menu("All", Box::new(SelectAll))
                .menu("Deselect", Box::new(Deselect))
                .menu("Inverse", Box::new(InvertSelection))
        })
    }

    pub(super) fn view_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("view", "View", p, cx, |menu, _, _| {
            menu.menu("Zoom In", Box::new(ZoomIn))
                .menu("Zoom Out", Box::new(ZoomOut))
                .menu("Fit on Screen", Box::new(ZoomFit))
                .menu("100%", Box::new(Zoom100))
                .separator()
                .menu("Rotate View Clockwise", Box::new(RotateCw))
                .menu("Rotate View Counter-clockwise", Box::new(RotateCcw))
                .menu("Reset View Rotation", Box::new(ResetRotation))
                .separator()
                .menu("Rulers", Box::new(ToggleRulers))
                .menu("Light or Dark Interface", Box::new(ToggleTheme))
        })
    }

    /// Photoshop's Window menu: workspaces, then every panel and toolbar.
    pub(super) fn window_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("window", "Window", p, cx, |mut menu, editor, cx| {
            let view = editor.read(cx);
            let draw = view.draw_mode;
            let overlay = view.compact.overlay;
            let panels = !view.sidebar_layout.collapsed;
            let open: Vec<bool> = Bar::ALL
                .iter()
                .map(|bar| view.compact.bars[*bar as usize].open)
                .collect();
            let item =
                |label: &str,
                 checked: bool,
                 f: fn(&mut EditorView, &mut Window, &mut Context<EditorView>)| {
                    let editor = editor.downgrade();
                    PopupMenuItem::new(label.to_string())
                        .checked(checked)
                        .on_click(move |_, window, cx| {
                            editor.update(cx, |this, cx| f(this, window, cx)).ok();
                        })
                };
            menu = menu
                .label("Workspace")
                .item(item("Photo (Essentials)", !draw, |this, _, cx| {
                    if this.draw_mode {
                        this.toggle_draw_mode(cx)
                    }
                }))
                .item(item("Draw", draw, |this, _, cx| {
                    if !this.draw_mode {
                        this.toggle_draw_mode(cx)
                    }
                }))
                .item(item("Reset Workspace", false, |this, _, cx| {
                    this.reset_workspace(cx)
                }))
                .item(item("Customize Workspace…", false, |this, window, cx| {
                    this.toggle_workspace_customizer(window, cx)
                }))
                .separator()
                .label("Toolbars");
            for (bar, open) in Bar::ALL.into_iter().zip(open) {
                let editor = editor.downgrade();
                menu = menu.item(PopupMenuItem::new(bar.label()).checked(open).on_click(
                    move |_, _, cx| {
                        editor
                            .update(cx, |this, cx| {
                                let state = &mut this.compact.bars[bar as usize];
                                state.open = !state.open;
                                this.rail.flyout = None;
                                cx.notify();
                            })
                            .ok();
                    },
                ));
            }
            // Photoshop lists each panel by name; choosing one opens the
            // dock on it.
            menu = menu
                .separator()
                .label("Panels")
                .item(item("Properties", false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::Properties, cx)
                }))
                .item(item("Adjustments", false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::Adjustments, cx)
                }))
                .item(item("History", false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::History, cx)
                }))
                .item(item("Info", false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::Info, cx)
                }))
                .item(item("Layers", false, |this, window, cx| {
                    this.show_dock_tab(DockTab::Layers, window, cx)
                }))
                .item(item("Channels", false, |this, window, cx| {
                    this.show_dock_tab(DockTab::Channels, window, cx)
                }))
                .item(item("Paths", false, |this, window, cx| {
                    this.show_dock_tab(DockTab::Paths, window, cx)
                }));
            menu.separator()
                .item(item("Show Panel Dock", panels, |this, _, cx| {
                    this.sidebar_layout.collapsed = !this.sidebar_layout.collapsed;
                    cx.notify();
                }))
                .item(item(
                    "Toolbars Beside the Canvas",
                    !overlay,
                    |this, _, cx| {
                        this.compact.overlay = !this.compact.overlay;
                        cx.notify();
                    },
                ))
        })
    }
}
