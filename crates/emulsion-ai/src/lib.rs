//! `emulsion-ai` — optional intelligence that never becomes load-bearing.
//!
//! * [`decide`] answers small, structured questions about a request: which
//!   operation, which nodes. [`decide::Keywords`] works offline;
//!   [`jev::JevDecider`] asks TypeSafe's Jev when a key is configured.
//! * [`palette`] turns a typed request into Commands when the answers are
//!   confident, so simple requests never need the coding CLI.
//! * [`suggest`] proposes adjustments from image statistics (tier 0: no
//!   models, no network).

pub mod decide;
pub mod jev;
pub mod palette;
pub mod suggest;
