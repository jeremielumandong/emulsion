//! Width queries inherit directly from base settings and reject sizing cycles.
use super::{Align, Child, Document, Flow, Frame, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointReference {
    #[default]
    Canvas,
    Container,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FrameLimits {
    pub min_width: Option<f64>,
    pub max_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_height: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FrameOverrides {
    pub flow: Option<Flow>,
    pub padding: Option<[f64; 4]>,
    pub gap: Option<f64>,
    pub columns: Option<u32>,
    pub wrap: Option<bool>,
    pub align: Option<Align>,
    pub hug_width: Option<bool>,
    pub hug_height: Option<bool>,
    pub clip_content: Option<bool>,
    /// Omission inherits all four bounds; an empty object removes all bounds.
    pub limits: Option<FrameLimits>,
    /// A named child replaces its complete base sizing; omitted children inherit.
    pub children: BTreeMap<NodeId, Child>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Breakpoint {
    /// Inclusive lower bound in canvas pixels. Entries need not be sorted.
    pub min_width: f64,
    #[serde(default)]
    pub overrides: FrameOverrides,
}

pub(super) fn validate(entries: &[Breakpoint]) -> Result<(), String> {
    if entries.len() > 16 {
        return Err("A responsive frame supports up to 16 canvas width breakpoints.".into());
    }
    for (i, entry) in entries.iter().enumerate() {
        if !entry.min_width.is_finite() || !(1. ..=100000.).contains(&entry.min_width) {
            return Err("Breakpoint canvas widths must be 1–100000 px.".into());
        }
        if entries[..i].iter().any(|b| b.min_width == entry.min_width) {
            return Err("Each canvas width breakpoint needs a unique minimum width.".into());
        }
    }
    Ok(())
}

pub fn active_breakpoint(doc: &Document, id: NodeId) -> Option<f64> {
    selected(doc.design.frames.get(&id)?, reference_width(doc, id)?).map(|b| b.min_width)
}
/// Container rules use the immediate responsive parent's content width. Top-level
/// frames use canvas width. Validation rejects content-sized query ancestors.
pub fn reference_width(doc: &Document, id: NodeId) -> Option<f64> {
    let frame = doc.design.frames.get(&id)?;
    if frame.breakpoint_reference == BreakpointReference::Container
        && let Some(parent) = doc
            .node(id)?
            .parent
            .filter(|p| doc.design.frames.contains_key(p))
    {
        let width = super::bounds(doc, parent)?.2;
        let parent = effective_frame(doc, parent)?;
        return Some((width - parent.padding[1] - parent.padding[3]).max(1.));
    }
    Some(f64::from(doc.width))
}
fn selected(frame: &Frame, width: f64) -> Option<&Breakpoint> {
    frame
        .breakpoints
        .iter()
        .filter(|b| b.min_width <= width)
        .max_by(|a, b| a.min_width.total_cmp(&b.min_width))
}
pub(super) fn resolve(base: &Frame, width: f64) -> Frame {
    let mut resolved = base.clone();
    resolved.breakpoints.clear();
    if let Some(entry) = selected(base, width) {
        macro_rules! apply { ($($name:ident),*) => { $(if let Some(value) = entry.overrides.$name { resolved.$name = value; })* }; }
        apply!(
            flow,
            padding,
            gap,
            columns,
            wrap,
            align,
            hug_width,
            hug_height,
            clip_content
        );
        if let Some(limits) = entry.overrides.limits {
            resolved.min_width = limits.min_width;
            resolved.max_width = limits.max_width;
            resolved.min_height = limits.min_height;
            resolved.max_height = limits.max_height;
        }
        resolved.children.extend(entry.overrides.children.clone());
    }
    resolved
}
/// Current settings after applying the highest canvas-width threshold.
/// The returned frame has no breakpoint entries; persisted base remains intact.
pub fn effective_frame(doc: &Document, id: NodeId) -> Option<Frame> {
    Some(resolve(
        doc.design.frames.get(&id)?,
        reference_width(doc, id)?,
    ))
}
