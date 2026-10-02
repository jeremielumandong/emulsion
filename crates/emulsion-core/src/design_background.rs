//! Page backgrounds are ordinary native nodes. These IDs identify their role;
//! the regular compositor, clipboard and exporters remain the source of truth.
use crate::{Command, Document, Editor, Node, NodeId, NodeKind, command::Slot};
use emulsion_raster::{BlendMode, Raster, vector::PathStyle, vector_geometry};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageBackground {
    pub fill: NodeId,
    pub image: Option<PageBackgroundImage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageBackgroundImage {
    pub group: NodeId,
    pub boundary: NodeId,
    pub image: NodeId,
}

impl PageBackground {
    pub(crate) fn retain(&mut self, ids: &HashSet<NodeId>) -> bool {
        if self.image.is_some_and(|image| {
            ![image.group, image.boundary, image.image]
                .iter()
                .all(|id| ids.contains(id))
        }) {
            self.image = None;
        }
        ids.contains(&self.fill)
    }

    pub(crate) fn remap(self, ids: &HashMap<NodeId, NodeId>) -> Self {
        let id = |id| ids.get(&id).copied().unwrap_or(id);
        Self {
            fill: id(self.fill),
            image: self.image.map(|image| PageBackgroundImage {
                group: id(image.group),
                boundary: id(image.boundary),
                image: id(image.image),
            }),
        }
    }
}

fn plain(node: &Node) -> bool {
    node.visible
        && node.opacity == 1.
        && node.blend == BlendMode::Normal
        && node.blending == Default::default()
        && node.mask.is_none()
        && node.styles.is_empty()
        && node.clip_to.is_none()
        && node.link_group.is_none()
}

/// Recognize old template backgrounds by their native bottom-root Fill, never
/// by a translated/editable name. A styled or masked Fill remains user artwork.
pub fn parts(doc: &Document) -> Option<PageBackground> {
    doc.design.page_background.or_else(|| {
        let fill = *doc.children(None).first()?;
        let node = doc.node(fill)?;
        (matches!(node.kind, NodeKind::Fill { .. }) && plain(node))
            .then_some(PageBackground { fill, image: None })
    })
}

pub fn color(doc: &Document) -> [u8; 4] {
    parts(doc)
        .and_then(|background| doc.node(background.fill))
        .and_then(|node| match node.kind {
            NodeKind::Fill { rgba } => Some(rgba),
            _ => None,
        })
        .unwrap_or([0; 4])
}

pub fn is_background_node(doc: &Document, id: NodeId) -> bool {
    parts(doc).is_some_and(|background| {
        background.fill == id
            || background
                .image
                .is_some_and(|image| [image.group, image.boundary, image.image].contains(&id))
    })
}

/// First root slot available for ordinary artwork, counted bottom to top.
pub fn foreground_start(doc: &Document) -> usize {
    parts(doc).map_or(0, |background| 1 + usize::from(background.image.is_some()))
}

fn valid_fill(doc: &Document, fill: NodeId) -> bool {
    doc.node(fill)
        .is_some_and(|node| node.parent.is_none() && matches!(node.kind, NodeKind::Fill { .. }))
}

fn valid_image(doc: &Document, image: PageBackgroundImage) -> bool {
    doc.node(image.group)
        .is_some_and(|node| node.parent.is_none() && node.is_group())
        && doc.node(image.boundary).is_some_and(|node| {
            node.parent == Some(image.group)
                && node.clip_to.is_none()
                && matches!(node.kind, NodeKind::Path { .. })
        })
        && doc.node(image.image).is_some_and(|node| {
            node.parent == Some(image.group)
                && node.clip_to == Some(image.boundary)
                && matches!(node.kind, NodeKind::Raster { .. })
        })
        && doc.children(Some(image.group)) == [image.boundary, image.image]
}

pub(crate) fn validate(doc: &Document, background: PageBackground) -> Result<(), String> {
    if !valid_fill(doc, background.fill)
        || background
            .image
            .is_some_and(|image| !valid_image(doc, image))
    {
        return Err("Page background settings need their native fill and image frame.".into());
    }
    let roots = doc.children(None);
    if roots.first() != Some(&background.fill)
        || background
            .image
            .is_some_and(|image| roots.get(1) != Some(&image.group))
    {
        return Err("Page backgrounds must remain below the page's other objects.".into());
    }
    Ok(())
}

/// Advanced layer edits can detach/delete any part of a background. Drop only
/// invalid role metadata, never user nodes. Existing valid backgrounds stay
/// pinned beneath normal artwork even when that artwork is sent to the back.
pub(crate) fn pin(doc: &mut Document) {
    let Some(mut background) = doc.design.page_background else {
        return;
    };
    if !valid_fill(doc, background.fill) {
        doc.design.page_background = None;
        return;
    }
    if background
        .image
        .is_some_and(|image| !valid_image(doc, image))
    {
        background.image = None;
    }
    doc.design.page_background = Some(background);
    let roots = doc.children(None);
    if roots.first() == Some(&background.fill)
        && background
            .image
            .is_none_or(|image| roots.get(1) == Some(&image.group))
    {
        return;
    }
    let mut pinned = HashSet::from([background.fill]);
    if let Some(image) = background.image {
        pinned.extend([image.group, image.boundary, image.image]);
    }
    let nodes = std::mem::take(&mut doc.nodes);
    // Move whole root blocks. All other nodes retain their relative order.
    let (mut bottom, rest): (Vec<_>, Vec<_>) = nodes
        .into_iter()
        .partition(|node| pinned.contains(&node.id));
    bottom.sort_by_key(|node| if node.id == background.fill { 0 } else { 1 });
    bottom.extend(rest);
    doc.nodes = bottom;
    doc.normalize();
}

fn apply(doc: &mut Document, command: Command) -> Result<Option<NodeId>, String> {
    command.apply(doc).map_err(|error| error.to_string())
}

/// Use inside an existing caller-owned paste transaction. The copied page's
/// background remains copied artwork; it must not acquire the destination role.
pub(crate) fn ensure_destination(editor: &mut Editor) -> Result<(), String> {
    if parts(&editor.doc).is_some() {
        return Ok(());
    }
    let fill = editor
        .execute(Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Page background",
                NodeKind::Fill { rgba: [0; 4] },
            )),
            slot: Slot {
                parent: None,
                index: 0,
            },
        })
        .map_err(|error| error.to_string())?
        .ok_or("Background was not created")?;
    let mut design = editor.doc.design.clone();
    design.page_background = Some(PageBackground { fill, image: None });
    editor
        .execute(Command::SetDesign {
            design: Box::new(design),
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn prepare(editor: &Editor) -> Result<(Document, PageBackground), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before changing the page background.".into());
    }
    let mut next = editor.doc.clone();
    let background = if let Some(background) = parts(&next) {
        background
    } else {
        let fill = apply(
            &mut next,
            Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Page background",
                    NodeKind::Fill { rgba: [0; 4] },
                )),
                slot: Slot {
                    parent: None,
                    index: 0,
                },
            },
        )?
        .ok_or("Background was not created")?;
        PageBackground { fill, image: None }
    };
    // Only the prepared clone loses role metadata while native commands build
    // the complete replacement. No intermediate state enters session history.
    next.design.page_background = None;
    Ok((next, background))
}

