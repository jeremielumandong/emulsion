//! Modal Photo transforms. The repeat recipe is a document-space affine delta,
//! independent of the selected object IDs and of Undo/Redo snapshots.
use super::*;
use emulsion_core::{Document, project::PageId};
use glam::DAffine2;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Default)]
pub(crate) struct PhotoTransformState {
    active: Option<PhotoTransformSession>,
    last: Option<RepeatTransform>,
    reflow_before: Option<(NodeId, Arc<emulsion_core::text::TextSpec>)>,
}

#[derive(Clone, Copy)]
struct RepeatTransform {
    target: MaskEditTarget,
    page: PageId,
    delta: DAffine2,
}

struct PhotoTransformSession {
    duplicate: bool,
    page: PageId,
    revision: u64,
    epoch: u64,
    prefix: Vec<Command>,
    targets: Vec<NodeId>,
    original_selection: Vec<NodeId>,
    original_active: Option<NodeId>,
    original_tool: Tool,
    original_target: MaskEditTarget,
    mask_basis: Option<(NodeId, glam::DAffine2, [f64; 6])>,
    delta: DAffine2,
    gesture_base: DAffine2,
    invalid: bool,
}

/// Keep target resolution separate from session/history ownership. Active
/// raster/vector mask targeting can replace this adapter without changing the
/// transaction protocol. An unsupported active mask MUST NOT target artwork.
fn artwork_targets(doc: &Document, ids: &[NodeId]) -> Result<Vec<NodeId>, String> {
    let roots = emulsion_core::layer_links::movement_roots(doc, ids).map_err(|e| e.to_string())?;
    if roots.is_empty() {
        return Err("Select artwork to transform.".into());
    }
    emulsion_core::layer_links::check_movable(doc, &roots).map_err(|e| e.to_string())?;
    for root in &roots {
        for id in doc.subtree(*root) {
            let node = doc
                .node(id)
                .ok_or("The transform target is no longer available.")?;
            if matches!(node.kind, NodeKind::Fill { .. } | NodeKind::Adjust(_)) {
                return Err("Transform finite artwork; full-canvas fills and adjustments are not supported.".into());
            }
        }
    }
    Ok(roots)
}

/// Duplicate the entire expanded movement forest. Native commands clone Arcs,
/// retain editable content, and remap copy-internal clips. Rebuild movement
/// links using NEW copy IDs, never links back to the source forest.
fn copy_prefix(doc: &Document, roots: &[NodeId]) -> Result<(Vec<Command>, Vec<NodeId>), String> {
    let included: HashSet<_> = roots.iter().flat_map(|id| doc.subtree(*id)).collect();
    if roots
        .iter()
        .any(|id| doc.node(*id).is_some_and(|n| n.clip_to.is_some()))
    {
        // Duplicating independent sibling roots changes clip-stack ordering.
        // Until a forest-copy command owns that ordering, reject before copying.
        return Err(
            "Duplicate the clipping group together; copying clipped roots is not supported.".into(),
        );
    }
    let mut trial = doc.clone();
    let mut prefix = Vec::new();
    let mut copied = Vec::new();
    let mut mapping = HashMap::new();
    for id in roots {
        let command = Command::DuplicateNode { id: *id };
        let copy = command
            .apply(&mut trial)
            .map_err(|e| e.to_string())?
            .ok_or("Copy did not create artwork.")?;
        mapping.extend(doc.subtree(*id).into_iter().zip(trial.subtree(copy)));
        prefix.push(command);
        copied.push(copy);
    }
    let mut links: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for node in &doc.nodes {
        if included.contains(&node.id)
            && let Some(group) = node.link_group
        {
            links.entry(group).or_default().push(mapping[&node.id]);
        }
    }
    for ids in links.into_values().filter(|ids| ids.len() > 1) {
        // SetLayerLinks intentionally deduplicates parent/child selection. A
        // linked ancestor+descendant forest cannot be faithfully reconstructed.
        if emulsion_core::layer_links::selected_roots(&trial, &ids)
            .map_err(|e| e.to_string())?
            .len()
            != ids.len()
        {
            return Err("Copying linked ancestors and descendants is not supported.".into());
        }
        let command = Command::SetLayerLinks { ids, linked: true };
        command.apply(&mut trial).map_err(|e| e.to_string())?;
        prefix.push(command);
    }
    Ok((prefix, copied))
}

