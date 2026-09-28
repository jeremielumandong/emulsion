//! Shared native view/tool state for the UI and originating MCP relay.
use super::*;
use emulsion_mcp::editor_host_tools::{Action, StatePatch};
use serde_json::{Value, json};
impl EditorView {
    pub(crate) fn editor_host_state(&self, cx: &Context<Self>) -> Value {
        json!({"origin_tab_id":cx.entity_id().as_u64(),"page":self.editor.active_page(),"revision":self.editor.revision,"tool":format!("{:?}",self.tool).to_lowercase(),"rulers":self.rulers,"snapping":self.snap,"channel":format!("{:?}",self.channels.view).to_lowercase(),"quick_mask":self.tools.quick_mask,"guides":self.editor.doc.guides,"selected_nodes":self.selected_layer_ids(),"workspace":self.workspace_snapshot()})
    }
    pub(crate) fn editor_host_action(
        &mut self,
        action: Action,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if !matches!(action, Action::Inspect)
            && (self.editor.in_transaction()
                || self.drag.is_some()
                || self.raw.is_pending()
                || self.previewing())
        {
            return Err("Finish the active edit or preview first.".into());
        }
        match action {
            Action::Inspect => {}
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
