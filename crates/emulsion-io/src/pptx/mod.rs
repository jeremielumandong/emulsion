//! Bounded, editable Office Open XML presentation interchange. No remote resources execute.
use crate::{IoError, Result};
use emulsion_core::{
    Document, Node, NodeId, NodeKind,
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use emulsion_raster::{
    Placement, Raster,
    vector::{Path as VectorPath, PathPaint, PathStyle},
    vector_geometry,
};
use glam::{DAffine2, dvec2};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::Arc,
};
mod export;
mod geometry;
mod import;
mod package;
#[cfg(test)]
mod tests;
mod text;
pub use export::write;
pub use import::read;
use package::{Package, Xml};
const EMU: f64 = 9525.;
const MAX_OBJECTS: usize = 16_384;
fn error(s: impl Into<String>) -> IoError {
    IoError::Unsupported(s.into())
}
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Report {
    pub pages: usize,
    pub objects: usize,
    pub warnings: Vec<String>,
}
pub struct Imported {
    pub project: Project,
    pub warnings: Vec<String>,
}
pub fn is_pptx(path: &Path) -> bool {
    path.extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("pptx"))
}
fn warn(w: &mut Vec<String>, slide: usize, object: &str, message: impl AsRef<str>) {
    if w.len() < 4096 {
        w.push(format!("Slide {slide} · {object}: {}", message.as_ref()));
    }
}
fn escaped(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

fn valid_xml_text(s: &str) -> bool {
    s.chars().all(|c| {
        matches!(c, '\t' | '\r' | '\n')
            || ((c as u32) >= 0x20 && !matches!(c, '\u{fffe}' | '\u{ffff}'))
    })
}
