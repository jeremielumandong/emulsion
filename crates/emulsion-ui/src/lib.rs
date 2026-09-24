//! `emulsion-ui` — GPUI views for Emulsion: the workspace, the Home and
//! Editor screens, the tiled canvas viewport, and design tokens.

mod about;
pub mod actions;
pub mod app_state;
mod assistant;
mod batch;
mod busy_card;
pub mod editor;
mod home;
pub mod landing;
mod reference;
mod settings_models;
mod settings_screen;
mod settings_writer;
pub mod tablet;
pub mod theme;
pub mod viewport;
pub mod widgets;
pub mod workspace;

pub use workspace::Workspace;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../../emulsion-io/tests/common/raw_fixture.rs"]
mod raw_test_fixture;
