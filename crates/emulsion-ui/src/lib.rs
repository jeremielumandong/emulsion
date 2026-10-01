//! `emulsion-ui` — GPUI views for Emulsion: the workspace, the Home and
//! Editor screens, the tiled canvas viewport, and design tokens.

mod about;
pub mod actions;
pub mod app_state;
mod appearance;
mod assistant;
mod batch;
mod busy_card;
mod cloud_screen;
pub mod editor;
mod home;
mod home_projects;
pub mod image_viewer;
pub mod landing;
pub(crate) mod playback;
mod playback_setup;
mod print_dialog;
pub mod prompt;
mod reference;
mod settings_models;
mod settings_screen;
mod settings_storyboard;
mod settings_writer;
pub mod tablet;
pub mod theme;
pub mod viewport;
pub mod viewport_gpu;
mod viewport_svg;
pub mod web_player;
pub mod widgets;
pub mod workspace;

pub use workspace::Workspace;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../../emulsion-io/tests/common/raw_fixture.rs"]
mod raw_test_fixture;

#[cfg(test)]
mod popup_scroll_tests;
