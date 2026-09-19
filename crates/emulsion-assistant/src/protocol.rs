//! Claude Code stream-json protocol.
//!
//! Output events arrive one JSON object per line. Assistant messages carry
//! the full accumulated text each time, so text and thinking are diffed
//! against what was already emitted. Input is one JSON object per line too.

use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Init {
        session_id: String,
        tools: Vec<String>,
        model: Option<String>,
    },
    /// New assistant text since the last event.
    Text(String),
    /// New thinking text since the last event.
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        id: String,
        text: String,
        is_error: bool,
    },
    /// The CLI asks whether a tool call may run.
    Permission {
        request_id: String,
        tool_name: String,
        input: Value,
        tool_use_id: String,
    },
    /// A turn finished.
    Result {
        text: String,
        cost_usd: f64,
        duration_ms: u64,
        turns: u64,
        input_tokens: u64,
        output_tokens: u64,
    },
    Error(String),
    Stderr(String),
    Exited(Option<i32>),
}

/// Which CLI's stream this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flavor {
    #[default]
    Claude,
    Codex,
    OpenCode,
    Kimi,
}

#[derive(Default)]
pub struct Parser {
    pub flavor: Flavor,
    last_text_len: usize,
    last_thinking_len: usize,
    seen_tools: HashSet<String>,
    /// OpenCode resends whole text parts; remember each part's last text.
    part_text: std::collections::HashMap<String, String>,
    /// Tool ids whose result was already reported.
    finished_tools: HashSet<String>,
    /// A Result was emitted for the current turn (one-shot CLIs report
    /// nothing at exit otherwise).
    pub saw_result: bool,
    /// Usage gathered along a one-shot turn: input tokens, output tokens, cost.
    pub usage: (u64, u64, f64),
    pub session_id: Option<String>,
}

