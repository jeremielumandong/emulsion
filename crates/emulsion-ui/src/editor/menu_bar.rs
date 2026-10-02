//! Photoshop's menu bar order: File · Edit · Image · Layer · Select ·
//! Filter · View · Window. Items dispatch the same actions as their
//! shortcuts, so the menus show and share the person's key bindings.
use super::compact::Bar;
use super::*;
use crate::actions::*;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};

/// Menus a workspace may hide, in menu-bar order; `menu_name` shows each.
pub(super) const MENUS: [&str; 10] = [
    "file", "edit", "image", "layer", "select", "filter", "view", "window", "recipes", "help",
];

/// A menu's displayed name in the interface language.
pub(super) fn menu_name(id: &str) -> SharedString {
    t!(format!("menu.{id}")).into()
}

impl EditorView {
    fn menu_button(
        &self,
        id: &'static str,
        p: &Palette,
        cx: &Context<Self>,
        build: fn(
            PopupMenu,
            &Entity<EditorView>,
            &mut Window,
            &mut Context<PopupMenu>,
        ) -> PopupMenu,
    ) -> AnyElement {
        let editor = cx.entity().downgrade();
        div()
            .id(SharedString::from(format!("{id}-menu")))
            .when(!self.menu_visible(id), |d| d.hidden())
            .test_support()
            .child(
                Button::new(SharedString::from(format!("{id}-menu-button")))
                    .label(menu_name(id))
                    .small()
                    .ghost()
                    .text_color(p.ink)
                    .dropdown_menu(move |menu, window, cx| {
                        let Some(editor) = editor.upgrade() else {
                            return menu;
                        };
                        let focus = editor.read(cx).canvas_focus.clone();
                        build(menu.action_context(focus), &editor, window, cx)
                    }),
            )
            .into_any_element()
    }

