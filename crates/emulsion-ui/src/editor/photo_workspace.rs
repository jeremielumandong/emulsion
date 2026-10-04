//! Photo-only presentation conventions. Other creative workspaces retain their
//! own chrome, tools and inspectors, including their saved custom layouts.
use super::*;

impl EditorView {
    pub(crate) fn is_photo_workflow(&self) -> bool {
        self.editor.kind().is_none() && !self.draw_mode && !self.library_only
    }

    /// Give Photo a restrained, neutral desktop-editing surface without
    /// changing the user's theme, accent or any other workspace's palette.
    pub(crate) fn workspace_palette(&self, cx: &App) -> Palette {
        let mut p = theme::palette(cx);
        if self.is_photo_workflow() {
            p.paper = p
                .paper
                .blend(p.ink.opacity(if p.dark { 0.08 } else { 0.03 }));
            p.panel = p
                .panel
                .blend(p.ink.opacity(if p.dark { 0.14 } else { 0.02 }));
            p.stage = p
                .stage
                .blend(p.ink.opacity(if p.dark { 0.08 } else { 0.02 }));
            p.soft_bg = p
                .soft_bg
                .blend(p.ink.opacity(if p.dark { 0.04 } else { 0.02 }));
        }
        p
    }
}