impl Parser {
    pub fn with_flavor(flavor: Flavor) -> Self {
        Self {
            flavor,
            ..Default::default()
        }
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// Flatten a tool_result `content` (string or block array) to text.
fn result_text(c: &Value) -> String {
    match c {
        Value::String(t) => t.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|b| match b["type"].as_str() {
                Some("text") => s(&b["text"]),
                Some("image") => "[image]".into(),
                _ => b.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => "done".into(),
        other => other.to_string(),
    }
}

impl Parser {
    fn reset_turn(&mut self) {
        self.last_text_len = 0;
        self.last_thinking_len = 0;
        self.seen_tools.clear();
    }

    /// Parse one output line. Blank and non-JSON lines yield nothing.
    pub fn feed(&mut self, line: &str) -> Vec<Event> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        match self.flavor {
            Flavor::Claude => self.feed_claude(&v),
            Flavor::Codex => self.feed_codex(&v),
            Flavor::OpenCode => self.feed_opencode(&v),
            Flavor::Kimi => self.feed_kimi(&v),
        }
    }

    fn feed_claude(&mut self, v: &Value) -> Vec<Event> {
        let mut out = Vec::new();
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => {
                let sid = s(&v["session_id"]);
                if !sid.is_empty() {
                    self.session_id = Some(sid.clone());
                }
                let tools = v["tools"]
                    .as_array()
                    .map(|a| a.iter().map(s).collect())
                    .unwrap_or_default();
                out.push(Event::Init {
                    session_id: sid,
                    tools,
                    model: v["model"].as_str().map(str::to_string),
                });
            }
            "assistant" => {
                // One counter per kind across the message, as the CLI resends
                // the whole message each time.
                let mut text = String::new();
                let mut thinking = String::new();
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => text.push_str(b["text"].as_str().unwrap_or_default()),
                        Some("thinking") => {
                            thinking.push_str(b["thinking"].as_str().unwrap_or_default())
                        }
                        Some("tool_use") => {
                            let id = s(&b["id"]);
                            let name = s(&b["name"]);
                            if !id.is_empty()
                                && !name.is_empty()
                                && self.seen_tools.insert(id.clone())
                            {
                                out.push(Event::ToolUse {
                                    id,
                                    name,
                                    input: b["input"].clone(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                if thinking.len() > self.last_thinking_len
                    && thinking.is_char_boundary(self.last_thinking_len)
                {
                    out.insert(
                        0,
                        Event::Thinking(thinking[self.last_thinking_len..].to_string()),
                    );
                    self.last_thinking_len = thinking.len();
                }
                if text.len() > self.last_text_len && text.is_char_boundary(self.last_text_len) {
                    out.insert(0, Event::Text(text[self.last_text_len..].to_string()));
                    self.last_text_len = text.len();
                }
            }
            "user" => {
                self.reset_turn();
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_result" {
                        out.push(Event::ToolResult {
                            id: s(&b["tool_use_id"]),
                            text: result_text(&b["content"]),
                            is_error: b["is_error"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
            "result" => {
                self.reset_turn();
                let sid = s(&v["session_id"]);
                if !sid.is_empty() {
                    self.session_id = Some(sid);
                }
                let subtype = s(&v["subtype"]);
                let is_error =
                    v["is_error"].as_bool().unwrap_or(false) || subtype.contains("error");
                let text = v["result"]
                    .as_str()
                    .or_else(|| v["error"].as_str())
                    .unwrap_or_default()
                    .to_string();
                if is_error {
                    let msg = if text.is_empty() {
                        format!(
                            "Claude Code reported an error ({})",
                            if subtype.is_empty() {
                                "unknown"
                            } else {
                                &subtype
                            }
                        )
                    } else {
                        text
                    };
                    out.push(Event::Error(msg));
                } else {
                    let u = &v["usage"];
                    self.saw_result = true;
                    out.push(Event::Result {
                        text,
                        cost_usd: v
                            .get("total_cost_usd")
                            .or_else(|| v.get("cost_usd"))
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0),
                        duration_ms: v["duration_ms"].as_u64().unwrap_or(0),
                        turns: v["num_turns"].as_u64().unwrap_or(0),
                        input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                    });
                }
            }
            "control_request" if v["request"]["subtype"] == "can_use_tool" => {
                let r = &v["request"];
                out.push(Event::Permission {
                    request_id: s(&v["request_id"]),
                    tool_name: r["tool_name"]
                        .as_str()
                        .unwrap_or("unknown tool")
                        .to_string(),
                    input: if r["input"].is_null() {
                        json!({})
                    } else {
                        r["input"].clone()
                    },
                    tool_use_id: s(&r["tool_use_id"]),
                });
            }
            _ => {}
        }
        out
    }
}

impl Parser {
    /// Start a new turn for one-shot CLIs.
    pub fn begin_turn(&mut self) {
        self.reset_turn();
        self.part_text.clear();
        self.saw_result = false;
        self.usage = (0, 0, 0.0);
    }

    fn tool_use(&mut self, id: String, name: String, input: Value) -> Option<Event> {
        if id.is_empty() || name.is_empty() || !self.seen_tools.insert(id.clone()) {
            return None;
        }
        Some(Event::ToolUse { id, name, input })
    }

    fn tool_result(&mut self, id: String, text: String, is_error: bool) -> Option<Event> {
        if id.is_empty() || !self.finished_tools.insert(id.clone()) {
            return None;
        }
        Some(Event::ToolResult { id, text, is_error })
    }

    /// Codex `exec --json`: `thread.started`, `item.*` with agent_message /
    /// mcp_tool_call items, `turn.completed`, `error`.
    fn feed_codex(&mut self, v: &Value) -> Vec<Event> {
        let mut out = Vec::new();
        match v["type"].as_str().unwrap_or_default() {
            "thread.started" => {
                let id = s(&v["thread_id"]);
                if !id.is_empty() {
                    self.session_id = Some(id.clone());
                    out.push(Event::Init {
                        session_id: id,
                        tools: vec![],
                        model: None,
                    });
                }
            }
            kind @ ("item.started" | "item.updated" | "item.completed") => {
                let item = &v["item"];
                let id = s(&item["id"]);
                match item["type"].as_str().unwrap_or_default() {
                    "agent_message" if kind == "item.completed" => {
                        let t = s(&item["text"]);
                        if !t.is_empty() {
                            out.push(Event::Text(t));
                        }
                    }
                    "reasoning" => {
                        if kind != "item.completed" {
                            out.push(Event::Thinking(String::new()));
                        }
                    }
                    "mcp_tool_call" => {
                        let name = s(&item["tool"]);
                        let server = s(&item["server"]);
                        let full = if server.is_empty() {
                            name.clone()
                        } else {
                            format!("mcp__{server}__{name}")
                        };
                        let input = item.get("arguments").cloned().unwrap_or(Value::Null);
                        if let Some(e) = self.tool_use(id.clone(), full, input) {
                            out.push(e);
                        }
                        if kind == "item.completed" {
                            let status = s(&item["status"]);
                            let err = item.get("error").filter(|e| !e.is_null());
                            let is_error = status.contains("fail") || err.is_some();
                            let text = err
                                .map(|e| {
                                    e.pointer("/message")
                                        .map(s)
                                        .unwrap_or_else(|| s(e).trim_matches('"').to_string())
                                })
                                .filter(|e| !e.is_empty())
                                .or_else(|| {
                                    item.get("result").map(|r| match r.get("content") {
                                        Some(c) => result_text(c),
                                        None => result_text(r),
                                    })
                                })
                                .unwrap_or_default();
                            if let Some(e) = self.tool_result(id, text, is_error) {
                                out.push(e);
                            }
                        }
                    }
                    "command_execution" => {
                        let cmd = s(&item["command"]);
                        if let Some(e) =
                            self.tool_use(id.clone(), "shell".into(), Value::String(cmd))
                        {
                            out.push(e);
                        }
                        if kind == "item.completed"
                            && let Some(e) = self.tool_result(
                                id,
                                s(&item["aggregated_output"]),
                                item["exit_code"].as_i64().unwrap_or(0) != 0,
                            )
                        {
                            out.push(e);
                        }
                    }
                    _ => {}
                }
            }
            "turn.completed" => {
                let u = &v["usage"];
                self.saw_result = true;
                out.push(Event::Result {
                    text: String::new(),
                    cost_usd: 0.0,
                    duration_ms: 0,
                    turns: 1,
                    input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                });
            }
            "turn.failed" | "error" => {
                let msg = v
                    .pointer("/error/message")
                    .or_else(|| v.get("message"))
                    .map(s)
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| "Codex reported an error".into());
                self.saw_result = true;
                out.push(Event::Error(msg));
            }
            _ => {}
        }
        out
    }

    /// OpenCode `run --format json`: events carry `part` objects (text,
    /// tool, reasoning, step markers) and a `sessionID`.
    fn feed_opencode(&mut self, v: &Value) -> Vec<Event> {
        let mut out = Vec::new();
        let sid = v
            .get("sessionID")
            .or_else(|| v.pointer("/part/sessionID"))
            .or_else(|| v.pointer("/info/sessionID"))
            .or_else(|| v.pointer("/properties/sessionID"))
            .map(s)
            .filter(|x| !x.is_empty());
        if let Some(sid) = sid
            && self.session_id.as_deref() != Some(sid.as_str())
        {
            self.session_id = Some(sid.clone());
            out.push(Event::Init {
                session_id: sid,
                tools: vec![],
                model: None,
            });
        }
        if v["type"] == "error" {
            let msg = v
                .pointer("/error/data/message")
                .or_else(|| v.pointer("/error/message"))
                .or_else(|| v.get("error"))
                .map(s)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "OpenCode reported an error".into());
            self.saw_result = true;
            out.push(Event::Error(msg));
            return out;
        }
        let Some(part) = v.get("part").or_else(|| v.pointer("/properties/part")) else {
            return out;
        };
        let pid = s(&part["id"]);
        match part["type"].as_str().unwrap_or_default() {
            "text" => {
                let full = s(&part["text"]);
                let prev = self.part_text.get(&pid).cloned().unwrap_or_default();
                if full.len() > prev.len() && full.starts_with(&prev) {
                    out.push(Event::Text(full[prev.len()..].to_string()));
                } else if full != prev && !full.is_empty() && prev.is_empty() {
                    out.push(Event::Text(full.clone()));
                }
                self.part_text.insert(pid, full);
            }
            "reasoning" => out.push(Event::Thinking(String::new())),
            "tool" => {
                let name = s(&part["tool"]);
                let id = {
                    let c = s(&part["callID"]);
                    if c.is_empty() { pid.clone() } else { c }
                };
                let state = &part["state"];
                let input = state.get("input").cloned().unwrap_or(Value::Null);
                if let Some(e) = self.tool_use(id.clone(), name, input) {
                    out.push(e);
                }
                let status = s(&state["status"]);
                if status == "completed" || status == "error" {
                    let text = state
                        .get("output")
                        .or_else(|| state.get("error"))
                        .map(s)
                        .unwrap_or_default();
                    if let Some(e) = self.tool_result(id, text, status == "error") {
                        out.push(e);
                    }
                }
            }
            "step-finish" => {
                // One per step; the totals accumulate and the process
                // exiting ends the turn.
                let t = &part["tokens"];
                self.usage.0 += t["input"].as_u64().unwrap_or(0);
                self.usage.1 += t["output"].as_u64().unwrap_or(0);
                self.usage.2 += part["cost"].as_f64().unwrap_or(0.0);
            }
            _ => {}
        }
        out
    }

    /// Kimi Code `--output-format stream-json`: Claude-like `assistant` /
    /// `result` events, plus OpenAI-style `tool_calls`.
    fn feed_kimi(&mut self, v: &Value) -> Vec<Event> {
        // Anything shaped like Claude's stream parses as such.
        let mut out = self.feed_claude(v);
        if !out.is_empty() {
            return out;
        }
        if let Some(sid) = v.get("session_id").map(s).filter(|x| !x.is_empty())
            && self.session_id.as_deref() != Some(sid.as_str())
        {
            self.session_id = Some(sid.clone());
            out.push(Event::Init {
                session_id: sid,
                tools: vec![],
                model: None,
            });
        }
        if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
            self.saw_result = true;
            out.push(Event::Error(s(err)));
            return out;
        }
        for call in v
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = call
                .get("name")
                .or_else(|| call.pointer("/function/name"))
                .map(s)
                .unwrap_or_default();
            let id = s(&call["id"]);
            let input = call
                .get("arguments")
                .or_else(|| call.pointer("/function/arguments"))
                .cloned()
                .map(|a| match a {
                    Value::String(t) => serde_json::from_str(&t).unwrap_or(Value::String(t)),
                    other => other,
                })
                .unwrap_or(Value::Null);
            if let Some(e) = self.tool_use(id, name, input) {
                out.push(e);
            }
        }
        if v["role"] == "assistant" || v["type"] == "assistant" {
            let text = v
                .get("content")
                .or_else(|| v.get("text"))
                .or_else(|| v.get("delta"))
                .map(|c| match c {
                    Value::String(t) => t.clone(),
                    other => result_text(other),
                })
                .unwrap_or_default();
            if !text.is_empty() && !text.starts_with('{') {
                out.push(Event::Text(text));
            }
        }
        if v["role"] == "tool"
            && let Some(e) = self.tool_result(
                s(&v["tool_call_id"]),
                v.get("content").map(result_text).unwrap_or_default(),
                false,
            )
        {
            out.push(e);
        }
        out
    }
}

fn millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Sent once, just before the first user message.
pub fn initialize() -> Value {
    json!({ "type": "control_request", "request_id": format!("init-{}", millis()), "request": { "subtype": "initialize" } })
}

/// Ask the CLI to stop the current turn.
pub fn interrupt() -> Value {
    json!({ "type": "control_request", "request_id": format!("interrupt-{}", millis()), "request": { "subtype": "interrupt" } })
}

/// A user message with optional base64 images `(media_type, data)`.
pub fn user_message(session_id: &str, text: &str, images: &[(String, String)]) -> Value {
    let mut content = vec![json!({ "type": "text", "text": text })];
    for (media, data) in images {
        content.push(json!({ "type": "image", "source": { "type": "base64", "media_type": media, "data": data } }));
    }
    json!({
        "type": "user",
        "session_id": session_id,
        "message": { "role": "user", "content": content },
        "parent_tool_use_id": null,
    })
}

/// Let a tool call run. Arrays must be arrays, not null.
pub fn allow(request_id: &str, tool_use_id: &str, input: &Value) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": {
                "behavior": "allow",
                "updatedInput": if input.is_object() { input.clone() } else { json!({}) },
                "updatedPermissions": [],
                "toolUseID": tool_use_id,
            }
        }
    })
}

