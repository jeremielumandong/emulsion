//! Canvas pixel clipboard and lifting a selection for Free Transform.
use super::*;
use emulsion_raster::{IRect, Mask, select};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::io::Cursor;

/// The OS clipboard keeps a portable PNG; placement stays local to Emulsion.
/// Reuse placement only in the source document with a matching image ID;
/// other documents center the pasted pixels so they remain on the canvas.
struct ClipboardOrigin {
    image_id: u64,
    editor_id: EntityId,
    rect: IRect,
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
    let ready =
        !e.assistant.running && e.drag.is_none() && !e.editor.in_transaction() && e.warp.is_none();
    let node = e.selected.and_then(|id| e.editor.doc.node(id));
    let unlocked =
        node.is_some_and(|node| node.visible && e.editor.doc.locked_ancestor(node.id).is_none());
    let raster = node.is_some_and(|node| matches!(node.kind, NodeKind::Raster { .. }));
    let smart = node.is_some_and(|node| matches!(node.kind, NodeKind::Smart { .. }));
    let enabled = ready
        && unlocked
        && !e.tools.mask_edit
        && (raster || smart)
        && (e.editor.doc.selection.is_none() || raster);
    let rotate_enabled = ready
        && unlocked
        && !e.tools.mask_edit
        && node.is_some_and(|node| {
            emulsion_core::geometry::node_bounds(&e.editor.doc, node.id).is_some()
        })
        && (e.editor.doc.selection.is_none() || raster);
    menu.separator()
        .menu_with_disabled(
            "Free transform",
            Box::new(crate::actions::FreeTransform),
            !enabled,
        )
        .submenu("Transform", window, cx, move |menu, _, _| {
            menu.action_context(focus.clone())
                .menu_with_disabled("Scale", Box::new(crate::actions::TransformScale), !enabled)
                .menu_with_disabled(
                    "Rotate",
                    Box::new(crate::actions::TransformRotate),
                    !enabled,
                )
                .menu_with_disabled(
                    "Distort",
                    Box::new(crate::actions::TransformDistort),
                    !enabled || !raster,
                )
                .menu_with_disabled(
                    "Warp",
                    Box::new(crate::actions::TransformWarp),
                    !enabled || !raster,
                )
                .separator()
                .menu_with_disabled(
                    "Rotate 180°",
                    Box::new(crate::actions::RotateLayer180),
                    !rotate_enabled,
                )
                .menu_with_disabled(
                    "Rotate 90° clockwise",
                    Box::new(crate::actions::RotateLayer90Cw),
                    !rotate_enabled,
                )
                .menu_with_disabled(
                    "Rotate 90° counterclockwise",
                    Box::new(crate::actions::RotateLayer90Ccw),
                    !rotate_enabled,
                )
                .separator()
                .menu_with_disabled(
                    "Flip horizontal",
                    Box::new(crate::actions::FlipLayerHorizontal),
                    !enabled,
                )
                .menu_with_disabled(
                    "Flip vertical",
                    Box::new(crate::actions::FlipLayerVertical),
                    !enabled,
                )
        })
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
            && self.selected.is_some_and(|id| {
                emulsion_core::geometry::node_bounds(&self.editor.doc, id).is_some()
            });
        let cut = copy && self.pixel_target().is_ok();
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
            .menu_with_disabled("Cut", Box::new(crate::actions::CutPixels), !cut)
            .menu_with_disabled("Copy", Box::new(crate::actions::CopyPixels), !copy)
            .menu_with_disabled("Paste", Box::new(crate::actions::PastePixels), !paste);
        if !canvas {
            return menu;
        }
        menu.separator()
            .menu_with_disabled(
                "Rectangle selection",
                Box::new(crate::actions::ToolMarquee),
                !ready,
            )
            .menu_with_disabled("Select all", Box::new(crate::actions::SelectAll), !ready)
            .menu_with_disabled(
                "Deselect",
                Box::new(crate::actions::Deselect),
                !ready || self.editor.doc.selection.is_none(),
            )
            .menu_with_disabled(
                "Invert selection",
                Box::new(crate::actions::InvertSelection),
                !ready || self.editor.doc.selection.is_none(),
            )
            .menu_with_disabled(
                "Delete selected pixels",
                Box::new(crate::actions::ClearPixels),
                !cut || self.editor.doc.selection.is_none(),
            )
            .separator()
            .menu_with_disabled(
                "Undo",
                Box::new(crate::actions::Undo),
                !ready || !self.editor.history.can_undo(),
            )
            .menu_with_disabled(
                "Redo",
                Box::new(crate::actions::Redo),
                !ready || !self.editor.history.can_redo(),
            )
    }

    fn clipboard_ready(&mut self, cx: &mut Context<Self>) -> bool {
        if self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            self.set_status(
                "Finish the current edit before changing canvas pixels.",
                true,
                cx,
            );
            false
        } else {
            true
        }
    }

    fn pixel_target(&self) -> Result<(NodeId, Arc<Raster>, Placement), String> {
        let id = self.selected.ok_or("Select a pixel layer first")?;
        if self.editor.doc.locked_ancestor(id).is_some() {
            return Err("That layer or its group is locked".into());
        }
        if self.tools.mask_edit {
            return Err("Switch from mask editing to the layer pixels first".into());
        }
        match &self
            .editor
            .doc
            .node(id)
            .ok_or("The layer no longer exists")?
            .kind
        {
            NodeKind::Raster { raster, placement } => Ok((id, raster.clone(), *placement)),
            _ => Err(
                "Select a pixel layer; rasterize an editable object before cutting its pixels"
                    .into(),
            ),
        }
    }

    /// Copy the selected layer/subtree as seen in document coordinates,
    /// including its placement, layer mask and opacity, clipped by selection.
    fn selected_pixels(&self) -> Result<(Raster, IRect), String> {
        let doc = &self.editor.doc;
        let id = self.selected.ok_or("Select a layer to copy")?;
        let rect = if let Some(selection) = &doc.selection {
            select::bounds(selection)
        } else {
            emulsion_core::geometry::node_bounds(doc, id)
                .ok_or("That layer has no pixels to copy")?
        }
        .intersect(&IRect::new(0, 0, doc.width as i32, doc.height as i32));
        if rect.is_empty() {
            return Err("There are no selected pixels to copy".into());
        }
        let ids = doc.subtree(id);
        let mut isolated = doc.clone();
        isolated.nodes.retain(|n| ids.contains(&n.id));
        for node in &mut isolated.nodes {
            if node.id == id {
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
            return Err("There are no selected pixels to copy".into());
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
                self.set_status(format!("Could not encode clipboard image: {e}"), true, cx);
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
            self.set_status(
                "Could not write the image clipboard. Canvas pixels were kept.",
                true,
                cx,
            );
            return false;
        }
        cx.set_global(ClipboardOrigin {
            image_id: image.id,
            editor_id: cx.entity_id(),
            rect,
        });
        true
    }

    pub fn copy_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) {
            return;
        }
        match self.selected_pixels() {
            Ok((pixels, rect)) => {
                if self.put_pixels_on_clipboard(&pixels, rect, cx) {
                    self.set_status("Copied selected layer pixels.", false, cx);
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

    pub fn cut_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) {
            return;
        }
        let (id, source, placement) = match self.pixel_target() {
            Ok(target) => target,
            Err(e) => {
                self.set_status(e, true, cx);
                return;
            }
        };
        let (pixels, rect) = match self.selected_pixels() {
            Ok(pixels) => pixels,
            Err(e) => {
                self.set_status(e, true, cx);
                return;
            }
        };
        let (cleared, dirty) = self.cleared_pixels(&source, placement);
        if !self.put_pixels_on_clipboard(&pixels, rect, cx) {
            return;
        }
        self.execute(
            Command::ReplacePixels {
                id,
                raster: Arc::new(cleared),
                dirty,
                label: "Cut pixels".into(),
            },
            cx,
        );
    }

    pub fn clear_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) {
            return;
        }
        let (id, source, placement) = match self.pixel_target() {
            Ok(target) => target,
            Err(e) => {
                self.set_status(e, true, cx);
                return;
            }
        };
        let (cleared, dirty) = self.cleared_pixels(&source, placement);
        self.execute(
            Command::ReplacePixels {
                id,
                raster: Arc::new(cleared),
                dirty,
                label: "Clear pixels".into(),
            },
            cx,
        );
    }

    pub fn delete_canvas_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) {
            return;
        }
        if self.tool == Tool::Pen && self.pen_delete(cx) {
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
            return Err("The destination group is locked".into());
        }
        Ok(slot)
    }

    pub fn paste_pixels(&mut self, cx: &mut Context<Self>) {
        if !self.clipboard_ready(cx) {
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
            self.set_status("The clipboard does not contain an image.", true, cx);
            return;
        };
        let raster = match clipboard_raster(&image) {
            Ok(raster) => raster,
            Err(e) => {
                self.set_status(format!("Could not read clipboard image: {e}"), true, cx);
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
            .filter(|origin| origin.image_id == image.id && origin.editor_id == cx.entity_id())
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
        self.selected = lift.selected;
        self.after_change(cx);
        true
    }

    pub(crate) fn transform_pixels_with(
        &mut self,
        transform: impl FnOnce(NodeId, &emulsion_core::Document) -> Option<Command>,
        cx: &mut Context<Self>,
    ) {
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
                self.selected = Some(id);
                self.set_tool(Tool::Move, cx);
                self.select_sidebar(SidebarTab::Properties, cx);
                self.after_change(cx);
            }
            error => {
                self.editor.cancel();
                self.set_status(format!("Could not update pixels: {error:?}"), true, cx);
                self.after_change(cx);
            }
        }
    }
}

#[cfg(test)]
#[path = "selection_delete_tests.rs"]
mod selection_delete_tests;
