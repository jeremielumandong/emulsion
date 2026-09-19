//! `emulsion-ui` — GPUI views for Emulsion: the workspace, the Home and
//! Editor screens, the tiled canvas viewport, and design tokens.

pub mod actions;
pub mod editor;
mod home;
pub mod theme;
pub mod viewport;
pub mod widgets;
pub mod workspace;

pub use workspace::Workspace;