    /// The brand is a direct Home link; application commands live in the menus.
    pub(super) fn app_menu(&self, p: &Palette, wide: bool, _cx: &Context<Self>) -> AnyElement {
        let focus = self.canvas_focus.clone();
        div()
            .id("app-menu")
            .test_support()
            .child(
                Button::new("app-menu-button")
                    .small()
                    .ghost()
                    .accessibility_label("Emulsion · Home")
                    .tooltip(t!("menu.go_home"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(img(crate::home::app_icon()).size(px(16.)).flex_none())
                            .when(wide, |d| {
                                d.child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.ink)
                                        .child("Emulsion"),
                                )
                            }),
                    )
                    .on_click(move |_, window, cx| {
                        focus.focus(window, cx);
                        window.dispatch_action(Box::new(ShowHome), cx);
                    }),
            )
            .into_any_element()
    }

    pub(super) fn file_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("file", p, cx, |menu, editor, window, cx| {
            let context = crate::workspace::destinations::Destination::for_editor(editor.read(cx));
            let owner = editor.downgrade();
            menu.menu(context.file_new_label(), Box::new(NewDocument))
                .menu(context.file_open_label(), Box::new(Open))
                .submenu(t!("file.import"), window, cx, move |menu, _, _| {
                    Self::file_import_items(menu, context, owner.clone())
                })
                .separator()
                .menu(t!("file.close"), Box::new(CloseTab))
                .menu(t!("file.save"), Box::new(Save))
                .menu(t!("file.save_as"), Box::new(SaveAs))
                .separator()
                .menu(t!("file.print"), Box::new(Print))
                .menu(t!("file.export"), Box::new(Export))
                .menu(t!("file.photo_library"), Box::new(ShowBatch))
                .separator()
                .menu(t!("file.quit"), Box::new(Quit))
        })
    }

    fn file_import_items(
        mut menu: PopupMenu,
        context: crate::workspace::destinations::Destination,
        owner: WeakEntity<Self>,
    ) -> PopupMenu {
        use crate::workspace::destinations::Destination;
        let item = |title: std::borrow::Cow<'static, str>,
                    run: fn(&mut Self, &mut Context<Self>)| {
            let owner = owner.clone();
            PopupMenuItem::new(title).on_click(move |_, _, cx| {
                owner.update(cx, run).ok();
            })
        };
        match context {
            Destination::Photo => {
                menu = menu
                    .item(item(t!("file.place_layers"), Self::choose_design_asset))
                    .item(item(t!("file.import_lut"), |this, cx| {
                        this.import_lut(None, cx)
                    }));
            }
            Destination::Paint => {
                menu = menu
                    .item(item(t!("file.place_layers"), Self::choose_design_asset))
                    .item(item(t!("file.import_brushes"), Self::import_brushes));
            }
            Destination::Design => {
                menu = menu
                    .item(item(t!("file.place_images_svg"), Self::choose_design_asset))
                    .item(item(t!("file.import_media"), Self::import_design_media))
                    .item(item(
                        t!("file.import_template"),
                        Self::import_local_template,
                    ));
            }
            Destination::Diagram => {
                menu = menu
                    .label(t!("file.add_diagram_pages"))
                    .item(item("Visio (.vsdx, .vdx, .vsd)…".into(), |this, cx| {
                        this.import_diagram_file_named(
                            "Import Visio pages (.vsdx, .vdx, .vsd) into this diagram",
                            cx,
                        )
                    }))
                    .item(item("draw.io (.drawio, .xml)…".into(), |this, cx| {
                        this.import_diagram_file_named(
                            "Import draw.io pages (.drawio, .xml) into this diagram",
                            cx,
                        )
                    }))
                    .item(item("Lucid (.lucid, .lucidjson)…".into(), |this, cx| {
                        this.import_diagram_file_named(
                            "Import Lucid export pages (.lucid, .lucidjson) into this diagram",
                            cx,
                        )
                    }))
                    .item(item(t!("file.import_text_diagrams"), |this, cx| {
                        this.import_diagram_file_named(
                            "Import Mermaid, D2, Graphviz, Markdown or Glyphtide diagrams",
                            cx,
                        )
                    }))
                    .item(item(t!("file.import_data"), Self::import_diagram_data))
                    .separator()
                    .item(item(
                        t!("file.import_stencils"),
                        Self::install_diagram_stencils,
                    ));
            }
            _ => {}
        }
        menu
    }

    pub(super) fn help_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("help", p, cx, |menu, _, _, _| {
            menu.menu(t!("help.ask_ai"), Box::new(crate::actions::Ask))
                .separator()
                .menu(t!("help.about"), Box::new(ShowAbout))
        })
    }

    pub(super) fn edit_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("edit", p, cx, |menu, _, _, _| {
            menu.menu(t!("edit.undo"), Box::new(Undo))
                .menu(t!("edit.redo"), Box::new(Redo))
                .separator()
                .menu(t!("edit.cut"), Box::new(CutPixels))
                .menu(t!("edit.copy"), Box::new(CopyPixels))
                .menu(t!("edit.paste"), Box::new(PastePixels))
                .menu(t!("edit.clear"), Box::new(ClearPixels))
                .separator()
                .menu(t!("edit.fill"), Box::new(FillSelection))
                .menu(t!("edit.content_aware_fill"), Box::new(ContentAwareFill))
                .separator()
                .menu(t!("edit.free_transform"), Box::new(FreeTransform))
                .menu(t!("edit.scale"), Box::new(TransformScale))
                .menu(t!("edit.rotate"), Box::new(TransformRotate))
                .menu(t!("edit.distort"), Box::new(TransformDistort))
                .menu(t!("edit.warp"), Box::new(TransformWarp))
                .separator()
                .menu(t!("edit.preferences"), Box::new(ShowSettings))
        })
    }

    pub(super) fn select_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("select", p, cx, |menu, _, _, _| {
            menu.menu(t!("select.all"), Box::new(SelectAll))
                .menu(t!("select.deselect"), Box::new(Deselect))
                .menu(t!("select.inverse"), Box::new(InvertSelection))
                .separator()
                .menu(t!("select.quick_mask"), Box::new(ToggleQuickMask))
        })
    }

    pub(super) fn view_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("view", p, cx, |menu, _, _, _| {
            menu.menu(t!("view.zoom_in"), Box::new(ZoomIn))
                .menu(t!("view.zoom_out"), Box::new(ZoomOut))
                .menu(t!("view.fit"), Box::new(ZoomFit))
                .menu("100%", Box::new(Zoom100))
                .separator()
                .menu(t!("view.rotate_cw"), Box::new(RotateCw))
                .menu(t!("view.rotate_ccw"), Box::new(RotateCcw))
                .menu(t!("view.reset_rotation"), Box::new(ResetRotation))
                .separator()
                .menu(t!("view.rulers"), Box::new(ToggleRulers))
                .menu(t!("view.theme"), Box::new(ToggleTheme))
        })
    }

    /// Photoshop's Window menu: workspaces, then every panel and toolbar.
    pub(super) fn window_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("window", p, cx, |mut menu, editor, window, cx| {
            let view = editor.read(cx);
            let overlay = view.compact.overlay;
            let panels = !view.sidebar_layout.collapsed;
            let open: Vec<bool> = Bar::ALL
                .iter()
                .map(|bar| view.compact.bars[*bar as usize].open)
                .collect();
            let item =
                |label: std::borrow::Cow<'static, str>,
                 checked: bool,
                 f: fn(&mut EditorView, &mut Window, &mut Context<EditorView>)| {
                    let editor = editor.downgrade();
                    PopupMenuItem::new(label)
                        .checked(checked)
                        .on_click(move |_, window, cx| {
                            editor.update(cx, |this, cx| f(this, window, cx)).ok();
                        })
                };
            menu = menu
                .menu(t!("window.home"), Box::new(ShowHome))
                .separator()
                .submenu(t!("window.layout"), window, cx, {
                    let editor = editor.downgrade();
                    move |menu, _, cx| Self::workspace_layout_items(menu, editor.clone(), cx)
                })
                .separator()
                .label(t!("window.toolbars"));
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
                .label(t!("window.panels"))
                .item(item(t!("window.properties"), false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::Properties, cx)
                }))
                .item(item(t!("window.adjustments"), false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::Adjustments, cx)
                }))
                .item(item(t!("window.history"), false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::History, cx)
                }))
                .item(item(t!("window.info"), false, |this, _, cx| {
                    this.show_sidebar_tab(SidebarTab::Info, cx)
                }))
                .item(item(t!("window.layers"), false, |this, window, cx| {
                    this.show_dock_tab(DockTab::Layers, window, cx)
                }))
                .item(item(t!("window.channels"), false, |this, window, cx| {
                    this.show_dock_tab(DockTab::Channels, window, cx)
                }))
                .item(item(t!("window.paths"), false, |this, window, cx| {
                    this.show_dock_tab(DockTab::Paths, window, cx)
                }));
            menu.separator()
                .item(item(t!("window.show_dock"), panels, |this, _, cx| {
                    this.sidebar_layout.collapsed = !this.sidebar_layout.collapsed;
                    cx.notify();
                }))
                .item(item(
                    t!("window.toolbars_beside"),
                    !overlay,
                    |this, _, cx| {
                        this.compact.overlay = !this.compact.overlay;
                        cx.notify();
                    },
                ))
        })
    }
}