fn identity(delta: DAffine2) -> bool {
    delta
        .to_cols_array()
        .iter()
        .zip(DAffine2::IDENTITY.to_cols_array())
        .all(|(a, b)| (*a - b).abs() < 1e-10)
}

fn transform_command(
    doc: &Document,
    targets: &[NodeId],
    delta: DAffine2,
) -> Result<Command, String> {
    if !delta.is_finite() || delta.matrix2.determinant().abs() < 1e-10 {
        return Err("The transform must be finite and invertible.".into());
    }
    artwork_targets(doc, targets)?;
    let a = delta.matrix2.x_axis;
    let b = delta.matrix2.y_axis;
    let conformal = a.dot(b).abs() <= 1e-7 * a.length() * b.length()
        && (a.length() - b.length()).abs() <= 1e-7 * a.length().max(b.length());
    if !conformal
        && targets.iter().flat_map(|id| doc.subtree(*id)).any(|id| {
            doc.node(id).is_some_and(|n| match &n.kind {
                NodeKind::Strokes { .. } => true,
                NodeKind::Path { style, .. } => style.width > 0.,
                _ => false,
            })
        })
    {
        return Err("Use proportional scaling for stroked paths; anisotropic stroke deformation is not supported.".into());
    }
    Ok(Command::TransformNodes {
        ids: targets.to_vec(),
        transform: delta.to_cols_array(),
    })
}

fn component_transform_command(
    doc: &Document,
    basis: (NodeId, DAffine2, [f64; 6]),
    target: MaskEditTarget,
    delta: DAffine2,
) -> Result<Command, String> {
    let (id, local, original) = basis;
    if !target.is_mask()
        || doc.node(id).is_none_or(|n| !target.exists(n))
        || doc.locked_ancestor(id).is_some()
        || doc.layer_locks(id).position
    {
        return Err("The mask is locked or no longer available.".into());
    }
    let command = target.transform_command(
        id,
        (local.inverse() * delta * local * DAffine2::from_cols_array(&original)).to_cols_array(),
    );
    command.apply(&mut doc.clone()).map_err(|e| e.to_string())?;
    Ok(command)
}

/// Validate against a local clone before beginning/publishing any operation.
fn operation_commands(
    doc: &Document,
    prefix: &[Command],
    targets: &[NodeId],
    delta: DAffine2,
) -> Result<Vec<Command>, String> {
    let mut trial = doc.clone();
    for command in prefix {
        command.apply(&mut trial).map_err(|e| e.to_string())?;
    }
    let command = transform_command(&trial, targets, delta)?;
    command.apply(&mut trial).map_err(|e| e.to_string())?;
    let mut commands = prefix.to_vec();
    if !identity(delta) {
        commands.push(command);
    }
    Ok(commands)
}

impl EditorView {
    pub(crate) fn photo_transform_active(&self) -> bool {
        self.photo_transform.active.is_some()
    }

    pub(super) fn photo_transform_preview_unchanged(&self) -> bool {
        self.photo_transform.active.as_ref().is_some_and(|s| {
            s.page == self.editor.active_page() && s.revision == self.editor.revision
        })
    }

    pub(super) fn note_photo_reflow(
        &mut self,
        id: NodeId,
        spec: Arc<emulsion_core::text::TextSpec>,
    ) {
        self.photo_transform.reflow_before.get_or_insert((id, spec));
    }

    pub(super) fn finish_photo_reflow(&mut self) {
        if let Some((id, before)) = self.photo_transform.reflow_before.take()
            && self.editor.doc.node(id).is_some_and(|n| {
                matches!(&n.kind,
                NodeKind::Text { spec, .. } if spec != &before)
            })
        {
            self.clear_photo_transform_repeat();
        }
    }

    pub(super) fn cancel_photo_reflow(&mut self) {
        self.photo_transform.reflow_before = None;
    }

    pub(crate) fn photo_transform_ready(&mut self, cx: &mut Context<Self>) -> bool {
        if self.photo_transform_active() || self.editor.in_preview() {
            self.set_status(t!("editor.photo_transform.finish_first"), false, cx);
            false
        } else {
            true
        }
    }

