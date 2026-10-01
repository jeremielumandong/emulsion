#![recursion_limit = "256"]
//! `emulsion-mcp` — Emulsion's MCP server, which exposes editor commands to a
//! coding CLI.
//!
//! * [`server`] speaks newline-delimited JSON-RPC 2.0 on stdio
//!   (`emulsion mcp-serve`).
//! * [`tools`] defines the tool schemas.
//! * [`exec`] runs a tool against an open document through the Command API.
//! * [`relay`] carries tool calls from the `mcp-serve` process to the running
//!   app over loopback TCP, authenticated with a per-session token.

pub mod audit;
mod blending;
mod brush_assets;
mod brush_catalog;
mod brush_discovery;
pub mod brush_tools;
mod design_appearance_tools;
mod design_asset_tools;
mod design_data_tools;
mod design_interaction_tools;
mod design_layout_tools;
pub mod design_motion_tools;
mod design_paragraph_tools;
mod design_selection_export_tools;
mod design_variable_tools;
mod design_vector_tools;
mod diagram_format_tools;
pub mod diagram_project_tools;
mod diagram_tools;
pub mod editor_host_tools;
pub mod exec;
mod export_tools;
mod image_import_tools;
pub mod library_tools;
pub mod photo_source_tools;
mod preview;
pub mod print_tools;
pub mod project_tools;
pub mod project_variable_tools;
#[cfg(test)]
#[path = "../../emulsion-io/tests/common/raw_fixture.rs"]
pub(crate) mod raw_fixture;
pub mod raw_looks;
pub mod raw_preview;
mod raw_tools;
pub mod recovery;
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

pub mod creative_catalog_tools;
pub mod workspace_tools;

pub mod design_brand_tools;

pub mod editor_layout_tools;

pub mod smart_source_tools;