fn commit(
    editor: &mut Editor,
    mut next: Document,
    background: PageBackground,
    label: &str,
) -> Result<(), String> {
    next.design.page_background = Some(background);
    pin(&mut next);
    editor.commit_design_document(next, label)
}

pub fn set_color(editor: &mut Editor, rgba: [u8; 4]) -> Result<NodeId, String> {
    let (mut next, background) = prepare(editor)?;
    apply(
        &mut next,
        Command::SetFillColor {
            id: background.fill,
            rgba,
        },
    )?;
    commit(editor, next, background, "Page background color")?;
    Ok(background.fill)
}

/// Only independent, ordinary raster objects may become backgrounds. Relocating
/// a masked, clipped, styled, linked, animated or nested object can alter other
/// artwork or discard meaning, so those objects need an explicit simplification.
pub fn can_set_image(doc: &Document, id: NodeId) -> Result<(), String> {
    let node = doc.node(id).ok_or("Select an image first.")?;
    if doc.locked_ancestor(id).is_some() || doc.layer_locks(id) != Default::default() {
        return Err("Unlock this image before setting it as the background.".into());
    }
    if !matches!(node.kind, NodeKind::Raster { .. }) {
        return Err("Select a raster image to use as the background.".into());
    }
    if parts(doc)
        .and_then(|background| background.image)
        .is_some_and(|image| image.image == id)
    {
        return crate::design::frame_image_replaceable(doc, id);
    }
    let design = &doc.design;
    let metadata = design.motion.contains_key(&id)
        || design.keyframes.contains_key(&id)
        || design.constraints.contains_key(&id)
        || design.style_links.contains_key(&id)
        || design.component_links.contains_key(&id)
        || design.data_bindings.contains_key(&id)
        || design.variable_bindings.contains_key(&id)
        || design.interactions.contains_key(&id)
        || design.overlays.contains(&id)
        || design.local_media.contains_key(&id)
        || design.media.contains_key(&id);
    if node.parent.is_some()
        || !plain(node)
        || metadata
        || doc.nodes.iter().any(|other| other.clip_to == Some(id))
    {
        return Err("Use an independent image without masks, effects, links or design behaviors as the background.".into());
    }
    Ok(())
}

