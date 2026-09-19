//! The assistant surfaces inside the editor: the Ask bar (Ctrl+K), the dock
//! that shows a turn as tool cards with Apply/Skip confirmations, and the
//! suggestion strip.
//!
//! A typed request is planned first without a language model (keywords, or
//! Jev when a key is set). A plan that fully resolves is applied directly as
//! one history step. Anything else goes to the coding CLI, whose tool calls
//! reach the document through the relay and land in one history step per
//! turn.

use crate::app_state::{self, CliStatus};
use crate::editor::EditorView;
use crate::theme::{MONO_FONT, Palette};
use crate::widgets::{button, chip, label, mono};
use emulsion_ai::decide::{Decide, Keywords};
use emulsion_ai::jev::{Jev, JevDecider};
use emulsion_ai::palette;
use emulsion_ai::suggest::{self, Suggestion};
use emulsion_assistant::{Event, ProdLauncher, Session, launch};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node, NodeKind};
use emulsion_mcp::relay::{Relay, RelayCall};
use emulsion_mcp::{exec, tools};
use gpui_kit::TestSupportExt;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum CardStatus {
    Running,
    Waiting,
    Done,
    Failed(String),
    Skipped,
}

#[derive(Clone, Debug)]
pub struct ToolCard {
    pub id: String,
    pub tool: String,
    pub summary: String,
    pub status: CardStatus,
}

#[derive(Clone, Debug)]
pub struct Pending {
    pub request_id: String,
    pub tool_use_id: String,
    pub input: Value,
}

#[derive(Clone, Debug, Default)]
pub struct Turn {
    pub prompt: String,
    pub text: String,
    pub thinking: bool,
    pub cards: Vec<ToolCard>,
    pub pending: Vec<Pending>,
    pub started: Option<Instant>,
    /// Wall time and cost once finished.
    pub done: Option<(Duration, f64)>,
    pub error: Option<String>,
    /// Resolved without the CLI; `Some(decider name)`.
    pub local: Option<&'static str>,
    /// A tool ran since the last text.
    pub text_break: bool,
}

#[derive(Default)]
pub struct Assistant {
    relay: Option<Relay>,
    session: Option<Session>,
    session_id: Option<String>,
    session_dir: Option<PathBuf>,
    pub turn: Option<Turn>,
    pub running: bool,
    pub history: Vec<Turn>,
    pub cost: f64,
    pub show_transcript: bool,
    pub dock_open: bool,
    /// A `paint` call being played back stroke by stroke.
    pub(crate) playback: Option<Playback>,
}

/// The assistant painting live on the canvas.
pub(crate) struct Playback {
    call: Option<RelayCall>,
    script: exec::PaintScript,
    stroke: usize,
    point: usize,
    current: Option<Box<emulsion_raster::paint::Stroke>>,
    /// Layer pixels advanced per tick.
    speed: f32,
    carry: f32,
    /// Where the brush is within the current segment, in layer pixels.
    pos: Option<(f32, f32)>,
    /// Ghost brush position in document pixels and its size.
    pub cursor: Option<((f64, f64), f32)>,
}

pub struct AskBar {
    pub state: Entity<InputState>,
    _sub: Subscription,
}

fn short(text: &str) -> String {
    let w: Vec<&str> = text.split_whitespace().take(6).collect();
    let s = w.join(" ");
    if text.split_whitespace().count() > 6 {
        format!("{s}…")
    } else {
        s
    }
}

fn node_name(doc: &Document, v: &Value) -> String {
    v.as_u64()
        .and_then(|id| doc.node(id))
        .map(|n| n.name.clone())
        .unwrap_or_else(|| format!("#{v}"))
}

