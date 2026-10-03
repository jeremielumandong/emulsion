//! Drag across the Layers panel's visibility or lock toggles to give every
//! row passed the state of the first one, as one Undo step. Press a toggle
//! and it flips at once; rows the pointer then enters take the same state.
//! Works in every workspace.
use super::*;

/// A per-row toggle in the Layers panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerToggle {
    Visible,
    Lock,
}

impl LayerToggle {
    fn get(self, node: &Node) -> bool {
        match self {
            Self::Visible => node.visible,
            Self::Lock => node.locked,
        }
    }

    fn command(self, id: NodeId, on: bool) -> Command {
        match self {
            Self::Visible => Command::SetVisible { id, visible: on },
            Self::Lock => Command::SetLocked { id, locked: on },
        }
    }

    fn step_name(self) -> &'static str {
        match self {
            Self::Visible => "Layer visibility",
            Self::Lock => "Layer locks",
        }
    }
}

/// A toggle drag in progress: which toggle, the state being set, and the
/// rows already set.
pub(crate) struct ToggleDrag {
    toggle: LayerToggle,
    on: bool,
    rows: Vec<NodeId>,
}

impl EditorView {
    /// Press on a row's toggle: flip it and start a drag that sets the same
    /// state on every row entered.
    pub(crate) fn layer_toggle_press(
        &mut self,
        id: NodeId,
        toggle: LayerToggle,
        cx: &mut Context<Self>,
    ) {
        // A drag whose release never arrived (outside the window) ends here.
        self.layer_toggle_release(cx);
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        if self.editor.in_transaction() {
            return;
        }
        let on = !toggle.get(node);
        self.editor.begin(toggle.step_name());
        self.layer_panel.toggle_drag = Some(ToggleDrag {
            toggle,
            on,
            rows: Vec::new(),
        });
        self.layer_toggle_enter(id, toggle, cx);
    }

    /// The pointer, still pressed, is over a row's toggle.
    pub(crate) fn layer_toggle_enter(
        &mut self,
        id: NodeId,
        toggle: LayerToggle,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = &mut self.layer_panel.toggle_drag else {
            return;
        };
        if drag.toggle != toggle || drag.rows.contains(&id) {
            return;
        }
        drag.rows.push(id);
        let on = drag.on;
        if self
            .editor
            .doc
            .node(id)
            .is_none_or(|node| toggle.get(node) == on)
        {
            return;
        }
        match self.editor.execute(toggle.command(id, on)) {
            Ok(_) => self.after_change(cx),
            Err(error) => self.set_status(error.to_string(), true, cx),
        }
    }

    /// The button came up: the rows set make one Undo step.
    pub(crate) fn layer_toggle_release(&mut self, cx: &mut Context<Self>) {
        if self.layer_panel.toggle_drag.take().is_some() {
            self.editor.end();
            self.after_change(cx);
        }
    }

    /// A row's toggle: press to flip it, drag across rows to set them too.
    pub(super) fn layer_toggle_target<E: InteractiveElement>(
        &self,
        element: E,
        id: NodeId,
        toggle: LayerToggle,
        cx: &mut Context<Self>,
    ) -> E {
        element
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.layer_toggle_press(id, toggle, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.layer_toggle_enter(id, toggle, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.layer_toggle_release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.layer_toggle_release(cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    fn layered(names: &[&str]) -> (Document, Vec<NodeId>) {
        let mut doc = Document::new(64, 48);
        let ids = names
            .iter()
            .map(|name| {
                Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        *name,
                        Arc::new(Raster::solid(64, 48, [0.5, 0.5, 0.5, 1.])),
                        Placement::default(),
                    )),
                    slot: emulsion_core::command::Slot::TOP,
                }
                .apply(&mut doc)
                .unwrap()
                .unwrap()
            })
            .collect();
        (doc, ids)
    }

    fn drag_across(cx: &mut VisualTestContext, selectors: &[String]) {
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        let points: Vec<_> = selectors
            .iter()
            .map(|s| {
                // Debug selectors are looked up by static name.
                let name: &'static str = Box::leak(s.clone().into_boxed_str());
                cx.debug_bounds(name).expect("toggle on screen").center()
            })
            .collect();
        cx.simulate_mouse_down(points[0], MouseButton::Left, Modifiers::none());
        for point in &points[1..] {
            cx.simulate_mouse_move(*point, Some(MouseButton::Left), Modifiers::none());
        }
        cx.simulate_mouse_up(
            *points.last().unwrap(),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn dragging_across_eyes_and_locks_sets_every_row_as_one_undo_step(cx: &mut TestAppContext) {
        let (doc, ids) = layered(&["A", "B", "C"]);
        let (ws, cx) = open(cx, doc.clone());
        cx.simulate_resize(gpui_kit::size(px(1400.), px(1000.)));
        let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        // B starts hidden; the drag starts on visible A, so every row hides.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.execute(
                    Command::SetVisible {
                        id: ids[1],
                        visible: false,
                    },
                    cx,
                )
            })
        });
        let history = cx.update(|_, cx| e.read(cx).editor.history.len());
        let eyes: Vec<_> = ids.iter().map(|id| format!("layer-eye-{id}")).collect();
        drag_across(cx, &eyes);
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert!(
                ids.iter()
                    .all(|id| !e.editor.doc.node(*id).unwrap().visible)
            );
            assert_eq!(e.editor.history.len(), history + 1);
            assert!(!e.editor.in_transaction());
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        cx.update(|_, cx| {
            let e = e.read(cx);
            let visible: Vec<_> = ids
                .iter()
                .map(|id| e.editor.doc.node(*id).unwrap().visible)
                .collect();
            assert_eq!(visible, [true, false, true]);
        });
        // Locks work the same way, from the bottom row up.
        let locks: Vec<_> = ids
            .iter()
            .rev()
            .map(|id| format!("layer-lock-{id}"))
            .collect();
        drag_across(cx, &locks);
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert!(ids.iter().all(|id| e.editor.doc.node(*id).unwrap().locked));
            assert_eq!(e.editor.history.len(), history + 1);
        });
        // A single press still toggles one row.
        drag_across(cx, &locks[..1]);
        cx.update(|_, cx| {
            let e = e.read(cx);
            let locked: Vec<_> = ids
                .iter()
                .map(|id| e.editor.doc.node(*id).unwrap().locked)
                .collect();
            assert_eq!(locked, [true, true, false]);
        });
    }
}
