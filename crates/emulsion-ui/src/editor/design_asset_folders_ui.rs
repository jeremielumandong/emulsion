//! Native nested folders for reusable creative asset references.
use super::*;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::cell::Cell;
impl EditorView {
    pub(super) fn creative_folder_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.creative.folder;
        let folders = self.creative.catalog.asset_folders.clone();
        let label = current
            .and_then(|id| folders.iter().find(|f| f.id == id))
            .map(|f| f.name.clone())
            .unwrap_or_else(|| t!("editor.design_asset_folders_ui.all_folders").into_owned());
        let owner = cx.weak_entity();
        let mut row = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Button::new("creative-folder-filter")
                    .label(label)
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        let all = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(t!("editor.design_asset_folders_ui.all_folders"))
                                .on_click(move |_, _, cx| {
                                    all.update(cx, |this, cx| {
                                        this.creative.folder = None;
                                        cx.notify();
                                    })
                                    .ok();
                                }),
                        );
                        for f in &folders {
                            let id = f.id;
                            let owner = owner.clone();
                            let name = folder_label(&folders, id);
                            menu = menu.item(PopupMenuItem::new(name).on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.creative.folder = Some(id);
                                        cx.notify();
                                    })
                                    .ok();
                            }));
                        }
                        menu
                    }),
            )
            .child(
                Button::new("creative-folder-new")
                    .label(t!("editor.design_asset_folders_ui.new_folder"))
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.creative_folder_dialog(None, window, cx)
                    })),
            );
        if let Some(id) = current {
            row = row
                .child(
                    Button::new("creative-folder-edit")
                        .label(t!("editor.design_asset_folders_ui.rename_move"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.creative_folder_dialog(Some(id), window, cx)
                        })),
                )
                .child(
                    Button::new("creative-folder-remove")
                        .label(t!("editor.design_asset_folders_ui.remove_folder"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.creative.folder = None;
                            this.catalog_edit(move |c| c.remove_asset_folder(id), cx);
                        })),
                );
        }
        row.into_any_element()
    }
    fn creative_folder_dialog(
        &mut self,
        id: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = id.and_then(|id| {
            self.creative
                .catalog
                .asset_folders
                .iter()
                .find(|f| f.id == id)
        });
        let parent = existing.map(|f| f.parent).unwrap_or(self.creative.folder);
        let name = existing
            .map(|f| f.name.clone())
            .unwrap_or_else(|| t!("editor.design_asset_folders_ui.default_name").into_owned());
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let parent = Rc::new(Cell::new(parent));
        let folders = self.creative.catalog.asset_folders.clone();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let name = name.clone();
            let parent = parent.clone();
            let apply_parent = parent.clone();
            let folders = folders.clone();
            let owner = owner.clone();
            let label = parent
                .get()
                .map(|id| folder_label(&folders, id))
                .unwrap_or_else(|| t!("editor.design_asset_folders_ui.top_level").into_owned());
            dialog
                .title(t!("editor.design_asset_folders_ui.title").to_string())
                .width(px(420.))
                .child(
                    div()
                        .child(t!("editor.design_asset_folders_ui.folder_name").to_string())
                        .child(Input::new(&name)),
                )
                .child(
                    Button::new("creative-folder-parent")
                        .label(t!("editor.design_asset_folders_ui.parent", name = label))
                        .small()
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            let root = parent.clone();
                            menu = menu.item(
                                PopupMenuItem::new(t!("editor.design_asset_folders_ui.top_level"))
                                    .on_click(move |_, _, cx| {
                                        root.set(None);
                                        cx.refresh_windows();
                                    }),
                            );
                            for f in folders.iter().filter(|f| Some(f.id) != id) {
                                let state = parent.clone();
                                let key = f.id;
                                menu = menu.item(
                                    PopupMenuItem::new(folder_label(&folders, key)).on_click(
                                        move |_, _, cx| {
                                            state.set(Some(key));
                                            cx.refresh_windows();
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_asset_folders_ui.save_folder"
                )))
                .on_ok(move |_, _, cx| {
                    let name = name.read(cx).value().trim().to_owned();
                    let parent = apply_parent.get();
                    owner
                        .update(cx, |this, cx| {
                            let mut trial = this.creative.catalog.clone();
                            if let Err(e) = trial.set_asset_folder(id, name.clone(), parent) {
                                this.set_status(e.to_string(), true, cx);
                                return false;
                            }
                            this.catalog_edit(
                                move |c| c.set_asset_folder(id, name, parent).map(|_| ()),
                                cx,
                            );
                            true
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(super) fn move_creative_asset_dialog(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asset) = self.creative.catalog.assets.iter().find(|a| a.id == id) else {
            return;
        };
        let selected = Rc::new(Cell::new(asset.folder));
        let folders = self.creative.catalog.asset_folders.clone();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let state = selected.clone();
            let apply = selected.clone();
            let folders = folders.clone();
            let label = state
                .get()
                .map(|id| folder_label(&folders, id))
                .unwrap_or_else(|| t!("home.unfiled").into_owned());
            let owner = owner.clone();
            dialog
                .title(t!("editor.design_asset_folders_ui.move_title").to_string())
                .width(px(420.))
                .child(
                    Button::new("creative-asset-folder-choice")
                        .label(label)
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            let root = state.clone();
                            menu = menu.item(PopupMenuItem::new(t!("home.unfiled")).on_click(
                                move |_, _, cx| {
                                    root.set(None);
                                    cx.refresh_windows();
                                },
                            ));
                            for folder in &folders {
                                let id = folder.id;
                                let state = state.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(folder_label(&folders, id)).on_click(
                                        move |_, _, cx| {
                                            state.set(Some(id));
                                            cx.refresh_windows();
                                        },
                                    ),
                                );
                            }
                            menu
                        }),
                )
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_asset_folders_ui.move_reference"
                )))
                .on_ok(move |_, _, cx| {
                    let folder = apply.get();
                    owner
                        .update(cx, |this, cx| {
                            this.catalog_edit(move |c| c.move_creative_asset(id, folder), cx)
                        })
                        .is_ok()
                })
        });
    }
}
fn folder_label(folders: &[emulsion_io::creative_library::AssetFolder], id: u64) -> String {
    let mut path = Vec::new();
    let mut current = Some(id);
    for _ in 0..32 {
        let Some(id) = current else { break };
        let Some(folder) = folders.iter().find(|f| f.id == id) else {
            break;
        };
        path.push(folder.name.clone());
        current = folder.parent;
    }
    path.reverse();
    path.join(" / ")
}
