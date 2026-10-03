//! Shot Generator edits on storyboard panels (see `storyboard_shot`): set
//! edits with the layers that follow them, the reference layer, snapshots
//! and model import, each one project Undo step.
use super::{PageId, ProjectEditor};
use crate::history::edit_order;
use crate::storyboard::Storyboard;
use crate::storyboard_shot::{
    PanelShot, REFERENCE_LAYER, SNAPSHOT_LAYER, ShotLibrary, image_raster,
};
use crate::{Command, Document, Node, NodeId, NodeKind, command::Slot};
use emulsion_raster::Placement;
use emulsion_scene::{ObjectId, ObjectKind, Prop, RgbaImage, Scene, prepare};
use glam::Vec3;
use std::collections::BTreeMap;
use std::sync::Arc;

/// What `describe_panel_shot` understood.
#[derive(Clone, Debug, PartialEq)]
pub struct DescribedShot {
    /// A one-line summary of the shot that was built.
    pub interpretation: String,
    /// Words the offline parser did not know.
    pub unrecognized: Vec<String>,
    /// The reference layer should be rendered again.
    pub stale: bool,
}

impl ProjectEditor {
    /// `panel`'s Shot Generator set.
    pub fn panel_shot(&self, panel: PageId) -> Option<&PanelShot> {
        self.storyboard()?.panels.get(&panel)?.shot.as_deref()
    }

    /// The board's models, parsed for rendering.
    pub fn shot_assets(&self) -> emulsion_scene::AssetLibrary {
        self.storyboard()
            .map(|b| b.shot_library.assets())
            .unwrap_or_default()
    }

    /// Edit `panel`'s set (made empty first when the panel has none) and the
    /// shot library as one Undo step labelled `label`. Layers attached to the
    /// set follow it in the same step, and models no set uses any more are
    /// dropped. Returns whether the reference layer is now out of date and
    /// should be rendered again (`set_shot_reference` with `join`).
    pub fn edit_panel_shot(
        &mut self,
        panel: PageId,
        label: &str,
        edit: impl FnOnce(&mut PanelShot, &mut ShotLibrary) -> Result<(), String>,
    ) -> Result<bool, String> {
        self.edit_panel_shot_and_doc(panel, label, |shot, library, _| edit(shot, library))
    }

    /// [`Self::edit_panel_shot`] that may also change the panel's drawing.
    fn edit_panel_shot_and_doc(
        &mut self,
        panel: PageId,
        label: &str,
        edit: impl FnOnce(&mut PanelShot, &mut ShotLibrary, &mut Document) -> Result<(), String>,
    ) -> Result<bool, String> {
        let board = self.board()?.clone();
        let size = (board.settings.width, board.settings.height);
        let before = board
            .panels
            .get(&panel)
            .ok_or("Panel does not exist.")?
            .shot
            .clone();
        let mut shot = before
            .as_deref()
            .cloned()
            .unwrap_or_else(|| PanelShot::new(board.aspect()));
        let mut next = Storyboard::clone(&board);
        let mut library = std::mem::take(&mut next.shot_library);
        let drawn = &self.page(panel).ok_or("Panel does not exist.")?.doc;
        let mut doc = drawn.clone();
        edit(&mut shot, &mut library, &mut doc)?;
        shot.set.normalize();
        shot.validate()?;
        let set_changed = before.as_ref().is_none_or(|b| b.set != shot.set);
        let attached = before
            .as_ref()
            .is_none_or(|b| b.attachments != shot.attachments);
        if (set_changed || attached) && !shot.attachments.is_empty() {
            let prepared = prepare(&shot.set, &library.assets()).map_err(|e| e.to_string())?;
            shot.follow(&prepared, &mut doc, size)?;
        }
        let mut documents = BTreeMap::new();
        if doc != *drawn {
            documents.insert(panel, doc);
        }
        let stale = set_changed
            && shot.reference.auto_update
            && self.reference_layer(panel, &shot).is_some();
        next.shot_library = library;
        next.panels.get_mut(&panel).unwrap().shot = Some(Box::new(shot));
        next.prune_models();
        self.commit_shot(next, documents, label, None, &[])?;
        Ok(stale)
    }

