//! `emulsion-assistant` — drives an installed coding CLI (Claude Code) as the
//! in-app assistant, with Emulsion's MCP server attached.
//!
//! * [`provider`] finds the CLI.
//! * [`launch`] builds the argv, MCP config and system prompt. The CLI runs
//!   with no built-in tools, only Emulsion's MCP tools, and every tool that
//!   changes the document is confirmed by the person through the host.
//! * [`protocol`] parses the CLI's stream-json output and builds its input.
//! * [`session`] owns the child process.

pub mod launch;
pub mod protocol;
pub mod provider;
pub mod review;
pub mod session;
pub mod storage;

pub use protocol::{Event, Parser};
pub use session::{Launcher, ProdLauncher, Session};
