//! Local Home folders organize persistent references; source files stay in place.
use crate::{
    home::{control, file_name},
    theme::Palette,
    workspace::Workspace,
};
use emulsion_core::creation::CanvasKind;
use emulsion_io::creative_library::{self as library, Catalog};
use emulsion_io::recent;
use gpui_kit::component::WindowExt;
use gpui_kit::component::{
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{collections::HashSet, path::Path};

#[derive(Default)]
pub(crate) struct HomeProjects {
    pub catalog: Catalog,
    pub folder: Option<u64>,
    pub trash: bool,
    pub kind: Option<CanvasKind>,
    loaded: bool,
    loading: bool,
    recents: Vec<recent::Recent>,
    message: String,
}

pub(crate) fn file_classification(
    path: &Path,
    record: Option<&library::ProjectRecord>,
) -> Option<CanvasKind> {
    record
        .and_then(|p| p.kind_override)
        .or_else(|| {
            (image::ImageFormat::from_path(path).is_ok() || emulsion_io::raw::is_raw(path))
                .then_some(CanvasKind::Photo)
        })
        .or_else(|| record.and_then(|p| p.kind))
}
impl Workspace {
    pub(crate) fn home_project_notice(&self) -> Option<AnyElement> {
        (!self.home_state.projects.message.is_empty()).then(|| {
            div()
                .text_sm()
                .child(self.home_state.projects.message.clone())
                .into_any_element()
        })
    }
    pub(crate) fn home_file_menu(
        &self,
        path: std::path::PathBuf,
        star: bool,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + Clone + use<> {
        let owner = cx.weak_entity();
        let record = self
            .home_state
            .projects
            .catalog
            .projects
            .iter()
            .find(|p| p.path == path)
            .cloned();
        let folders = self.home_state.projects.catalog.folders.clone();
        let cloud_menu = self.cloud_file_menu(path.clone(), cx);
        move |menu, window, cx| {
            let open = owner.clone();
            let open_path = path.clone();
            let details = owner.clone();
            let details_path = path.clone();
            let pinned = path.clone();
            let mut menu = menu
                .item(PopupMenuItem::new("Open").on_click(move |_, window, cx| {
                    open.update(cx, |this, cx| this.open_path(open_path.clone(), window, cx))
                        .ok();
                }))
                .item(
                    PopupMenuItem::new("File details").on_click(move |_, _, cx| {
                        details
                            .update(cx, |this, cx| {
                                this.home_state.selected = Some(details_path.clone());
                                this.home_state.details = true;
                                cx.notify();
                            })
                            .ok();
                    }),
                )
                .item(
                    PopupMenuItem::new(if star { "Unpin" } else { "Pin" }).on_click(
                        move |_, _, cx| {
                            crate::app_state::update_settings(cx, |s| {
                                if s.starred_files.contains(&pinned) {
                                    s.starred_files.retain(|p| p != &pinned);
                                } else {
                                    s.starred_files.push(pinned.clone());
                                }
                            });
                            cx.refresh_windows();
                        },
                    ),
                )
                .separator();
            if let Some(record) = &record {
                let rename = owner.clone();
                let id = record.id;
                menu = menu.item(PopupMenuItem::new("Rename in Home…").on_click(
                    move |_, window, cx| {
                        rename
                            .update(cx, |this, cx| {
                                this.home_project_name_dialog(Some(id), None, window, cx)
                            })
                            .ok();
                    },
                ));
            }
            let move_owner = owner.clone();
            let move_path = path.clone();
            let destinations = folders.clone();
            let current = record.as_ref().and_then(|p| p.folder);
            menu = menu.submenu("Move to project…", window, cx, move |mut menu, _, _| {
                for (folder, name) in std::iter::once((None, "Unfiled".to_string()))
                    .chain(destinations.iter().map(|f| (Some(f.id), f.name.clone())))
                {
                    let owner = move_owner.clone();
                    let path = move_path.clone();
                    menu = menu.item(
                        PopupMenuItem::new(name)
                            .checked(folder == current)
                            .disabled(folder == current)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.home_edit_file(
                                            path.clone(),
                                            move |c, id| c.move_project(id, folder),
                                            cx,
                                        );
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            });
            let classify = owner.clone();
            let classify_path = path.clone();
            let selected = record.as_ref().and_then(|p| p.kind_override);
            menu = menu.submenu("Classify as…", window, cx, move |mut menu, _, _| {
                for kind in std::iter::once(None).chain(CanvasKind::ALL.into_iter().map(Some)) {
                    let owner = classify.clone();
                    let path = classify_path.clone();
                    menu = menu.item(
                        PopupMenuItem::new(kind.map_or("Automatic", CanvasKind::label))
                            .checked(selected == kind)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.home_edit_file(
                                            path.clone(),
                                            move |c, id| c.classify_project(id, kind),
                                            cx,
                                        );
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            });
            let trash = owner.clone();
            let trash_path = path.clone();
            let trashed = record.as_ref().is_some_and(|p| p.trashed);
            menu = menu.separator().item(
                PopupMenuItem::new(if trashed {
                    "Restore file"
                } else {
                    "Move to Trash"
                })
                .on_click(move |_, _, cx| {
                    trash
                        .update(cx, |this, cx| {
                            this.home_edit_file(
                                trash_path.clone(),
                                move |c, id| {
                                    c.projects.iter_mut().find(|p| p.id == id).unwrap().trashed =
                                        !trashed;
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .ok();
                }),
            );
            let forget = owner.clone();
            let forget_path = path.clone();
            cloud_menu(menu).item(PopupMenuItem::new("Forget entry · keep file").on_click(
                move |_, _, cx| {
                    forget
                        .update(cx, |this, cx| this.remove_recent(&forget_path, cx))
                        .ok();
                },
            ))
        }
    }
    fn home_edit_file(
        &mut self,
        path: std::path::PathBuf,
        edit: impl FnOnce(&mut Catalog, u64) -> emulsion_io::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let kind = self.home_project_kind(&path);
        let entry = self
            .home_project_entries()
            .into_iter()
            .find(|r| r.path == path)
            .unwrap_or(recent::Recent {
                path,
                opened: recent::now(),
                summary: String::new(),
            });
        self.home_project_edit(
            move |c| {
                let id = c.remember_project(&entry, kind)?;
                edit(c, id)
            },
            cx,
        );
    }
    fn home_editor_kind(&self, e: &crate::editor::EditorView) -> CanvasKind {
        match e.editor.kind() {
            Some(emulsion_core::project::ProjectKind::Design) => CanvasKind::Design,
            Some(emulsion_core::project::ProjectKind::Diagram) => CanvasKind::Diagram,
            None => e.home_canvas_kind.unwrap_or_else(|| {
                let path = e.editor.path.as_deref().or(e.source.as_deref());
                path.and_then(|path| {
                    self.home_state
                        .projects
                        .catalog
                        .projects
                        .iter()
                        .find(|p| p.path == path)
                        .and_then(|p| {
                            p.kind_override.or_else(|| {
                                emulsion_io::is_native(path).then_some(p.kind).flatten()
                            })
                        })
                })
                .unwrap_or(CanvasKind::Photo)
            }),
        }
    }
    pub(crate) fn remember_saved_project(
        &mut self,
        path: std::path::PathBuf,
        editor: Entity<crate::editor::EditorView>,
        cx: &mut Context<Self>,
    ) {
        let e = editor.read(cx);
        let folder = e.home_folder_on_save;
        let kind = self.home_editor_kind(e);
        let explicit_kind = e.home_canvas_kind;
        let entry = recent::Recent {
            path,
            opened: recent::now(),
            summary: format!("{} · {} layers", kind.label(), e.editor.doc.nodes.len()),
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    library::update(&library::root(), move |catalog| {
                        let id = catalog.remember_project(&entry, Some(kind))?;
                        if let Some(kind) = explicit_kind {
                            catalog
                                .projects
                                .iter_mut()
                                .find(|p| p.id == id)
                                .unwrap()
                                .kind_override
                                .get_or_insert(kind);
                        }
                        if let Some(folder) = folder {
                            let folder =
                                folder.filter(|id| catalog.folders.iter().any(|f| f.id == *id));
                            catalog
                                .projects
                                .iter_mut()
                                .find(|p| p.id == id)
                                .unwrap()
                                .folder = folder;
                        }
                        Ok(())
                    })
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((catalog, _)) => {
                    if catalog.revision >= this.home_state.projects.catalog.revision {
                        this.home_state.projects.catalog = catalog;
                    }
                    editor.update(cx, |e, _| {
                        if e.home_folder_on_save == folder {
                            e.home_folder_on_save = None;
                        }
                        if e.home_canvas_kind == explicit_kind {
                            e.home_canvas_kind = None;
                        }
                    });
                    cx.notify();
                }
                Err(error) => editor.update(cx, |e, cx| {
                    e.set_status(
                        format!("File saved; could not update Home: {error}"),
                        true,
                        cx,
                    )
                }),
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn ensure_home_projects(&mut self, cx: &mut Context<Self>) {
        if self.home_state.projects.loading
            || (self.home_state.projects.loaded && self.home_state.projects.recents == self.recents)
        {
            return;
        }
        self.home_state.projects.loading = true;
        let recents = self.recents.clone();
        let entries = recents
            .iter()
            .map(|r| {
                let kind = self.tabs.iter().find_map(|ed| {
                    let ed = ed.read(cx);
                    let path = ed.editor.path.as_ref().or(ed.source.as_ref());
                    (path == Some(&r.path)).then(|| self.home_editor_kind(ed))
                });
                (r.clone(), kind)
            })
            .collect::<Vec<_>>();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    library::update(&library::root(), |c| {
                        for (entry, kind) in entries {
                            if entry.path.is_file() {
                                c.remember_project(&entry, kind)?;
                            }
                        }
                        Ok(())
                    })
                    .map(|(c, _)| c)
                })
                .await;
            this.update(cx, |this, cx| {
                let state = &mut this.home_state.projects;
                state.loading = false;
                state.loaded = true;
                state.recents = recents;
                match result {
                    Ok(c) if c.revision >= state.catalog.revision => state.catalog = c,
                    Ok(_) => {}
                    Err(e) => state.message = e.to_string(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn home_project_edit(
        &mut self,
        edit: impl FnOnce(&mut Catalog) -> emulsion_io::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { library::update(&library::root(), edit) })
                .await;
            this.update(cx, |this, cx| {
                let state = &mut this.home_state.projects;
                match result {
                    Ok((c, _)) => {
                        if c.revision >= state.catalog.revision {
                            state.catalog = c;
                        }
                        state.message.clear();
                    }
                    Err(e) => state.message = e.to_string(),
                }
                this.home_state.checked.clear();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn forget_home_project(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = path.to_path_buf();
        self.home_project_edit(
            move |c| {
                c.projects.retain(|p| p.path != path);
                Ok(())
            },
            cx,
        );
    }
    pub(crate) fn home_project_entries(&self) -> Vec<recent::Recent> {
        let mut all = self.recents.clone();
        let paths = all.iter().map(|r| r.path.clone()).collect::<HashSet<_>>();
        all.extend(
            self.home_state
                .projects
                .catalog
                .projects
                .iter()
                .filter(|p| !paths.contains(&p.path))
                .map(|p| recent::Recent {
                    path: p.path.clone(),
                    opened: p.opened,
                    summary: p.summary.clone(),
                }),
        );
        all.sort_by_key(|a| std::cmp::Reverse(a.opened));
        all
    }
    pub(crate) fn home_project_name(&self, path: &Path) -> String {
        self.home_state
            .projects
            .catalog
            .projects
            .iter()
            .find(|p| p.path == path)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| file_name(path))
    }
    pub(crate) fn home_project_kind(&self, path: &Path) -> Option<CanvasKind> {
        file_classification(
            path,
            self.home_state
                .projects
                .catalog
                .projects
                .iter()
                .find(|p| p.path == path),
        )
    }
    pub(crate) fn home_project_name_dialog(
        &mut self,
        project: Option<u64>,
        folder: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = project
            .and_then(|id| {
                self.home_state
                    .projects
                    .catalog
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
            })
            .or_else(|| {
                folder.and_then(|id| {
                    self.home_state
                        .projects
                        .catalog
                        .folders
                        .iter()
                        .find(|f| f.id == id)
                        .map(|f| f.name.clone())
                })
            })
            .unwrap_or_else(|| "New folder".into());
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let input_value = input.clone();
            let owner = owner.clone();
            dialog
                .title(if project.is_some() {
                    "Rename in Home"
                } else if folder.is_some() {
                    "Rename folder"
                } else {
                    "New project folder"
                })
                .width(px(400.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child("Name")
                        .child(Input::new(&input))
                        .child("This changes library organization; source files stay in place."),
                )
                .footer(crate::widgets::form_dialog_footer("Save"))
                .on_ok(move |_, _, cx| {
                    let name = input_value.read(cx).value().trim().to_string();
                    if name.is_empty()
                        || name.chars().count() > 200
                        || name.chars().any(char::is_control)
                    {
                        return false;
                    }
                    owner
                        .update(cx, |this, cx| {
                            this.home_project_edit(
                                move |c| {
                                    if let Some(id) = project {
                                        let p =
                                            c.projects.iter_mut().find(|p| p.id == id).ok_or_else(
                                                || {
                                                    emulsion_io::IoError::Manifest(
                                                        "Project no longer exists.".into(),
                                                    )
                                                },
                                            )?;
                                        p.name = name;
                                    } else if let Some(id) = folder {
                                        let f =
                                            c.folders.iter_mut().find(|f| f.id == id).ok_or_else(
                                                || {
                                                    emulsion_io::IoError::Manifest(
                                                        "Folder no longer exists.".into(),
                                                    )
                                                },
                                            )?;
                                        f.name = name;
                                    } else {
                                        c.add_project_folder(name)?;
                                    }
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .is_ok()
                })
        });
    }
    pub(crate) fn home_projects_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let state = &self.home_state.projects;
        let folders = state.catalog.folders.clone();
        let owner = cx.weak_entity();
        let mut row = div()
            .id("home-project-controls")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(p.line)
            .child(
                control(
                    "home-project-folder",
                    if state.trash {
                        "Trash".into()
                    } else {
                        state
                            .folder
                            .and_then(|id| folders.iter().find(|f| f.id == id))
                            .map(|f| f.name.clone())
                            .unwrap_or_else(|| "All projects".into())
                    },
                    p,
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for (id, name, trash) in
                        std::iter::once((None, "All projects".to_string(), false))
                            .chain(folders.iter().map(|f| (Some(f.id), f.name.clone(), false)))
                            .chain(std::iter::once((None, "Trash · files kept".into(), true)))
                    {
                        let owner = owner.clone();
                        menu = menu.item(PopupMenuItem::new(name).on_click(move |_, _, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    this.home_state.projects.folder = id;
                                    this.home_state.unfiled = false;
                                    this.home_state.projects.trash = trash;
                                    this.home_state.folder = None;
                                    this.home_state.selected = None;
                                    this.home_state.checked.clear();
                                    cx.notify();
                                })
                                .ok();
                        }));
                    }
                    menu
                }),
            );
        let owner = cx.weak_entity();
        row = row
            .child(
                control(
                    "home-project-kind",
                    state.kind.map_or("All workspaces", CanvasKind::label),
                    p,
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for kind in std::iter::once(None).chain(CanvasKind::ALL.into_iter().map(Some)) {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(kind.map_or("All workspaces", CanvasKind::label))
                                .on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.home_state.projects.kind = kind;
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                        );
                    }
                    menu
                }),
            )
            .child(
                control("home-new-folder-menu", "New folder…", p).on_click(cx.listener(
                    |this, _, window, cx| this.home_project_name_dialog(None, None, window, cx),
                )),
            )
            .child(
                control("home-project-reload", "Reload", p).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.home_state.projects.loaded = false;
                        this.ensure_home_projects(cx);
                    },
                )),
            );
        if let Some(folder) = state.folder {
            let owner = cx.weak_entity();
            row = row.child(control("home-folder-actions", "Folder ▾", p).dropdown_menu(
                move |menu, _, _| {
                    let rename = owner.clone();
                    let remove = owner.clone();
                    menu.item(
                        PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                            rename
                                .update(cx, |this, cx| {
                                    this.home_project_name_dialog(None, Some(folder), window, cx)
                                })
                                .ok();
                        }),
                    )
                    .item(
                        PopupMenuItem::new("Remove folder · keep projects").on_click(
                            move |_, _, cx| {
                                remove
                                    .update(cx, |this, cx| {
                                        this.home_state.projects.folder = None;
                                        this.home_project_edit(
                                            move |c| {
                                                c.remove_project_folder(folder);
                                                Ok(())
                                            },
                                            cx,
                                        );
                                    })
                                    .ok();
                            },
                        ),
                    )
                },
            ));
        }
        div()
            .flex()
            .flex_col()
            .child(row)
            .when(!state.message.is_empty(), |d| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_size(px(11.))
                        .child(state.message.clone()),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[gpui_kit::test]
    fn photo_classification_does_not_follow_paint_workspace_preferences(cx: &mut TestAppContext) {
        let (workspace, cx) = crate::tests::open(cx, emulsion_core::Document::new(32, 32));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = workspace.read(cx).editor.as_ref().unwrap().clone();
            editor.update(cx, |editor, _| {
                editor.draw_mode = true;
                editor.source = Some("/tmp/portrait.jpg".into());
                editor.home_canvas_kind = None;
            });
            assert_eq!(
                workspace.read(cx).home_editor_kind(editor.read(cx)),
                CanvasKind::Photo
            );
            editor.update(cx, |editor, _| {
                editor.home_canvas_kind = Some(CanvasKind::Paint)
            });
            assert_eq!(
                workspace.read(cx).home_editor_kind(editor.read(cx)),
                CanvasKind::Paint
            );
        });
    }
}