pub fn deny(request_id: &str, tool_use_id: &str, message: &str) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": { "behavior": "deny", "message": message, "toolUseID": tool_use_id }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(p: &mut Parser, lines: &[&str]) -> Vec<Event> {
        lines.iter().flat_map(|l| p.feed(l)).collect()
    }

    #[test]
    fn turn_with_tool_and_permission() {
        let mut p = Parser::default();
        let ev = feed_all(
            &mut p,
            &[
                r#"{"type":"system","subtype":"init","session_id":"s-1","tools":["mcp__emulsion__describe_document"],"model":"claude-x"}"#,
                "not json",
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Let me "}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Let me look."},{"type":"tool_use","id":"tu1","name":"mcp__emulsion__set_visibility","input":{"node":3,"visible":false}}]}}"#,
                r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"mcp__emulsion__set_visibility","input":{"node":3,"visible":false},"tool_use_id":"tu1"}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","content":[{"type":"text","text":"Hid Sun (#3)"}]}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done."}]}}"#,
                r#"{"type":"result","subtype":"success","result":"Done.","total_cost_usd":0.012,"duration_ms":900,"num_turns":2,"usage":{"input_tokens":10,"output_tokens":5},"session_id":"s-1"}"#,
            ],
        );
        assert_eq!(p.session_id.as_deref(), Some("s-1"));
        assert!(matches!(&ev[0], Event::Init { session_id, .. } if session_id == "s-1"));
        assert_eq!(ev[1], Event::Text("Let me ".into()));
        assert_eq!(ev[2], Event::Text("look.".into()), "only the new suffix");
        assert!(matches!(&ev[3], Event::ToolUse { name, .. } if name.ends_with("set_visibility")));
        assert!(
            matches!(&ev[4], Event::Permission { request_id, tool_use_id, .. } if request_id == "r1" && tool_use_id == "tu1")
        );
        assert_eq!(
            ev[5],
            Event::ToolResult {
                id: "tu1".into(),
                text: "Hid Sun (#3)".into(),
                is_error: false
            }
        );
        assert_eq!(
            ev[6],
            Event::Text("Done.".into()),
            "counters reset after tool results"
        );
        assert!(
            matches!(ev[7], Event::Result { cost_usd, turns: 2, .. } if (cost_usd - 0.012).abs() < 1e-9)
        );
    }

    #[test]
    fn error_results_become_errors() {
        let mut p = Parser::default();
        let ev = p.feed(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"API Error: 429 rate_limit_error"}"#);
        assert_eq!(
            ev,
            vec![Event::Error("API Error: 429 rate_limit_error".into())]
        );
    }

    #[test]
    fn responses_have_the_shapes_the_cli_expects() {
        let a = allow("r1", "tu1", &Value::Null);
        assert_eq!(a["response"]["response"]["updatedInput"], json!({}));
        assert_eq!(a["response"]["response"]["updatedPermissions"], json!([]));
        assert_eq!(a["response"]["response"]["toolUseID"], "tu1");
        let d = deny("r1", "tu1", "Skipped");
        assert_eq!(d["response"]["subtype"], "success");
        assert_eq!(d["response"]["response"]["behavior"], "deny");
        let m = user_message("", "hi", &[("image/png".into(), "AAAA".into())]);
        assert_eq!(
            m["message"]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert!(m["parent_tool_use_id"].is_null());
    }
}