/// One line describing a tool call, in the person's terms.
pub fn summarize(doc: &Document, tool: &str, input: &Value) -> String {
    let n = || node_name(doc, &input["node"]);
    match tool {
        "describe_document" => "read the document".into(),
        "get_view" => "look at the image".into(),
        "set_visibility" => format!(
            "{} {}",
            if input["visible"].as_bool() == Some(true) {
                "show"
            } else {
                "hide"
            },
            n()
        ),
        "rename_node" => format!("rename {} → {}", n(), input["name"].as_str().unwrap_or("?")),
        "set_opacity" => format!("{} opacity {}%", n(), input["opacity"]),
        "set_blend_mode" => format!("{} → {}", n(), input["mode"].as_str().unwrap_or("?")),
        "move_node" => format!("move {}", n()),
        "group_nodes" => format!(
            "group {} nodes",
            input["nodes"].as_array().map_or(0, |a| a.len())
        ),
        "ungroup" => format!("ungroup {}", n()),
        "delete_node" => format!("delete {}", n()),
        "duplicate_node" => format!("duplicate {}", n()),
        "add_adjustment" => format!(
            "add {}",
            input["kind"]
                .as_str()
                .unwrap_or("adjustment")
                .replace('_', " ")
        ),
        "set_adjustment" => format!("adjust {} {}", n(), input["params"]),
        "set_transform" => format!("place {}", n()),
        "select_rect" | "select_ellipse" => format!(
            "select {} {}×{} at {}, {}",
            if tool == "select_rect" {
                "rectangle"
            } else {
                "ellipse"
            },
            input["width"],
            input["height"],
            input["x"],
            input["y"]
        ),
        "select_color" => format!("select similar colour at {}, {}", input["x"], input["y"]),
        "select_all" => "select everything".into(),
        "invert_selection" => "invert the selection".into(),
        "modify_selection" => "adjust the selection edge".into(),
        "content_aware_fill" => "fill the selection from its surroundings".into(),
        "fill_selection" => format!(
            "fill {} with {}",
            n(),
            input["color"].as_str().unwrap_or("?")
        ),
        "crop" => format!("crop to {}×{}", input["width"], input["height"]),
        "image_size" => format!("resize to {} px wide", input["width"]),
        "canvas_size" => format!("canvas {}×{}", input["width"], input["height"]),
        "add_layer" => format!("add layer {}", input["name"].as_str().unwrap_or("Layer")),
        "list_recipes" => "look at the recipes".into(),
        "apply_recipe" => format!(
            "apply recipe {}",
            input["name"].as_str().unwrap_or("from text")
        ),
        "draw_path" => format!("draw path {}", input["name"].as_str().unwrap_or("Path")),
        "set_path" => format!("edit path {}", n()),
        "path_to_selection" => format!("select inside {}", n()),
        "list_brushes" => "look at the brushes".into(),
        "paint" => format!(
            "paint {} stroke{} with {}",
            input["strokes"].as_array().map_or(0, |a| a.len()),
            if input["strokes"].as_array().is_some_and(|a| a.len() == 1) {
                ""
            } else {
                "s"
            },
            input["brush"].as_str().unwrap_or("custom settings")
        ),
        "select_node" => format!("select {}", n()),
        "transform_selection" => "move or resize the selection".into(),
        "list_history" => "read the history".into(),
        "create_branch" => format!("start branch {}", input["name"].as_str().unwrap_or("?")),
        "switch_branch" => format!("switch to {}", input["name"].as_str().unwrap_or("?")),
        "compare" => "compare two versions".into(),
        "merge_branch" => format!("merge {}", input["branch"].as_str().unwrap_or("?")),
        other => other.replace('_', " "),
    }
}

fn strip_prefix(tool: &str) -> String {
    tool.rsplit("__").next().unwrap_or(tool).to_string()
}

impl EditorView {
    // ── Ask bar ─────────────────────────────────────────────────────────

