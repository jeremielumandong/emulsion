//! One independently editable raster mask for the complete Smart filter stack.
use super::*;
use emulsion_core::{MaskProperties, SmartFilterMask};
use emulsion_raster::{IRect, Mask, select};
use glam::{DAffine2, dvec2};
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable, Sizable};

pub(super) fn descriptor(node: &Node) -> Option<&SmartFilterMask> {
    match &node.kind {
        NodeKind::Smart { filter_mask, .. } => filter_mask.as_ref(),
        _ => None,
    }
}

pub(super) fn raw_bounds(node: &Node) -> Option<IRect> {
    let mask = descriptor(node)?;
    let transform = emulsion_core::smart_filter_mask::to_document(node)?;
    let (w, h) = (mask.pixels.width() as f64, mask.pixels.height() as f64);
    let mut min = glam::DVec2::splat(f64::INFINITY);
    let mut max = glam::DVec2::splat(f64::NEG_INFINITY);
    for p in [dvec2(0., 0.), dvec2(w, 0.), dvec2(w, h), dvec2(0., h)] {
        let p = transform.transform_point2(p);
        min = min.min(p);
        max = max.max(p);
    }
    min = min.floor();
    max = max.ceil();
    let size = max - min;
    if !min.is_finite()
        || !max.is_finite()
        || min.min_element() < i32::MIN as f64
        || max.max_element() > i32::MAX as f64
        || size.max_element() > i32::MAX as f64
    {
        return None;
    }
    Some(IRect::new(
        min.x as i32,
        min.y as i32,
        size.x as i32,
        size.y as i32,
    ))
}

/// Capture selection and source placement at scheduling time; build in the
/// first result's cache footprint, keeping its origin in the stored affine.
/// A white/black constant mask has the same coverage beyond its raw plane.
pub(super) fn initial_mask(
    width: u32,
    height: u32,
    offset: (i32, i32),
    source_to_document: DAffine2,
    selection: Option<&Mask>,
    hide_all: bool,
) -> Result<SmartFilterMask, &'static str> {
    // Constant masks are sparse and use the full native size contract. Only
    // captured selection materialization is subject to the interactive cap.
    if width == 0
        || height == 0
        || width > 30_000
        || height > 30_000
        || u64::from(width) * u64::from(height) > emulsion_core::document::MAX_PIXELS
    {
        return Err("Filter mask exceeds the editable mask size limit");
    }
    let transform = if selection.is_some() {
        DAffine2::from_translation(dvec2(offset.0 as f64, offset.1 as f64))
    } else {
        DAffine2::IDENTITY
    };
    let to_doc = source_to_document * transform;
    let pixels = if let Some(selection) = selection {
        emulsion_core::smart_filter_mask::sampled_edit_plane(width, height, 0, |x, y| {
            let p = to_doc.transform_point2(dvec2(x as f64 + 0.5, y as f64 + 0.5));
            if p.x < 0.
                || p.y < 0.
                || p.x >= selection.width() as f64
                || p.y >= selection.height() as f64
            {
                0
            } else {
                selection.get(p.x as u32, p.y as u32)
            }
        })?
    } else {
        Mask::empty(width, height, if hide_all { 0 } else { 255 })
    };
    Ok(SmartFilterMask {
        pixels: Arc::new(pixels),
        enabled: true,
        linked: true,
        transform: transform.to_cols_array(),
        properties: MaskProperties::default(),
    })
}

impl EditorView {
    pub(super) fn select_smart_filter_mask(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if !self.photo_transform_ready(cx)
            || !self.layer_menu_ready()
            || self.editor.doc.node(id).and_then(descriptor).is_none()
        {
            return;
        }
        self.select_layer_row(id, false, false, cx);
        self.set_mask_edit_target(MaskEditTarget::SmartFilterMask, cx);
        self.set_paint(PaintKind::Brush, cx);
    }

