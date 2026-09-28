//! Page width is the stable reference: content sizing cannot feed back into
//! breakpoint selection. A matching override always inherits from base settings.
use super::{Align, Document, Flow, Frame, NodeId};
use serde::{Deserialize, Serialize};

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
    selected(doc.design.frames.get(&id)?, f64::from(doc.width)).map(|b| b.min_width)
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
    }
    resolved
}
/// Current settings after applying the highest canvas-width threshold.
/// The returned frame has no breakpoint entries; persisted base remains intact.
pub fn effective_frame(doc: &Document, id: NodeId) -> Option<Frame> {
    Some(resolve(doc.design.frames.get(&id)?, f64::from(doc.width)))
}