#[cfg(test)]
mod flavor_tests {
    use super::*;

    #[test]
    fn codex_stream_maps_to_events() {
        let mut p = Parser::with_flavor(Flavor::Codex);
        assert_eq!(
            p.feed(r#"{"type":"thread.started","thread_id":"t1"}"#),
            vec![Event::Init {
                session_id: "t1".into(),
                tools: vec![],
                model: None
            }]
        );
        let started = p.feed(r#"{"type":"item.started","item":{"id":"c1","type":"mcp_tool_call","server":"emulsion","tool":"describe_document","arguments":{},"status":"in_progress"}}"#);
        assert!(
            matches!(&started[0], Event::ToolUse { id, name, .. } if id == "c1" && name == "mcp__emulsion__describe_document")
        );
        let done = p.feed(r#"{"type":"item.completed","item":{"id":"c1","type":"mcp_tool_call","server":"emulsion","tool":"describe_document","arguments":{},"status":"completed","result":{"content":[{"type":"text","text":"ok"}]}}}"#);
        assert!(matches!(&done[0], Event::ToolResult { id, is_error: false, .. } if id == "c1"));
        let msg = p.feed(
            r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"Done."}}"#,
        );
        assert_eq!(msg, vec![Event::Text("Done.".into())]);
        let end =
            p.feed(r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":3}}"#);
        assert!(matches!(
            &end[0],
            Event::Result {
                input_tokens: 10,
                output_tokens: 3,
                ..
            }
        ));
        assert!(p.saw_result);
    }

    #[test]
    fn opencode_stream_maps_to_events() {
        let mut p = Parser::with_flavor(Flavor::OpenCode);
        let e = p.feed(r#"{"type":"message.part.updated","sessionID":"s9","part":{"id":"p1","type":"text","text":"Hel"}}"#);
        assert!(matches!(&e[0], Event::Init { session_id, .. } if session_id == "s9"));
        assert_eq!(e[1], Event::Text("Hel".into()));
        let e = p.feed(
            r#"{"type":"message.part.updated","part":{"id":"p1","type":"text","text":"Hello"}}"#,
        );
        assert_eq!(e, vec![Event::Text("lo".into())]);
        let e = p.feed(r#"{"part":{"id":"t1","type":"tool","callID":"call1","tool":"emulsion_describe_document","state":{"status":"running","input":{"a":1}}}}"#);
        assert!(matches!(&e[0], Event::ToolUse { id, .. } if id == "call1"));
        let e = p.feed(r#"{"part":{"id":"t1","type":"tool","callID":"call1","tool":"emulsion_describe_document","state":{"status":"completed","input":{"a":1},"output":"fine"}}}"#);
        assert!(
            matches!(&e[0], Event::ToolResult { id, text, is_error: false } if id == "call1" && text == "fine")
        );
        let e = p.feed(r#"{"part":{"id":"s1","type":"step-finish","tokens":{"input":5,"output":2},"cost":0.01}}"#);
        assert!(
            e.is_empty() && p.usage == (5, 2, 0.01),
            "steps accumulate; exit ends the turn"
        );
        let e = p.feed(r#"{"type":"error","error":{"data":{"message":"boom"}}}"#);
        assert_eq!(e, vec![Event::Error("boom".into())]);
    }

    #[test]
    fn kimi_stream_maps_to_events() {
        let mut p = Parser::with_flavor(Flavor::Kimi);
        let e = p.feed(r#"{"role":"assistant","content":"Working on it","session_id":"k1"}"#);
        assert!(
            e.iter()
                .any(|x| matches!(x, Event::Init { session_id, .. } if session_id == "k1"))
        );
        assert!(e.iter().any(|x| *x == Event::Text("Working on it".into())));
        let e = p.feed(r#"{"role":"assistant","tool_calls":[{"id":"tc1","function":{"name":"emulsion__set_opacity","arguments":"{\"node\":3,\"opacity\":50}"}}]}"#);
        assert!(
            matches!(&e[0], Event::ToolUse { id, input, .. } if id == "tc1" && input["opacity"] == 50)
        );
        let e = p.feed(r#"{"role":"tool","tool_call_id":"tc1","content":"Set opacity"}"#);
        assert!(matches!(&e[0], Event::ToolResult { id, .. } if id == "tc1"));
        // A Claude-shaped result line is understood too.
        let e = p.feed(r#"{"type":"result","subtype":"success","result":"done","usage":{"input_tokens":1,"output_tokens":1}}"#);
        assert!(matches!(&e[0], Event::Result { .. }));
    }
}