    /// Replace `panel`'s set with one built from a description such as
    /// "low-angle close-up of two people at a table" (SG6), as one Undo
    /// step. Layers stop following the old set.
    pub fn describe_panel_shot(
        &mut self,
        panel: PageId,
        text: &str,
    ) -> Result<DescribedShot, String> {
        let aspect = self.board()?.aspect();
        let generated = emulsion_scene::text_to_shot(text, aspect).map_err(|e| e.to_string())?;
        let stale = self.edit_panel_shot(panel, "Describe a shot", |shot, _| {
            shot.set = generated.scene.clone();
            shot.attachments.clear();
            Ok(())
        })?;
        Ok(DescribedShot {
            interpretation: generated.interpretation,
            unrecognized: generated.description.unrecognized,
            stale,
        })
    }

    /// Remove `panel`'s set. Its layers stay as they are.
    pub fn remove_panel_shot(&mut self, panel: PageId) -> Result<(), String> {
        self.edit_storyboard(|b| {
            b.panels
                .get_mut(&panel)
                .ok_or("Panel does not exist.")?
                .shot = None;
            b.prune_models();
            Ok(())
        })
    }

    /// The reference layer `shot` renders into on `panel`, if it is there.
    fn reference_layer(&self, panel: PageId, shot: &PanelShot) -> Option<NodeId> {
        let doc = &self.page(panel)?.doc;
        shot.layer.filter(|id| doc.node(*id).is_some()).or_else(|| {
            doc.nodes
                .iter()
                .find(|n| n.parent.is_none() && n.name == REFERENCE_LAYER)
                .map(|n| n.id)
        })
    }

    /// `panel`'s set rendered at the panel's resolution for its reference
    /// layer, with the set it shows (rendering can run off the UI thread
    /// from the same inputs: `PanelShot::render_reference`).
    pub fn render_panel_shot(&self, panel: PageId) -> Result<(RgbaImage, Scene), String> {
        let board = self.board()?;
        let shot = self
            .panel_shot(panel)
            .ok_or("That panel has no Shot Generator set.")?;
        let image = shot.render_reference(
            &board.shot_library.assets(),
            board.settings.width,
            board.settings.height,
        )?;
        Ok((image, shot.set.clone()))
    }

    /// Put `image` (the panel's set `rendered`, at the panel's resolution)
    /// in `panel`'s reference layer, replacing the one before or adding it
    /// just above the paper, locked and at the set's reference opacity.
    /// Refused when the set changed since it was rendered. With `join`, it
    /// joins the Undo step of the set edit it follows when that is still the
    /// last thing done. Returns the layer.
    pub fn set_shot_reference(
        &mut self,
        panel: PageId,
        image: &RgbaImage,
        rendered: &Scene,
        join: bool,
    ) -> Result<NodeId, String> {
        let board = self.board()?.clone();
        let shot = self
            .panel_shot(panel)
            .ok_or("That panel has no Shot Generator set.")?
            .clone();
        if shot.set != *rendered {
            return Err("The set changed while it was rendering.".into());
        }
        if (image.width, image.height) != (board.settings.width, board.settings.height) {
            return Err("The reference must be rendered at the panel's size.".into());
        }
        let raster = image_raster(image);
        let mut doc = self.page(panel).ok_or("Panel does not exist.")?.doc.clone();
        let id = match self.reference_layer(panel, &shot) {
            Some(id) => {
                crate::motion::with_layers_unlocked(&mut doc, |doc| {
                    Command::ReplaceContent {
                        id,
                        raster,
                        mask: None,
                        placement: Placement::default(),
                        label: "Shot Generator reference".into(),
                    }
                    .apply(doc)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
                })?;
                id
            }
            None => {
                let mut node = Node::raster(0, REFERENCE_LAYER, raster, Placement::default());
                node.locked = true;
                Command::AddNode {
                    node: Box::new(node),
                    slot: above_paper(&doc),
                }
                .apply(&mut doc)
                .map_err(|e| e.to_string())?;
                doc.nodes
                    .iter()
                    .filter(|n| n.name == REFERENCE_LAYER)
                    .map(|n| n.id)
                    .max()
                    .ok_or("The reference layer could not be added.")?
            }
        };
        if let Some(node) = doc.node_mut(id) {
            node.opacity = shot.reference.opacity;
            node.locked = true;
        }
        let mut next = Storyboard::clone(&board);
        next.panels
            .get_mut(&panel)
            .unwrap()
            .shot
            .as_mut()
            .unwrap()
            .layer = Some(id);
        let join = join.then_some(panel);
        self.commit_shot(
            next,
            BTreeMap::from([(panel, doc)]),
            "Shot Generator reference",
            join,
            &[id],
        )?;
        Ok(id)
    }