fn remove_owned_image(doc: &mut Document, background: &mut PageBackground) -> Result<(), String> {
    if let Some(image) = background.image.take() {
        // Refuse stale role metadata rather than deleting any additional objects
        // someone may have placed inside the background group in Layers.
        if !valid_image(doc, image) {
            return Err("The background frame changed. Select its image in Layers first.".into());
        }
        apply(doc, Command::RemoveNode { id: image.group })?;
    }
    Ok(())
}

fn add_image_frame(doc: &mut Document, image: NodeId) -> Result<PageBackgroundImage, String> {
    let group = apply(
        doc,
        Command::AddNode {
            node: Box::new(Node::group(0, "Page background image")),
            slot: Slot {
                parent: None,
                index: 1,
            },
        },
    )?
    .ok_or("Background frame was not created")?;
    let mut boundary = Node::path(
        0,
        "Page background boundary",
        Arc::new(vector_geometry::rectangle(
            0.,
            0.,
            f64::from(doc.width),
            f64::from(doc.height),
        )),
        PathStyle {
            fill: Some([255; 4]),
            stroke: None,
            ..Default::default()
        },
        doc.width,
        doc.height,
    );
    // Clip coverage comes from the native vector source's alpha, before layer
    // opacity. Hide the boundary's paint so image transparency reveals the Fill.
    boundary.opacity = 0.;
    let boundary = apply(
        doc,
        Command::AddNode {
            node: Box::new(boundary),
            slot: Slot::top_of(Some(group)),
        },
    )?
    .ok_or("Background boundary was not created")?;
    apply(
        doc,
        Command::MoveNode {
            id: image,
            slot: Slot::top_of(Some(group)),
        },
    )?;
    apply(
        doc,
        Command::SetClip {
            id: image,
            clip_to: Some(boundary),
        },
    )?;
    let fit = crate::design::fit_frame_image(doc, image, crate::design::ImageFit::Cover, [0.5; 2])?;
    apply(doc, fit)?;
    Ok(PageBackgroundImage {
        group,
        boundary,
        image,
    })
}

/// Move the selected source node into a page-sized native clipped frame. Its
/// identity, original Arc pixels, rotation and flips survive Cover fitting.
pub fn set_image(editor: &mut Editor, id: NodeId) -> Result<NodeId, String> {
    can_set_image(&editor.doc, id)?;
    if parts(&editor.doc)
        .and_then(|background| background.image)
        .is_some_and(|image| image.image == id)
    {
        return Ok(id);
    }
    let (mut next, mut background) = prepare(editor)?;
    remove_owned_image(&mut next, &mut background)?;
    background.image = Some(add_image_frame(&mut next, id)?);
    commit(editor, next, background, "Set image as page background")?;
    Ok(id)
}

/// Replace only source pixels and Cover placement, keeping the frame/image IDs.
/// On an empty background, create one source node in the same single undo step.
pub fn replace_image(editor: &mut Editor, raster: Arc<Raster>) -> Result<NodeId, String> {
    let (mut next, mut background) = prepare(editor)?;
    let image = if let Some(image) = background.image {
        crate::design::frame_image_replaceable(&next, image.group)?;
        if next
            .node(image.image)
            .is_some_and(|node| node.mask.is_some())
        {
            return Err("Remove this image's mask before replacing the page background.".into());
        }
        let NodeKind::Raster { placement, .. } = next.node(image.image).unwrap().kind else {
            unreachable!()
        };
        apply(
            &mut next,
            Command::ReplaceContent {
                id: image.image,
                raster,
                mask: None,
                placement,
                label: "Replace page background image".into(),
            },
        )?;
        let fit = crate::design::fit_frame_image(
            &next,
            image.image,
            crate::design::ImageFit::Cover,
            [0.5; 2],
        )?;
        apply(&mut next, fit)?;
        image.image
    } else {
        let image = apply(
            &mut next,
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Background photo",
                    raster,
                    Default::default(),
                )),
                slot: Slot::TOP,
            },
        )?
        .ok_or("Background image was not created")?;
        background.image = Some(add_image_frame(&mut next, image)?);
        image
    };
    commit(editor, next, background, "Replace page background image")?;
    Ok(image)
}

/// Removes the owned image frame only, leaving page color and all other objects.
pub fn remove_image(editor: &mut Editor) -> Result<(), String> {
    if parts(&editor.doc).is_none_or(|background| background.image.is_none()) {
        return Ok(());
    }
    let (mut next, mut background) = prepare(editor)?;
    remove_owned_image(&mut next, &mut background)?;
    commit(editor, next, background, "Remove page background image")
}

#[cfg(test)]
#[path = "design_background_tests.rs"]
mod tests;