    pub(super) fn add_smart_filter_mask(
        &mut self,
        id: NodeId,
        hide_all: bool,
        from_selection: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.layer_menu_ready()
            || !self.photo_transform_ready(cx)
            || self.editor.doc.locked_ancestor(id).is_some()
        {
            return;
        }
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let NodeKind::Smart {
            cache,
            offset,
            filter_mask,
            ..
        } = &node.kind
        else {
            return;
        };
        if filter_mask.is_some() && !from_selection {
            return;
        }
        let selection = if from_selection {
            let Some(selection) = self.editor.doc.selection.as_deref() else {
                return;
            };
            Some(selection)
        } else {
            None
        };
        let mask = match initial_mask(
            cache.width(),
            cache.height(),
            *offset,
            emulsion_core::transform::local_to_document(node),
            selection,
            hide_all,
        ) {
            Ok(mask) => mask,
            Err(error) => {
                self.set_status(error, true, cx);
                return;
            }
        };
        self.execute(
            Command::SetSmartFilterMask {
                id,
                mask: Some(mask),
            },
            cx,
        );
        self.select_smart_filter_mask(id, cx);
    }

    pub(super) fn invert_smart_filter_mask(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.finish_mask_properties();
        if !self.layer_menu_ready() || !self.photo_transform_ready(cx) {
            return;
        }
        let Some(mask) = self.editor.doc.node(id).and_then(descriptor) else {
            return;
        };
        let pixels = Arc::new(select::invert(&mask.pixels));
        self.execute(Command::SetSmartFilterMaskPixels { id, pixels }, cx);
    }

    pub(super) fn delete_smart_filter_mask(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.finish_mask_properties();
        if !self.layer_menu_ready() || !self.photo_transform_ready(cx) {
            return;
        }
        self.execute(Command::SetSmartFilterMask { id, mask: None }, cx);
        if self.editor.doc.node(id).and_then(descriptor).is_none() {
            if self.selected == Some(id)
                && self.tools.mask_edit_target == MaskEditTarget::SmartFilterMask
            {
                self.set_mask_edit_target(MaskEditTarget::Content, cx);
            }
            if self.mask_view.target == Some((id, MaskEditTarget::SmartFilterMask)) {
                self.mask_view.target = None;
            }
        }
    }

