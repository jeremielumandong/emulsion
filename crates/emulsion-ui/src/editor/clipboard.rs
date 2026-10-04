//! Canvas pixel clipboard and lifting a selection for Free Transform.
use super::*;
use emulsion_raster::{IRect, Mask, select};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::io::Cursor;

/// The OS clipboard keeps a portable PNG; placement and editable text stay
/// local to Emulsion, valid only while the clipboard image still matches.
/// Reuse placement only in the source document with a matching image ID;
/// other documents center the pasted pixels so they remain on the canvas.
struct ClipboardOrigin {
    image_id: u64,
    editor_id: EntityId,
    page_id: emulsion_core::project::PageId,
    rect: IRect,
    objects: Option<emulsion_core::fragment::Fragment>,
}
impl Global for ClipboardOrigin {}

/// A newly lifted selection remains cancellable until a transform is committed.
pub(crate) struct TransformLift {
    revision: u64,
    selected: Option<NodeId>,
    history: emulsion_core::History,
}

pub(super) fn transform_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    editor: &Entity<EditorView>,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut Context<gpui_kit::component::menu::PopupMenu>,
) -> gpui_kit::component::menu::PopupMenu {
    let e = editor.read(cx);
    // Design objects resize and rotate through their canvas handles. Keep the
    // pixel-transform menu, including destructive modes, in Photo only.
    if e.is_design() {
        return menu;
    }
    let ready =
        !e.assistant.running && e.drag.is_none() && !e.editor.in_transaction() && e.warp.is_none();
    let single = e.selected_layer_ids().len() == 1;
    let node = e.selected.and_then(|id| e.editor.doc.node(id));
    let unlocked = node.is_some_and(|node| {
        node.visible
            && e.editor.doc.locked_ancestor(node.id).is_none()
            && !e.editor.doc.layer_locks(node.id).position
    });
    let raster = node.is_some_and(|node| matches!(node.kind, NodeKind::Raster { .. }));
    let roots = e.movement_layer_roots();
    let spatial = !roots.is_empty()
        && roots.iter().all(|id| {
            emulsion_core::geometry::node_bounds(&e.editor.doc, *id).is_some()
                && e.editor.doc.subtree(*id).into_iter().all(|member| {
                    e.editor.doc.locked_ancestor(member).is_none()
                        && !e.editor.doc.layer_locks(member).position
                })
        });
    let enabled = ready
        && unlocked
        && if e.editor.doc.selection.is_some() {
            single && raster && !e.tools.mask_edit_target.is_mask()
        } else {
            spatial || e.mask_transform_target().is_some()
        };
    let rotate_enabled = enabled;
    let distort =
        enabled && single && roots.len() == 1 && raster && !e.tools.mask_edit_target.is_mask();
    menu.separator()
        .menu_with_disabled(
            t!("editor.clipboard.free_transform").to_string(),
            Box::new(crate::actions::FreeTransform),
            !enabled,
        )
        .submenu(
            t!("editor.clipboard.transform").to_string(),
            window,
            cx,
            move |menu, _, _| {
                menu.action_context(focus.clone())
                    .menu_with_disabled(
                        t!("edit.scale").to_string(),
                        Box::new(crate::actions::TransformScale),
                        !enabled,
                    )
                    .menu_with_disabled(
                        t!("edit.rotate").to_string(),
                        Box::new(crate::actions::TransformRotate),
                        !enabled,
                    )
                    .menu_with_disabled(
                        t!("edit.distort").to_string(),
                        Box::new(crate::actions::TransformDistort),
                        !distort,
                    )
                    .menu_with_disabled(
                        t!("edit.warp").to_string(),
                        Box::new(crate::actions::TransformWarp),
                        !distort,
                    )
                    .separator()
                    .menu_with_disabled(
                        t!("editor.clipboard.rotate_180").to_string(),
                        Box::new(crate::actions::RotateLayer180),
                        !rotate_enabled,
                    )
                    .menu_with_disabled(
                        t!("editor.clipboard.rotate_90_cw").to_string(),
                        Box::new(crate::actions::RotateLayer90Cw),
                        !rotate_enabled,
                    )
                    .menu_with_disabled(
                        t!("editor.clipboard.rotate_90_ccw").to_string(),
                        Box::new(crate::actions::RotateLayer90Ccw),
                        !rotate_enabled,
                    )
                    .separator()
                    .menu_with_disabled(
                        t!("editor.clipboard.flip_horizontal").to_string(),
                        Box::new(crate::actions::FlipLayerHorizontal),
                        !enabled,
                    )
                    .menu_with_disabled(
                        t!("editor.clipboard.flip_vertical").to_string(),
                        Box::new(crate::actions::FlipLayerVertical),
                        !enabled,
                    )
            },
        )
}

