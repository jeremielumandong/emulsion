//! Shared native view/tool state for the UI and originating MCP relay.
use super::*;
use emulsion_mcp::editor_host_tools::{Action, StatePatch};
use serde_json::{Value, json};
type WorkspaceUpdate = (Value, Option<Task<Result<(), String>>>);
impl EditorView {
    pub(crate) fn editor_host_state(&self, cx: &Context<Self>) -> Value {
        json!({"origin_tab_id":cx.entity_id().as_u64(),"page":self.editor.active_page(),"revision":self.editor.revision,"tool":format!("{:?}",self.tool).to_lowercase(),"rulers":self.rulers,"snapping":self.snap,"channel":format!("{:?}",self.channels.view).to_lowercase(),"quick_mask":self.tools.quick_mask,"guides":self.editor.doc.guides,"selected_nodes":self.selected_layer_ids(),"pending_edit_job":self.pending_edit_job.is_some(),"raw_pending":self.raw.is_pending(),"stroke_pending":self.tools.stroke_preview_pending,"workspace":self.workspace_snapshot()})
    }
    pub(crate) fn editor_host_action(
        &mut self,
        action: Action,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if !matches!(
            action,
            Action::Inspect | Action::Controls | Action::PlaybackInspect
        ) && (self.editor.in_transaction()
            || self.drag.is_some()
            || self.raw.is_pending()
            || self.previewing())
        {
            return Err("Finish the active edit or preview first.".into());
        }
        match action {
            Action::Inspect => {}
            Action::Controls => return Ok(self.host_controls(cx)),
            Action::Layout(patch) => self.patch_host_layout(patch, cx)?,
            Action::PlaybackInspect => {
                let setup = crate::playback_setup::current();
                return Ok(
                    json!({"message":setup.message,"install_available":setup.install.is_some(),"packages":setup.install.map(|p|p.packages),"website":setup.website,"installing":crate::playback_setup::installing()}),
                );
            }
            Action::Workspace { .. }
            | Action::Gesture(_)
            | Action::PlaybackSetup
            | Action::PlaybackInstall => {
                return Err("This control requires its native host dispatcher.".into());
            }
            Action::State(patch) => self.apply_host_state(patch, cx),
            Action::Guides(guides) => {
                self.editor
                    .execute(Command::SetGuides { guides })
                    .map_err(|e| e.to_string())?;
                self.after_change(cx);
            }
            Action::Clipboard(action) => self.clipboard_host(&action, cx)?,
            Action::Restore(id) => {
                self.editor
                    .execute(Command::ConvertToLayers { id })
                    .map_err(|e| e.to_string())?;
                self.after_change(cx);
            }
            Action::Print => return Err("Print dialogs require the workspace window.".into()),
        }
        Ok(self.editor_host_state(cx))
    }
    fn host_controls(&self, cx: &Context<Self>) -> Value {
        let canvas = self.canvas_bounds().map(|b| json!({"x":f32::from(b.origin.x),"y":f32::from(b.origin.y),"width":f32::from(b.size.width),"height":f32::from(b.size.height),"zoom":self.view.zoom,"document_center":self.view.center,"rotation_degrees":self.view.rotation}));
        json!({"state":self.editor_host_state(cx),"canvas":canvas,"gesture_coordinates":"document pixels","tools":rail::GROUPS.iter().flat_map(|g|g.iter()).map(|i|i.name).collect::<Vec<_>>(),"menus":super::menu_bar::MENUS.iter().collect::<Vec<_>>(),"presets":crate::app_state::settings(cx).workspace_presets,"default_layout":crate::app_state::settings(cx).workspace_default,"layout_schema":emulsion_mcp::editor_layout_tools::layout_schema()})
    }
    fn patch_host_layout(&mut self, patch: Value, cx: &mut Context<Self>) -> Result<(), String> {
        emulsion_mcp::editor_layout_tools::validate(
            &emulsion_mcp::editor_layout_tools::layout_schema(),
            &patch,
        )?;
        let mut snapshot =
            serde_json::to_value(self.workspace_snapshot()).map_err(|e| e.to_string())?;
        for (key, value) in patch.as_object().ok_or("Expected layout object")? {
            if key == "toolbar_placements" {
                let mut seen = std::collections::HashSet::new();
                for bar in value.as_array().unwrap() {
                    let id = bar["id"].as_str().unwrap();
                    if !seen.insert(id) {
                        return Err("Duplicate toolbar ID".into());
                    }
                    let current = snapshot[key]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|b| b["id"] == id)
                        .ok_or("Unknown toolbar")?;
                    for (k, v) in bar.as_object().unwrap() {
                        current[k] = v.clone();
                    }
                }
            } else {
                if key == "tool_ids"
                    && value.as_array().unwrap().iter().any(|id| {
                        !rail::GROUPS
                            .iter()
                            .flat_map(|g| g.iter())
                            .any(|item| Some(item.name) == id.as_str())
                    })
                {
                    return Err("Unknown tool ID; inspect get_editor_controls".into());
                }
                if key == "hidden_menu_ids"
                    && value.as_array().unwrap().iter().any(|id| {
                        !super::menu_bar::MENUS
                            .iter()
                            .any(|menu| Some(*menu) == id.as_str())
                    })
                {
                    return Err("Unknown menu ID; inspect get_editor_controls".into());
                }
                snapshot[key] = value.clone();
            }
        }
        let layout = serde_json::from_value(snapshot).map_err(|e| e.to_string())?;
        self.apply_workspace_layout(&layout, cx);
        Ok(())
    }
    pub(crate) fn host_canvas_gesture(
        &mut self,
        gesture: emulsion_mcp::editor_layout_tools::Gesture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let visible = self
            .library_workspace
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .is_some_and(|workspace| {
                let ws = workspace.read(cx);
                ws.screen == crate::workspace::Screen::Editor
                    && ws
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.entity_id() == cx.entity_id())
            });
        if !visible {
            return Err(
                "The originating editor must be the visible active tab for canvas gestures.".into(),
            );
        }
        if self.editor.in_transaction()
            || self.pending_edit_job.is_some()
            || self.drag.is_some()
            || self.raw.is_pending()
            || self.previewing()
            || self.motion.presenting
            || self.responsive_preview_active()
        {
            return Err("Finish the active edit or preview first.".into());
        }
        if gesture
            .expected_page
            .is_some_and(|page| page != self.editor.active_page())
        {
            return Err("The active page changed; inspect editor state and retry.".into());
        }
        if gesture
            .expected_revision
            .is_some_and(|v| v != self.editor.revision)
        {
            return Err("Stale document revision; inspect editor state and retry.".into());
        }
        if gesture.points.is_empty() || gesture.points.len() > 2048 {
            return Err("A gesture needs 1–2048 points.".into());
        }
        let bounds = self
            .canvas_bounds()
            .ok_or("Canvas has not been laid out yet")?;
        let mut points = Vec::with_capacity(gesture.points.len());
        for [x, y] in gesture.points {
            if !x.is_finite() || !y.is_finite() || x.abs() > 1e6 || y.abs() > 1e6 {
                return Err("Invalid gesture coordinate".into());
            }
            let (sx, sy) = self.view.doc_to_screen((x, y), &bounds);
            let pos = point(px(sx as f32), px(sy as f32));
            if !bounds.contains(&pos) {
                return Err(
                    "Gesture lies outside the visible canvas; adjust the viewport first.".into(),
                );
            }
            points.push(pos);
        }
        let old_bypass = self.snap_bypass;
        let old_shift = self.drag_shift;
        self.snap_bypass = gesture.control;
        self.drag_shift = gesture.shift;
        self.canvas_down(
            &MouseDownEvent {
                button: if gesture.middle_button {
                    MouseButton::Middle
                } else {
                    MouseButton::Left
                },
                position: points[0],
                modifiers: Modifiers {
                    shift: gesture.shift,
                    control: gesture.control,
                    alt: gesture.alt,
                    platform: gesture.platform,
                    ..Default::default()
                },
                click_count: gesture.click_count as usize,
                first_mouse: false,
            },
            window,
            cx,
        );
        for pos in &points[1..] {
            self.drag_move(*pos, window, cx);
        }
        let end = self.doc_point(*points.last().unwrap());
        if !gesture.middle_button {
            self.diagram_pointer_up(end, cx);
        }
        self.drag_end(cx);
        self.snap_bypass = old_bypass;
        self.drag_shift = old_shift;
        Ok(self.editor_host_state(cx))
    }
    pub(crate) fn host_workspace(
        &mut self,
        operation: &str,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<WorkspaceUpdate, String> {
        if self.drag.is_some()
            || self.editor.in_transaction()
            || self.previewing()
            || self.raw.is_pending()
            || self.pending_edit_job.is_some()
        {
            return Err("Finish the active edit or preview first.".into());
        }
        let mut settings = crate::app_state::settings(cx).clone();
        let named = || {
            name.as_deref()
                .filter(|s| !s.trim().is_empty() && s.chars().count() <= 80)
                .ok_or_else(|| "A preset name of 1–80 characters is required.".to_owned())
        };
        match operation {
            "apply" => {
                let n = named()?;
                let layout = settings
                    .workspace_presets
                    .iter()
                    .find(|p| p.name == n)
                    .ok_or("Unknown workspace preset")?
                    .layout
                    .clone();
                self.apply_workspace_layout(&layout, cx);
            }
            "load_default" => {
                let layout = settings
                    .workspace_default
                    .as_ref()
                    .ok_or("No default workspace is saved")?;
                self.apply_workspace_layout(layout, cx);
            }
            "reset" => self.reset_workspace(cx),
            "save" => {
                let n = named()?;
                let layout = self.workspace_snapshot();
                if let Some(p) = settings.workspace_presets.iter_mut().find(|p| p.name == n) {
                    p.layout = layout;
                } else {
                    if settings.workspace_presets.len() >= 32 {
                        return Err("Remove a workspace preset first (32 maximum).".into());
                    }
                    settings
                        .workspace_presets
                        .push(emulsion_io::settings::WorkspacePreset {
                            name: n.into(),
                            layout,
                        });
                }
            }
            "remove" => {
                let n = named()?;
                if !settings.workspace_presets.iter().any(|p| p.name == n) {
                    return Err("Unknown workspace preset".into());
                }
                settings.workspace_presets.retain(|p| p.name != n);
            }
            "save_default" => settings.workspace_default = Some(self.workspace_snapshot()),
            _ => return Err("Unknown workspace operation".into()),
        }
        let save = if matches!(operation, "save" | "remove" | "save_default") {
            cx.global_mut::<crate::app_state::AppSettings>().0 = settings.clone();
            cx.refresh_windows();
            Some(crate::settings_writer::save(settings, cx))
        } else {
            None
        };
        Ok((self.host_controls(cx), save))
    }
    fn apply_host_state(&mut self, patch: StatePatch, cx: &mut Context<Self>) {
        if let Some(v) = patch.rulers {
            self.rulers = v;
        }
        if let Some(v) = patch.snapping {
            self.snap = v;
        }
        if let Some(v) = patch.channel {
            use super::channels::ChannelView;
            self.select_channel(
                match v.as_str() {
                    "red" => ChannelView::Red,
                    "green" => ChannelView::Green,
                    "blue" => ChannelView::Blue,
                    _ => ChannelView::Rgb,
                },
                cx,
            );
        }
        if let Some(v) = patch.tool {
            self.set_tool(
                match v.as_str() {
                    "move" => Tool::Move,
                    "select" => Tool::Select,
                    "mask" => Tool::Mask,
                    "brush" => Tool::Brush,
                    "heal" => Tool::Heal,
                    "clone" => Tool::Clone,
                    "grade" => Tool::Grade,
                    "type" => Tool::Type,
                    "crop" => Tool::Crop,
                    "shape" => Tool::Shape,
                    "pen" => Tool::Pen,
                    "eyedropper" => Tool::Eyedropper,
                    "zoom" => Tool::Zoom,
                    _ => Tool::Hand,
                },
                cx,
            );
        }
        if let Some(v) = patch.quick_mask
            && v != self.tools.quick_mask
        {
            self.toggle_quick_mask(cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[gpui_kit::test]
    fn host_canvas_gesture_rejects_hidden_origin_without_mutation(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(200, 150));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|_, cx| ws.update(cx, |ws, _| ws.screen = crate::workspace::Screen::Home));
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                let before = v.editor.doc.clone();
                let gesture = serde_json::from_value(json!({"points":[[10,10],[80,60]]})).unwrap();
                assert!(
                    v.host_canvas_gesture(gesture, window, cx)
                        .unwrap_err()
                        .contains("visible active tab")
                );
                assert_eq!(v.editor.doc, before);
                assert!(v.drag.is_none());
            })
        });
    }
    #[gpui_kit::test]
    fn host_canvas_gesture_connects_diagram_ports_in_one_undo(cx: &mut TestAppContext) {
        use emulsion_core::{
            diagram::{Builder, ShapeKind},
            project::{ProjectEditor, ProjectKind},
        };
        let mut builder = Builder::new(800, 600).unwrap();
        let source = builder
            .add_shape(ShapeKind::Process, [80., 150., 140., 80.], "Source")
            .unwrap();
        let target = builder
            .add_shape(ShapeKind::Decision, [450., 150., 140., 80.], "Target")
            .unwrap();
        let doc = builder.finish().unwrap();
        let (ws, cx) = crate::tests::open(cx, doc.clone());
        let view = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(
                    ProjectEditor::new_project(ProjectKind::Diagram, doc).unwrap(),
                    "Connectors".into(),
                    window,
                    cx,
                )
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.canvas_bounds.set(Some(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(800.), px(600.)),
                )));
                v.view = View {
                    center: (400., 300.),
                    ..Default::default()
                };
                v.set_tool(Tool::Move, cx);
                v.set_layer_selection(vec![source], Some(source));
                let before = v.editor.doc.clone();
                let gesture = serde_json::from_value(
                    json!({"points":[[232,190],[450,190]],"expected_revision":v.editor.revision}),
                )
                .unwrap();
                v.host_canvas_gesture(gesture, window, cx).unwrap();
                let graph = v.editor.doc.diagram.as_ref().unwrap();
                assert_eq!(graph.edges.len(), 1);
                let edge = graph.edges.values().next().unwrap();
                assert_eq!(edge.source.shape, source);
                assert_eq!(edge.target.shape, target);
                assert!(v.editor.undo());
                assert_eq!(v.editor.doc, before);
            })
        });
    }
    #[gpui_kit::test]
    fn host_layout_patch_is_atomic_and_preserves_unmentioned_panels(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(100, 80));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|_,cx|view.update(cx,|v,cx|{
            let before=v.editor.doc.clone();
            let layout=v.workspace_snapshot();
            v.editor_host_action(Action::Layout(json!({"sidebar_width":410,"toolbar_placements":[{"id":"tools","edge":"floating","x":70,"scale":1.5}]})),cx).unwrap();
            let patched=v.workspace_snapshot();
            assert_eq!(patched.sidebar_width,410.);
            assert_eq!(patched.toolbar_placements.iter().find(|b|b.id=="tools").unwrap().scale,1.5);
            assert_eq!(patched.toolbar_placements.iter().find(|b|b.id=="dock"),layout.toolbar_placements.iter().find(|b|b.id=="dock"));
            assert_eq!(v.editor.doc,before);
            assert!(!v.editor.undo());
            assert!(v.editor_host_action(Action::Layout(json!({"sidebar_width":300,"tool_ids":["unknown"]})),cx).is_err());
            assert_eq!(serde_json::to_value(v.workspace_snapshot()).unwrap(),serde_json::to_value(patched).unwrap());
        }));
    }
    #[gpui_kit::test]
    fn host_canvas_gesture_uses_native_shape_and_undo_with_atomic_validation(
        cx: &mut TestAppContext,
    ) {
        let (ws, cx) = crate::tests::open(cx, Document::new(200, 150));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.canvas_bounds.set(Some(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(600.), px(400.)),
                )));
                v.view = View::default();
                v.set_tool(Tool::Shape, cx);
                let before = v.editor.doc.clone();
                let gesture = |points: Value, revision: u64| {
                    serde_json::from_value(json!({"points":points,"expected_revision":revision}))
                        .unwrap()
                };
                assert!(
                    v.host_canvas_gesture(
                        gesture(json!([[10, 10], [100000, 100000]]), v.editor.revision),
                        window,
                        cx
                    )
                    .is_err()
                );
                assert_eq!(v.editor.doc, before);
                let wrong_page = serde_json::from_value(
                    json!({"points":[[10,10],[80,60]],"expected_page":v.editor.active_page()+1}),
                )
                .unwrap();
                assert!(
                    v.host_canvas_gesture(wrong_page, window, cx)
                        .unwrap_err()
                        .contains("active page changed")
                );
                assert_eq!(v.editor.doc, before);
                assert!(
                    v.host_canvas_gesture(
                        gesture(json!([[10, 10], [80, 60]]), v.editor.revision + 1),
                        window,
                        cx
                    )
                    .is_err()
                );
                v.host_canvas_gesture(
                    gesture(json!([[10, 10], [80, 60]]), v.editor.revision),
                    window,
                    cx,
                )
                .unwrap();
                assert_eq!(v.editor.doc.nodes.len(), before.nodes.len() + 1);
                assert!(v.drag.is_none());
                assert!(!v.editor.in_transaction());
                assert!(v.editor.undo());
                assert_eq!(v.editor.doc, before);
            })
        });
    }
    #[gpui_kit::test]
    fn editor_host_state_guides_channel_and_quick_mask_preserve_pixels(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(100, 80));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                let before = v.editor.doc.clone();
                v.editor_host_action(
                    Action::State(StatePatch {
                        rulers: Some(false),
                        snapping: Some(false),
                        channel: Some("red".into()),
                        ..Default::default()
                    }),
                    cx,
                )
                .unwrap();
                assert_eq!(v.editor.doc, before);
                assert!(!v.rulers && !v.snap);
                assert_eq!(v.channels.view, super::super::channels::ChannelView::Red);
                v.editor_host_action(
                    Action::Guides(vec![emulsion_core::document::Guide {
                        vertical: true,
                        pos: 14.5,
                    }]),
                    cx,
                )
                .unwrap();
                assert_eq!(v.editor.doc.guides.len(), 1);
                v.editor.undo();
                assert_eq!(v.editor.doc, before);
                v.editor_host_action(
                    Action::State(StatePatch {
                        quick_mask: Some(true),
                        ..Default::default()
                    }),
                    cx,
                )
                .unwrap();
                assert!(v.tools.quick_mask);
                assert_eq!(v.tool, Tool::Brush);
                assert_eq!(v.editor.doc, before);
                v.editor.begin("User gesture");
                assert!(
                    v.editor_host_action(
                        Action::State(StatePatch {
                            tool: Some("move".into()),
                            ..Default::default()
                        }),
                        cx
                    )
                    .is_err()
                );
                v.editor.cancel();
            })
        });
    }
    #[gpui_kit::test]
    fn editor_host_clipboard_keeps_native_objects_during_assistant_turn(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(200, 150));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                let id = v
                    .editor
                    .execute(Command::AddNode {
                        node: Box::new(
                            emulsion_core::design::Element::Rectangle
                                .node((200, 150), [30, 80, 120, 255]),
                        ),
                        slot: emulsion_core::command::Slot::TOP,
                    })
                    .unwrap()
                    .unwrap();
                v.set_layer_selection(vec![id], Some(id));
                v.assistant.running = true;
                let before = v.editor.doc.clone();
                v.editor_host_action(Action::Clipboard("copy".into()), cx)
                    .unwrap();
                assert_eq!(v.editor.doc, before);
                v.editor_host_action(Action::Clipboard("paste".into()), cx)
                    .unwrap();
                assert_eq!(v.editor.doc.nodes.len(), before.nodes.len() + 1);
                assert!(matches!(
                    v.editor.doc.node(v.selected.unwrap()).unwrap().kind,
                    NodeKind::Path { .. }
                ));
                v.editor.undo();
                assert_eq!(v.editor.doc, before);
                v.assistant.running = false;
            })
        });
    }
}
