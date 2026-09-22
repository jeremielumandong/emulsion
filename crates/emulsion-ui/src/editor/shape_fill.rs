//! Object fills preserve editable shapes; mask fills target coverage, not new layers.
use super::*;

impl EditorView {
    /// Return true when this request belongs to an editable shape or a mask,
    /// including rejected requests that must never fall through to a new layer.
    pub(super) fn fill_object_or_mask(
        &mut self,
        point: Option<(f64, f64)>,
        rgba: [u8; 4],
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selected_layer_ids().len() > 1 {
            self.set_status("Select one layer to fill.", true, cx);
            return true;
        }
        if self.tools.mask_edit || self.tool == Tool::Mask {
            self.fill_layer_mask(point, rgba, cx);
            return true;
        }
        let Some(id) = self.selected else {
            return false;
        };
        let Some(node) = self.editor.doc.node(id) else {
            return false;
        };
        if matches!(node.kind, NodeKind::Path { .. }) {
            self.fill_vector_shape(id, point, rgba, cx);
            return true;
        }
        let NodeKind::Fill { rgba: old_color } = node.kind else {
            return false;
        };
        if old_color == rgba {
            return true;
        }
        let mask = node.mask_enabled.then_some(node.mask.as_ref()).flatten();
        if let Some((x, y)) = point
            && (mask.is_some_and(|m| m.get(x.floor() as u32, y.floor() as u32) == 0)
                || self
                    .editor
                    .doc
                    .selection
                    .as_ref()
                    .is_some_and(|m| m.get(x.floor() as u32, y.floor() as u32) == 0))
        {
            return true;
        }
        if let Some(selection) = &self.editor.doc.selection {
            let bounds = mask.map_or_else(
                || {
                    IRect::new(
                        0,
                        0,
                        self.editor.doc.width as i32,
                        self.editor.doc.height as i32,
                    )
                },
                |m| select::bounds(m),
            );
            let partial = (bounds.y..bounds.bottom()).any(|y| {
                (bounds.x..bounds.right()).any(|x| {
                    mask.is_none_or(|m| m.get(x as u32, y as u32) > 0)
                        && selection.get(x as u32, y as u32) < 255
                })
            });
            if partial {
                let selection = selection.clone();
                let mask = mask.cloned();
                self.fill_shape_pixels(id, old_color, rgba, selection, mask, cx);
                return true;
            }
        }
        self.execute(Command::SetFillColor { id, rgba }, cx);
        true
    }

