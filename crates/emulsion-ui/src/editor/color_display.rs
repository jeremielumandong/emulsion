//! OpenColorIO on the canvas: the display transform for this document,
//! resolved from the colour management preferences and the project's
//! working colour space. Off, nothing changes. On, tiles are drawn through
//! the baked display LUT (the GPU canvas stands down, since it cannot apply
//! it), and the storyboard player shows the same transform.
use super::*;
use emulsion_io::color_management::{self, BakedLut};

/// The display transform an editor last drew with.
#[derive(Default)]
pub(crate) struct DisplayTransform {
    key: Option<u64>,
    pub(crate) lut: Option<Arc<BakedLut>>,
}

impl DisplayTransform {
    /// Identifies the transform, so pictures made with another redo.
    pub(crate) fn key(&self) -> Option<u64> {
        self.key
    }
}

impl EditorView {
    fn working_colorspace(&self) -> Option<String> {
        self.editor
            .storyboard()
            .and_then(|b| b.working_colorspace.clone())
    }

    /// Bring the display transform up to date, dropping tiles drawn with
    /// another. Returns whether colours go through OpenColorIO.
    pub(crate) fn sync_display_transform(&mut self) -> bool {
        let working = self.working_colorspace();
        let key = color_management::display_key(working.as_deref());
        if key != self.ocio_display.key {
            self.ocio_display = DisplayTransform {
                key,
                lut: key.and_then(|_| color_management::display_lut(working.as_deref())),
            };
            self.cache.borrow_mut().clear();
            self.gen_counter += 1;
            self.render_gen = self.gen_counter;
            self.gen_counter += 1;
            self.before_gen = self.gen_counter;
        }
        self.ocio_display.key.is_some()
    }
}
