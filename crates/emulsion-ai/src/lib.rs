//! `emulsion-ai` — optional intelligence that never becomes load-bearing.
//!
//! * [`decide`] answers small, structured questions about a request: which
//!   operation, which nodes. [`decide::Keywords`] works offline;
//!   [`jev::JevDecider`] asks TypeSafe's Jev when a key is configured.
//! * [`palette`] turns a typed request into Commands when the answers are
//!   confident, so simple requests never need the coding CLI.
//! * [`suggest`] proposes adjustments from image statistics (tier 0: no
//!   models, no network).

//! * [`models`], [`runner`] and [`jobs`] bring in optional local models
//!   (tier 1): a manifest with on-demand downloads, ONNX Runtime sessions,
//!   and progress/cancel shared with the UI. [`prep`] moves pixels in and
//!   out of tensors.

pub mod critique;
pub mod decide;
pub mod depth;
pub mod face;
pub mod inpaint;
pub mod jev;
pub mod jobs;
pub mod matte;
pub mod models;
pub mod palette;
pub mod prep;
pub mod runner;
pub mod sam;
pub mod suggest;
pub mod upscale;
