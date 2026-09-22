//! Editable vector shapes and their creation settings.
use super::*;
use emulsion_raster::vector::{Path, PathStyle};
use emulsion_raster::vector_geometry as geometry;

#[path = "shape_properties.rs"]
mod properties;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ShapeMode {
    #[default]
    Shape,
    Path,
    Pixels,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ShapeOperation {
    #[default]
    NewLayer,
    Component,
    Add,
    Subtract,
    Intersect,
    Exclude,
}

pub(crate) struct ShapeUi {
    pub mode: ShapeMode,
    pub operation: ShapeOperation,
    pub style: Option<PathStyle>,
    pub fixed_size: bool,
    pub width: f64,
    pub height: f64,
    pub linked: bool,
    pub align_edges: bool,
    pub component: Option<usize>,
    color_edit: Option<(NodeId, &'static str)>,
    fields: Option<properties::ShapeFields>,
    error: Option<String>,
}
impl Default for ShapeUi {
    fn default() -> Self {
        Self {
            mode: ShapeMode::Shape,
            operation: ShapeOperation::NewLayer,
            style: None,
            fixed_size: false,
            width: 100.,
            height: 100.,
            linked: false,
            align_edges: false,
            component: None,
            color_edit: None,
            fields: None,
            error: None,
        }
    }
}

impl EditorView {
    pub(crate) fn shape_drag_rect(
        &self,
        start: (f64, f64),
        end: (f64, f64),
        shift: bool,
    ) -> (f64, f64, f64, f64) {
        let (mut x, mut y, mut w, mut h) = super::tools::shape_rect(start, end, shift);
        if self.shape_ui.fixed_size {
            w = self.shape_ui.width;
            h = self.shape_ui.height;
            x = if end.0 < start.0 {
                start.0 - w
            } else {
                start.0
            };
            y = if end.1 < start.1 {
                start.1 - h
            } else {
                start.1
            };
        } else if self.shape_ui.linked && !shift {
            let ratio = self.shape_ui.width / self.shape_ui.height;
            if w / h.max(1e-9) > ratio {
                h = w / ratio;
            } else {
                w = h * ratio;
            }
            x = if end.0 < start.0 {
                start.0 - w
            } else {
                start.0
            };
            y = if end.1 < start.1 {
                start.1 - h
            } else {
                start.1
            };
        }
        if self.shape_ui.align_edges {
            x = x.round();
            y = y.round();
            w = w.round();
            h = h.round();
        }
        (x, y, w, h)
    }

    pub(crate) fn current_shape_style(&self) -> PathStyle {
        self.selected
            .and_then(|id| self.editor.doc.node(id))
            .and_then(|n| match &n.kind {
                NodeKind::Path { style, .. } => Some(*style),
                _ => None,
            })
            .unwrap_or_else(|| {
                self.shape_ui.style.unwrap_or(PathStyle {
                    fill: Some(self.tools.fg),
                    stroke: None,
                    ..Default::default()
                })
            })
    }