    pub(crate) fn photo_transform_shortcut_ready(&self, window: &Window) -> bool {
        self.is_photo_workflow()
            && (self.canvas_focus.is_focused(window) || self.panel_focus.is_focused(window))
            && self.type_tool.field.is_none()
            && self.renaming.is_none()
    }

    pub(crate) fn begin_photo_transform(&mut self, copy: bool, cx: &mut Context<Self>) {
        if self.refuse_projective_tool("Affine transform preview", cx) {
            return;
        }
        if !self.is_photo_workflow() || !self.photo_transform_ready(cx) {
            return;
        }
        if copy && self.tools.mask_edit_target.is_mask() {
            self.set_status("Mask transform copies are not supported.", true, cx);
            return;
        }
        if self.editor.in_transaction()
            || self.drag.is_some()
            || self.warp.is_some()
            || self.assistant.running
            || self.pages_ui.export_pending
            || self.raw.is_pending()
            || self.editor.is_read_only()
        {
            self.set_status("Finish the current edit before transforming.", true, cx);
            return;
        }
        self.close_text_field(cx);
        let original_target = self.tools.mask_edit_target;
        let mask_basis = if original_target.is_mask() {
            let Some((id, _)) = self.mask_transform_target() else {
                self.set_status("Select an unlocked mask.", true, cx);
                return;
            };
            let node = self.editor.doc.node(id).unwrap();
            Some((
                id,
                match super::transform::affine_tool_mapping(node) {
                    Ok(mapping) => mapping,
                    Err(error) => {
                        self.set_status(error.to_string(), true, cx);
                        return;
                    }
                },
                original_target.affine(node).expect("captured mask target"),
            ))
        } else {
            None
        };
        let prepared = (|| {
            if let Some((id, _, _)) = mask_basis {
                return Ok((Vec::new(), vec![id]));
            }

            let roots = artwork_targets(&self.editor.doc, &self.selected_layer_ids())?;
            if self.editor.doc.selection.is_some() {
                if roots.len() != 1 {
                    return Err("Select one pixel layer to transform selected pixels.".into());
                }
                self.photo_pixel_transform_prefix(copy)
            } else if copy {
                copy_prefix(&self.editor.doc, &roots)
            } else {
                Ok((Vec::new(), roots))
            }
        })();
        let (prefix, targets) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                self.set_status(message, true, cx);
                return;
            }
        };
        let commands = match if let Some(basis) = mask_basis {
            component_transform_command(
                &self.editor.doc,
                basis,
                original_target,
                DAffine2::IDENTITY,
            )
            .map(|_| Vec::new())
        } else {
            operation_commands(&self.editor.doc, &prefix, &targets, DAffine2::IDENTITY)
        } {
            Ok(commands) => commands,
            Err(message) => {
                self.set_status(message, true, cx);
                return;
            }
        };
        let original_selection = self.selected_layer_ids();
        let original_active = self.selected;
        let original_tool = self.tool;
        self.set_tool(Tool::Move, cx);
        self.invalidate_pending_edits();
        if let Err(error) = self.editor.begin_preview(if copy {
            "Duplicate and transform"
        } else {
            "Free Transform"
        }) {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        if let Err(error) = self.editor.preview_commands(&commands) {
            self.editor.cancel_preview();
            self.set_status(error.to_string(), true, cx);
            return;
        }
        self.set_layer_selection(targets.clone(), targets.last().copied());
        self.tools.mask_edit_target = original_target;
        self.after_change(cx);
        self.photo_transform.active = Some(PhotoTransformSession {
            duplicate: copy,
            page: self.editor.active_page(),
            revision: self.editor.revision,
            epoch: self.operation_epoch,
            prefix,
            targets,
            original_selection,
            original_active,
            original_tool,
            original_target,
            mask_basis,
            delta: DAffine2::IDENTITY,
            gesture_base: DAffine2::IDENTITY,
            invalid: false,
        });
        self.set_status(t!("editor.photo_transform.hint"), false, cx);
    }

    fn photo_session_current(&self) -> bool {
        self.photo_transform.active.as_ref().is_some_and(|s| {
            s.page == self.editor.active_page()
                && s.revision == self.editor.revision
                && s.epoch == self.operation_epoch
                && self.editor.in_preview()
        })
    }

    /// Each pointer gesture starts from the previously accepted cumulative D.
    pub(super) fn photo_transform_begin_gesture(&mut self) {
        if let Some(s) = &mut self.photo_transform.active {
            s.gesture_base = s.delta;
        }
    }

    pub(super) fn photo_transform_gesture(
        &mut self,
        delta: DAffine2,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(s) = &self.photo_transform.active else {
            return false;
        };
        let total = delta * s.gesture_base;
        self.preview_photo_transform(total, cx)
    }

    pub(crate) fn photo_transform_delta(
        &mut self,
        delta: DAffine2,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(s) = &self.photo_transform.active else {
            return false;
        };
        self.preview_photo_transform(delta * s.delta, cx)
    }

    fn preview_photo_transform(&mut self, total: DAffine2, cx: &mut Context<Self>) -> bool {
        if !self.photo_session_current() {
            // Never restore a stale baseline over a changed page/document.
            self.set_status(
                "The transform was interrupted. Cancel it before continuing.",
                true,
                cx,
            );
            if let Some(s) = &mut self.photo_transform.active {
                s.invalid = true;
            }
            return false;
        }
        let s = self
            .photo_transform
            .active
            .as_ref()
            .expect("active session");
        // Capability checks use the ORIGINAL prefix document via baseline replay;
        // core TransformNodes validates each resulting editable placement.
        let command = match if let Some(basis) = s.mask_basis {
            component_transform_command(&self.editor.doc, basis, s.original_target, total)
        } else {
            transform_command(&self.editor.doc, &s.targets, total)
        } {
            Ok(command) => command,
            Err(message) => {
                self.photo_transform.active.as_mut().unwrap().invalid = true;
                self.set_status(message, true, cx);
                return false;
            }
        };
        let mut commands = s.prefix.clone();
        if !identity(total) {
            commands.push(command);
        }
        match self.editor.preview_commands(&commands) {
            Ok(_) => {
                self.after_change(cx);
                let s = self.photo_transform.active.as_mut().unwrap();
                s.delta = total;
                s.revision = self.editor.revision;
                s.epoch = self.operation_epoch;
                s.invalid = false;
                self.status = None;
                true
            }
            Err(error) => {
                self.photo_transform.active.as_mut().unwrap().invalid = true;
                self.set_status(error.to_string(), true, cx);
                false
            }
        }
    }

    pub(crate) fn commit_photo_transform(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(s) = &self.photo_transform.active else {
            return false;
        };
        if s.invalid || !self.photo_session_current() {
            self.set_status(t!("editor.photo_transform.invalid"), true, cx);
            return true;
        }
        if !s.duplicate && identity(s.delta) {
            self.cancel_photo_transform(cx);
            self.status = None;
            return true;
        }
        let s = self.photo_transform.active.take().unwrap();
        self.transform_control_mode = TransformControlMode::Resize;
        self.drag = None;
        self.snap_lines.clear();
        self.editor.commit_preview();
        if !identity(s.delta) {
            self.photo_transform.last = Some(RepeatTransform {
                target: s.original_target,
                page: s.page,
                delta: s.delta,
            });
        }
        self.after_change(cx);
        self.status = None;
        true
    }

    pub(crate) fn cancel_photo_transform(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(s) = self.photo_transform.active.take() else {
            return false;
        };
        self.drag = None;
        self.snap_lines.clear();
        if s.page == self.editor.active_page()
            && s.revision == self.editor.revision
            && self.editor.in_preview()
        {
            self.editor.cancel_preview();
            self.set_layer_selection(s.original_selection, s.original_active);
            self.set_tool(s.original_tool, cx);
            self.tools.mask_edit_target = s.original_target;
        } else {
            // A core Undo may already have canceled it. Never undo unrelated work.
            self.set_status(
                "The transform was interrupted; newer work was retained.",
                true,
                cx,
            );
        }
        self.invalidate_pending_edits();
        self.after_change(cx);
        true
    }

    pub(crate) fn repeat_photo_transform(&mut self, copy: bool, cx: &mut Context<Self>) {
        if !self.is_photo_workflow() || !self.photo_transform_ready(cx) {
            return;
        }
        if copy && self.tools.mask_edit_target.is_mask() {
            self.set_status("Again with Copy is not supported for masks.", true, cx);
            return;
        }
        let Some(recipe) = self
            .photo_transform
            .last
            .filter(|r| r.page == self.editor.active_page())
        else {
            self.set_status(t!("editor.photo_transform.no_recipe"), false, cx);
            return;
        };
        if recipe.target != self.tools.mask_edit_target {
            self.set_status(
                "The last transform belongs to a different editing target.",
                false,
                cx,
            );
            return;
        }
        if self.editor.in_transaction()
            || self.drag.is_some()
            || self.warp.is_some()
            || self.assistant.running
            || self.pages_ui.export_pending
            || self.raw.is_pending()
            || self.editor.is_read_only()
        {
            self.set_status(
                "Finish the current edit and select artwork before using Again.",
                true,
                cx,
            );
            return;
        }
        let target = self.tools.mask_edit_target;
        if target.is_mask() && self.refuse_projective_tool("Again on mask", cx) {
            return;
        }
        let prepared = (|| {
            if target.is_mask() {
                let (id, _) = self
                    .mask_transform_target()
                    .ok_or("Select an unlocked mask.")?;
                let node = self.editor.doc.node(id).unwrap();
                let basis = (
                    id,
                    super::transform::affine_tool_mapping(node).map_err(|e| e.to_string())?,
                    target.affine(node).expect("captured mask target"),
                );
                return Ok((
                    vec![component_transform_command(
                        &self.editor.doc,
                        basis,
                        target,
                        recipe.delta,
                    )?],
                    vec![id],
                ));
            }

            let roots = artwork_targets(&self.editor.doc, &self.selected_layer_ids())?;
            let (prefix, targets) = if self.editor.doc.selection.is_some() {
                if roots.len() != 1 {
                    return Err("Select one pixel layer to transform selected pixels.".into());
                }
                self.photo_pixel_transform_prefix(copy)?
            } else if copy {
                copy_prefix(&self.editor.doc, &roots)?
            } else {
                (Vec::new(), roots)
            };
            let commands = operation_commands(&self.editor.doc, &prefix, &targets, recipe.delta)?;
            Ok::<_, String>((commands, targets))
        })();
        let (commands, targets) = match prepared {
            Ok(prepared) => prepared,
            Err(message) => {
                self.set_status(message, true, cx);
                return;
            }
        };
        self.invalidate_pending_edits();
        if let Err(error) = self.editor.begin_preview(if copy {
            "Transform Again with Copy"
        } else {
            "Transform Again"
        }) {
            self.set_status(error.to_string(), true, cx);
            return;
        }
        match self.editor.preview_commands(&commands) {
            Ok(_) => {
                self.editor.commit_preview();
                self.set_layer_selection(targets.clone(), targets.last().copied());
                self.tools.mask_edit_target = target;
                self.after_change(cx);
                self.status = None;
            }
            Err(error) => {
                self.editor.cancel_preview();
                self.set_status(error.to_string(), true, cx);
            }
        }
    }

    pub(super) fn photo_transform_controls(
        &self,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        vec![
            chip("photo-transform-apply", t!("editor.tools.apply"), true, p)
                .test_support()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.commit_photo_transform(cx);
                    window.focus(&this.canvas_focus, cx);
                }))
                .into_any_element(),
            chip(
                "photo-transform-cancel",
                t!("editor.tools.cancel"),
                false,
                p,
            )
            .test_support()
            .on_click(cx.listener(|this, _, window, cx| {
                this.cancel_photo_transform(cx);
                window.focus(&this.canvas_focus, cx);
            }))
            .into_any_element(),
        ]
    }

    pub(super) fn invalidate_photo_transform_preview(&mut self) {
        if let Some(s) = &mut self.photo_transform.active {
            s.invalid = true;
        }
    }

    pub(super) fn clear_photo_transform_repeat(&mut self) {
        self.photo_transform.last = None;
    }
}
