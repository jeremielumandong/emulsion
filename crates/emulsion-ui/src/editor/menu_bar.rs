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
pub(super) const MENUS: [(&str, &str); 10] = [
    ("file", "File"),
    ("edit", "Edit"),
    ("image", "Image"),
    ("layer", "Layer"),
    ("select", "Select"),
    ("filter", "Filter"),
    ("view", "View"),
    ("window", "Window"),
    ("recipes", "Recipes"),
    ("help", "Help"),
];

impl EditorView {
    fn menu_button(
        &self,
        id: &'static str,
        name: &'static str,
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
                    .label(name)
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
                    .tooltip("Go to Home")
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
        self.menu_button("file", "File", p, cx, |menu, editor, window, cx| {
            let context = crate::workspace::destinations::Destination::for_editor(editor.read(cx));
            let owner = editor.downgrade();
            let template = owner.clone();
            let export = owner.clone();
            let storyboard = editor.read(cx).editor.storyboard().is_some();
            menu.menu(context.file_new_label(), Box::new(NewDocument))
                .menu(context.file_open_label(), Box::new(Open))
                .submenu("Import", window, cx, move |menu, _, _| {
                    Self::file_import_items(menu, context, owner.clone())
                })
                .separator()
                .menu("Close", Box::new(CloseTab))
                .menu("Save", Box::new(Save))
                .menu("Save As…", Box::new(SaveAs))
                .when(storyboard, |menu| {
                    menu.item(PopupMenuItem::new("Save as Storyboard Template…").on_click(
                        move |_, window, cx| {
                            template
                                .update(cx, |e, cx| e.save_storyboard_template_dialog(window, cx))
                                .ok();
                        },
                    ))
                })
                .separator()
                .menu("Print…", Box::new(Print))
                .menu("Export…", Box::new(Export))
                .when(storyboard, |menu| {
                    Self::storyboard_export_items(menu, export.clone())
                })
                .menu("Photo Library…", Box::new(ShowBatch))
                .separator()
                .menu("Quit", Box::new(Quit))
        })
    }

    /// File menu entries for storyboard exports.
    fn storyboard_export_items(menu: PopupMenu, owner: WeakEntity<Self>) -> PopupMenu {
        let (pdf, images, csv) = (owner.clone(), owner.clone(), owner.clone());
        let (movie, gif) = (owner.clone(), owner);
        menu.item(
            PopupMenuItem::new("Export Storyboard PDF…").on_click(move |_, window, cx| {
                pdf.update(cx, |e, cx| e.storyboard_print(true, window, cx))
                    .ok();
            }),
        )
        .item(
            PopupMenuItem::new("Export Panel Images…").on_click(move |_, window, cx| {
                images
                    .update(cx, |e, cx| e.storyboard_images_dialog(window, cx))
                    .ok();
            }),
        )
        .item(
            PopupMenuItem::new("Export Captions CSV…").on_click(move |_, _, cx| {
                csv.update(cx, |e, cx| e.storyboard_csv(cx)).ok();
            }),
        )
        .item(
            PopupMenuItem::new("Export Movie…").on_click(move |_, window, cx| {
                movie
                    .update(cx, |e, cx| {
                        e.storyboard_movie_dialog(super::storyboard_movie::Kind::Movie, window, cx)
                    })
                    .ok();
            }),
        )
        .item(
            PopupMenuItem::new("Export Animated GIF…").on_click(move |_, window, cx| {
                gif.update(cx, |e, cx| {
                    e.storyboard_movie_dialog(super::storyboard_movie::Kind::Gif, window, cx)
                })
                .ok();
            }),
        )
    }