    fn fill_vector_shape(
        &mut self,
        id: NodeId,
        point: Option<(f64, f64)>,
        rgba: [u8; 4],
        cx: &mut Context<Self>,
    ) {
        use emulsion_raster::vector::PathPaint;
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        if self.editor.doc.locked_ancestor(id).is_some() || node.locks.pixels {
            self.set_status("That layer or its pixels are locked.", true, cx);
            return;
        }
        let NodeKind::Path { path, style, cache } = &node.kind else {
            return;
        };
        if style.fill == Some(rgba) && style.fill_paint == PathPaint::Solid {
            return;
        }
        let selection = self.editor.doc.selection.clone();
        let mask = emulsion_core::Document::composite_mask(node);
        if let Some((x, y)) = point
            && (x < 0.0
                || y < 0.0
                || x >= self.editor.doc.width as f64
                || y >= self.editor.doc.height as f64
                || !path_contains(path, (x, y))
                || mask
                    .as_ref()
                    .is_some_and(|m| m.get(x.floor() as u32, y.floor() as u32) == 0)
                || selection
                    .as_ref()
                    .is_some_and(|m| m.get(x.floor() as u32, y.floor() as u32) == 0))
        {
            return;
        }
        let (path, cache) = (path.clone(), cache.clone());
        let style = emulsion_raster::vector::PathStyle {
            fill: Some(rgba),
            fill_paint: PathPaint::Solid,
            ..*style
        };
        let Some(selection) = selection else {
            self.execute(Command::SetPath { id, path, style }, cx);
            return;
        };
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let compute_path = path.clone();
            let result = cx
                .background_spawn(async move {
                    let recolored = compute_path.rasterize(&style, w, h);
                    let bounds = cache.coverage_bounds().union(&recolored.coverage_bounds());
                    let mut partial = false;
                    let mut changed = false;
                    for y in bounds.y..bounds.bottom() {
                        for x in bounds.x..bounds.right() {
                            let (x, y) = (x as u32, y as u32);
                            if mask.as_ref().is_some_and(|m| m.get(x, y) == 0)
                                || cache.get(x, y) == recolored.get(x, y)
                            {
                                continue;
                            }
                            let coverage = selection.get(x, y);
                            partial |= coverage < 255;
                            changed |= coverage > 0;
                        }
                    }
                    if !changed {
                        return None;
                    }
                    if !partial {
                        return Some(None);
                    }
                    let dirty = bounds.intersect(&selection.coverage_bounds());
                    let raster =
                        blend_vector_fill(&cache, &recolored, &selection, mask.as_deref(), dirty);
                    Some(Some((raster, dirty)))
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill shape", cx) {
                    return;
                }
                match result {
                    None => {}
                    Some(None) => {
                        this.execute(Command::SetPath { id, path, style }, cx);
                    }
                    Some(Some((raster, dirty))) => {
                        if this
                            .execute_layer_commands(
                                "Fill shape selection",
                                vec![
                                    Command::Rasterize { id },
                                    Command::ReplacePixels {
                                        id,
                                        raster: Arc::new(raster),
                                        dirty,
                                        label: "Fill shape selection".into(),
                                    },
                                ],
                                cx,
                            )
                            .is_some()
                        {
                            this.set_status(
                                "Filled selected pixels. Undo restores the editable shape.",
                                false,
                                cx,
                            );
                        }
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn fill_shape_pixels(
        &mut self,
        id: NodeId,
        old_color: [u8; 4],
        new_color: [u8; 4],
        selection: Arc<Mask>,
        mask: Option<Arc<Mask>>,
        cx: &mut Context<Self>,
    ) {
        let mut trial = self.editor.doc.clone();
        if let Err(error) = (Command::Rasterize { id }).apply(&mut trial) {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let bounds = select::bounds(&selection).intersect(
            &mask
                .as_ref()
                .map_or(IRect::new(0, 0, w as i32, h as i32), |m| select::bounds(m)),
        );
        if bounds.is_empty()
            || !(bounds.y..bounds.bottom()).any(|y| {
                (bounds.x..bounds.right()).any(|x| {
                    selection.get(x as u32, y as u32) > 0
                        && mask.as_ref().is_none_or(|m| m.get(x as u32, y as u32) > 0)
                })
            })
        {
            return;
        }
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let (raster, dirty) = cx
                .background_spawn(async move {
                    let raster = Raster::solid(w, h, premul(old_color));
                    fill_color(
                        &raster,
                        bounds,
                        &|x, y| {
                            if mask.as_ref().is_none_or(|m| m.get(x as u32, y as u32) > 0) {
                                selection.get(x as u32, y as u32) as f32 / 255.0
                            } else {
                                0.0
                            }
                        },
                        premul(new_color),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill shape selection", cx) {
                    return;
                }
                if this
                    .execute_layer_commands(
                        "Fill shape selection",
                        vec![
                            Command::Rasterize { id },
                            Command::ReplacePixels {
                                id,
                                raster: Arc::new(raster),
                                dirty,
                                label: "Fill shape selection".into(),
                            },
                        ],
                        cx,
                    )
                    .is_some()
                {
                    this.set_status(
                        "Filled selected pixels. Undo restores the editable shape.",
                        false,
                        cx,
                    );
                }
            })
            .ok();
        })
        .detach();
    }

    fn fill_layer_mask(
        &mut self,
        point: Option<(f64, f64)>,
        rgba: [u8; 4],
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected else {
            self.set_status("Select a layer whose mask you want to fill.", true, cx);
            return;
        };
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        if self.editor.doc.locked_ancestor(id).is_some() {
            self.set_status("That layer or its pixels are locked.", true, cx);
            return;
        }
        let (w, h, to_doc) = match &node.kind {
            NodeKind::Raster { raster, placement }
            | NodeKind::Smart {
                source: raster,
                placement,
                ..
            } => (
                raster.width(),
                raster.height(),
                placement.to_doc(raster.width(), raster.height()),
            ),
            _ => (
                self.editor.doc.width,
                self.editor.doc.height,
                DAffine2::IDENTITY,
            ),
        };
        let (w, h, to_doc) = if let Some(mask) = &node.mask {
            (
                mask.width(),
                mask.height(),
                emulsion_core::transform::mask_to_document(node),
            )
        } else {
            (w, h, to_doc)
        };
        let point = point.map(|(x, y)| to_doc.inverse().transform_point2(dvec2(x, y)));
        if point.is_some_and(|p| p.x < 0.0 || p.y < 0.0 || p.x >= w as f64 || p.y >= h as f64) {
            return;
        }
        let mask = node
            .mask
            .clone()
            .unwrap_or_else(|| Arc::new(Mask::white(w, h)));
        let selection = self.editor.doc.selection.clone();
        let (tolerance, contiguous) = (self.tools.tolerance, self.tools.contiguous);
        let gray = 0.2126 * rgba[0] as f32 + 0.7152 * rgba[1] as f32 + 0.0722 * rgba[2] as f32;
        let alpha = rgba[3] as f32 / 255.0;
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let flood = point.map(|p| {
                        let bytes: Vec<u8> = mask
                            .read_rect(mask.bounds())
                            .into_iter()
                            .flat_map(|v| [v, v, v, 255])
                            .collect();
                        select::by_color(
                            &bytes,
                            w,
                            h,
                            p.x.floor() as u32,
                            p.y.floor() as u32,
                            tolerance,
                            contiguous,
                        )
                    });
                    let selection = selection.map(|s| local_clip(s, to_doc));
                    let mut changed = false;
                    let next = Mask::from_fn(w, h, mask.fill(), |x, y| {
                        let coverage = flood.as_ref().map_or(1.0, |m| m.get(x, y) as f32 / 255.0)
                            * selection
                                .as_ref()
                                .map_or(1.0, |clip| clip(x as i32, y as i32))
                            * alpha;
                        (mask.get(x, y) as f32 * (1.0 - coverage) + gray * coverage).round() as u8
                    });
                    // Plane equality is based on shared storage; compare values so
                    // clicking an already-filled mask creates no undo entry.
                    for y in 0..h {
                        for x in 0..w {
                            if next.get(x, y) != mask.get(x, y) {
                                changed = true;
                                break;
                            }
                        }
                        if changed {
                            break;
                        }
                    }
                    changed.then_some(next)
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Fill mask", cx) {
                    return;
                }
                if let Some(mask) = result {
                    this.execute(
                        Command::SetMask {
                            id,
                            mask: Some(Arc::new(mask)),
                        },
                        cx,
                    );
                }
            })
            .ok();
        })
        .detach();
    }
}

