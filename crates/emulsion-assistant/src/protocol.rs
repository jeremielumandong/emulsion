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

#[derive(Default)]
pub struct Parser {
    last_text_len: usize,
    last_thinking_len: usize,
    seen_tools: HashSet<String>,
    pub session_id: Option<String>,
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
