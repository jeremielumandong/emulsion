//! Newline-delimited JSON-RPC 2.0 MCP server on stdio.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "emulsion";

#[derive(Debug, Deserialize)]
pub struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
pub struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    code: i64,
    message: String,
}

/// A tool the server exposes. Phase 2 generates these from the Command API.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Result of a tool call in MCP's content-block shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub content: Vec<Value>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": s.into()})],
            is_error: false,
        }
    }
    pub fn error(s: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": s.into()})],
            is_error: true,
        }
    }
}

/// The server's behaviour, separated from transport so tests and the
/// in-process path (§5.4) share it.
pub trait ToolHost {
    fn tools(&self) -> Vec<ToolDef>;
    fn call(&mut self, name: &str, args: &Value) -> ToolResult;
}

/// Phase 0 host: no tools yet.
#[derive(Default)]
pub struct EmptyHost;

impl ToolHost for EmptyHost {
    fn tools(&self) -> Vec<ToolDef> {
        Vec::new()
    }
    fn call(&mut self, name: &str, _args: &Value) -> ToolResult {
        ToolResult::error(format!("unknown tool: {name}"))
    }
}

/// Handle one decoded request. Returns `None` for notifications.
pub fn handle(host: &mut dyn ToolHost, req: Request) -> Option<Response> {
    let id = req.id.clone()?; // notifications carry no id and get no reply
    let ok = |result: Value| Response {
        jsonrpc: "2.0",
        id: id.clone(),
        result: Some(result),
        error: None,
    };
    let err = |code: i64, message: String| Response {
        jsonrpc: "2.0",
        id: id.clone(),
        result: None,
        error: Some(RpcError { code, message }),
    };
    Some(match req.method.as_str() {
        "initialize" => ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({ "tools": host.tools() })),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = req.params.get("arguments").cloned().unwrap_or(Value::Null);
            let result = host.call(name, &args);
            ok(serde_json::to_value(result).expect("serializable"))
        }
        other => err(-32601, format!("method not found: {other}")),
    })
}

/// Run the server over this process's stdin/stdout until EOF. When started
/// by Emulsion (relay address and token in the environment) tool calls go to
/// the running app; otherwise every call reports that Emulsion is not running.
pub fn serve_stdio() -> anyhow::Result<()> {
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    match crate::relay::RelayHost::from_env() {
        Some(host) => serve(host, stdin, stdout),
        None => serve(OfflineHost, stdin, stdout),
    }
}

/// Lists the real tools but cannot run them.
struct OfflineHost;

impl ToolHost for OfflineHost {
    fn tools(&self) -> Vec<ToolDef> {
        crate::tools::definitions()
    }
    fn call(&mut self, _name: &str, _args: &Value) -> ToolResult {
        ToolResult::error("Emulsion is not running. Start this server from the Emulsion app.")
    }
}

pub fn serve(
    mut host: impl ToolHost,
    input: impl BufRead,
    mut output: impl Write,
) -> anyhow::Result<()> {
    tracing::info!(protocol = PROTOCOL_VERSION, "emulsion mcp-serve ready");
    for line in input.lines() {
        let line = line.context("reading stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "malformed request");
                let resp = Response {
                    jsonrpc: "2.0",
                    id: Value::Null,
                    result: None,
                    error: Some(RpcError {
                        code: -32700,
                        message: format!("parse error: {e}"),
                    }),
                };
                writeln!(output, "{}", serde_json::to_string(&resp)?)?;
                output.flush()?;
                continue;
            }
        };
        if let Some(resp) = handle(&mut host, req) {
            writeln!(output, "{}", serde_json::to_string(&resp)?)?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        serve(EmptyHost, input.as_bytes(), &mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn initialize_and_list_tools() {
        let out = roundtrip(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
            "\n",
        ));
        assert_eq!(out.len(), 3, "notification must not get a reply");
        assert_eq!(out[0]["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(out[0]["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(out[1]["result"]["tools"], json!([]));
        assert_eq!(out[2]["id"], 3);
    }

    #[test]
    fn unknown_method_and_tool() {
        let out = roundtrip(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"x","arguments":{}}}"#,
            "\n",
            "not json\n",
        ));
        assert_eq!(out[0]["error"]["code"], -32601);
        assert_eq!(out[1]["result"]["isError"], true);
        assert_eq!(out[2]["error"]["code"], -32700);
    }
}
