//! Source editing and linked-source controls shared by Photo and Design.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_mcp::smart_source_tools::Action;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use serde_json::{Value, json};
#[derive(Clone)]
pub(crate) struct SourceSession {
    pub parent: WeakEntity<EditorView>,
    pub node: NodeId,
    pub page: u64,
    pub expected: NodeKind,
    /// Last successfully opened or applied child content, independent of parent metadata.
    pub baseline: Document,
    pub depth: usize,
}
pub(crate) fn same_source(current: Option<&NodeKind>, expected: &NodeKind) -> bool {
    match (current, expected) {
        (
            Some(NodeKind::Smart {
                source: a,
                editable: ae,
                original_image: ao,
                ..
            }),
            NodeKind::Smart {
                source: b,
                editable: be,
                original_image: bo,
                ..
            },
        ) => {
            Arc::ptr_eq(a, b)
                && ae == be
                && match (ao, bo) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => false,
    }
}
#[derive(Clone, PartialEq)]
pub(crate) struct WatchFact {
    link: emulsion_core::smart_source::ExternalLink,
    file: Option<(u64, std::time::SystemTime)>,
    error: Option<String>,
}
impl EditorView {
    pub(crate) fn smart_source_ready(&self) -> Result<(), String> {
        if self.editor.in_transaction()
            || self.smart.has_pending()
            || self.raw.is_pending()
            || self.drag.is_some()
            || self.motion.presenting
            || self.responsive_preview_active()
        {
            return Err(
                "Finish active edits and exit preview before changing Smart sources.".into(),
            );
        }
        Ok(())
    }
    pub(crate) fn inspect_smart_source(&self, node: NodeId) -> Result<Value, String> {
        let n = self.editor.doc.node(node).ok_or("Missing Smart Object.")?;
        let NodeKind::Smart {
            source, editable, ..
        } = &n.kind
        else {
            return Err("Select a Smart Object.".into());
        };
        let session=self.smart.source_session.as_ref().map(|s|json!({"parent_tab_id":s.parent.entity_id().as_u64(),"parent_page":s.page,"parent_node":s.node,"depth":s.depth}));
        Ok(
            json!({"node":node,"revision":self.editor.revision,"dimensions":[source.width(),source.height()],"layered_source":matches!(editable,Some(emulsion_core::node::SmartEditable::Document{..})),"external":emulsion_core::smart_source::link(&self.editor.doc,node),"session":session,"watch_error":self.smart.source_watch_error}),
        )
    }
    pub(crate) fn dispatch_smart_source(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(ws) = self
            .library_workspace
            .as_ref()
            .and_then(WeakEntity::upgrade)
        else {
            self.set_status(t!("editor.smart_source_ui.no_workspace"), true, cx);
            return;
        };
        let handle = ws.read(cx).library_window;
        let owner = cx.entity();
        let weak = cx.weak_entity();
        cx.defer(move |cx| {
            let task = cx.update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| ws.smart_source_task(owner, action, window, cx))
            });
            if let Ok(task) = task {
                cx.spawn(async move |cx| {
                    if let Err(error) = task.await {
                        weak.update(cx, |e, cx| e.set_status(error, true, cx)).ok();
                    }
                })
                .detach();
            }
        });
    }
    pub(crate) fn smart_link_dialog(&mut self, node: NodeId, cx: &mut Context<Self>) {
        let pick = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.smart_source_ui.link_prompt").into()),
        });
        let ticket = self.edit_ticket();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = pick.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |e, cx| {
                    if e.edit_ticket() != ticket {
                        e.set_status(t!("editor.smart_source_ui.changed_file"), true, cx);
                        return;
                    }
                    e.dispatch_smart_source(
                        Action::Link {
                            node,
                            path,
                            auto_refresh: false,
                        },
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }
    fn smart_save_as_dialog(&mut self, node: NodeId, cx: &mut Context<Self>) {
        let folder = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let pick = cx.prompt_save_path(&folder, Some("Smart source.ora"));
        let ticket = self.edit_ticket();
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = pick.await {
                this.update(cx, |e, cx| {
                    if e.edit_ticket() != ticket {
                        e.set_status(t!("editor.smart_source_ui.changed_path"), true, cx);
                        return;
                    }
                    e.dispatch_smart_source(Action::SaveAs { node, path }, cx);
                })
                .ok();
            }
        })
        .detach();
    }
    pub(crate) fn smart_source_controls(
        &mut self,
        node: NodeId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.weak_entity();
        let link = emulsion_core::smart_source::link(&self.editor.doc, node).cloned();
        div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .child(
                Button::new("smart-edit-source")
                    .label(t!("editor.smart_source_ui.edit_source"))
                    .small()
                    .on_click(cx.listener(move |e, _, _, cx| {
                        e.dispatch_smart_source(Action::Open { node }, cx)
                    })),
            )
            .child(
                Button::new("smart-source-options")
                    .label(t!("editor.smart_source_ui.source_menu"))
                    .small()
                    .ghost()
                    .dropdown_menu(move |mut menu, _, _| {
                        let o = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(t!("editor.smart_source_ui.link_file")).on_click(
                                move |_, _, cx| {
                                    o.update(cx, |e, cx| e.smart_link_dialog(node, cx)).ok();
                                },
                            ),
                        );
                        let o = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(t!("editor.smart_source_ui.save_layered")).on_click(
                                move |_, _, cx| {
                                    o.update(cx, |e, cx| e.smart_save_as_dialog(node, cx)).ok();
                                },
                            ),
                        );
                        if let Some(link) = &link {
                            for (label, action) in [
                                (
                                    t!("editor.smart_source_ui.refresh"),
                                    Action::Refresh {
                                        node,
                                        discard_local: false,
                                    },
                                ),
                                (
                                    t!("editor.smart_source_ui.auto_refresh"),
                                    Action::Auto {
                                        node,
                                        enabled: !link.auto_refresh,
                                    },
                                ),
                                (t!("editor.smart_source_ui.unlink"), Action::Unlink { node }),
                                (
                                    t!("editor.smart_source_ui.write_ora"),
                                    Action::Write { node },
                                ),
                            ] {
                                let o = owner.clone();
                                let mut item = PopupMenuItem::new(label);
                                if matches!(action, Action::Auto { .. }) {
                                    item = item.checked(link.auto_refresh);
                                }
                                menu = menu.item(item.on_click(move |_, _, cx| {
                                    o.update(cx, |e, cx| {
                                        e.dispatch_smart_source(action.clone(), cx)
                                    })
                                    .ok();
                                }));
                            }
                            if link.locally_modified {
                                let o = owner.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(t!(
                                        "editor.smart_source_ui.discard_refresh"
                                    ))
                                    .on_click(
                                        move |_, window, cx| {
                                            let answer = window.prompt(
                                                PromptLevel::Warning,
                                                &t!("editor.smart_source_ui.replace_title"),
                                                Some(&t!("editor.smart_source_ui.replace_body")),
                                                &[
                                                    &*t!("editor.smart_source_ui.refresh_button"),
                                                    &*t!("shell.cancel"),
                                                ],
                                                cx,
                                            );
                                            let o = o.clone();
                                            cx.spawn(async move |cx| {
                                                if answer.await == Ok(0) {
                                                    o.update(cx, |e, cx| {
                                                        e.dispatch_smart_source(
                                                            Action::Refresh {
                                                                node,
                                                                discard_local: true,
                                                            },
                                                            cx,
                                                        )
                                                    })
                                                    .ok();
                                                }
                                            })
                                            .detach();
                                        },
                                    ),
                                );
                            }
                        }
                        menu
                    }),
            )
            .text_color(p.ink)
            .into_any_element()
    }
    pub(crate) fn smart_source_banner(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let session = self.smart.source_session.clone()?;
        let p = theme::palette(cx);
        let parent = session.parent.clone();
        Some(
            div()
                .id("smart-source-session")
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(6.))
                .bg(p.soft_bg)
                .text_color(p.ink)
                .child(t!("editor.smart_source_ui.banner", depth = session.depth))
                .child(
                    Button::new("smart-source-apply")
                        .label(t!("editor.smart_source_ui.apply_parent"))
                        .small()
                        .on_click(
                            cx.listener(|e, _, _, cx| e.dispatch_smart_source(Action::Apply, cx)),
                        ),
                )
                .child(
                    Button::new("smart-source-return")
                        .label(t!("editor.smart_source_ui.return_parent"))
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |e, _, _, cx| {
                            if let Some(ws) =
                                e.library_workspace.as_ref().and_then(WeakEntity::upgrade)
                            {
                                let handle = ws.read(cx).library_window;
                                let parent = parent.clone();
                                cx.defer(move |cx| {
                                    cx.update_window(handle, |_, window, cx| {
                                        ws.update(cx, |ws, cx| {
                                            if let Some(index) = ws
                                                .tabs
                                                .iter()
                                                .position(|t| t.entity_id() == parent.entity_id())
                                            {
                                                ws.activate_tab(index, window, cx);
                                            }
                                        })
                                    })
                                    .ok();
                                });
                            }
                        })),
                )
                .into_any_element(),
        )
    }
    pub(crate) fn start_smart_source_watch(&mut self, cx: &mut Context<Self>) {
        if self.smart.source_watch_started {
            return;
        }
        self.smart.source_watch_started = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(3))
                    .await;
                let snapshot = match this.update(cx, |e, cx| {
                    if e.smart_source_ready().is_err() {
                        return None;
                    }
                    let ids = e
                        .editor
                        .doc
                        .nodes
                        .iter()
                        .filter_map(|n| {
                            emulsion_core::smart_source::link(&e.editor.doc, n.id)
                                .filter(|l| l.auto_refresh)
                                .map(|_| n.id)
                        })
                        .collect::<Vec<_>>();
                    e.smart.source_watch_facts.retain(|id, _| ids.contains(id));
                    if ids.is_empty() {
                        if e.smart.source_watch_error.take().is_some() {
                            e.set_status(t!("editor.smart_source_ui.monitor_stopped"), false, cx);
                        }
                        return None;
                    }
                    // At most eight files per turn; every link eventually runs.
                    let start = e.smart.source_watch_cursor % ids.len();
                    let batch = ids
                        .iter()
                        .cycle()
                        .skip(start)
                        .take(ids.len().min(8))
                        .copied()
                        .collect::<Vec<_>>();
                    e.smart.source_watch_cursor = (start + batch.len()) % ids.len();
                    Some((
                        e.editor.doc.clone(),
                        e.edit_ticket(),
                        e.editor.active_page(),
                        batch,
                        e.smart.source_watch_facts.clone(),
                    ))
                }) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let Some((doc, ticket, page, ids, mut facts)) = snapshot else {
                    continue;
                };
                let refreshed = cx
                    .background_spawn(async move {
                        let mut trial =
                            emulsion_core::Editor::try_new(doc, None).map_err(|e| e.to_string())?;
                        for id in ids {
                            let Some(link) =
                                emulsion_core::smart_source::link(&trial.doc, id).cloned()
                            else {
                                continue;
                            };
                            let file = std::fs::metadata(&link.path)
                                .ok()
                                .and_then(|m| Some((m.len(), m.modified().ok()?)));
                            if facts.get(&id).is_some_and(|old| {
                                old.error.is_none()
                                    && old.link.path == link.path
                                    && old.link.auto_refresh == link.auto_refresh
                                    && old.file == file
                            }) {
                                continue;
                            }
                            let error = emulsion_io::smart_source::refresh(&mut trial, id, false)
                                .err()
                                .map(|e| e.to_string());
                            let link = emulsion_core::smart_source::link(&trial.doc, id)
                                .cloned()
                                .unwrap_or(link);
                            facts.insert(id, WatchFact { link, file, error });
                        }
                        Ok::<_, String>((trial.doc, facts))
                    })
                    .await;
                if this
                    .update(cx, |e, cx| {
                        if e.edit_ticket() != ticket
                            || e.editor.active_page() != page
                            || e.smart_source_ready().is_err()
                        {
                            return;
                        }
                        let (updated, facts) = match refreshed {
                            Ok(result) => result,
                            Err(error) => {
                                e.smart.source_watch_error = Some(error.clone());
                                e.set_status(error, true, cx);
                                return;
                            }
                        };
                        let errors = facts
                            .iter()
                            .filter_map(|(id, fact)| {
                                fact.error
                                    .as_ref()
                                    .map(|error| format!("Source {id}: {error}"))
                            })
                            .collect::<Vec<_>>();
                        let error = (!errors.is_empty()).then(|| errors.join("\n"));
                        if e.smart.source_watch_error != error {
                            e.smart.source_watch_error = error.clone();
                            if let Some(error) = error {
                                e.set_status(error, true, cx);
                            } else {
                                e.set_status(t!("editor.smart_source_ui.up_to_date"), false, cx);
                            }
                        }
                        e.smart.source_watch_facts = facts;
                        if updated != e.editor.doc {
                            match e
                                .editor
                                .commit_design_document(updated, "Refresh linked Smart sources")
                            {
                                Ok(()) => e.after_change(cx),
                                Err(error) => e.set_status(error, true, cx),
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod original_source_tests {
    use super::same_source;
    use emulsion_core::{Node, NodeKind, node::OriginalImage};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    #[test]
    fn source_session_tracks_original_identity_without_tracking_placement_or_cache() {
        let mut node = Node::smart(
            1,
            "Smart",
            Arc::new(Raster::solid(2, 2, [1.; 4])),
            vec![],
            Placement::default(),
        );
        let original = Arc::new(OriginalImage::new(Arc::new(vec![1, 2]), [1; 32], [2; 32]));
        let NodeKind::Smart { original_image, .. } = &mut node.kind else {
            unreachable!()
        };
        *original_image = Some(original.clone());
        let expected = node.kind.clone();
        let NodeKind::Smart {
            placement, cache, ..
        } = &mut node.kind
        else {
            unreachable!()
        };
        *placement = emulsion_core::SmartPlacement::Legacy(Placement::at(3., 4.));
        *cache = Arc::new(Raster::solid(2, 2, [0.; 4]));
        assert!(same_source(Some(&node.kind), &expected));
        let NodeKind::Smart { original_image, .. } = &mut node.kind else {
            unreachable!()
        };
        *original_image = Some(Arc::new((*original).clone()));
        assert!(!same_source(Some(&node.kind), &expected));
        let NodeKind::Smart { original_image, .. } = &mut node.kind else {
            unreachable!()
        };
        *original_image = None;
        assert!(!same_source(Some(&node.kind), &expected));
    }
}
