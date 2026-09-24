//! Newline-delimited JSON-RPC 2.0 MCP server on stdio.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "emulsion";
/// Shared workflow bootstrap for every MCP client, including external hosts.
pub const SERVER_INSTRUCTIONS: &str = include_str!("instructions.md");

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

/// A tool the server exposes; definitions are hand-written in `tools` and its
/// sibling modules.
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

/// The server's behaviour, separated from transport so the relay, offline and
/// test hosts share it.
pub trait ToolHost {
    fn tools(&self) -> Vec<ToolDef>;
    fn call(&mut self, name: &str, args: &Value) -> ToolResult;
}

/// A host that exposes no tools, used by the server's round-trip tests.
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
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            "instructions": SERVER_INSTRUCTIONS
        })),
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({ "tools": host.tools() })),
        "tools/call" => {
            let result = call_tool(host, &req.params);
            ok(serde_json::to_value(result).expect("serializable"))
        }
        other => err(-32601, format!("method not found: {other}")),
    })
}

/// Reject malformed or unknown calls before they can enter the editor queue.
fn call_tool(host: &mut dyn ToolHost, params: &Value) -> ToolResult {
    use crate::recovery::{ErrorCode, ExecutionState, RetryPolicy, tool_error};
    let invalid = |message| {
        tool_error(
            ErrorCode::InvalidRequest,
            message,
            ExecutionState::NotStarted,
            RetryPolicy::CorrectRequest,
        )
    };
    let Some(name) = params
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
    else {
        return invalid("tools/call requires a nonempty string name");
    };
    let args = match params.get("arguments") {
        None => json!({}),
        Some(args) if args.is_object() => args.clone(),
        Some(_) => return invalid("tools/call arguments must be an object"),
    };
    if !host.tools().iter().any(|tool| tool.name == name) {
        return tool_error(
            ErrorCode::UnknownTool,
            format!("unknown tool: {name}"),
            ExecutionState::NotStarted,
            RetryPolicy::CorrectRequest,
        );
    }
    host.call(name, &args)
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
        crate::recovery::tool_error(
            crate::recovery::ErrorCode::AppUnavailable,
            "Emulsion is not connected. Start this server from the Emulsion app.",
            crate::recovery::ExecutionState::NotStarted,
            crate::recovery::RetryPolicy::Reconnect,
        )
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
        assert_eq!(out[0]["result"]["instructions"], SERVER_INSTRUCTIONS);
        assert_eq!(out[1]["result"]["tools"], json!([]));
        assert_eq!(out[2]["id"], 3);
    }

    #[test]
    fn shape_tools_are_discoverable_over_json_rpc() {
        let request: Request =
            serde_json::from_value(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        let response = handle(&mut OfflineHost, request).unwrap();
        let tools = response.result.unwrap()["tools"]
            .as_array()
            .unwrap()
            .clone();
        for name in [
            "draw_shape",
            "combine_path",
            "resize_path",
            "align_path_components",
            "list_shape_stroke_presets",
            "save_shape_stroke_preset",
            "apply_shape_stroke_preset",
        ] {
            let tool = tools.iter().find(|tool| tool["name"] == name).expect(name);
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        for name in ["draw_path", "set_path"] {
            let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
            for field in [
                "fill_paint",
                "stroke_paint",
                "stroke_alignment",
                "cap",
                "join",
                "dashes",
                "dash_offset",
                "miter_limit",
            ] {
                assert!(
                    tool["inputSchema"]["properties"].get(field).is_some(),
                    "{name}.{field}"
                );
            }
        }
    }

    #[test]
    fn relay_discovers_all_tools_and_paints_with_paged_non_pen_brush_ids() {
        let relay = crate::relay::Relay::start().unwrap();
        let calls = relay.calls.clone();
        let app = std::thread::spawn(move || {
            let mut painted = 0;
            while let Ok(call) = calls.recv_blocking() {
                // A fresh layer makes each returned composite a comparable swatch.
                let mut editor =
                    emulsion_core::Editor::new(emulsion_core::Document::new(128, 96), None);
                let added =
                    crate::exec::execute(&mut editor, "add_layer", &json!({"name": "Brush audit"}));
                assert!(!added.is_error);
                let is_paint = call.name == "paint";
                let result = crate::exec::execute(&mut editor, &call.name, &call.arguments);
                call.reply(result);
                if is_paint {
                    painted += 1;
                    if painted == 3 {
                        break;
                    }
                }
            }
        });
        let mut host = crate::relay::RelayHost::new(relay.addr.to_string(), relay.token.clone());
        let rpc = |host: &mut dyn ToolHost, method: &str, params: Value| {
            let request = serde_json::from_value(json!({
                "jsonrpc": "2.0", "id": 1, "method": method, "params": params
            }))
            .unwrap();
            handle(host, request).unwrap().result.unwrap()
        };
        let listed = rpc(&mut host, "tools/list", json!({}));
        let definitions = crate::tools::definitions();
        assert_eq!(listed["tools"], serde_json::to_value(&definitions).unwrap());
        for name in [
            "list_brushes",
            "describe_brush_library",
            "preview_brush",
            "edit_brush",
            "paint",
            "hatch",
        ] {
            assert!(definitions.iter().any(|tool| tool.name == name), "{name}");
        }
        eprintln!("MCP exposes {} tools", definitions.len());
        let mut brushes = Vec::new();
        let mut offset = 0;
        loop {
            let result = rpc(
                &mut host,
                "tools/call",
                json!({
                    "name": "list_brushes", "arguments": {"swatches": false, "offset": offset}
                }),
            );
            assert_eq!(result["isError"], false, "{result}");
            let metadata: Value =
                serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
            brushes.extend(metadata["brushes"].as_array().unwrap().iter().cloned());
            let Some(next) = metadata["next_offset"].as_u64() else {
                assert_eq!(brushes.len() as u64, metadata["total"].as_u64().unwrap());
                break;
            };
            assert!(next > offset, "discovery must advance");
            offset = next;
        }
        let mut images = Vec::new();
        let mut settings = Vec::new();
        for name in ["Fude brush", "Screentone 40%", "Ink wash"] {
            let brush = brushes.iter().find(|brush| brush["name"] == name).unwrap();
            assert!(brush["id"].as_str().is_some_and(|id| !id.is_empty()));
            let result = rpc(
                &mut host,
                "tools/call",
                json!({
                    "name": "paint", "arguments": {
                        "node": 1, "brush": brush["id"], "color": "#000000",
                        "strokes": [{"points": [[20, 48, 0.2], [64, 48, 1.0], [108, 48, 0.2]]}]
                    }
                }),
            );
            assert_eq!(result["isError"], false, "{name}: {result}");
            let image = result["content"]
                .as_array()
                .unwrap()
                .iter()
                .find(|content| content["type"] == "image")
                .expect("completed composite");
            assert!(
                !images.contains(&image["data"]),
                "{name} rendered identically"
            );
            assert!(
                !settings.contains(&brush["settings"]),
                "{name} settings duplicated"
            );
            images.push(image["data"].clone());
            settings.push(brush["settings"].clone());
        }
        app.join().unwrap();
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

    #[test]
    fn invalid_tool_calls_never_reach_the_host_and_omitted_arguments_are_an_object() {
        #[derive(Default)]
        struct RecordingHost {
            calls: Vec<(String, Value)>,
        }
        impl ToolHost for RecordingHost {
            fn tools(&self) -> Vec<ToolDef> {
                vec![ToolDef {
                    name: "inspect".into(),
                    description: "Read state".into(),
                    input_schema: json!({"type": "object", "properties": {}}),
                }]
            }
            fn call(&mut self, name: &str, args: &Value) -> ToolResult {
                self.calls.push((name.into(), args.clone()));
                ToolResult::text("observed")
            }
        }
        let mut host = RecordingHost::default();
        for params in [
            Value::Null,
            json!({}),
            json!({"name": 7}),
            json!({"name": " "}),
            json!({"name": "inspect", "arguments": null}),
            json!({"name": "inspect", "arguments": []}),
            json!({"name": "inspect", "arguments": "{}"}),
            json!({"name": "unknown", "arguments": {}}),
        ] {
            let request = serde_json::from_value(json!({
                "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": params
            }))
            .unwrap();
            let reply = handle(&mut host, request).unwrap().result.unwrap();
            assert_eq!(reply["isError"], true, "{params}");
            let error: Value =
                serde_json::from_str(reply["content"][1]["text"].as_str().unwrap()).unwrap();
            assert_eq!(error["execution_state"], "not_started");
            assert_eq!(error["retry_policy"], "correct_request");
            assert!(host.calls.is_empty());
        }
        assert_eq!(
            call_tool(&mut host, &json!({"name": "inspect"})),
            ToolResult::text("observed")
        );
        assert_eq!(host.calls, vec![("inspect".into(), json!({}))]);
    }

    #[test]
    fn offline_discovery_does_not_claim_an_editor_connection() {
        let mut host = OfflineHost;
        assert!(!host.tools().is_empty());
        let result = call_tool(&mut host, &json!({"name": "describe_document"}));
        assert!(result.is_error);
        let error: Value =
            serde_json::from_str(result.content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(error["code"], "app_unavailable");
        assert_eq!(error["execution_state"], "not_started");
    }

    #[test]
    fn tool_errors_are_preserved_without_guessing_whether_they_changed_state() {
        struct RefusingHost;
        impl ToolHost for RefusingHost {
            fn tools(&self) -> Vec<ToolDef> {
                crate::tools::definitions()
            }
            fn call(&mut self, _: &str, _: &Value) -> ToolResult {
                ToolResult::error("Skipped by the person")
            }
        }
        let result = call_tool(
            &mut RefusingHost,
            &json!({"name": "paint", "arguments": {}}),
        );
        assert_eq!(result, ToolResult::error("Skipped by the person"));
    }
}