    /// Render `panel`'s set and put it in its reference layer, joining the
    /// set edit just made (for tools that edit and render in one go).
    pub fn update_shot_reference(&mut self, panel: PageId) -> Result<NodeId, String> {
        let (image, set) = self.render_panel_shot(panel)?;
        self.set_shot_reference(panel, &image, &set, true)
    }

    /// Put `image` (a render of `panel`'s set at the panel's size) on the
    /// panel as a new editable layer at the top (C13). One Undo step.
    pub fn snapshot_shot(&mut self, panel: PageId, image: &RgbaImage) -> Result<NodeId, String> {
        let board = self.board()?;
        if (image.width, image.height) != (board.settings.width, board.settings.height) {
            return Err("The snapshot must be rendered at the panel's size.".into());
        }
        let mut doc = self.page(panel).ok_or("Panel does not exist.")?.doc.clone();
        let before = doc.nodes.iter().map(|n| n.id).max().unwrap_or(0);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                SNAPSHOT_LAYER,
                image_raster(image),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .map_err(|e| e.to_string())?;
        let id = doc
            .nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| *id > before)
            .max()
            .ok_or("The snapshot layer could not be added.")?;
        self.edit_panels_checked(panel, doc, "Shot Generator snapshot")?;
        Ok(id)
    }

    /// Import a model file into the project and place it in `panel`'s set
    /// at `position` (C7), as one Undo step. Returns the new object.
    pub fn import_shot_model(
        &mut self,
        panel: PageId,
        file_name: &str,
        bytes: Vec<u8>,
        position: Vec3,
    ) -> Result<ObjectId, String> {
        let mut object = None;
        self.edit_panel_shot(panel, "Import model", |shot, library| {
            let asset = library.add_model(file_name, bytes)?;
            let name = library.models[&asset].name.clone();
            let id = shot.set.add_prop(
                &name,
                Prop::Model(emulsion_scene::ModelRef {
                    asset,
                    joint_rotations: BTreeMap::new(),
                }),
                position,
                0.,
            );
            object = Some(id);
            Ok(())
        })?;
        object.ok_or_else(|| "The model could not be placed.".into())
    }

    /// Tie layer `node` of `panel` to `object` (or its `bone`) at the world
    /// `point` (the attachment's origin when `None`), so it follows the set
    /// (C12). With `normal` (the surface's normal at `point`, as picked),
    /// the layer is laid on the surface instead: warped onto its plane as
    /// the set's camera sees it, from a hidden copy of its flat drawing.
    /// One Undo step.
    pub fn attach_layer_to_shot(
        &mut self,
        panel: PageId,
        node: NodeId,
        object: ObjectId,
        bone: Option<emulsion_scene::Bone>,
        point: Option<Vec3>,
        normal: Option<Vec3>,
    ) -> Result<(), String> {
        let doc = &self.page(panel).ok_or("Panel does not exist.")?.doc;
        if doc.node(node).is_none() {
            return Err("That layer is not on the panel.".into());
        }
        let board = self.board()?;
        let size = (board.settings.width, board.settings.height);
        let shot = self
            .panel_shot(panel)
            .ok_or("That panel has no Shot Generator set.")?;
        if !matches!(
            shot.set.object(object).map(|o| &o.kind),
            Some(ObjectKind::Character(_) | ObjectKind::Prop(_))
        ) {
            return Err("Attach layers to characters and props.".into());
        }
        let prepared =
            prepare(&shot.set, &board.shot_library.assets()).map_err(|e| e.to_string())?;
        let label = if normal.is_some() {
            "Lay layer on surface"
        } else {
            "Attach layer to set"
        };
        self.edit_panel_shot_and_doc(panel, label, |shot, _, doc| {
            // A layer laid on a surface again keeps its flat drawing.
            let kept = shot
                .attachments
                .get(&node)
                .and_then(|a| a.surface)
                .map(|f| f.flat)
                .filter(|flat| doc.node(*flat).is_some());
            let surface = match normal {
                Some(normal) => Some((
                    normal,
                    match kept {
                        Some(flat) => flat,
                        None => PanelShot::add_flat_layer(doc, node)?,
                    },
                )),
                None => None,
            };
            shot.attach(&prepared, node, object, bone, point, surface, size)
        })?;
        Ok(())
    }

    /// What `panel`'s set camera sees at panel pixel (`x`, `y`): the
    /// object (and bone), the point and the surface's normal there, for
    /// laying a layer on that surface.
    pub fn pick_panel_shot(
        &self,
        panel: PageId,
        x: f32,
        y: f32,
    ) -> Result<emulsion_scene::PickHit, String> {
        let board = self.board()?;
        let shot = self
            .panel_shot(panel)
            .ok_or("That panel has no Shot Generator set.")?;
        let prepared =
            prepare(&shot.set, &board.shot_library.assets()).map_err(|e| e.to_string())?;
        let (w, h) = (board.settings.width, board.settings.height);
        emulsion_scene::pick(&prepared, &shot.set.camera, w, h, x, y)
            .ok_or_else(|| "No object of the set shows there.".into())
    }

    /// Stop layer `node` following `panel`'s set.
    pub fn detach_layer_from_shot(&mut self, panel: PageId, node: NodeId) -> Result<(), String> {
        self.edit_panel_shot(panel, "Detach layer from set", |shot, _| {
            shot.attachments
                .remove(&node)
                .map(|_| ())
                .ok_or_else(|| "That layer does not follow the set.".into())
        })
        .map(|_| ())
    }

    /// Commit one panel's drawing as an Undo step, refused on a locked panel.
    fn edit_panels_checked(
        &mut self,
        panel: PageId,
        doc: Document,
        label: &str,
    ) -> Result<(), String> {
        if self.board()?.is_locked(panel) {
            return Err(crate::CommandError::ReadOnly.to_string());
        }
        self.commit_documents(BTreeMap::from([(panel, doc)]), label)
    }

    /// Whether the last thing done was an edit of `panel`'s set or its
    /// settings, so a reference render of it can join that Undo step.
    fn joins_shot_edit(&self, panel: PageId) -> bool {
        let set = |b: &Storyboard| b.panels.get(&panel).map(|p| p.shot.clone());
        let Some(now) = self.storyboard.as_deref().map(set) else {
            return false;
        };
        self.last_page_edit > 0
            && self.last_page_edit == self.last_fresh_edit()
            && self.undo_pages.last().is_some_and(|step| {
                step.order == self.last_page_edit
                    && step.storyboard.as_deref().is_some_and(|b| set(b) != now)
            })
    }

    /// Commit `next` and panel `documents` together as one Undo step; with
    /// `join`, into the set edit on that panel just made when possible. The
    /// locked layers `unlocked` may change.
    fn commit_shot(
        &mut self,
        next: Storyboard,
        documents: BTreeMap<PageId, Document>,
        label: &str,
        join: Option<PageId>,
        unlocked: &[NodeId],
    ) -> Result<(), String> {
        let current = self.board()?.clone();
        for id in documents.keys() {
            if current.is_locked(*id) {
                return Err(crate::CommandError::ReadOnly.to_string());
            }
        }
        let documents: BTreeMap<_, _> = documents
            .into_iter()
            .filter(|(id, doc)| self.page(*id).is_some_and(|e| e.doc != *doc))
            .collect();
        let documents = self.prepare_documents_unlocking(documents, label, unlocked)?;
        if join.is_some_and(|panel| self.joins_shot_edit(panel)) {
            let order: Vec<_> = self.layout.iter().map(|m| m.id).collect();
            current.check_locks_kept(&next)?;
            next.validate(&order)?;
            self.storyboard = Some(Arc::new(next));
            self.refresh_locks();
            let order = self.last_page_edit;
            self.apply_documents(documents, label, order);
            return Ok(());
        }
        let before = self.last_page_edit;
        if next != *current {
            self.edit_storyboard(|b| {
                *b = next;
                Ok(())
            })?;
        }
        let order = if self.last_page_edit != before {
            self.last_page_edit
        } else {
            edit_order()
        };
        self.apply_documents(documents, label, order);
        Ok(())
    }
}

/// Just above the panel's paper (a bottom fill layer), else at the bottom.
fn above_paper(doc: &Document) -> Slot {
    let roots = doc.children(None);
    let paper = roots
        .first()
        .and_then(|id| doc.node(*id))
        .is_some_and(|n| matches!(n.kind, NodeKind::Fill { .. }));
    Slot {
        parent: None,
        index: usize::from(paper),
    }
}

#[cfg(test)]
#[path = "panel_shot_tests.rs"]
mod tests;
