// Modified by Emulsion: export `wait_for_submitted_frames` (macOS canvas spike).
#![cfg(target_os = "macos")]
//! Shared Apple platform support for GPUI.
//!
//! This crate contains the Metal renderer and GPU resource management shared
//! by GPUI's Apple platform backends.

mod metal_atlas;
pub mod metal_renderer;

pub use metal_renderer::wait_for_submitted_frames;