fn png_image(raster: &Raster) -> Result<Image, String> {
    let bytes = emulsion_io::export::png16(raster.width(), raster.height(), &raster.to_srgba16())
        .map_err(|e| e.to_string())?;
    Ok(Image::from_bytes(ImageFormat::Png, bytes))
}

fn clipboard_raster(image: &Image) -> Result<Raster, String> {
    let reader = ImageReader::new(Cursor::new(&image.bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    emulsion_io::import::check_size(w, h).map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().map_err(|e| e.to_string())?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    decoded.apply_orientation(orientation);
    let rgba = decoded.into_rgba16();
    Ok(Raster::from_srgba16(
        rgba.width(),
        rgba.height(),
        rgba.as_raw(),
    ))
}

impl EditorView {
    pub(super) fn clipboard_menu(
        &self,
        menu: gpui_kit::component::menu::PopupMenu,
        focus: FocusHandle,
        cx: &mut App,
    ) -> gpui_kit::component::menu::PopupMenu {
        let ready = !self.assistant.running
            && self.drag.is_none()
            && !self.editor.in_transaction()
            && self.warp.is_none();
        let copy = ready
            && self
                .selected_layer_roots()
                .iter()
                .any(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, *id).is_some());
        let cut = copy && self.selected_pixel_targets().is_ok();
        let paste = ready
            && self.clipboard_slot().is_ok()
            && cx.read_from_clipboard().is_some_and(|item| {
                item.entries
                    .iter()
                    .any(|entry| matches!(entry, ClipboardEntry::Image(_)))
            });
        let canvas = focus == self.canvas_focus;
        let menu = menu
            .action_context(focus)
            .menu_with_disabled(
                t!("edit.cut").to_string(),
                Box::new(crate::actions::CutPixels),
                !cut,
            )
            .menu_with_disabled(
                t!("edit.copy").to_string(),
                Box::new(crate::actions::CopyPixels),
                !copy,
            )
            .menu_with_disabled(
                t!("edit.paste").to_string(),
                Box::new(crate::actions::PastePixels),
                !paste,
            );
        if !canvas {
            let node = self.selected.and_then(|id| self.editor.doc.node(id));
            let editable = ready
                && self.selected_layer_ids().len() == 1
                && node.is_some_and(|node| self.editor.doc.locked_ancestor(node.id).is_none());
            let can_smart = editable
                && node.is_some_and(|node| {
                    matches!(
                        node.kind,
                        NodeKind::Raster { .. } | NodeKind::Text { .. } | NodeKind::Path { .. }
                    )
                });
            let smart = node.is_some_and(|node| matches!(node.kind, NodeKind::Smart { .. }));
            let rasterize = editable
                && node.is_some_and(|node| {
                    matches!(
                        node.kind,
                        NodeKind::Smart { .. } | NodeKind::Text { .. } | NodeKind::Path { .. }
                    )
                });
            return menu
                .separator()
                .menu_with_disabled(
                    t!("editor.clipboard.convert_smart").to_string(),
                    Box::new(crate::actions::ConvertToSmartObject),
                    !can_smart,
                )
                .menu_with_disabled(
                    t!("editor.clipboard.convert_layers").to_string(),
                    Box::new(crate::actions::ConvertSmartToLayers),
                    !editable || !smart,
                )
                .menu_with_disabled(
                    t!("editor.clipboard.rasterize").to_string(),
                    Box::new(crate::actions::RasterizeLayer),
                    !rasterize,
                );
        }
        if self.is_design() {
            let ids = self.selected_layer_roots();
            let editable = ready
                && !ids.is_empty()
                && ids
                    .iter()
                    .all(|id| self.editor.doc.locked_ancestor(*id).is_none());
            return menu
                .separator()
                .menu_with_disabled(
                    t!("design.direct.duplicate").to_string(),
                    Box::new(crate::actions::DuplicateNode),
                    !editable,
                )
                .menu_with_disabled(
                    t!("design.direct.delete").to_string(),
                    Box::new(crate::actions::DeleteNode),
                    !editable,
                )
                .separator()
                .menu_with_disabled(
                    t!("edit.undo").to_string(),
                    Box::new(crate::actions::Undo),
                    !ready || !self.editor.can_undo(),
                )
                .menu_with_disabled(
                    t!("edit.redo").to_string(),
                    Box::new(crate::actions::Redo),
                    !ready || !self.editor.can_redo(),
                );
        }
        menu.separator()
            .menu_with_disabled(
                t!("editor.clipboard.rectangle_selection").to_string(),
                Box::new(crate::actions::ToolRectangularMarquee),
                !ready,
            )
            .menu_with_disabled(
                t!("editor.clipboard.select_all").to_string(),
                Box::new(crate::actions::SelectAll),
                !ready,
            )
            .menu_with_disabled(
                t!("select.deselect").to_string(),
                Box::new(crate::actions::Deselect),
                !ready || self.editor.doc.selection.is_none(),
            )
            .menu_with_disabled(
                t!("editor.clipboard.invert_selection").to_string(),
                Box::new(crate::actions::InvertSelection),
                !ready || self.editor.doc.selection.is_none(),
            )
            .menu_with_disabled(
                t!("editor.clipboard.delete_selected_pixels").to_string(),
                Box::new(crate::actions::ClearPixels),
                !cut || self.editor.doc.selection.is_none(),
            )
            .separator()
            .menu_with_disabled(
                t!("edit.undo").to_string(),
                Box::new(crate::actions::Undo),
                !ready || !self.editor.can_undo(),
            )
            .menu_with_disabled(
                t!("edit.redo").to_string(),
                Box::new(crate::actions::Redo),
                !ready || !self.editor.can_redo(),
            )
    }

    pub(crate) fn clipboard_host(
        &mut self,
        action: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.clipboard_ready_from(true, cx) {
            return Err("Finish the active edit before clipboard operations.".into());
        }
        self.status = None;
        match action {
            "copy" => self.copy_pixels_from(true, cx),
            "cut" => self.cut_pixels_from(true, cx),
            "paste" => self.paste_pixels_from(true, false, cx),
            _ => return Err("Unknown clipboard action.".into()),
        }
        if let Some((message, true)) = &self.status {
            return Err(message.to_string());
        }
        Ok(())
    }
    fn clipboard_ready(&mut self, cx: &mut Context<Self>) -> bool {
        self.clipboard_ready_from(false, cx)
    }
    fn clipboard_ready_from(&mut self, host: bool, cx: &mut Context<Self>) -> bool {
        if (self.assistant.running && !host)
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            self.set_status(t!("editor.clipboard.finish_edit"), true, cx);
            false
        } else {
            true
        }
    }

    fn pixel_target(&self) -> Result<(NodeId, Arc<Raster>, Placement), String> {
        let id = self
            .selected
            .ok_or_else(|| t!("editor.clipboard.select_pixel_layer").into_owned())?;
        let locks = self.editor.doc.layer_locks(id);
        if self.editor.doc.locked_ancestor(id).is_some() || locks.pixels || locks.transparency {
            return Err(t!("editor.clipboard.layer_locked").into_owned());
        }
        if self.tools.mask_edit_target.is_mask() {
            return Err(t!("editor.clipboard.leave_mask_edit").into_owned());
        }
        match &self
            .editor
            .doc
            .node(id)
            .ok_or_else(|| t!("editor.clipboard.layer_gone").into_owned())?
            .kind
        {
            NodeKind::Raster { raster, placement } => Ok((id, raster.clone(), *placement)),
            _ => Err(t!("editor.clipboard.needs_pixel_layer").into_owned()),
        }
    }

    fn selected_pixel_targets(&self) -> Result<Vec<(NodeId, Arc<Raster>, Placement)>, String> {
        if self.tools.mask_edit_target.is_mask() {
            return Err(t!("editor.clipboard.leave_mask_edit").into_owned());
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            return Err(t!("editor.clipboard.select_pixel_layer").into_owned());
        }
        ids.into_iter()
            .map(|id| {
                let locks = self.editor.doc.layer_locks(id);
                if self.editor.doc.locked_ancestor(id).is_some()
                    || locks.pixels
                    || locks.transparency
                {
                    return Err(t!("editor.clipboard.selected_locked").into_owned());
                }
                match &self
                    .editor
                    .doc
                    .node(id)
                    .ok_or_else(|| t!("editor.clipboard.layer_gone").into_owned())?
                    .kind
                {
                    NodeKind::Raster { raster, placement } => Ok((id, raster.clone(), *placement)),
                    _ => Err(t!("editor.clipboard.needs_pixel_layers").into_owned()),
                }
            })
            .collect()
    }

    /// Copy the selected layer/subtree as seen in document coordinates,
    /// including its placement, layer mask and opacity, clipped by selection.
    fn selected_pixels(&self) -> Result<(Raster, IRect), String> {
        let doc = &self.editor.doc;
        let roots = self.selected_layer_roots();
        if roots.is_empty() {
            return Err(t!("editor.clipboard.select_layer_to_copy").into_owned());
        }
        let rect = if let Some(selection) = &doc.selection {
            select::bounds(selection)
        } else {
            roots
                .iter()
                .filter_map(|id| emulsion_core::geometry::node_bounds(doc, *id))
                .reduce(|a, b| a.union(&b))
                .ok_or_else(|| t!("editor.clipboard.no_layer_pixels").into_owned())?
        }
        .intersect(&IRect::new(0, 0, doc.width as i32, doc.height as i32));
        if rect.is_empty() {
            return Err(t!("editor.clipboard.no_selected_pixels").into_owned());
        }
        let ids: Vec<_> = roots.iter().flat_map(|id| doc.subtree(*id)).collect();
        let mut isolated = doc.clone();
        isolated.nodes.retain(|n| ids.contains(&n.id));
        for node in &mut isolated.nodes {
            if roots.contains(&node.id) {
                node.parent = None;
                node.visible = true;
            }
            if node.clip_to.is_some_and(|base| !ids.contains(&base)) {
                node.clip_to = None;
            }
        }
        let mut pixels = emulsion_raster::composite::region(&isolated.composite_tree(), rect);
        for (i, pixel) in pixels.iter_mut().enumerate() {
            let x = rect.x as u32 + (i % rect.w as usize) as u32;
            let y = rect.y as u32 + (i / rect.w as usize) as u32;
            let cover = doc
                .selection
                .as_ref()
                .map_or(1.0, |s| s.get(x, y) as f32 / 255.0);
            *pixel = pixel.map(|v| v * cover);
        }
        if !pixels.iter().any(|p| p[3] > 0.0) {
            return Err(t!("editor.clipboard.no_selected_pixels").into_owned());
        }
        Ok((
            Raster::from_fn(rect.w as u32, rect.h as u32, [0; 4], |x, y| {
                color::f_to_px(pixels[y as usize * rect.w as usize + x as usize])
            }),
            rect,
        ))
    }

    fn put_pixels_on_clipboard(
        &mut self,
        pixels: &Raster,
        rect: IRect,
        cx: &mut Context<Self>,
    ) -> bool {
        let image = match png_image(pixels) {
            Ok(image) => image,
            Err(e) => {
                self.set_status(t!("editor.clipboard.encode_failed", error = e), true, cx);
                return false;
            }
        };
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
        // GPUI's write API has no Result. Verify ownership/read-back before
        // a cut removes anything, rather than assuming the OS accepted it.
        let written = cx.read_from_clipboard().is_some_and(|item| {
            item.entries.iter().any(
                |entry| matches!(entry, ClipboardEntry::Image(stored) if stored.id == image.id),
            )
        });
        if !written {
            self.set_status(t!("editor.clipboard.write_failed"), true, cx);
            return false;
        }
        cx.set_global(ClipboardOrigin {
            image_id: image.id,
            editor_id: cx.entity_id(),
            page_id: self.editor.active_page(),
            rect,
            objects: None,
        });
        true
    }

    pub fn copy_pixels(&mut self, cx: &mut Context<Self>) {
        self.copy_pixels_from(false, cx);
    }
    fn copy_pixels_from(&mut self, host: bool, cx: &mut Context<Self>) {
        if !self.clipboard_ready_from(host, cx) {
            return;
        }
        let roots = self.selected_layer_roots();
        let has_vectors = roots
            .iter()
            .flat_map(|id| self.editor.doc.subtree(*id))
            .any(|id| {
                self.editor.doc.node(id).is_some_and(|n| {
                    matches!(n.kind, NodeKind::Text { .. } | NodeKind::Path { .. })
                })
            });
        let objects = if self.editor.doc.selection.is_none()
            && (has_vectors || self.editor.kind().is_some())
        {
            match emulsion_core::fragment::Fragment::capture(&self.editor.doc, &roots) {
                Ok(fragment) => Some(fragment),
                Err(error) => {
                    // Native capture may refuse a role-dependent frame. Do
                    // not silently flatten it or overwrite the old clipboard.
                    self.set_status(error, true, cx);
                    return;
                }
            }
        } else {
            None
        };
        match self.selected_pixels() {
            Ok((pixels, rect)) => {
                if self.put_pixels_on_clipboard(&pixels, rect, cx) {
                    let editable = objects.is_some();
                    cx.global_mut::<ClipboardOrigin>().objects = objects;
                    self.set_status(
                        if editable {
                            t!("editor.clipboard.copied_objects")
                        } else {
                            t!("editor.clipboard.copied_pixels")
                        },
                        false,
                        cx,
                    );
                }
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }

    fn cleared_pixels(&self, raster: &Raster, placement: Placement) -> (Raster, IRect) {
        let to_doc = placement.to_doc(raster.width(), raster.height());
        let corners = [
            (0., 0.),
            (raster.width() as f64, 0.),
            (raster.width() as f64, raster.height() as f64),
            (0., raster.height() as f64),
        ];
        if self.editor.doc.selection.is_none()
            && corners.iter().all(|&(x, y)| {
                let p = to_doc.transform_point2(glam::dvec2(x, y));
                p.x >= 0.
                    && p.y >= 0.
                    && p.x <= self.editor.doc.width as f64
                    && p.y <= self.editor.doc.height as f64
            })
        {
            return (
                Raster::empty(raster.width(), raster.height(), [0; 4]),
                raster.bounds(),
            );
        }
        // Copy renders the visible canvas. Match that extent without a
        // selection, retaining off-canvas pixels that were never copied.
        let selection = self.editor.doc.selection.clone().unwrap_or_else(|| {
            Arc::new(Mask::empty(
                self.editor.doc.width,
                self.editor.doc.height,
                255,
            ))
        });
        let selected = select::bounds(&selection);
        if selected.is_empty() {
            return (raster.clone(), IRect::default());
        }
        let inv = to_doc.inverse();
        let points = [
            (selected.x, selected.y),
            (selected.right(), selected.y),
            (selected.right(), selected.bottom()),
            (selected.x, selected.bottom()),
        ]
        .map(|(x, y)| inv.transform_point2(glam::dvec2(x as f64, y as f64)));
        let lo = points
            .iter()
            .fold(glam::DVec2::splat(f64::INFINITY), |a, b| a.min(*b));
        let hi = points
            .iter()
            .fold(glam::DVec2::splat(f64::NEG_INFINITY), |a, b| a.max(*b));
        let support = if raster.fill()[3] > 0 {
            raster.bounds()
        } else {
            raster.tile_bounds()
        };
        let bounds = IRect::new(
            lo.x.floor() as i32,
            lo.y.floor() as i32,
            (hi.x.ceil() - lo.x.floor()) as i32,
            (hi.y.ceil() - lo.y.floor()) as i32,
        )
        .intersect(&support);
        if bounds.is_empty() {
            return (raster.clone(), bounds);
        }
        let clip = super::tools::local_clip(selection, to_doc);
        // Work one tile at a time; a large implicit fill must not require
        // two full-canvas temporary pixel buffers.
        let t = emulsion_raster::TILE as i32;
        let mut changes = Vec::new();
        for ty in bounds.y.div_euclid(t)..=(bounds.bottom() - 1).div_euclid(t) {
            for tx in bounds.x.div_euclid(t)..=(bounds.right() - 1).div_euclid(t) {
                let coord = TileCoord::new(tx, ty);
                let mut pixels = raster
                    .tile(0, coord)
                    .map(|p| p.to_vec())
                    .unwrap_or_else(|| vec![raster.fill(); emulsion_raster::TILE_PX]);
                let rect = IRect::new(tx * t, ty * t, t, t).intersect(&bounds);
                let mut changed = false;
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        let i = ((y - ty * t) * t + x - tx * t) as usize;
                        let coverage = clip(x, y);
                        let erased =
                            pixels[i].map(|v| (v as f32 * (1.0 - coverage)).round() as u16);
                        changed |= erased != pixels[i];
                        pixels[i] = erased;
                    }
                }
                if changed {
                    changes.push((coord, Some(pixels)));
                }
            }
        }
        (raster.with_changes(changes), bounds)
    }

    fn clear_selected_pixel_commands(&self, label: &str) -> Result<Vec<Command>, String> {
        self.selected_pixel_targets()?
            .into_iter()
            .map(|(id, source, placement)| {
                let (cleared, dirty) = self.cleared_pixels(&source, placement);
                Ok(Command::ReplacePixels {
                    id,
                    raster: Arc::new(cleared),
                    dirty,
                    label: label.into(),
                })
            })
            .collect()
    }

    pub fn cut_pixels(&mut self, cx: &mut Context<Self>) {
        self.cut_pixels_from(false, cx);
    }
    fn cut_pixels_from(&mut self, host: bool, cx: &mut Context<Self>) {
        if !self.clipboard_ready_from(host, cx) {
            return;
        }
        let roots = self.selected_layer_roots();
        let native = self.editor.doc.selection.is_none()
            && (self.editor.kind().is_some()
                || roots
                    .iter()
                    .flat_map(|id| self.editor.doc.subtree(*id))
                    .any(|id| {
                        self.editor.doc.node(id).is_some_and(|n| {
                            matches!(n.kind, NodeKind::Text { .. } | NodeKind::Path { .. })
                        })
                    }));
        if native {
            let prepared = (|| {
                let fragment =
                    emulsion_core::fragment::Fragment::capture(&self.editor.doc, &roots)?;
                let mut trial = self.editor.doc.clone();
                let mut commands = Vec::new();
                // Deleting an endpoint also deletes its connectors; skip those
                // already removed by the preceding command.
                for id in &fragment.roots {
                    if trial.node(*id).is_some() {
                        let command = Command::RemoveNode { id: *id };
                        command.apply(&mut trial).map_err(|e| e.to_string())?;
                        commands.push(command);
                    }
                }
                let (pixels, rect) = self.selected_pixels()?;
                Ok::<_, String>((fragment, commands, pixels, rect))
            })();
            match prepared {
                Ok((fragment, commands, pixels, rect)) => {
                    if self.put_pixels_on_clipboard(&pixels, rect, cx) {
                        cx.global_mut::<ClipboardOrigin>().objects = Some(fragment);
                        self.execute_layer_commands("Cut editable objects", commands, cx);
                    }
                }
                Err(error) => self.set_status(error, true, cx),
            }
            return;
        }
        let prepared = (|| {
            let commands = self.clear_selected_pixel_commands("Cut pixels")?;
            let mut trial = self.editor.doc.clone();
            for command in &commands {
                command.apply(&mut trial).map_err(|e| e.to_string())?;
            }
            let (pixels, rect) = self.selected_pixels()?;
            Ok::<_, String>((commands, pixels, rect))
        })();
        let (commands, pixels, rect) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.set_status(error, true, cx);
                return;
            }
        };
        if self.put_pixels_on_clipboard(&pixels, rect, cx) {
            self.execute_layer_commands("Cut pixels", commands, cx);
        }
    }

    pub fn clear_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) {
            return;
        }
        match self.clear_selected_pixel_commands("Clear pixels") {
            Ok(commands) => {
                self.execute_layer_commands("Clear pixels", commands, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    /// Keyboard deletion belongs to the selected component, even after a
    /// transform switches the active tool to Move. Explicit Delete Layer is
    /// a distinct command and deliberately does not call this adapter.
    fn consume_mask_delete_key(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.tools.mask_edit_target.is_mask() {
            return false;
        }
        if self.tools.mask_edit_target == MaskEditTarget::VectorMask && self.tool == Tool::Pen {
            self.pen_delete(cx);
        } else {
            self.set_status("A mask is selected. Use its editing controls, or select the content thumbnail to delete the layer.", false, cx);
        }
        true
    }

    pub(crate) fn delete_panel_target(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) || self.consume_mask_delete_key(cx) {
            return;
        }
        if self.tool == Tool::Pen && self.pen_delete(cx) {
            return;
        }
        self.delete_selected(cx);
    }

    pub fn delete_canvas_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) || self.consume_mask_delete_key(cx) {
            return;
        }
        if self.tool == Tool::Pen && self.pen_delete(cx) {
            return;
        }
        if self.vector_delete(cx) {
            return;
        }
        if self.tool == Tool::Select
            && matches!(
                self.tools.select,
                SelectShape::Polygon | SelectShape::Magnetic
            )
            && self.tools.polygon.pop().is_some()
        {
            // The live magnetic segment starts at the removed point. Drop it
            // too; the next pointer movement will trace from the new endpoint.
            self.tools.magnetic_live.clear();
            cx.notify();
            return;
        }
        if self.editor.doc.selection.is_some() {
            self.clear_pixels(cx);
        } else {
            self.delete_selected(cx);
        }
    }

    fn clipboard_slot(&self) -> Result<Slot, String> {
        let slot = self.insertion_slot();
        if slot
            .parent
            .is_some_and(|id| self.editor.doc.locked_ancestor(id).is_some())
        {
            return Err(t!("editor.clipboard.group_locked").into_owned());
        }
        Ok(slot)
    }

    pub fn paste_pixels(&mut self, cx: &mut Context<Self>) {
        self.paste_pixels_from(false, false, cx);
    }

    /// Paste at the position the layers or pixels were copied from, even on
    /// another page, storyboard panel or open document. On the Board it
    /// pastes panels, which have no position.
    pub fn paste_in_place(&mut self, cx: &mut Context<Self>) {
        if self.board_open() {
            self.storyboard_paste_panels(cx);
            return;
        }
        self.paste_pixels_from(false, true, cx);
    }

    /// `in_place` keeps the copied position everywhere; otherwise only the
    /// page it came from does, and elsewhere the paste is centred.
    fn paste_pixels_from(&mut self, host: bool, in_place: bool, cx: &mut Context<Self>) {
        if !self.clipboard_ready_from(host, cx) {
            return;
        }
        let image = cx.read_from_clipboard().and_then(|item| {
            item.entries.into_iter().find_map(|e| {
                if let ClipboardEntry::Image(image) = e {
                    Some(image)
                } else {
                    None
                }
            })
        });
        let Some(image) = image else {
            self.set_status(t!("editor.clipboard.no_image"), true, cx);
            return;
        };
        let objects = cx
            .try_global::<ClipboardOrigin>()
            .filter(|origin| origin.image_id == image.id)
            .and_then(|origin| {
                origin.objects.clone().map(|objects| {
                    (
                        objects,
                        in_place
                            || (origin.editor_id == cx.entity_id()
                                && origin.page_id == self.editor.active_page()),
                        origin.rect,
                    )
                })
            });
        if let Some((objects, keep_position, rect)) = objects {
            let slot = match self.clipboard_slot() {
                Ok(slot) => slot,
                Err(error) => {
                    self.set_status(error, true, cx);
                    return;
                }
            };
            let offset = if keep_position {
                (0., 0.)
            } else {
                (
                    (self.editor.doc.width as f64 - rect.w as f64) / 2. - rect.x as f64,
                    (self.editor.doc.height as f64 - rect.h as f64) / 2. - rect.y as f64,
                )
            };
            match objects.paste_into_project(&mut self.editor, slot, offset) {
                Ok(ids) => {
                    self.set_layer_selection(ids.clone(), ids.last().copied());
                    self.after_change(cx);
                }
                Err(error) => self.set_status(error, true, cx),
            }
            return;
        }
        let raster = match clipboard_raster(&image) {
            Ok(raster) => raster,
            Err(e) => {
                self.set_status(t!("editor.clipboard.read_failed", error = e), true, cx);
                return;
            }
        };
        let slot = match self.clipboard_slot() {
            Ok(slot) => slot,
            Err(e) => {
                self.set_status(e, true, cx);
                return;
            }
        };
        let placement = cx
            .try_global::<ClipboardOrigin>()
            .filter(|origin| {
                origin.image_id == image.id
                    && (in_place
                        || (origin.editor_id == cx.entity_id()
                            && origin.page_id == self.editor.active_page()))
            })
            .map(|origin| Placement::at(origin.rect.x as f64, origin.rect.y as f64))
            .unwrap_or_else(|| {
                Placement::at(
                    (self.editor.doc.width as f64 - raster.width() as f64) / 2.0,
                    (self.editor.doc.height as f64 - raster.height() as f64) / 2.0,
                )
            });
        self.editor.begin("Paste pixels");
        let result = self
            .editor
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Pasted pixels",
                    Arc::new(raster),
                    placement,
                )),
                slot,
            })
            .and_then(|id| {
                self.editor
                    .execute(Command::SetSelection { selection: None })?;
                Ok(id)
            });
        self.finish_pixel_transaction(result, cx);
    }

    /// Lift into a new layer so the existing Move handles transform the
    /// selected pixels. This never reads or writes the OS clipboard.
    pub fn transform_pixels(&mut self, cx: &mut Context<Self>) {
        if self.is_photo_workflow() {
            self.begin_photo_transform(false, cx);
            return;
        }
        self.prepare_transform_pixels(cx);
    }

    /// Legacy non-modal transforms still own a cancellable selected-pixel lift.
    /// Photo Warp/Distort use this route too, without starting affine Free Transform.
    pub(super) fn prepare_transform_pixels(&mut self, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        if self.editor.doc.selection.is_none()
            && (self.selected_layer_ids().len() > 1
                || self
                    .selected
                    .and_then(|id| self.editor.doc.node(id))
                    .is_some_and(|n| n.is_group())
                || self.tools.mask_edit_target.is_mask())
        {
            let mask_edit_target = self.tools.mask_edit_target;
            self.set_tool(Tool::Move, cx);
            self.tools.mask_edit_target = mask_edit_target;
            return;
        }
        let before = self.editor.doc.selection.as_ref().map(|_| {
            (
                self.selected,
                self.editor.history.clone(),
                self.editor.revision,
            )
        });
        self.transform_pixels_with(|_, _| None, cx);
        if let Some((selected, history, revision)) = before
            && self.editor.doc.selection.is_none()
            && self.editor.revision != revision
        {
            self.tools.transform_lift = Some(TransformLift {
                revision: self.editor.revision,
                selected,
                history,
            });
        }
    }

    /// Prepare selected-pixel lift/copy without publishing or touching the OS clipboard.
    pub(super) fn photo_pixel_transform_prefix(
        &self,
        copy: bool,
    ) -> Result<(Vec<Command>, Vec<NodeId>), String> {
        let (id, source, placement) = self.pixel_target()?;
        let (pixels, rect) = self.selected_pixels()?;
        let slot = self.clipboard_slot()?;
        let mut prefix = Vec::new();
        if !copy {
            let (cleared, dirty) = self.cleared_pixels(&source, placement);
            prefix.push(Command::ReplacePixels {
                id,
                raster: Arc::new(cleared),
                dirty,
                label: "Lift selection".into(),
            });
        }
        let command = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Selection",
                Arc::new(pixels),
                Placement::at(rect.x as f64, rect.y as f64),
            )),
            slot,
        };
        let mut trial = self.editor.doc.clone();
        for command in &prefix {
            command.apply(&mut trial).map_err(|e| e.to_string())?;
        }
        let created = command
            .apply(&mut trial)
            .map_err(|e| e.to_string())?
            .ok_or("Selection did not create artwork.")?;
        prefix.push(command);
        prefix.push(Command::SetSelection { selection: None });
        Ok((prefix, vec![created]))
    }

    pub(super) fn cancel_transform_lift(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(lift) = self.tools.transform_lift.take() else {
            return false;
        };
        // Undo only our lift, never a later committed edit or another document's history.
        if self.editor.in_transaction() || self.editor.revision != lift.revision {
            return false;
        }
        self.invalidate_pending_edits();
        if !self.editor.undo() {
            return false;
        }
        self.editor.history = lift.history;
        let selected = lift.selected;
        self.set_layer_selection(selected.into_iter().collect(), selected);
        self.after_change(cx);
        true
    }

    pub(crate) fn transform_pixels_with(
        &mut self,
        transform: impl FnOnce(NodeId, &emulsion_core::Document) -> Option<Command>,
        cx: &mut Context<Self>,
    ) {
        if self.selected_layer_ids().len() > 1 {
            self.set_status(t!("editor.clipboard.select_one_to_transform"), true, cx);
            return;
        }
        if !self.clipboard_ready(cx) {
            return;
        }
        if self.editor.doc.selection.is_none() {
            if let Some(id) = self.selected
                && let Some(command) = transform(id, &self.editor.doc)
            {
                self.execute(command, cx);
            }
            self.set_tool(Tool::Move, cx);
            return;
        }
        let result = (|| {
            let (id, source, placement) = self.pixel_target()?;
            let (pixels, rect) = self.selected_pixels()?;
            let slot = self.clipboard_slot()?;
            Ok::<_, String>((id, source, placement, pixels, rect, slot))
        })();
        let (id, source, placement, pixels, rect, slot) = match result {
            Ok(parts) => parts,
            Err(e) => {
                self.set_status(e, true, cx);
                return;
            }
        };
        let (cleared, dirty) = self.cleared_pixels(&source, placement);
        self.editor.begin("Lift selection");
        let result = self
            .editor
            .execute(Command::ReplacePixels {
                id,
                raster: Arc::new(cleared),
                dirty,
                label: "Lift selection".into(),
            })
            .and_then(|_| {
                self.editor.execute(Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Selection",
                        Arc::new(pixels),
                        Placement::at(rect.x as f64, rect.y as f64),
                    )),
                    slot,
                })
            })
            .and_then(|id| {
                self.editor
                    .execute(Command::SetSelection { selection: None })?;
                if let Some(id) = id
                    && let Some(command) = transform(id, &self.editor.doc)
                {
                    self.editor.execute(command)?;
                }
                Ok(id)
            });
        self.finish_pixel_transaction(result, cx);
    }

    fn finish_pixel_transaction(
        &mut self,
        result: Result<Option<NodeId>, emulsion_core::CommandError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(Some(id)) => {
                self.editor.end();
                self.set_layer_selection(vec![id], Some(id));
                self.set_tool(Tool::Move, cx);
                self.select_sidebar(SidebarTab::Properties, cx);
                self.after_change(cx);
            }
            error => {
                self.editor.cancel();
                self.set_status(
                    t!(
                        "editor.clipboard.update_failed",
                        error = format!("{error:?}")
                    ),
                    true,
                    cx,
                );
                self.after_change(cx);
            }
        }
    }
}

#[cfg(test)]
#[path = "selection_delete_tests.rs"]
mod selection_delete_tests;
