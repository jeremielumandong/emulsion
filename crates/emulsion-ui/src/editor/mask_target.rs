//! The single editing target and component-space adapter. Stored targets are
//! captured in gestures and numeric keys so switching thumbnails cannot route
//! a pending edit to different content.
use super::*;
use emulsion_core::MaskProperties;
use emulsion_raster::{IRect, Mask};
use glam::DAffine2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MaskEditTarget {
    #[default]
    Content,
    RasterMask,
    VectorMask,
    SmartFilterMask,
}

impl MaskEditTarget {
    pub(crate) fn is_mask(self) -> bool {
        self != Self::Content
    }
    pub(crate) fn exists(self, node: &Node) -> bool {
        match self {
            Self::Content => true,
            Self::RasterMask => node.mask.is_some(),
            Self::VectorMask => node.vector_mask.is_some(),
            Self::SmartFilterMask => smart_filter_mask_ui::descriptor(node).is_some(),
        }
    }
    pub(crate) fn properties(self, node: &Node) -> Option<MaskProperties> {
        match self {
            Self::Content => None,
            Self::RasterMask => node.mask.as_ref().map(|_| node.mask_properties),
            Self::VectorMask => node.vector_mask.as_ref().map(|m| m.properties),
            Self::SmartFilterMask => smart_filter_mask_ui::descriptor(node).map(|m| m.properties),
        }
    }
    pub(crate) fn properties_command(self, id: NodeId, properties: MaskProperties) -> Command {
        match self {
            Self::VectorMask => Command::SetVectorMaskProperties { id, properties },
            Self::SmartFilterMask => Command::SetSmartFilterMaskProperties { id, properties },
            Self::RasterMask => Command::SetMaskProperties { id, properties },
            Self::Content => unreachable!("content has no mask properties"),
        }
    }
    pub(crate) fn affine(self, node: &Node) -> Option<[f64; 6]> {
        match self {
            Self::Content => None,
            Self::RasterMask => node.mask.as_ref().map(|_| node.mask_transform),
            Self::VectorMask => node.vector_mask.as_ref().map(|m| m.transform),
            Self::SmartFilterMask => smart_filter_mask_ui::descriptor(node).map(|m| m.transform),
        }
    }
    pub(crate) fn to_document(self, node: &Node) -> Option<DAffine2> {
        self.affine(node).map(|m| {
            emulsion_core::transform::local_to_document(node) * DAffine2::from_cols_array(&m)
        })
    }
    pub(crate) fn transform_command(self, id: NodeId, transform: [f64; 6]) -> Command {
        match self {
            Self::VectorMask => Command::SetVectorMaskTransform { id, transform },
            Self::SmartFilterMask => Command::SetSmartFilterMaskTransform { id, transform },
            Self::RasterMask => Command::SetMaskTransform { id, transform },
            Self::Content => unreachable!("content has no mask affine"),
        }
    }
    pub(crate) fn inspection(self, doc: &Document, node: &Node) -> Option<Arc<Mask>> {
        match self {
            Self::Content => None,
            Self::RasterMask => doc.raster_mask_for_inspection(node),
            Self::VectorMask => doc.vector_mask_for_inspection(node),
            Self::SmartFilterMask => emulsion_core::smart_filter_mask::for_inspection(node),
        }
    }
    pub(crate) fn bounds(self, doc: &Document, node: &Node) -> Option<IRect> {
        match self {
            Self::Content => None,
            Self::RasterMask => emulsion_core::transform::mask_bounds(node),
            Self::SmartFilterMask => smart_filter_mask_ui::raw_bounds(node),
            Self::VectorMask => {
                let mask = node.vector_mask.as_ref()?;
                if mask.path.is_empty() {
                    return Some(IRect::new(0, 0, doc.width as i32, doc.height as i32));
                }
                let mut path = (*mask.path).clone();
                path.transform(self.to_document(node)?);
                let mut min = glam::DVec2::splat(f64::INFINITY);
                let mut max = glam::DVec2::splat(f64::NEG_INFINITY);
                for point in path
                    .subpaths
                    .iter()
                    .flat_map(|s| &s.anchors)
                    .flat_map(|a| [a.p, a.h_in, a.h_out])
                {
                    let point = glam::dvec2(point.0, point.1);
                    min = min.min(point);
                    max = max.max(point);
                }
                min = min.floor() - glam::DVec2::splat(2.);
                max = max.ceil() + glam::DVec2::splat(3.);
                let size = max - min;
                // The UI frame must fit IRect arithmetic. Keep authoritative
                // off-canvas geometry untouched when its hull cannot fit.
                if !min.is_finite()
                    || !max.is_finite()
                    || min.min_element() < f64::from(i32::MIN)
                    || max.max_element() > f64::from(i32::MAX)
                    || size.max_element() > f64::from(i32::MAX)
                {
                    return None;
                }
                Some(IRect::new(
                    min.x as i32,
                    min.y as i32,
                    size.x as i32,
                    size.y as i32,
                ))
            }
        }
    }
}

impl EditorView {
    /// Properties remain available from the layer inspector with the content
    /// thumbnail selected. An explicit component target always takes priority.
    pub(super) fn mask_properties_component(&self, id: NodeId) -> Option<MaskEditTarget> {
        let node = self.editor.doc.node(id)?;
        let target = self.tools.mask_edit_target;
        if target.is_mask() {
            return target.exists(node).then_some(target);
        }
        if node.mask.is_some() {
            Some(MaskEditTarget::RasterMask)
        } else if node.vector_mask.is_some() {
            Some(MaskEditTarget::VectorMask)
        } else {
            smart_filter_mask_ui::descriptor(node).map(|_| MaskEditTarget::SmartFilterMask)
        }
    }
    pub(super) fn mask_component_ready(
        &self,
        id: NodeId,
        target: MaskEditTarget,
        geometry: bool,
    ) -> bool {
        self.selected == Some(id)
            && !self.tools.quick_mask
            && target.is_mask()
            && self.editor.doc.node(id).is_some_and(|n| target.exists(n))
            && self.editor.doc.locked_ancestor(id).is_none()
            && (!geometry || !self.editor.doc.layer_locks(id).position)
    }
    pub(crate) fn set_mask_edit_target(&mut self, target: MaskEditTarget, cx: &mut Context<Self>) {
        if !self.photo_transform_ready(cx) {
            return;
        }
        if self.tools.mask_edit_target != target {
            self.finish_tool_interaction(cx);
            self.finish_mask_properties();
            self.mask_view.target = None;
            self.tools.pen.selected = None;
            self.tools.mask_edit_target = target;
            self.invalidate_pending_edits();
        }
        cx.notify();
    }
}
