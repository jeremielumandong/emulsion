//! The Pen and primitive tools share the same path operations.
use super::shapes::ShapeOperation;
use super::*;
use emulsion_raster::vector::Path;
use emulsion_raster::vector_geometry::{self as geometry, BooleanOp};
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

impl EditorView {
    /// Returns true when an operation was requested, including validation failures.
    pub(crate) fn apply_shape_operation(&mut self, path: Path, cx: &mut Context<Self>) -> bool {
        let operation = self.shape_ui.operation;
        if operation == ShapeOperation::NewLayer {
            return false;
        }
        let Some((id, existing, style)) = self.pen_target() else {
            self.set_status(t!("editor.shape_path_ops.select_shape"), true, cx);
            return true;
        };
        let result = if operation == ShapeOperation::Component {
            let mut result = (*existing).clone();
            result.subpaths.extend(path.subpaths);
            Ok(result)
        } else {
            let op = match operation {
                ShapeOperation::Add => BooleanOp::Add,
                ShapeOperation::Subtract => BooleanOp::Subtract,
                ShapeOperation::Intersect => BooleanOp::Intersect,
                _ => BooleanOp::Exclude,
            };
            geometry::boolean(&existing, &path, op)
        };
        match result {
            Ok(path) => {
                self.execute(
                    Command::SetPath {
                        id,
                        path: Arc::new(path),
                        style,
                    },
                    cx,
                );
                self.tools.pen.selected = None;
                self.shape_ui.component = None;
            }
            Err(error) => self.set_status(error, true, cx),
        }
        true
    }
    pub(crate) fn pen_operation_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.shape_ui.operation;
        let choices = [
            (
                ShapeOperation::NewLayer,
                t!("editor.shape_path_ops.new_layer"),
            ),
            (
                ShapeOperation::Component,
                t!("editor.shape_path_ops.add_component"),
            ),
            (ShapeOperation::Add, t!("editor.shape_path_ops.combine")),
            (
                ShapeOperation::Subtract,
                t!("editor.shape_path_ops.subtract"),
            ),
            (
                ShapeOperation::Intersect,
                t!("editor.shape_path_ops.intersect"),
            ),
            (ShapeOperation::Exclude, t!("editor.shape_path_ops.exclude")),
        ];
        let title = choices
            .iter()
            .find(|(op, _)| *op == current)
            .unwrap()
            .1
            .clone();
        let weak = cx.entity().downgrade();
        div()
            .id("pen-path-operation")
            .test_support()
            .child(
                Button::new("pen-path-operation-button")
                    .small()
                    .label(title)
                    .dropdown_menu(move |mut menu, _, _| {
                        for (op, title) in choices.clone() {
                            let weak = weak.clone();
                            menu = menu.item(
                                PopupMenuItem::new(title).checked(op == current).on_click(
                                    move |_, _, cx| {
                                        if let Some(editor) = weak.upgrade() {
                                            editor.update(cx, |this, cx| {
                                                this.shape_ui.operation = op;
                                                cx.notify();
                                            });
                                        }
                                    },
                                ),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
}
