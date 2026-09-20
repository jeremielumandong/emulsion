//! Canvas pixel clipboard and lifting a selection for Free Transform.
use super::*;
use emulsion_raster::{IRect, Mask, select};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::io::Cursor;

/// The OS clipboard keeps a portable PNG; placement stays local to Emulsion.
/// Match the image ID before reusing it, so external copies cannot inherit
/// an unrelated placement.
struct ClipboardOrigin {
    image_id: u64,
    rect: IRect,
}
impl Global for ClipboardOrigin {}

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
            .filter(|origin| origin.image_id == image.id)
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
        if !self.clipboard_ready(cx) {
            return;
        }
        if self.editor.doc.selection.is_none() {
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