    pub(crate) fn finish_shape(
        &mut self,
        (x, y, w, h): (f64, f64, f64, f64),
        ellipse: bool,
        cx: &mut Context<Self>,
    ) {
        if w < 1. || h < 1. || ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return;
        }
        if self.editor.in_transaction() || self.assistant.running {
            return;
        }
        let path = if ellipse {
            geometry::ellipse(x, y, w, h)
        } else {
            geometry::rectangle(x, y, w, h)
        };
        if self.shape_ui.mode != ShapeMode::Pixels
            && self.shape_ui.operation != ShapeOperation::NewLayer
        {
            self.apply_shape_operation(path, cx);
            return;
        }
        let mut style = self.shape_ui.style.unwrap_or(PathStyle {
            fill: Some(self.tools.fg),
            stroke: None,
            ..Default::default()
        });
        if self.shape_ui.mode == ShapeMode::Path {
            style.fill = None;
            style.stroke = None;
        }
        let name = if ellipse { "Ellipse" } else { "Rectangle" };
        let node = if self.shape_ui.mode == ShapeMode::Pixels {
            Node::raster(
                0,
                name,
                Arc::new(path.rasterize(&style, self.editor.doc.width, self.editor.doc.height)),
                Placement::default(),
            )
        } else {
            Node::path(
                0,
                name,
                Arc::new(path),
                style,
                self.editor.doc.width,
                self.editor.doc.height,
            )
        };
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot: self.insertion_slot(),
            },
            cx,
        ) {
            self.set_layer_selection(vec![id], Some(id));
            self.shape_ui.width = w;
            self.shape_ui.height = h;
            self.tools.pen.selected = None;
            self.shape_ui.component = None;
            if self.shape_ui.mode == ShapeMode::Path {
                self.set_tool(Tool::Pen, cx);
            }
        }
    }

    fn change_shape_style(&mut self, style: PathStyle, cx: &mut Context<Self>) {
        self.finish_shape_color_edit(cx);
        self.apply_shape_style(style, false, cx);
    }

    fn apply_shape_style(&mut self, style: PathStyle, color_edit: bool, cx: &mut Context<Self>) {
        let owns_transaction = color_edit
            && self
                .shape_ui
                .color_edit
                .is_some_and(|(id, _)| self.selected == Some(id));
        if (self.editor.in_transaction() && !owns_transaction)
            || self.drag.is_some()
            || self.assistant.running
        {
            return;
        }
        let style = style.sanitized();
        if let Some(id) = self.selected
            && let Some(node) = self.editor.doc.node(id)
            && let NodeKind::Path {
                path, style: old, ..
            } = &node.kind
            && *old != style
        {
            self.execute(
                Command::SetPath {
                    id,
                    path: path.clone(),
                    style,
                },
                cx,
            );
        }
        self.shape_ui.style = Some(style);
        self.shape_ui.error = None;
        cx.notify();
    }

    /// Selection changes lack a GPUI context; finalize history synchronously
    /// before changing the inspector target. Its subscriptions are then replaced.
    pub(crate) fn commit_shape_color_edit(&mut self) {
        if self.shape_ui.color_edit.take().is_some() {
            self.editor.end();
        }
    }

    /// Resolve the popup before navigation, undo, or another editing gesture.
    pub(crate) fn finish_shape_color_edit(&mut self, cx: &mut Context<Self>) {
        self.commit_shape_color_edit();
        self.close_shape_color_pickers(cx);
    }

    fn resize_shape(&mut self, width: bool, value: f64, cx: &mut Context<Self>) {
        if self.editor.in_transaction() || self.drag.is_some() {
            return;
        }
        if let Some((id, path, style)) = self.pen_target() {
            let Some((x, y, w, h)) = geometry::bounds(&path) else {
                return;
            };
            if w <= 0. || h <= 0. {
                return;
            }
            let factor = value / if width { w } else { h };
            if (factor - 1.).abs() < 1e-9 {
                return;
            }
            let (sx, sy) = if self.shape_ui.linked {
                (factor, factor)
            } else if width {
                (factor, 1.)
            } else {
                (1., factor)
            };
            let mut path = (*path).clone();
            path.transform(
                glam::DAffine2::from_translation(glam::dvec2(x, y))
                    * glam::DAffine2::from_scale(glam::dvec2(sx, sy))
                    * glam::DAffine2::from_translation(glam::dvec2(-x, -y)),
            );
            self.execute(
                Command::SetPath {
                    id,
                    path: Arc::new(path),
                    style,
                },
                cx,
            );
            self.shape_ui.width = w * sx;
            self.shape_ui.height = h * sy;
        } else {
            if self.shape_ui.linked {
                if width {
                    self.shape_ui.height *= value / self.shape_ui.width;
                } else {
                    self.shape_ui.width *= value / self.shape_ui.height;
                }
            }
            if width {
                self.shape_ui.width = value;
            } else {
                self.shape_ui.height = value;
            }
        }
        cx.notify();
    }

    fn align_shape_components(
        &mut self,
        axis: usize,
        position: usize,
        distribute: bool,
        cx: &mut Context<Self>,
    ) {
        if self.editor.in_transaction() || self.drag.is_some() {
            return;
        }
        let Some((id, path, style)) = self.pen_target() else {
            return;
        };
        let Some(total) = geometry::bounds(&path) else {
            return;
        };
        let mut result = (*path).clone();
        let items: Vec<_> = path
            .subpaths
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                geometry::bounds(&Path {
                    subpaths: vec![s.clone()],
                })
                .map(|b| (i, b))
            })
            .collect();
        if distribute {
            if items.len() < 3 {
                self.set_status(
                    "At least three path components are needed to distribute.",
                    true,
                    cx,
                );
                return;
            }
            let mut sorted = items.clone();
            let center = |b: (f64, f64, f64, f64)| {
                if axis == 0 {
                    b.0 + b.2 / 2.
                } else {
                    b.1 + b.3 / 2.
                }
            };
            sorted.sort_by(|a, b| center(a.1).total_cmp(&center(b.1)));
            let first = center(sorted[0].1);
            let step = (center(sorted[sorted.len() - 1].1) - first) / (sorted.len() - 1) as f64;
            for (rank, (index, b)) in sorted.iter().enumerate() {
                translate_component(
                    &mut result,
                    *index,
                    axis,
                    first + rank as f64 * step - center(*b),
                );
            }
        } else {
            let target = if axis == 0 {
                total.0 + total.2 * position as f64 / 2.
            } else {
                total.1 + total.3 * position as f64 / 2.
            };
            for (index, b) in items {
                if self
                    .shape_ui
                    .component
                    .is_some_and(|selected| selected != index)
                {
                    continue;
                }
                let at = if axis == 0 {
                    b.0 + b.2 * position as f64 / 2.
                } else {
                    b.1 + b.3 * position as f64 / 2.
                };
                translate_component(&mut result, index, axis, target - at);
            }
        }
        self.execute(
            Command::SetPath {
                id,
                path: Arc::new(result),
                style,
            },
            cx,
        );
    }
}
fn translate_component(path: &mut Path, index: usize, axis: usize, delta: f64) {
    for a in &mut path.subpaths[index].anchors {
        for p in [&mut a.p, &mut a.h_in, &mut a.h_out] {
            if axis == 0 {
                p.0 += delta;
            } else {
                p.1 += delta;
            }
        }
    }
}
