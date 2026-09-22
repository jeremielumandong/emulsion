//! `emulsion-mcp` — Emulsion's MCP server, which exposes editor commands to a
//! coding CLI.
//!
//! * [`server`] speaks newline-delimited JSON-RPC 2.0 on stdio
//!   (`emulsion mcp-serve`).
//! * [`tools`] defines the tool schemas.
//! * [`exec`] runs a tool against an open document through the Command API.
//! * [`relay`] carries tool calls from the `mcp-serve` process to the running
//!   app over loopback TCP, authenticated with a per-session token.

mod brush_discovery;
pub mod exec;
mod preview;
pub mod reference;
pub mod relay;
mod review;
pub mod server;
mod shape_geometry;
pub mod shape_presets;
mod shape_style;
mod text_tools;
pub mod tools;

pub use server::{
    EmptyHost, PROTOCOL_VERSION, SERVER_NAME, ToolDef, ToolHost, ToolResult, handle, serve,
    serve_stdio,
};