    fn file_import_items(
        mut menu: PopupMenu,
        context: crate::workspace::destinations::Destination,
        owner: WeakEntity<Self>,
    ) -> PopupMenu {
        use crate::workspace::destinations::Destination;
        let item = |title: &'static str, run: fn(&mut Self, &mut Context<Self>)| {
            let owner = owner.clone();
            PopupMenuItem::new(title).on_click(move |_, _, cx| {
                owner.update(cx, run).ok();
            })
        };
        match context {
            Destination::Photo => {
                menu = menu
                    .item(item("Place images as layers…", Self::choose_design_asset))
                    .item(item("Import color lookup table…", |this, cx| {
                        this.import_lut(None, cx)
                    }));
            }
            Destination::Paint => {
                menu = menu
                    .item(item("Place images as layers…", Self::choose_design_asset))
                    .item(item("Import brushes…", Self::import_brushes));
            }
            Destination::Storyboard => {
                menu = menu
                    .item(item("Import into panel…", Self::import_into_panel))
                    .item(item("Import as panels…", Self::import_as_panels))
                    .item({
                        let owner = owner.clone();
                        PopupMenuItem::new("Import script…").on_click(move |_, window, cx| {
                            owner
                                .update(cx, |this, cx| this.open_script_import(window, cx))
                                .ok();
                        })
                    })
                    .separator()
                    .item(item("Place images as layers…", Self::choose_design_asset))
                    .item(item("Import brushes…", Self::import_brushes));
            }
            Destination::Design => {
                menu = menu
                    .item(item("Place images or SVG…", Self::choose_design_asset))
                    .item(item("Import video or audio…", Self::import_design_media))
                    .item(item("Import template…", Self::import_local_template));
            }
            Destination::Diagram => {
                menu = menu
                    .label("Add pages to this diagram")
                    .item(item("Visio (.vsdx, .vdx, .vsd)…", |this, cx| {
                        this.import_diagram_file_named(
                            "Import Visio pages (.vsdx, .vdx, .vsd) into this diagram",
                            cx,
                        )
                    }))
                    .item(item("draw.io (.drawio, .xml)…", |this, cx| {
                        this.import_diagram_file_named(
                            "Import draw.io pages (.drawio, .xml) into this diagram",
                            cx,
                        )
                    }))
                    .item(item("Lucid (.lucid, .lucidjson)…", |this, cx| {
                        this.import_diagram_file_named(
                            "Import Lucid export pages (.lucid, .lucidjson) into this diagram",
                            cx,
                        )
                    }))
                    .item(item("Mermaid, D2, Graphviz or Markdown…", |this, cx| {
                        this.import_diagram_file_named(
                            "Import Mermaid, D2, Graphviz, Markdown or Glyphtide diagrams",
                            cx,
                        )
                    }))
                    .item(item("CSV, SQL or text…", Self::import_diagram_data))
                    .separator()
                    .item(item(
                        "Import stencil library…",
                        Self::install_diagram_stencils,
                    ));
            }
            _ => {}
        }
        menu
    }

    pub(super) fn help_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("help", "Help", p, cx, |menu, _, _, _| {
            menu.menu("Ask AI Assistant…", Box::new(crate::actions::Ask))
                .separator()
                .menu("About Emulsion", Box::new(ShowAbout))
        })
    }

    pub(super) fn edit_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("edit", "Edit", p, cx, |menu, editor, _, cx| {
            let storyboard = editor.read(cx).editor.storyboard().is_some();
            let menu = menu
                .menu("Undo", Box::new(Undo))
                .menu("Redo", Box::new(Redo))
                .separator()
                .menu("Cut", Box::new(CutPixels))
                .menu("Copy", Box::new(CopyPixels))
                .menu("Paste", Box::new(PastePixels))
                .menu("Paste in Place", Box::new(PasteInPlace))
                .menu("Clear", Box::new(ClearPixels))
                .separator()
                .menu("Fill", Box::new(FillSelection))
                .menu("Content-Aware Fill", Box::new(ContentAwareFill))
                .separator()
                .menu("Free Transform", Box::new(FreeTransform))
                .menu("Scale", Box::new(TransformScale))
                .menu("Rotate", Box::new(TransformRotate))
                .menu("Distort", Box::new(TransformDistort))
                .menu("Warp", Box::new(TransformWarp));
            let menu = if storyboard {
                menu.separator()
                    .menu("Find and Replace Captions…", Box::new(FindReplaceCaptions))
                    .menu("Check Spelling…", Box::new(CheckCaptionSpelling))
            } else {
                menu
            };
            menu.separator().menu(
                "Keyboard Shortcuts and Preferences…",
                Box::new(ShowSettings),
            )
        })
    }

    pub(super) fn select_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("select", "Select", p, cx, |menu, _, _, _| {
            menu.menu("All", Box::new(SelectAll))
                .menu("Deselect", Box::new(Deselect))
                .menu("Inverse", Box::new(InvertSelection))
                .separator()
                .menu("Edit in Quick Mask Mode", Box::new(ToggleQuickMask))
        })
    }

    pub(super) fn view_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("view", "View", p, cx, |menu, editor, window, cx| {
            let menu = Self::storyboard_view_items(menu, editor, cx);
            let menu = Self::playback_view_items(menu, editor, cx);
            let menu = Self::stage_view_items(menu, editor, window, cx);
            let menu = Self::camera_view_items(menu, editor, window, cx);
            menu.menu("Zoom In", Box::new(ZoomIn))
                .menu("Zoom Out", Box::new(ZoomOut))
                .menu("Fit on Screen", Box::new(ZoomFit))
                .menu("100%", Box::new(Zoom100))
                .separator()
                .menu("Rotate View Clockwise", Box::new(RotateCw))
                .menu("Rotate View Counter-clockwise", Box::new(RotateCcw))
                .menu("Reset View Rotation", Box::new(ResetRotation))
                .menu("Flip View Horizontally", Box::new(FlipViewHorizontal))
                .menu("Flip View Vertically", Box::new(FlipViewVertical))
                .separator()
                .menu("Rulers", Box::new(ToggleRulers))
                .menu("Light or Dark Interface", Box::new(ToggleTheme))
        })
    }

    /// Photoshop's Window menu: workspaces, then every panel and toolbar.
    pub(super) fn window_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        self.menu_button("window", "Window", p, cx, |mut menu, editor, window, cx| {
            let view = editor.read(cx);
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
                .menu("Home", Box::new(ShowHome))
                .separator()
                .submenu("Layout", window, cx, {
                    let editor = editor.downgrade();
                    move |menu, _, cx| Self::workspace_layout_items(menu, editor.clone(), cx)
                })
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