fn path_contains(path: &emulsion_raster::vector::Path, point: (f64, f64)) -> bool {
    let mut winding = 0;
    for sub in &path.subpaths {
        let points = sub.flatten(0.25);
        if points.len() < 3 {
            continue;
        }
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let cross = (b.0 - a.0) * (point.1 - a.1) - (point.0 - a.0) * (b.1 - a.1);
            if a.1 <= point.1 && b.1 > point.1 && cross > 0.0 {
                winding += 1;
            } else if a.1 > point.1 && b.1 <= point.1 && cross < 0.0 {
                winding -= 1;
            }
        }
    }
    winding != 0
}

/// Share untouched tiles and allocate only changed tiles, avoiding a second
/// full-canvas pixel buffer for large selections.
fn blend_vector_fill(
    old: &Raster,
    new: &Raster,
    selection: &Mask,
    mask: Option<&Mask>,
    dirty: IRect,
) -> Raster {
    let tile_size = emulsion_raster::TILE as i32;
    let mut changes = Vec::new();
    for ty in dirty.y.div_euclid(tile_size)..=(dirty.bottom() - 1).div_euclid(tile_size) {
        for tx in dirty.x.div_euclid(tile_size)..=(dirty.right() - 1).div_euclid(tile_size) {
            let tile = IRect::new(tx * tile_size, ty * tile_size, tile_size, tile_size);
            let region = tile.intersect(&dirty);
            let mut pixels = old.read_rect(tile);
            let mut changed = false;
            for y in region.y..region.bottom() {
                for x in region.x..region.right() {
                    if mask.is_some_and(|m| m.get(x as u32, y as u32) == 0) {
                        continue;
                    }
                    let coverage = selection.get(x as u32, y as u32) as u32;
                    let new = new.get(x as u32, y as u32);
                    let at = ((y - tile.y) * tile_size + x - tile.x) as usize;
                    for (old, new) in pixels[at].iter_mut().zip(new) {
                        let mixed = ((*old as u32 * (255 - coverage) + new as u32 * coverage + 127)
                            / 255) as u16;
                        changed |= *old != mixed;
                        *old = mixed;
                    }
                }
            }
            if changed {
                changes.push((TileCoord::new(tx, ty), Some(pixels)));
            }
        }
    }
    old.with_changes(changes)
}