    pub fn open_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = &self.ask {
            bar.state.update(cx, |s, cx| s.focus(window, cx));
            return;
        }
        let state = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                "Ask Emulsion — e.g. hide the top two nodes and rename the third to Sky",
            )
        });
        state.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe_in(&state, window, |this, st, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let text = st.read(cx).value().to_string();
                if !text.trim().is_empty() {
                    this.close_ask(window, cx);
                    this.submit_ask(text.trim().to_string(), cx);
                }
            }
        });
        self.ask = Some(AskBar { state, _sub: sub });
        cx.notify();
    }

    /// Close the bar and give the keyboard back to the canvas, so shortcuts
    /// keep working without a click.
    pub fn close_ask(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask = None;
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    /// Plan without a language model; apply if complete, else hand over.
    pub fn submit_ask(&mut self, text: String, cx: &mut Context<Self>) {
        let doc = self.editor.doc.clone();
        let key = app_state::settings(cx).jev_key().map(|(k, _)| k);
        self.set_status(
            if key.is_some() {
                "Planning with Jev…"
            } else {
                "Planning…"
            },
            false,
            cx,
        );
        let t = text.clone();
        cx.spawn(async move |this, cx| {
            let (plan, note) = cx
                .background_spawn(async move {
                    if let Some(k) = key {
                        match palette::plan(&t, &doc, &JevDecider { jev: Jev::new(k) }) {
                            Ok(p) => return (Some(p), None),
                            Err(e) => {
                                let fallback = palette::plan(&t, &doc, &Keywords).ok();
                                return (
                                    fallback,
                                    Some(format!("Jev unavailable ({e}); used keywords")),
                                );
                            }
                        }
                    }
                    (palette::plan(&t, &doc, &Keywords).ok(), None)
                })
                .await;
            this.update(cx, |this, cx| match plan {
                Some(p) if p.is_complete() => this.apply_plan(&text, p, note, cx),
                _ => {
                    if let Err(e) = this.start_turn(text.clone(), cx) {
                        this.set_status(e, true, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_plan(
        &mut self,
        text: &str,
        plan: palette::Plan,
        note: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.editor.begin(format!("Ask: {}", short(text)));
        let mut cards = Vec::new();
        let mut error = None;
        for (cmd, summary) in plan.steps.into_iter().zip(plan.summary) {
            match self.editor.execute(cmd) {
                Ok(_) => cards.push(ToolCard {
                    id: String::new(),
                    tool: String::new(),
                    summary,
                    status: CardStatus::Done,
                }),
                Err(e) => {
                    error = Some(e.to_string());
                    cards.push(ToolCard {
                        id: String::new(),
                        tool: String::new(),
                        summary,
                        status: CardStatus::Failed(e.to_string()),
                    });
                    break;
                }
            }
        }
        self.editor.end();
        let n = cards
            .iter()
            .filter(|c| c.status == CardStatus::Done)
            .count();
        let msg = format!(
            "Applied {n} change{} · {}{}",
            if n == 1 { "" } else { "s" },
            if plan.decider == "Jev" {
                "resolved by Jev"
            } else {
                "resolved offline"
            },
            note.map(|n| format!(" · {n}")).unwrap_or_default()
        );
        let turn = Turn {
            prompt: text.to_string(),
            text: msg.clone(),
            cards,
            done: Some((Duration::ZERO, 0.0)),
            error,
            local: Some(plan.decider),
            ..Default::default()
        };
        self.finish_into_history(turn);
        self.assistant.dock_open = true;
        self.after_change(cx);
        self.set_status(msg, false, cx);
    }

    fn finish_into_history(&mut self, turn: Turn) {
        self.assistant.history.push(turn.clone());
        if self.assistant.history.len() > 50 {
            self.assistant.history.remove(0);
        }
        self.assistant.turn = Some(turn);
    }

    // ── CLI session ─────────────────────────────────────────────────────

    fn ensure_session(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if self.assistant.session.is_some() {
            return Ok(());
        }
        let cli = match app_state::cli(cx) {
            CliStatus::Found { path, .. } => path,
            CliStatus::Checking => {
                return Err("Still looking for Claude Code; try again in a moment.".into());
            }
            CliStatus::Missing => {
                return Err(
                    "That needs the assistant, and Claude Code is not installed. See Settings."
                        .into(),
                );
            }
        };
        if self.assistant.relay.is_none() {
            let relay =
                Relay::start().map_err(|e| format!("Could not start the tool relay: {e}"))?;
            let calls = relay.calls.clone();
            cx.spawn(async move |this, cx| {
                while let Ok(call) = calls.recv().await {
                    if this.update(cx, |v, cx| v.run_tool(call, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
            self.assistant.relay = Some(relay);
        }
        let dir = self.assistant.session_dir.get_or_insert_with(|| {
            emulsion_io::recent::data_dir()
                .join("sessions")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    cx.entity_id().as_u64()
                ))
        });
        // The `mcp-serve` binary: this executable, unless overridden (tests and
        // development builds run from elsewhere).
        let exe = match std::env::var_os("EMULSION_EXE") {
            Some(p) => PathBuf::from(p),
            None => std::env::current_exe().map_err(|e| e.to_string())?,
        };
        let s = app_state::settings(cx);
        let opts = launch::Options {
            model: s.model.clone(),
            resume: self.assistant.session_id.clone(),
        };
        let relay_env = self
            .assistant
            .relay
            .as_ref()
            .map(|r| r.env())
            .unwrap_or_default();
        let spec = launch::claude(cli, dir, &exe, &relay_env, &opts).map_err(|e| e.to_string())?;
        let session = Session::start(&ProdLauncher, &spec)
            .map_err(|e| format!("Could not start Claude Code: {e}"))?;
        let events = session.events.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(ev) = events.recv().await {
                if this.update(cx, |v, cx| v.on_event(ev, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.assistant.session = Some(session);
        Ok(())
    }

    fn start_turn(&mut self, text: String, cx: &mut Context<Self>) -> Result<(), String> {
        if self.assistant.running {
            return Err("The assistant is still working on the last request.".into());
        }
        self.ensure_session(cx)?;
        self.editor.begin(format!("Assistant: {}", short(&text)));
        let sent = self.assistant.session.as_mut().map(|s| s.send(&text, &[]));
        if let Some(Err(e)) = sent {
            self.editor.end();
            self.assistant.session = None;
            return Err(format!("Could not reach Claude Code: {e}"));
        }
        self.assistant.running = true;
        self.assistant.dock_open = true;
        self.assistant.turn = Some(Turn {
            prompt: text,
            started: Some(Instant::now()),
            thinking: true,
            ..Default::default()
        });
        self.set_status("Assistant is working…", false, cx);
        Ok(())
    }

    fn end_turn(&mut self, cost: f64, error: Option<String>, cx: &mut Context<Self>) {
        if !self.assistant.running {
            return;
        }
        self.assistant.running = false;
        if self.editor.in_transaction() {
            self.editor.end();
        }
        if let Some(mut t) = self.assistant.turn.take() {
            let elapsed = t.started.map(|s| s.elapsed()).unwrap_or_default();
            t.done = Some((elapsed, cost));
            t.thinking = false;
            t.pending.clear();
            for c in &mut t.cards {
                if matches!(c.status, CardStatus::Running | CardStatus::Waiting) {
                    c.status = CardStatus::Skipped;
                }
            }
            t.error = error.clone();
            self.assistant.cost += cost;
            self.finish_into_history(t);
        }
        match error {
            Some(e) => self.set_status(e, true, cx),
            None => self.set_status(
                format!(
                    "Assistant done · ${cost:.3} this turn · ${:.3} total",
                    self.assistant.cost
                ),
                false,
                cx,
            ),
        }
        self.after_change(cx);
    }

    fn on_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        let doc = self.editor.doc.clone();
        match ev {
            Event::Init { session_id, .. } => {
                if !session_id.is_empty() {
                    self.assistant.session_id = Some(session_id);
                }
            }
            Event::Text(t) => {
                if let Some(turn) = &mut self.assistant.turn {
                    // A tool ran since the last text: this is a new message.
                    if turn.text_break && !turn.text.is_empty() && !turn.text.ends_with('\n') {
                        turn.text.push('\n');
                    }
                    turn.text_break = false;
                    turn.text.push_str(&t);
                    turn.thinking = false;
                }
            }
            Event::Thinking(_) => {
                if let Some(turn) = &mut self.assistant.turn {
                    turn.thinking = true;
                }
            }
            Event::ToolUse { id, name, input } => {
                let tool = strip_prefix(&name);
                if let Some(turn) = &mut self.assistant.turn {
                    turn.text_break = true;
                    turn.cards.push(ToolCard {
                        id,
                        summary: summarize(&doc, &tool, &input),
                        tool,
                        status: CardStatus::Running,
                    });
                }
            }
            Event::Permission {
                request_id,
                tool_name,
                input,
                tool_use_id,
            } => {
                let tool = strip_prefix(&tool_name);
                let settings = app_state::settings(cx);
                let auto = settings.approve_all
                    || (settings.auto_apply && !tools::DESTRUCTIVE.contains(&tool.as_str()));
                if auto {
                    if let Some(s) = &mut self.assistant.session {
                        let _ = s.allow(&request_id, &tool_use_id, &input);
                    }
                } else if let Some(turn) = &mut self.assistant.turn {
                    if let Some(c) = turn.cards.iter_mut().find(|c| c.id == tool_use_id) {
                        c.status = CardStatus::Waiting;
                    } else {
                        turn.cards.push(ToolCard {
                            id: tool_use_id.clone(),
                            summary: summarize(&doc, &tool, &input),
                            tool,
                            status: CardStatus::Waiting,
                        });
                    }
                    turn.pending.push(Pending {
                        request_id,
                        tool_use_id,
                        input,
                    });
                    self.assistant.dock_open = true;
                }
            }
            Event::ToolResult { id, text, is_error } => {
                if let Some(turn) = &mut self.assistant.turn
                    && let Some(c) = turn.cards.iter_mut().find(|c| c.id == id)
                {
                    c.status = if c.status == CardStatus::Skipped {
                        CardStatus::Skipped
                    } else if is_error {
                        CardStatus::Failed(text)
                    } else {
                        CardStatus::Done
                    };
                }
            }
            Event::Result { cost_usd, .. } => self.end_turn(cost_usd, None, cx),
            Event::Error(e) => self.end_turn(0.0, Some(format!("Assistant: {e}")), cx),
            Event::Exited(code) => {
                self.assistant.session = None;
                if self.assistant.running {
                    self.end_turn(
                        0.0,
                        Some(format!("Claude Code stopped unexpectedly (exit {code:?}).")),
                        cx,
                    );
                }
            }
            Event::Stderr(l) => tracing::debug!(target: "assistant", "{l}"),
        }
        cx.notify();
    }

    /// Run a relayed tool call against this document.
    fn run_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        if call.name == "get_view" {
            let doc = self.editor.doc.clone();
            let args = call.arguments.clone();
            cx.background_spawn(async move {
                let r = exec::view(&doc, &args).unwrap_or_else(|e| e);
                call.reply(r);
            })
            .detach();
            return;
        }
        if call.name == "paint"
            && app_state::settings(cx).show_drawing
            && self.assistant.playback.is_none()
        {
            match exec::paint_script(&self.editor.doc, &call.arguments) {
                Ok(script) => self.start_playback(Some(call), script, cx),
                Err(e) => call.reply(e),
            }
            return;
        }
        if tools::HEAVY.contains(&call.name.as_str()) {
            // Compute off the UI thread against a snapshot, then apply only
            // if nobody edited the document in the meantime.
            let (doc, rev) = (self.editor.doc.clone(), self.editor.revision);
            self.set_status("Working…", false, cx);
            cx.spawn(async move |this, cx| {
                let (name, args) = (call.name.clone(), call.arguments.clone());
                let planned = cx.background_spawn(async move { exec::plan_heavy(&doc, &name, &args) }).await;
                this.update(cx, |this, cx| {
                    this.status = None;
                    let r = match planned {
                        Err(e) => e,
                        Ok(_) if this.editor.revision != rev => {
                            emulsion_mcp::server::ToolResult::error("the document changed while this was computing; call the tool again")
                        }
                        Ok(p) => exec::apply(&mut this.editor, p),
                    };
                    call.reply(r);
                    this.after_change(cx);
                })
                .ok();
            })
            .detach();
            return;
        }
        let r = exec::execute(&mut self.editor, &call.name, &call.arguments);
        call.reply(r);
        self.after_change(cx);
    }

    // ── Live playback of the assistant's strokes ────────────────────────

    /// Play the strokes on the canvas over a few seconds, so the person
    /// sees the drawing happen; the model gets its answer at the end.
    pub(crate) fn start_playback(
        &mut self,
        call: Option<RelayCall>,
        script: exec::PaintScript,
        cx: &mut Context<Self>,
    ) {
        const TICK_MS: u64 = 16;
        const MIN_SPEED: f32 = 2400.0; // layer px per second
        const MAX_SECS: f32 = 6.0;
        let len = script.length().max(1.0);
        let speed = (len / MAX_SECS).max(MIN_SPEED) * TICK_MS as f32 / 1000.0;
        self.editor.begin(script.label.clone());
        self.assistant.playback = Some(Playback {
            call,
            script,
            stroke: 0,
            point: 0,
            current: None,
            speed,
            carry: 0.0,
            pos: None,
            cursor: None,
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(TICK_MS))
                    .await;
                let more = this.update(cx, |this, cx| this.playback_tick(cx));
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Advance the playback by one tick. Returns whether it continues.
    fn playback_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(pb) = &mut self.assistant.playback else {
            return false;
        };
        let id = pb.script.id;
        let Some(NodeKind::Raster { raster, .. }) = self.editor.doc.node(id).map(|n| &n.kind)
        else {
            self.finish_playback(Some("the layer disappeared while painting"), cx);
            return false;
        };
        let mut layer = raster.clone();
        let mut budget = pb.speed + pb.carry;
        let mut done = false;
        let mut dirty_all = emulsion_raster::IRect::default();
        let mut changed: Option<Arc<emulsion_raster::Raster>> = None;
        let scale = pb.script.to_doc.matrix2.determinant().abs().sqrt();
        loop {
            let Some(s) = pb.script.strokes.get(pb.stroke) else {
                done = true;
                break;
            };
            if pb.current.is_none() {
                pb.current = Some(Box::new(emulsion_raster::paint::Stroke::new(
                    layer.clone(),
                    s.brush,
                    s.ink.clone(),
                    pb.script.clip.clone(),
                )));
                pb.point = 0;
                pb.pos = None;
            }
            let stroke = pb.current.as_mut().expect("set above");
            let size = (s.brush.size as f64 * scale) as f32;
            let to_doc = pb.script.to_doc;
            // Feed points until the distance budget runs out, stepping part
            // way along a long segment so the brush visibly travels.
            while pb.point < s.points.len() {
                let (x, y, p) = s.points[pb.point];
                if let Some((px0, py0)) = pb.pos {
                    let d = (x - px0).hypot(y - py0);
                    if d > budget {
                        let t = budget / d;
                        let step = (px0 + (x - px0) * t, py0 + (y - py0) * t);
                        stroke.point_at(step.0, step.1, p, None);
                        pb.pos = Some(step);
                        let dp = to_doc.transform_point2(glam::dvec2(step.0 as f64, step.1 as f64));
                        pb.cursor = Some(((dp.x, dp.y), size));
                        budget = 0.0;
                        break;
                    }
                    budget -= d;
                }
                stroke.point_at(x, y, p, None);
                pb.pos = Some((x, y));
                pb.point += 1;
                let dp = to_doc.transform_point2(glam::dvec2(x as f64, y as f64));
                pb.cursor = Some(((dp.x, dp.y), size));
            }
            let finished = pb.point >= s.points.len();
            if finished {
                stroke.finish();
            }
            let (r, d) = stroke.render(&layer);
            if !d.is_empty() {
                layer = Arc::new(r);
                changed = Some(layer.clone());
                dirty_all = dirty_all.union(&d);
            }
            if finished {
                pb.current = None;
                pb.pos = None;
                pb.stroke += 1;
                if budget <= 0.0 {
                    break;
                }
            } else {
                break;
            }
        }
        pb.carry = if done { 0.0 } else { budget.min(pb.speed) };
        let label = pb.script.label.clone();
        if let Some(r) = changed {
            self.execute(
                Command::ReplacePixels {
                    id,
                    raster: r,
                    dirty: dirty_all,
                    label,
                },
                cx,
            );
        }
        if done {
            self.finish_playback(None, cx);
            return false;
        }
        cx.notify();
        true
    }

    fn finish_playback(&mut self, error: Option<&str>, cx: &mut Context<Self>) {
        let Some(pb) = self.assistant.playback.take() else {
            return;
        };
        if self.editor.in_transaction() {
            self.editor.end();
        }
        if let Some(call) = pb.call {
            call.reply(match error {
                Some(e) => emulsion_mcp::server::ToolResult::error(e),
                None => emulsion_mcp::server::ToolResult::text(pb.script.message),
            });
        }
        self.after_change(cx);
    }

    /// Ghost brush for the canvas overlay while the assistant paints.
    pub(crate) fn ghost_brush(&self) -> Option<((f64, f64), f32)> {
        self.assistant.playback.as_ref().and_then(|p| p.cursor)
    }

    /// Answer the confirmation at `i` (or all of them).
    fn answer(&mut self, i: Option<usize>, allow: bool, cx: &mut Context<Self>) {
        let Some(turn) = &mut self.assistant.turn else {
            return;
        };
        let picked: Vec<Pending> = match i {
            Some(i) if i < turn.pending.len() => vec![turn.pending.remove(i)],
            Some(_) => return,
            None => std::mem::take(&mut turn.pending),
        };
        for p in picked {
            if !allow && let Some(c) = turn.cards.iter_mut().find(|c| c.id == p.tool_use_id) {
                c.status = CardStatus::Skipped;
            }
            if allow && let Some(c) = turn.cards.iter_mut().find(|c| c.id == p.tool_use_id) {
                c.status = CardStatus::Running;
            }
            if let Some(s) = &mut self.assistant.session {
                let r = if allow {
                    s.allow(&p.request_id, &p.tool_use_id, &p.input)
                } else {
                    s.deny(
                        &p.request_id,
                        &p.tool_use_id,
                        "The person skipped this change. Do not retry it.",
                    )
                };
                if let Err(e) = r {
                    self.status = Some((format!("Could not reach Claude Code: {e}").into(), true));
                }
            }
        }
        cx.notify();
    }

    pub fn stop_assistant(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.assistant.session {
            let _ = s.interrupt();
        }
        self.answer(None, false, cx);
        self.set_status("Stopping the assistant…", false, cx);
    }

    // ── Suggestions ─────────────────────────────────────────────────────

    pub(crate) fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        if !app_state::settings(cx).suggestions {
            self.suggestions.clear();
            return;
        }
        if self.suggest_busy || self.suggest_rev == self.editor.revision {
            return;
        }
        self.suggest_busy = true;
        let rev = self.editor.revision;
        let doc = self.editor.doc.clone();
        cx.spawn(async move |this, cx| {
            let s = cx
                .background_spawn(async move { suggest::suggest(&doc) })
                .await;
            this.update(cx, |this, cx| {
                this.suggest_busy = false;
                this.suggest_rev = rev;
                this.suggestions = s;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Accepting a suggestion adds one labelled node: one undo step.
    pub fn accept_suggestion(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(s) = self.suggestions.get(i).cloned() else {
            return;
        };
        let mut node = Node::adjust(0, s.adjustment.clone());
        node.name = s.node_name.clone();
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            },
            cx,
        ) {
            self.selected = Some(id);
            self.suggestions.remove(i);
            self.set_status(
                format!("Added {} — tune it in the node panel", s.node_name),
                false,
                cx,
            );
        }
    }

    // ── Rendering ───────────────────────────────────────────────────────

    pub(crate) fn ask_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.ask.as_ref()?;
        let jev = app_state::settings(cx).jev_key().is_some();
        let cli = matches!(app_state::cli(cx), CliStatus::Found { .. });
        let route = match (jev, cli) {
            (true, true) => "Jev plans simple requests · Claude Code takes the rest",
            (true, false) => "Jev plans requests · no assistant for the rest",
            (false, true) => "simple requests resolve offline · Claude Code takes the rest",
            (false, false) => "simple requests resolve offline",
        };
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(8.))
                .border_b_1()
                .border_color(p.ink)
                .bg(p.panel)
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        this.close_ask(window, cx);
                    }
                }))
                .child(mono("ASK", 10., p.accent).flex_none())
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&bar.state).appearance(false).bordered(false)),
                )
                .child(mono(route, 9.5, p.muted).flex_none())
                .child(
                    chip("ask-close", "esc", false, p)
                        .on_click(cx.listener(|this, _, window, cx| this.close_ask(window, cx))),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn assistant_dock(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let a = &self.assistant;
        if !a.dock_open || a.turn.is_none() {
            return None;
        }
        let turn = a.turn.clone().unwrap_or_default();
        let running = a.running;
        let status = if running {
            if !turn.pending.is_empty() {
                "waiting for you".to_string()
            } else if turn.thinking && turn.text.is_empty() {
                "thinking…".to_string()
            } else {
                "working…".to_string()
            }
        } else if let Some(e) = &turn.error {
            e.chars().take(80).collect()
        } else if let Some(d) = turn.local {
            format!("done · {}", if d == "Jev" { "Jev" } else { "offline" })
        } else if let Some((t, c)) = turn.done {
            format!("done · {:.1} s · ${c:.3}", t.as_secs_f32())
        } else {
            String::new()
        };
        let who = if turn.local.is_some() {
            "PALETTE"
        } else {
            "CLAUDE CODE"
        };
        // Finished cards fold into a count once there are many, so the dock
        // stays a strip above the canvas instead of covering it.
        let shown: Vec<&ToolCard> = turn
            .cards
            .iter()
            .filter(|c| !matches!(c.tool.as_str(), "describe_document"))
            .collect();
        let done_total = shown
            .iter()
            .filter(|c| c.status == CardStatus::Done)
            .count();
        const KEEP_DONE: usize = 4;
        let mut done_seen = 0usize;
        let folded = done_total.saturating_sub(KEEP_DONE);
        let mut cards: Vec<AnyElement> = Vec::new();
        if folded > 0 {
            cards.push(
                div()
                    .flex()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(3.))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.soft_bg)
                    .font_family(MONO_FONT)
                    .text_size(px(10.5))
                    .child(div().text_color(p.ink).child("✓"))
                    .child(
                        div()
                            .text_color(p.muted)
                            .child(format!("{folded} more done · see transcript")),
                    )
                    .into_any_element(),
            );
        }
        cards.extend(
            shown
                .iter()
                .filter(|c| {
                    if c.status == CardStatus::Done {
                        done_seen += 1;
                        done_seen > folded
                    } else {
                        true
                    }
                })
                .map(|c| {
                    let (glyph, color) = match &c.status {
                        CardStatus::Running => ("…", p.muted),
                        CardStatus::Waiting => ("?", p.accent),
                        CardStatus::Done => ("✓", p.ink),
                        CardStatus::Failed(_) => ("✗", p.accent),
                        CardStatus::Skipped => ("–", p.muted),
                    };
                    div()
                        .flex()
                        .gap(px(6.))
                        .px(px(8.))
                        .py(px(3.))
                        .border_1()
                        .border_color(if c.status == CardStatus::Waiting {
                            p.accent
                        } else {
                            p.line
                        })
                        .bg(p.soft_bg)
                        .font_family(MONO_FONT)
                        .text_size(px(10.5))
                        .child(div().text_color(color).child(glyph))
                        .child(div().text_color(p.ink).child(c.summary.clone()))
                        .into_any_element()
                }),
        );
        let pending: Vec<AnyElement> = turn
            .pending
            .iter()
            .enumerate()
            .map(|(i, pd)| {
                let summary = turn
                    .cards
                    .iter()
                    .find(|c| c.id == pd.tool_use_id)
                    .map(|c| c.summary.clone())
                    .unwrap_or_default();
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(6.))
                    .border_1()
                    .border_color(p.accent)
                    .bg(p.panel)
                    .child(mono("CONFIRM", 9.5, p.accent))
                    .child(div().flex_1().text_size(px(12.5)).child(summary))
                    .child(
                        button(("apply", i), "Apply", true, p)
                            .py(px(4.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.answer(Some(i), true, cx)),
                            )
                            .test_support(),
                    )
                    .child(
                        button(("skip", i), "Skip", false, p)
                            .py(px(4.))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.answer(Some(i), false, cx)),
                            )
                            .test_support(),
                    )
                    .child(
                        button(("always", i), "Always", false, p)
                            .py(px(4.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                app_state::update_settings(cx, |s| s.approve_all = true);
                                this.answer(None, true, cx);
                                this.set_status(
                                    "Assistant changes now apply without asking. Change it in Settings.",
                                    false,
                                    cx,
                                );
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let many = turn.pending.len() > 1;
        let text: String = {
            let t = turn.text.trim();
            let n = t.chars().count();
            if n > 420 {
                format!("…{}", t.chars().skip(n - 420).collect::<String>())
            } else {
                t.to_string()
            }
        };
        let transcript = a.show_transcript.then(|| {
            div()
                .id("transcript")
                .flex()
                .flex_col()
                .gap(px(8.))
                .max_h(px(220.))
                .overflow_y_scroll()
                .border_t_1()
                .border_color(p.line)
                .pt(px(8.))
                .children(self.assistant.history.iter().rev().map(|t| {
                    let meta = match (t.local, t.done) {
                        (Some(d), _) => format!(
                            "{} · {} change(s)",
                            if d == "Jev" { "Jev" } else { "offline" },
                            t.cards.len()
                        ),
                        (None, Some((d, c))) => format!("{:.1} s · ${c:.3}", d.as_secs_f32()),
                        _ => String::new(),
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(mono(format!("> {}", t.prompt), 10.5, p.ink))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.muted)
                                .child(t.text.trim().to_string()),
                        )
                        .child(mono(meta, 9.5, p.muted))
                }))
        });
        let show_t = a.show_transcript;
        Some(
            div()
                .flex()
                .flex_none()
                .flex_col()
                .gap(px(8.))
                .px(px(16.))
                .py(px(10.))
                .border_t_1()
                .border_color(p.line)
                .bg(p.panel)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(label(who, p))
                        .child(
                            mono(format!("> {}", short(&turn.prompt)), 10.5, p.ink)
                                .whitespace_nowrap(),
                        )
                        .child(
                            mono(
                                status,
                                10.,
                                if turn.error.is_some() {
                                    p.accent
                                } else {
                                    p.muted
                                },
                            )
                            .whitespace_nowrap(),
                        )
                        .child(div().flex_1())
                        .child(
                            chip("transcript", "transcript", show_t, p).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.assistant.show_transcript =
                                        !this.assistant.show_transcript;
                                    cx.notify();
                                },
                            )),
                        )
                        .when(running, |d| {
                            d.child(
                                chip("stop", "stop", false, p).on_click(
                                    cx.listener(|this, _, _, cx| this.stop_assistant(cx)),
                                ),
                            )
                        })
                        .when(!running, |d| {
                            d.child(chip("dock-close", "close", false, p).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.assistant.dock_open = false;
                                    cx.notify();
                                },
                            )))
                        }),
                )
                .when(!text.is_empty(), |d| {
                    d.child(div().text_size(px(12.5)).text_color(p.ink).child(text))
                })
                .when(!cards.is_empty(), |d| {
                    d.child(
                        div()
                            .id("assistant-cards")
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .max_h(px(96.))
                            .overflow_y_scroll()
                            .children(cards),
                    )
                })
                .children(pending)
                .when(many, |d| {
                    d.child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                button("apply-all", "Apply all", true, p)
                                    .py(px(4.))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.answer(None, true, cx)),
                                    ),
                            )
                            .child(
                                button("always-apply", "Always apply", false, p)
                                    .py(px(4.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        app_state::update_settings(cx, |s| s.approve_all = true);
                                        this.answer(None, true, cx);
                                        this.set_status(
                                            "Assistant changes now apply without asking. Change it in Settings.",
                                            false,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                button("skip-all", "Skip all", false, p)
                                    .py(px(4.))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.answer(None, false, cx)),
                                    ),
                            ),
                    )
                })
                .children(transcript)
                .into_any_element(),
        )
    }

    pub(crate) fn suggestion_chips(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let ink = p.ink;
                div()
                    .id(("suggestion", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(11.))
                    .py(px(5.))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.soft_bg)
                    .text_size(px(12.))
                    .text_color(p.ink)
                    .cursor_pointer()
                    .hover(move |st| st.border_color(ink))
                    .on_click(cx.listener(move |this, _, _, cx| this.accept_suggestion(i, cx)))
                    .child(mono(format!("⌥{}", i + 1), 10., p.muted))
                    .child(s.label.clone())
                    .into_any_element()
            })
            .collect()
    }
}

/// For the Settings screen's "Test Jev" button.
pub fn test_jev(key: String) -> Result<String, String> {
    let nodes = vec![
        emulsion_ai::decide::NodeInfo {
            id: 2,
            row: 1,
            name: "Clouds".into(),
            kind: "pixels",
        },
        emulsion_ai::decide::NodeInfo {
            id: 1,
            row: 2,
            name: "Sky".into(),
            kind: "pixels",
        },
    ];
    let start = Instant::now();
    let d = JevDecider { jev: Jev::new(key) }
        .decide(&["hide the clouds".to_string()], &nodes)
        .map_err(|e| e.to_string())?;
    let c = d.first().ok_or("no answer")?;
    let target = c
        .targets
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, p)| format!("#{id} at {p:.2}"))
        .unwrap_or_default();
    Ok(format!(
        "Jev answered in {} ms: {} (confidence {:.2}), target {target}",
        start.elapsed().as_millis(),
        c.intent.key(),
        c.confidence
    ))
}

#[allow(dead_code)]
fn _assert_suggestion_is_clone(s: &Suggestion) -> Suggestion {
    s.clone()
}