    /// The only filter-mask thumbnail belongs to the stack header, never an
    /// individual filter or the independent layer-mask row.
    pub(super) fn smart_filter_mask_thumbnail(
        &mut self,
        id: NodeId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let node = self.editor.doc.node(id)?.clone();
        let mask = descriptor(&node)?;
        let enabled = mask.enabled;
        let coverage = emulsion_core::smart_filter_mask::for_inspection(&node)?;
        let image = self.mask_thumbnail(id, MaskEditTarget::SmartFilterMask, &coverage);
        let active = self.selected == Some(id)
            && self.tools.mask_edit_target == MaskEditTarget::SmartFilterMask;
        Some(
            div()
                .id(("smart-filter-mask", id))
                .test_support()
                .relative()
                .flex_none()
                .p_0p5()
                .border_1()
                .border_color(if active { p.accent } else { p.line })
                .opacity(if enabled { 1. } else { 0.45 })
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if !this.layer_menu_ready() || !this.photo_transform_ready(cx) {
                        return;
                    }
                    if event.modifiers().shift {
                        this.execute(
                            Command::SetSmartFilterMaskEnabled {
                                id,
                                enabled: !enabled,
                            },
                            cx,
                        );
                        return;
                    }
                    let target = (id, MaskEditTarget::SmartFilterMask);
                    let show = event.modifiers().alt && this.mask_view.target != Some(target);
                    this.select_smart_filter_mask(id, cx);
                    this.mask_view.target = show.then_some(target);
                    window.focus(&this.panel_focus, cx);
                    cx.notify();
                }))
                .child(
                    img(ImageSource::Render(image))
                        .size_5()
                        .object_fit(ObjectFit::Contain),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .right_0()
                        .text_size(px(8.))
                        .text_color(p.accent)
                        .child("F"),
                )
                .when(!enabled, |el| {
                    el.child(
                        div()
                            .id(("smart-filter-mask-disabled", id))
                            .test_support()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(p.accent)
                            .text_lg()
                            .child("×"),
                    )
                })
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(
                        t!("editor.filter_mask.thumbnail_tip").into_owned(),
                    )
                    .build(window, cx)
                })
                .into_any_element(),
        )
    }

    pub(super) fn smart_filters_header(
        &mut self,
        id: NodeId,
        depth: usize,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let thumbnail = self.smart_filter_mask_thumbnail(id, p, cx);
        div()
            .id(("smart-filters-header", id))
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .pl(px(26. + depth as f32 * 12.))
            .py_1()
            .children(thumbnail)
            .child(mono(t!("editor.smart.filters"), 11., p.muted))
            .into_any_element()
    }

    pub(super) fn smart_filter_mask_controls(
        &mut self,
        id: NodeId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(node) = self.editor.doc.node(id) else {
            return div().into_any_element();
        };
        let mask = descriptor(node);
        let exists = mask.is_some();
        let enabled = mask.is_some_and(|m| m.enabled);
        let linked = mask.is_some_and(|m| m.linked);
        let locked = !self.layer_menu_ready() || self.editor.doc.locked_ancestor(id).is_some();
        let geometry_locked = locked || self.editor.doc.layer_locks(id).position;
        let mut controls = div()
            .id("filter-mask-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .child(mono(t!("editor.filter_mask.title"), 11., p.muted));
        let mut row = div().flex().flex_wrap().gap_1();
        for (name, label, hide_all, from_selection) in [
            (
                "filter-mask-reveal",
                t!("editor.filter_mask.reveal_all"),
                false,
                false,
            ),
            (
                "filter-mask-hide",
                t!("editor.filter_mask.hide_all"),
                true,
                false,
            ),
            (
                "filter-mask-from-selection",
                t!("editor.filter_mask.from_selection"),
                false,
                true,
            ),
        ] {
            row = row.child(
                Button::new(name)
                    .label(label)
                    .small()
                    .outline()
                    .disabled(
                        locked
                            || (exists && !from_selection)
                            || (from_selection && self.editor.doc.selection.is_none()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.add_smart_filter_mask(id, hide_all, from_selection, cx)
                    })),
            );
        }
        controls = controls.child(row);
        if exists {
            controls = controls.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new("filter-mask-edit")
                            .label(t!("editor.filter_mask.edit"))
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_smart_filter_mask(id, cx)
                            })),
                    )
                    .child(
                        Button::new("filter-mask-enable")
                            .label(if enabled {
                                t!("editor.filter_mask.disable")
                            } else {
                                t!("editor.filter_mask.enable")
                            })
                            .small()
                            .outline()
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.layer_menu_ready() {
                                    this.execute(
                                        Command::SetSmartFilterMaskEnabled {
                                            id,
                                            enabled: !enabled,
                                        },
                                        cx,
                                    );
                                }
                            })),
                    )
                    .child(
                        Button::new("filter-mask-link")
                            .label(if linked {
                                t!("editor.filter_mask.unlink")
                            } else {
                                t!("editor.filter_mask.link")
                            })
                            .small()
                            .outline()
                            .disabled(geometry_locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.layer_menu_ready() {
                                    this.execute(
                                        Command::SetSmartFilterMaskLinked {
                                            id,
                                            linked: !linked,
                                        },
                                        cx,
                                    );
                                }
                            })),
                    )
                    .child(
                        Button::new("filter-mask-invert")
                            .label(t!("editor.mask_taskbar.invert"))
                            .small()
                            .outline()
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.invert_smart_filter_mask(id, cx)
                            })),
                    )
                    .child(
                        Button::new("filter-mask-selection")
                            .label(t!("editor.photo_panels.to_selection"))
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.component_mask_to_selection(
                                    id,
                                    MaskEditTarget::SmartFilterMask,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("filter-mask-delete")
                            .label(t!("editor.filter_mask.delete"))
                            .small()
                            .outline()
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.delete_smart_filter_mask(id, cx)
                            })),
                    ),
            );
        }
        controls
            .child(mono(t!("editor.filter_mask.effect_tip"), 10., p.muted))
            .into_any_element()
    }
}
