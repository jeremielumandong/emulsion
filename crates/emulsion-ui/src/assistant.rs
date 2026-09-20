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
use emulsion_assistant::review::{self, Completion, DrawingReview};
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
use std::collections::VecDeque;
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
    /// Visual inspection state and cost across bounded review continuations.
    review: DrawingReview,
    review_pending: bool,
    cost: f64,
}

#[derive(Default)]
pub struct Assistant {
    pub(crate) reference: Option<crate::reference::AttachedReference>,
    pub(crate) reference_loading: bool,
    pub(crate) reference_collapsed: bool,
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
    /// Document reads and mutations execute in arrival order, so inspection
    /// sees completed preceding edits and parallel writes retain each other.
    tool_busy: bool,
    tool_queue: VecDeque<RelayCall>,
    tool_generation: u64,
    tool_stopped: bool,
    tool_feedback_pending: usize,
    completion_pending: bool,
    /// A one-shot process can exit before queued tool work has drained.
    provider_exit: Option<Option<i32>>,
    /// Relayed tool calls waiting for the person's Apply/Skip, for CLIs
    /// that do not ask before running tools; keyed by card id.
    held: Vec<(String, RelayCall)>,
    held_counter: u64,
    /// Which provider the running session belongs to; a different choice
    /// in Settings ends it so the next request starts the chosen CLI.
    session_provider: Option<&'static str>,
    /// Phase (0–1) of the "working" shimmer, and whether its loop runs.
    anim: f32,
    anim_running: bool,
    /// Discards bookkeeping from previews belonging to an earlier user turn.
    turn_generation: u64,
}

/// "thinking…" / "working…" with a pulse and a highlight sweeping across
/// it, so it is plain the AI is busy. `phase` cycles 0–1.
fn working_badge(status: String, phase: f32, p: &Palette) -> AnyElement {
    let k = (phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
    let mut col = p.muted;
    col.l += (p.ink.l - p.muted.l) * k;
    col.a = 1.0;
    let band = 36.0f32;
    let travel = 110.0f32;
    let x = -band + (travel + band) * phase;
    let glow = p.accent.opacity(0.45);
    let clear = p.accent.opacity(0.0);
    div()
        .relative()
        .flex_none()
        .overflow_hidden()
        .child(mono(format!("✦ {status}"), 10., col).whitespace_nowrap())
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x))
                .w(px(band / 2.0))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(clear, 0.),
                    linear_color_stop(glow, 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x + band / 2.0))
                .w(px(band / 2.0))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(glow, 0.),
                    linear_color_stop(clear, 1.),
                )),
        )
        .into_any_element()
}

/// The CLI chosen in Settings.
fn provider(cx: &App) -> &'static emulsion_assistant::provider::Provider {
    emulsion_assistant::provider::by_id(&app_state::settings(cx).provider)
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
    pos: Option<(f32, f32, Option<f32>)>,
    revision_before: u64,
    tool_generation: u64,
    /// Ghost brush position in document pixels and its size.
    pub cursor: Option<((f64, f64), f32)>,
}

/// Interpolate the same effective pressure as the raster engine, then undo the
/// response curve before point_at applies it. Untimed script points without
/// pressure use full pressure.
fn playback_pressure(start: Option<f32>, end: Option<f32>, t: f32, curve: f32) -> Option<f32> {
    if start.is_none() && end.is_none() {
        return None;
    }
    let curve = if (curve - 1.0).abs() > 1e-3 {
        curve
    } else {
        1.0
    };
    let a = start.unwrap_or(1.0).clamp(0.0, 1.0).powf(curve);
    let b = end.unwrap_or(1.0).clamp(0.0, 1.0).powf(curve);
    Some((a + (b - a) * t).clamp(0.0, 1.0).powf(1.0 / curve))
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
        "get_reference_image" => "look at the reference".into(),
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
            "group {} layers",
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
        "save_recipe" => format!(
            "{} recipe {}",
            if input["overwrite"].as_bool().unwrap_or(false) {
                "update"
            } else {
                "save"
            },
            input["name"].as_str().unwrap_or("from current edits")
        ),
        "convert_to_smart" => format!("smart layer {}", n()),
        "add_style" => format!(
            "add {} to {}",
            input["kind"].as_str().unwrap_or("style").replace('_', " "),
            n()
        ),
        "set_style" => format!("tune style {} on {}", input["index"], n()),
        "remove_style" => format!("remove style {} from {}", input["index"], n()),
        "add_filter" => format!(
            "add {} to {}",
            input["kind"].as_str().unwrap_or("filter").replace('_', " "),
            n()
        ),
        "set_filter" => format!("tune filter {} on {}", input["index"], n()),
        "remove_filter" => format!("remove filter {} from {}", input["index"], n()),
        "apply_recipe" => format!(
            "apply recipe {}",
            input["name"].as_str().unwrap_or("from text")
        ),
        "draw_path" => format!("draw path {}", input["name"].as_str().unwrap_or("Path")),
        "set_path" => format!("edit path {}", n()),
        "path_to_selection" => format!("select inside {}", n()),
        "list_brushes" => "look at the brushes".into(),
        "hatch" => format!(
            "hatch {} with {}",
            n(),
            input["brush"].as_str().unwrap_or("the brush")
        ),
        "critique" => "critique the picture".into(),
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
            InputState::new(window, cx)
                .placeholder("Describe an edit or an image, then press Enter")
        });
        state.update(cx, |s, cx| s.focus(window, cx));
        let sub = cx.subscribe_in(&state, window, |this, st, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                let text = st.read(cx).value().to_string();
                if !text.trim().is_empty() {
                    if let Some(provider) = this.generate.ask_provider {
                        let cfg = crate::editor::generate_ui::config_for(provider, cx);
                        match this.generate_text(text, cfg, cx) {
                            Ok(()) => this.close_ask(window, cx),
                            Err(error) => this.set_status(error, true, cx),
                        }
                        return;
                    }
                    if this.assistant.reference_loading {
                        this.set_status(
                            "Wait for the reference image to finish loading.",
                            false,
                            cx,
                        );
                        return;
                    }
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
        if self.editor.in_transaction() && !self.assistant.running {
            self.set_status(
                "Finish the current edit before starting a request.",
                false,
                cx,
            );
            return;
        }
        if self.generate.busy {
            self.set_status("Wait for image generation to finish.", false, cx);
            return;
        }
        if self.assistant.running {
            self.set_status(
                "The assistant is still working on the last request.",
                false,
                cx,
            );
            return;
        }
        if self.assistant.reference_loading {
            self.set_status("Wait for the reference image to finish loading.", false, cx);
            return;
        }
        if self.assistant.reference.is_some() {
            if let Err(e) = self.start_turn(text, cx) {
                self.set_status(e, true, cx);
            }
            return;
        }
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
        if self.editor.in_transaction() {
            self.set_status(
                "Finish the current edit, then submit the request again.",
                false,
                cx,
            );
            return;
        }
        if self.generate.busy {
            self.set_status(
                "Wait for image generation to finish, then submit the edit again.",
                false,
                cx,
            );
            return;
        }
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
        let prov = provider(cx);
        if self.assistant.session.is_some() {
            if self.assistant.session_provider == Some(prov.id) {
                return Ok(());
            }
            // The person picked another CLI since this session started.
            if let Some(mut s) = self.assistant.session.take() {
                s.kill();
            }
            self.assistant.session_id = None;
        }
        let cli = match app_state::cli(cx) {
            CliStatus::Found { path, .. } => path,
            CliStatus::Checking => {
                return Err(format!(
                    "Still looking for {}; try again in a moment.",
                    prov.label
                ));
            }
            CliStatus::Missing => {
                return Err(format!(
                    "That needs the assistant, and {} is not installed. See Settings.",
                    prov.label
                ));
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
        let session = match prov.mode {
            emulsion_assistant::provider::Mode::Persistent => {
                let spec = launch::spec_for(prov, cli, dir, &exe, &relay_env, &opts, None)
                    .map_err(|e| e.to_string())?;
                Session::start(&ProdLauncher, &spec)
                    .map_err(|e| format!("Could not start {}: {e}", prov.label))?
            }
            emulsion_assistant::provider::Mode::OneShot => {
                let flavor = match prov.id {
                    "codex" => emulsion_assistant::protocol::Flavor::Codex,
                    "opencode" => emulsion_assistant::protocol::Flavor::OpenCode,
                    _ => emulsion_assistant::protocol::Flavor::Kimi,
                };
                let (dir, model) = (dir.clone(), opts.model.clone());
                Session::one_shot(
                    flavor,
                    Box::new(move |prompt: &str, resume: Option<String>| {
                        let opts = launch::Options {
                            model: model.clone(),
                            resume,
                        };
                        launch::spec_for(
                            prov,
                            cli.clone(),
                            &dir,
                            &exe,
                            &relay_env,
                            &opts,
                            Some(prompt),
                        )
                    }),
                )
            }
        };
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
        self.assistant.session_provider = Some(prov.id);
        Ok(())
    }

    fn start_turn(&mut self, text: String, cx: &mut Context<Self>) -> Result<(), String> {
        if self.editor.in_transaction() {
            return Err("Finish the current edit before starting the assistant.".into());
        }
        if self.generate.busy {
            return Err("Wait for image generation to finish.".into());
        }
        if self.assistant.reference_loading {
            return Err("Wait for the reference image to finish loading.".into());
        }
        if self.assistant.running {
            return Err("The assistant is still working on the last request.".into());
        }
        self.ensure_session(cx)?;
        self.editor.begin(format!("Assistant: {}", short(&text)));
        let prompt = crate::reference::reference_prompt(&text, self.assistant.reference.as_ref());
        let sent = self
            .assistant
            .session
            .as_mut()
            .map(|s| s.send(&prompt, &[]));
        if let Some(Err(e)) = sent {
            self.editor.end();
            self.assistant.session = None;
            return Err(format!("Could not reach {}: {e}", provider(cx).label));
        }
        self.assistant.running = true;
        self.assistant.tool_stopped = false;
        self.assistant.provider_exit = None;
        self.assistant.turn_generation = self.assistant.turn_generation.wrapping_add(1);
        self.assistant.dock_open = true;
        self.start_working_anim(cx);
        self.assistant.turn = Some(Turn {
            prompt: text,
            started: Some(Instant::now()),
            thinking: true,
            ..Default::default()
        });
        self.set_status("Assistant is working…", false, cx);
        Ok(())
    }

    /// Animate the dock's shimmer while a turn runs; stops itself when
    /// the turn ends.
    fn start_working_anim(&mut self, cx: &mut Context<Self>) {
        if self.assistant.anim_running {
            return;
        }
        self.assistant.anim_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let more = this.update(cx, |this, cx| {
                    if !this.assistant.running {
                        this.assistant.anim_running = false;
                        return false;
                    }
                    this.assistant.anim = (this.assistant.anim + 0.011).rem_euclid(1.0);
                    cx.notify();
                    true
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn end_turn(&mut self, cost: f64, error: Option<String>, cx: &mut Context<Self>) {
        if !self.assistant.running {
            return;
        }
        self.assistant.running = false;
        self.cancel_tool_work(
            "The assistant request ended before this change completed.",
            cx,
        );
        if self.editor.in_transaction() {
            self.editor.end();
        }
        let mut cost = cost;
        if let Some(mut t) = self.assistant.turn.take() {
            cost += t.cost;
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

    fn complete_provider_turn(&mut self, cost: f64, cx: &mut Context<Self>) {
        if !self.assistant.running {
            return;
        }
        let Some(turn) = &mut self.assistant.turn else {
            return;
        };
        turn.cost += cost;
        if self.assistant.tool_busy || self.assistant.tool_feedback_pending > 0 {
            self.assistant.completion_pending = true;
            return;
        }
        self.assistant.completion_pending = false;
        match turn.review.completion(self.editor.revision) {
            Completion::Finish => self.end_turn(0.0, None, cx),
            Completion::Unreviewed => {
                turn.text.push_str("\nThe final drawing was not visually inspected after its last change; the review limit was reached.");
                self.end_turn(0.0, None, cx);
            }
            Completion::Review => {
                turn.review_pending = true;
                // One-shot readers share parser state with the next process.
                // Wait for stdout to drain and Exited before resuming it.
                if !self
                    .assistant
                    .session
                    .as_ref()
                    .is_some_and(Session::is_one_shot)
                    || self.assistant.provider_exit == Some(Some(0))
                {
                    self.continue_visual_review(cx);
                } else if let Some(code) = self.assistant.provider_exit {
                    self.end_turn(
                        0.0,
                        Some(format!(
                            "Assistant stopped before the drawing review (exit {code:?})."
                        )),
                        cx,
                    );
                } else {
                    self.set_status("Preparing final drawing review…", false, cx);
                }
            }
        }
    }

    fn continue_visual_review(&mut self, cx: &mut Context<Self>) {
        let Some(turn) = &mut self.assistant.turn else {
            return;
        };
        if !self.assistant.running || !turn.review_pending {
            return;
        }
        turn.review_pending = false;
        turn.text_break = true;
        turn.thinking = true;
        let prompt = review::review_request(&turn.prompt);
        self.assistant.provider_exit = None;
        let result = self
            .assistant
            .session
            .as_mut()
            .ok_or_else(|| std::io::Error::other("assistant session is unavailable"))
            .and_then(|session| session.send(&prompt, &[]));
        match result {
            Ok(()) => self.set_status("Reviewing the drawing…", false, cx),
            Err(e) => self.end_turn(0.0, Some(format!("Could not review the drawing: {e}")), cx),
        }
    }

    fn observe_drawing_tool(
        &mut self,
        name: &str,
        args: &Value,
        result: &emulsion_mcp::server::ToolResult,
        revision: u64,
        changed: bool,
    ) {
        if self.assistant.running
            && let Some(turn) = &mut self.assistant.turn
        {
            turn.review.observe(name, args, result, revision, changed);
        }
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
            Event::Result { cost_usd, .. } => self.complete_provider_turn(cost_usd, cx),
            Event::Error(e) => self.end_turn(0.0, Some(format!("Assistant: {e}")), cx),
            Event::Exited(code) => {
                // One-shot CLIs exit after every turn; their Result or Error
                // came first. A persistent CLI leaving mid-turn is a failure.
                let one_shot = self
                    .assistant
                    .session
                    .as_ref()
                    .is_some_and(|s| s.is_one_shot());
                if one_shot {
                    self.assistant.provider_exit = Some(code);
                }
                if !one_shot {
                    self.assistant.session = None;
                }
                if one_shot
                    && self.assistant.running
                    && self
                        .assistant
                        .turn
                        .as_ref()
                        .is_some_and(|turn| turn.review_pending)
                {
                    if code == Some(0) {
                        self.continue_visual_review(cx);
                    } else {
                        self.end_turn(
                            0.0,
                            Some(format!(
                                "Assistant stopped before the drawing review (exit {code:?})."
                            )),
                            cx,
                        );
                    }
                }
                if self.assistant.running && !one_shot {
                    self.end_turn(
                        0.0,
                        Some(format!(
                            "{} stopped unexpectedly (exit {code:?}).",
                            provider(cx).label
                        )),
                        cx,
                    );
                }
            }
            Event::Stderr(l) => tracing::debug!(target: "assistant", "{l}"),
        }
        cx.notify();
    }

    /// Run a relayed tool call against this document, or hold it for the
    /// person's Apply/Skip when the CLI does not ask first itself.
    fn run_tool(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        if self.assistant.tool_stopped {
            call.reply(emulsion_mcp::server::ToolResult::error(
                "This assistant request has ended. Do not retry the change.",
            ));
            return;
        }
        let settings = app_state::settings(cx);
        let auto = settings.approve_all
            || (settings.auto_apply && !tools::DESTRUCTIVE.contains(&call.name.as_str()));
        let asks_itself = provider(cx).permission_prompts;
        if !asks_itself && !auto && !tools::READ_ONLY.contains(&call.name.as_str()) {
            let doc = self.editor.doc.clone();
            self.assistant.held_counter += 1;
            let id = format!("relay-{}", self.assistant.held_counter);
            if let Some(turn) = &mut self.assistant.turn {
                // Reuse the CLI's own card for this call when it made one.
                let existing = turn
                    .cards
                    .iter_mut()
                    .rev()
                    .find(|c| c.status == CardStatus::Running && c.tool == call.name);
                let card_id = match existing {
                    Some(c) => {
                        c.status = CardStatus::Waiting;
                        c.id.clone()
                    }
                    None => {
                        turn.cards.push(ToolCard {
                            id: id.clone(),
                            summary: summarize(&doc, &call.name, &call.arguments),
                            tool: call.name.clone(),
                            status: CardStatus::Waiting,
                        });
                        id.clone()
                    }
                };
                turn.pending.push(Pending {
                    request_id: card_id.clone(),
                    tool_use_id: card_id.clone(),
                    input: call.arguments.clone(),
                });
                self.assistant.held.push((card_id, call));
                self.assistant.dock_open = true;
                cx.notify();
                return;
            }
        }
        self.run_tool_now(call, cx);
    }

    fn run_tool_now(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        if self.assistant.tool_stopped {
            call.reply(emulsion_mcp::server::ToolResult::error(
                "This assistant request has ended. Do not retry the change.",
            ));
            return;
        }
        if Self::ordered_tool(&call.name) {
            if self.assistant.tool_busy {
                self.assistant.tool_queue.push_back(call);
                return;
            }
            self.assistant.tool_busy = true;
        }
        self.execute_tool_now(call, cx);
    }

    fn ordered_tool(name: &str) -> bool {
        // Discovery of brushes, fonts and the attached reference is independent
        // of document edits. Other reads can depend on preceding operations,
        // including list_models after a download or list_recipes after import.
        !matches!(name, "list_brushes" | "list_fonts" | "get_reference_image")
    }

    fn complete_tool_work(&mut self, generation: u64, cx: &mut Context<Self>) {
        if generation != self.assistant.tool_generation {
            return;
        }
        if let Some(call) = self.assistant.tool_queue.pop_front() {
            // Yield between document operations, and snapshot only when
            // the queued operation starts. Keep the queue reserved meanwhile.
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |this, cx| {
                    if generation == this.assistant.tool_generation {
                        this.execute_tool_now(call, cx);
                    } else {
                        call.reply(emulsion_mcp::server::ToolResult::error(
                            "The request was stopped before this change began. Do not retry it.",
                        ));
                    }
                });
            })
            .detach();
        } else {
            self.assistant.tool_busy = false;
            self.complete_deferred_provider_turn(cx);
        }
    }

    fn complete_deferred_provider_turn(&mut self, cx: &mut Context<Self>) {
        if self.assistant.completion_pending
            && !self.assistant.tool_busy
            && self.assistant.tool_feedback_pending == 0
        {
            self.complete_provider_turn(0.0, cx);
        }
    }

    fn cancel_tool_work(&mut self, reason: &str, cx: &mut Context<Self>) {
        self.assistant.tool_generation = self.assistant.tool_generation.wrapping_add(1);
        self.assistant.tool_busy = false;
        self.assistant.tool_stopped = true;
        self.assistant.tool_feedback_pending = 0;
        self.assistant.completion_pending = false;
        for call in self.assistant.tool_queue.drain(..) {
            call.reply(emulsion_mcp::server::ToolResult::error(reason));
        }
        self.finish_playback(Some(reason), cx);
    }

    fn execute_tool_now(&mut self, call: RelayCall, cx: &mut Context<Self>) {
        let tool_generation = self.assistant.tool_generation;
        let ordered = Self::ordered_tool(&call.name);
        if call.name == "get_reference_image" {
            call.reply(self.reference_result());
            return;
        }
        if matches!(call.name.as_str(), "get_view" | "critique" | "list_brushes") {
            let doc = self.editor.doc.clone();
            let args = call.arguments.clone();
            let name = call.name.clone();
            let revision = self.editor.revision;
            let generation = self.assistant.turn_generation;
            cx.spawn(async move |this, cx| {
                let r = cx
                    .background_spawn(async move {
                        exec::inspect(&doc, &name, &args).unwrap_or_else(|e| e)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.assistant.turn_generation != generation
                        || this.assistant.tool_generation != tool_generation
                    {
                        call.reply(emulsion_mcp::server::ToolResult::error(
                            "The assistant request ended while this inspection was computing. Do not use this preview or retry automatically.",
                        ));
                        return;
                    }
                    this.observe_drawing_tool(&call.name, &call.arguments, &r, revision, false);
                    call.reply(r);
                    if ordered {
                        this.complete_tool_work(tool_generation, cx);
                    }
                });
            })
            .detach();
            return;
        }
        if matches!(call.name.as_str(), "paint" | "hatch")
            && app_state::settings(cx).show_drawing
            && self.assistant.playback.is_none()
        {
            match exec::paint_script_for(&self.editor.doc, &call.name, &call.arguments) {
                Ok(script) => self.start_playback(Some(call), script, cx),
                Err(e) => {
                    call.reply(e);
                    self.complete_tool_work(tool_generation, cx);
                }
            }
            return;
        }
        if tools::HEAVY.contains(&call.name.as_str()) {
            // Compute off the UI thread against a snapshot, then apply only
            // if nobody edited the document in the meantime.
            let (doc, rev) = (self.editor.doc.clone(), self.editor.revision);
            let generation = self.assistant.turn_generation;
            self.set_status("Working…", false, cx);
            cx.spawn(async move |this, cx| {
                let (name, args) = (call.name.clone(), call.arguments.clone());
                let planned = cx.background_spawn(async move { exec::plan_heavy(&doc, &name, &args) }).await;
                this.update(cx, |this, cx| {
                    if this.assistant.tool_generation == tool_generation {
                        this.status = None;
                    }
                    let r = match planned {
                        Err(e) => e,
                        Ok(_) if this.assistant.turn_generation != generation
                            || this.assistant.tool_generation != tool_generation => {
                            emulsion_mcp::server::ToolResult::error("the assistant request ended while this was computing; the change was not applied")
                        }
                        Ok(_) if this.editor.revision != rev => {
                            emulsion_mcp::server::ToolResult::error("the document changed while this was computing; call the tool again")
                        }
                        Ok(p) => exec::apply(&mut this.editor, p),
                    };
                    this.observe_drawing_tool(&call.name, &call.arguments, &r, this.editor.revision, this.editor.revision != rev);
                    call.reply(r);
                    this.after_change(cx);
                    this.complete_tool_work(tool_generation, cx);
                })
                .ok();
            })
            .detach();
            return;
        }
        let before = self.editor.revision;
        let r = exec::execute(&mut self.editor, &call.name, &call.arguments);
        self.observe_drawing_tool(
            &call.name,
            &call.arguments,
            &r,
            self.editor.revision,
            self.editor.revision != before,
        );
        call.reply(r);
        self.after_change(cx);
        if ordered {
            self.complete_tool_work(tool_generation, cx);
        }
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
            revision_before: self.editor.revision,
            tool_generation: self.assistant.tool_generation,
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
                pb.current = Some(Box::new(pb.script.start_stroke(layer.clone(), s)));
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
                if let Some((px0, py0, pressure0)) = pb.pos {
                    let d = (x - px0).hypot(y - py0);
                    if d > budget {
                        let t = budget / d;
                        let pressure = playback_pressure(pressure0, p, t, s.brush.pressure_curve);
                        let step = (px0 + (x - px0) * t, py0 + (y - py0) * t, pressure);
                        stroke.point_at(step.0, step.1, step.2, None);
                        pb.pos = Some(step);
                        let dp = to_doc.transform_point2(glam::dvec2(step.0 as f64, step.1 as f64));
                        pb.cursor = Some(((dp.x, dp.y), size));
                        budget = 0.0;
                        break;
                    }
                    budget -= d;
                }
                stroke.point_at(x, y, p, None);
                pb.pos = Some((x, y, p));
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
            match error {
                Some(e) => call.reply(emulsion_mcp::server::ToolResult::error(e)),
                None => {
                    let doc = self.editor.doc.clone();
                    let revision = self.editor.revision;
                    let changed = revision != pb.revision_before;
                    let message = if changed {
                        pb.script.message
                    } else {
                        "No pixels changed. Check selection, alpha lock and stroke placement before changing the drawing.".into()
                    };
                    let generation = self.assistant.turn_generation;
                    let tool_generation = pb.tool_generation;
                    self.assistant.tool_feedback_pending += 1;
                    cx.spawn(async move |this, cx| {
                        let result = cx
                            .background_spawn(async move { exec::paint_feedback(&doc, message) })
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            if this.assistant.turn_generation == generation
                                && this.assistant.tool_generation == tool_generation
                            {
                                this.observe_drawing_tool(
                                    &call.name,
                                    &call.arguments,
                                    &result,
                                    revision,
                                    changed,
                                );
                                this.assistant.tool_feedback_pending -= 1;
                                this.complete_deferred_provider_turn(cx);
                            }
                        });
                        call.reply(result);
                    })
                    .detach();
                }
            }
            self.complete_tool_work(pb.tool_generation, cx);
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
        let mut release: Vec<(bool, RelayCall)> = Vec::new();
        for p in picked {
            if !allow && let Some(c) = turn.cards.iter_mut().find(|c| c.id == p.tool_use_id) {
                c.status = CardStatus::Skipped;
            }
            if allow && let Some(c) = turn.cards.iter_mut().find(|c| c.id == p.tool_use_id) {
                c.status = CardStatus::Running;
            }
            // A call held at the relay runs (or is refused) here.
            if let Some(i) = self
                .assistant
                .held
                .iter()
                .position(|(id, _)| *id == p.tool_use_id)
            {
                let (_, call) = self.assistant.held.remove(i);
                release.push((allow, call));
                continue;
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
                    self.status =
                        Some((format!("Could not reach the assistant: {e}").into(), true));
                }
            }
        }
        for (allow, call) in release {
            if allow {
                self.run_tool_now(call, cx);
            } else {
                call.reply(emulsion_mcp::server::ToolResult::error(
                    "The person skipped this change. Do not retry it.",
                ));
            }
        }
        cx.notify();
    }

    pub fn stop_assistant(&mut self, cx: &mut Context<Self>) {
        let mut finished = self.assistant.completion_pending;
        self.cancel_tool_work(
            "The person stopped this request. Do not retry this change.",
            cx,
        );
        if let Some(turn) = &mut self.assistant.turn {
            turn.review.stop();
            // A one-shot provider may already have completed its Result and
            // be waiting to exit. End here so cancelling cannot start a review.
            if turn.review_pending {
                turn.review_pending = false;
                finished = true;
            }
        }
        if finished {
            self.end_turn(0.0, None, cx);
        }
        if let Some(s) = &mut self.assistant.session {
            let _ = s.interrupt();
        }
        self.answer(None, false, cx);
        self.set_status(
            if finished {
                "Assistant stopped."
            } else {
                "Stopping the assistant…"
            },
            false,
            cx,
        );
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
        // Model-backed proposals need a small picture and what is installed.
        let tree = self.tree.clone();
        let faces_possible = emulsion_ai::face::detector_available().is_some()
            && emulsion_ai::face::available().is_some();
        let lens_possible = doc.info.is_some() && emulsion_io::lensfun::installed();
        let jev_key = app_state::settings(cx).jev_key().map(|(k, _)| k);
        cx.spawn(async move |this, cx| {
            let (s, kind) = cx
                .background_spawn(async move {
                    let kind = match &jev_key {
                        Some(k) => emulsion_ai::kind::classify_with_jev(
                            &doc,
                            &emulsion_ai::jev::Jev::new(k.clone()),
                        ),
                        None => emulsion_ai::kind::classify(&doc),
                    };
                    // Photographic proposals only for photographs (or when unsure).
                    let photographic = kind.kind.is_photographic() || kind.confidence < 0.5;
                    let lens_possible = lens_possible && photographic;
                    let faces_possible = faces_possible && photographic;
                    let mut out = suggest::suggest(&doc);
                    if lens_possible
                        && let Some(info) = &doc.info
                        && let Some(db) = emulsion_io::lensfun::Database::shared()
                        && let Some(p) = emulsion_io::lensfun::profile_for(
                            &db,
                            &info.make,
                            &info.model,
                            &info.lens,
                            info.focal_mm,
                            info.f_number,
                        )
                        && !doc.nodes.iter().any(|n| {
                            matches!(&n.kind, NodeKind::Smart { filters, .. }
                                if filters.iter().any(|f| f.key() == "lens_profile"))
                        })
                    {
                        out.push(suggest::Suggestion::action(
                            format!("Correct lens: {}", p.lens),
                            "lens_profile",
                        ));
                    }
                    if faces_possible
                        && !doc
                            .nodes
                            .iter()
                            .any(|n| n.name.starts_with("Faces restored"))
                    {
                        // Detect on a small composite: a few hundred ms.
                        let (w, h, bgra) = crate::editor::doc_thumb(&doc, 640);
                        let _ = tree;
                        let rgba: Vec<u8> = bgra
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .flat_map(|p| [p[2], p[1], p[0], p[3]])
                            .collect();
                        let small = emulsion_raster::Raster::from_srgba8(w, h, &rgba);
                        let job = emulsion_ai::jobs::Job::new();
                        if let Ok(faces) = emulsion_ai::face::detect(&small, &job)
                            && !faces.is_empty()
                        {
                            // Only small faces gain from restoration.
                            let biggest = faces
                                .iter()
                                .map(|f| (f.x1 - f.x0).max(f.y1 - f.y0))
                                .fold(0.0f32, f32::max)
                                / w.min(h).max(1) as f32;
                            if biggest < 0.45 {
                                out.push(suggest::Suggestion::action(
                                    format!(
                                        "Restore {} face{}",
                                        faces.len(),
                                        if faces.len() == 1 { "" } else { "s" }
                                    ),
                                    "restore_faces",
                                ));
                            }
                        }
                    }
                    (out, kind)
                })
                .await;
            this.update(cx, |this, cx| {
                this.suggest_busy = false;
                this.suggest_rev = rev;
                this.suggestions = s;
                this.doc_kind = Some(kind);
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
        if let suggest::Kind::Action(action) = &s.kind {
            self.suggestions.remove(i);
            match action.as_str() {
                "lens_profile" => {
                    // Aim at the photo's pixel node.
                    if self.selected.is_none() {
                        self.selected = self
                            .editor
                            .doc
                            .nodes
                            .iter()
                            .rev()
                            .find(|n| {
                                matches!(n.kind, NodeKind::Raster { .. } | NodeKind::Smart { .. })
                            })
                            .map(|n| n.id);
                    }
                    self.lens_profile_auto(cx);
                }
                "restore_faces" => self.restore_faces(cx),
                "remove_background" => self.remove_background(cx),
                "select_subject" => self.select_subject(cx),
                _ => {}
            }
            return;
        }
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
                format!("Added {} — tune it in the Layers panel", s.node_name),
                false,
                cx,
            );
        }
    }

    // ── Rendering ───────────────────────────────────────────────────────

    pub(crate) fn ask_bar(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        use emulsion_ai::generate::Provider;
        let state = self.ask.as_ref()?.state.clone();
        let chosen = self.generate.ask_provider;
        let route: String = match chosen {
            Some(Provider::A1111) => {
                "Generate locally · selection fills; otherwise adds a layer".into()
            }
            Some(provider) => format!(
                "{} receives the prompt and selected canvas · usage is billed",
                provider.label()
            ),
            None => {
                let cli = matches!(app_state::cli(cx), CliStatus::Found { .. });
                let planner = if app_state::settings(cx).jev_key().is_some() {
                    "Jev"
                } else {
                    "Offline"
                };
                if cli {
                    format!(
                        "{planner} edits · {} handles other requests",
                        provider(cx).label
                    )
                } else {
                    format!(
                        "{planner} edits · configure an assistant in Settings for other requests"
                    )
                }
            }
        };
        let mut modes = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(mono("ASK", 10., p.accent));
        for (id, title, choice) in [
            ("ask-assistant", "Assistant", None),
            ("ask-local", "Local SD", Some(Provider::A1111)),
            ("ask-openai", "OpenAI", Some(Provider::OpenAi)),
            ("ask-google", "Google", Some(Provider::Google)),
        ] {
            modes = modes.child(
                chip(id, title, chosen == choice, p)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.generate.ask_provider = choice;
                        if let Some(bar) = &this.ask {
                            bar.state.update(cx, |state, cx| state.focus(window, cx));
                        }
                        cx.notify();
                    }))
                    .test_support(),
            );
        }
        modes = modes.child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(mono(route, 9.5, p.muted)),
        );
        let mut input = div().flex().items_center().gap(px(10.)).child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&state).appearance(false).bordered(false)),
        );
        if chosen.is_none() {
            input = input.child(
                chip("ask-reference", "Add reference", false, p)
                    .on_click(cx.listener(|this, _, window, cx| this.prompt_reference(window, cx)))
                    .test_support(),
            );
        }
        input = input.child(
            chip("ask-close", "esc", false, p)
                .on_click(cx.listener(|this, _, window, cx| this.close_ask(window, cx))),
        );
        Some(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap(px(6.))
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
                .child(modes)
                .child(input)
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
        let who: SharedString = if turn.local.is_some() {
            "PALETTE".into()
        } else {
            provider(cx).label.to_uppercase().into()
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
        let phase = a.anim;
        // An indeterminate progress line along the top while the AI works.
        let progress = running.then(|| {
            div()
                .relative()
                .h(px(2.))
                .w_full()
                .flex_none()
                .overflow_hidden()
                .bg(p.line.opacity(0.4))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(phase * 1.35 - 0.35))
                        .w(relative(0.35))
                        .bg(linear_gradient(
                            90.,
                            linear_color_stop(p.accent.opacity(0.0), 0.),
                            linear_color_stop(p.accent, 1.),
                        )),
                )
        });
        let status_el = if running {
            working_badge(status, phase, p)
        } else {
            mono(
                status,
                10.,
                if turn.error.is_some() {
                    p.accent
                } else {
                    p.muted
                },
            )
            .whitespace_nowrap()
            .into_any_element()
        };
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
                .children(progress)
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
                        .child(status_el)
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

#[cfg(test)]
mod mutation_queue_tests {
    use super::*;
    use crate::theme;
    use core::prelude::v1::test;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    // Use the real local relay so tests exercise replies as well as pixels.
    fn call(relay: &Relay, name: &str, args: Value) -> (RelayCall, std::thread::JoinHandle<Value>) {
        let (addr, token, name) = (relay.addr, relay.token.clone(), name.to_string());
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            writeln!(
                stream,
                "{}",
                serde_json::json!({
                    "token": token, "name": name, "arguments": args,
                })
            )
            .unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        });
        (relay.calls.recv_blocking().unwrap(), client)
    }

    fn painting(cx: &mut TestAppContext, live: bool) -> Entity<EditorView> {
        cx.update(|cx| {
            theme::install(cx);
            cx.set_global(app_state::AppSettings(emulsion_io::settings::Settings {
                show_drawing: live,
                suggestions: false,
                ..Default::default()
            }));
            cx.new(|cx| {
                let mut doc = Document::new(300, 100);
                Command::AddNode {
                    node: Box::new(Node::raster(
                        0,
                        "Ink",
                        Arc::new(emulsion_raster::Raster::transparent(300, 100)),
                        emulsion_raster::Placement::default(),
                    )),
                    slot: Slot::TOP,
                }
                .apply(&mut doc)
                .unwrap();
                let mut view = EditorView::new(doc, None, None, None, "test".into(), cx);
                view.editor.begin("Assistant drawing");
                view.assistant.running = true;
                let mut turn = Turn::default();
                // This test isolates transport/painting from optional review.
                turn.review.stop();
                view.assistant.turn = Some(turn);
                view
            })
        })
    }

    fn stroke(y: u32, color: &str) -> Value {
        serde_json::json!({
            "node": 1, "brush": "Maru pen", "color": color,
            "settings": {"size": 8, "hardness": 1, "opacity": 1, "flow": 1,
                "size_pressure": 0, "taper_start": 0, "taper_end": 0},
            "strokes": [{"points": [[10, y], [290, y]]}]
        })
    }

    #[gpui_kit::test]
    fn parallel_paint_calls_keep_both_results_before_provider_completion(cx: &mut TestAppContext) {
        for live in [true, false] {
            let relay = Relay::start().unwrap();
            let view = painting(cx, live);
            let (first, first_reply) = call(&relay, "paint", stroke(25, "#ff0000"));
            let (second, second_reply) = call(&relay, "paint", stroke(75, "#0000ff"));
            let (inspect, inspect_reply) = call(&relay, "describe_document", serde_json::json!({}));
            view.update(cx, |view, cx| {
                view.run_tool_now(first, cx);
                view.run_tool_now(second, cx);
                view.run_tool_now(inspect, cx);
                assert_eq!(view.assistant.tool_queue.len(), 2);
                // A prematurely delivered provider result must not cancel the
                // active paint, discard its queued successor, or split undo.
                view.complete_provider_turn(0.25, cx);
                assert!(view.assistant.running && view.assistant.completion_pending);
                assert!(view.editor.history.is_empty());
            });
            for _ in 0..100 {
                cx.executor().advance_clock(Duration::from_millis(16));
                cx.run_until_parked();
                if view.read_with(cx, |view, _| !view.assistant.running) {
                    break;
                }
            }
            view.update(cx, |view, _| {
                assert!(!view.assistant.running, "both paints and feedback finished");
                assert!(!view.assistant.tool_busy && view.assistant.tool_queue.is_empty());
                assert_eq!(view.assistant.cost, 0.25);
                assert_eq!(view.editor.history.len(), 1);
                let NodeKind::Raster { raster, .. } = &view.editor.doc.node(1).unwrap().kind else {
                    panic!("ink layer");
                };
                assert!(raster.get(150, 25)[0] > 60000, "first call survives");
                assert!(raster.get(150, 75)[2] > 60000, "second call survives");
                assert!(view.editor.undo());
                let NodeKind::Raster { raster, .. } = &view.editor.doc.node(1).unwrap().kind else {
                    panic!("ink layer");
                };
                assert_eq!(raster.get(150, 25)[3], 0);
                assert_eq!(raster.get(150, 75)[3], 0);
            });
            for response in [
                first_reply.join().unwrap(),
                second_reply.join().unwrap(),
                inspect_reply.join().unwrap(),
            ] {
                assert_eq!(response["isError"], false, "{response}");
            }
        }
    }

    #[gpui_kit::test]
    fn alpha_locked_eraser_keeps_pixels_revision_and_history_in_both_playback_modes(
        cx: &mut TestAppContext,
    ) {
        for live in [true, false] {
            let relay = Relay::start().unwrap();
            let view = painting(cx, live);
            let before = view.update(cx, |view, _| {
                let mut doc = view.editor.doc.clone();
                let NodeKind::Raster { raster, .. } = &mut doc.node_mut(1).unwrap().kind else {
                    panic!("ink layer")
                };
                *raster = Arc::new(emulsion_raster::Raster::solid(300, 100, [0.5, 0., 0., 0.5]));
                view.editor = emulsion_core::Editor::new(doc.clone(), None);
                view.editor.begin("Assistant drawing");
                (doc, view.editor.revision)
            });
            let mut args = stroke(25, "#ff0000");
            args["brush"] = serde_json::json!("Hard eraser");
            args["alpha_lock"] = serde_json::json!(true);
            let (erase, reply) = call(&relay, "paint", args);
            view.update(cx, |view, cx| {
                view.run_tool_now(erase, cx);
                view.complete_provider_turn(0.0, cx);
            });
            for _ in 0..100 {
                cx.executor().advance_clock(Duration::from_millis(16));
                cx.run_until_parked();
                if view.read_with(cx, |view, _| !view.assistant.running) {
                    break;
                }
            }
            assert_eq!(reply.join().unwrap()["isError"], false);
            view.read_with(cx, |view, _| {
                assert!(!view.assistant.running);
                assert_eq!(view.editor.doc, before.0);
                assert_eq!(view.editor.revision, before.1);
                assert!(view.editor.history.is_empty());
            });
        }
    }

    #[gpui_kit::test]
    fn queued_view_sees_completed_paint_before_a_later_mutation(cx: &mut TestAppContext) {
        for live in [true, false] {
            let relay = Relay::start().unwrap();
            let view = painting(cx, live);
            let args = serde_json::json!({});
            let (paint, paint_reply) = call(&relay, "paint", stroke(25, "#ff0000"));
            let (inspect, inspect_reply) = call(&relay, "get_view", args.clone());
            let (hide, hide_reply) = call(
                &relay,
                "set_visibility",
                serde_json::json!({"node": 1, "visible": false}),
            );
            view.update(cx, |view, cx| {
                view.run_tool_now(paint, cx);
                view.run_tool_now(inspect, cx);
                view.run_tool_now(hide, cx);
                assert_eq!(view.assistant.tool_queue.len(), 2);
                view.complete_provider_turn(0.25, cx);
                assert!(view.assistant.running && view.assistant.completion_pending);
            });
            for _ in 0..100 {
                cx.executor().advance_clock(Duration::from_millis(16));
                cx.run_until_parked();
                if view.read_with(cx, |view, _| !view.assistant.running) {
                    break;
                }
            }
            let expected = view.update(cx, |view, _| {
                assert!(!view.assistant.running);
                assert!(!view.assistant.tool_busy && view.assistant.tool_queue.is_empty());
                assert_eq!(view.assistant.cost, 0.25);
                let node = view.editor.doc.node(1).unwrap();
                assert!(!node.visible, "the mutation following inspection must run");
                let NodeKind::Raster { raster, .. } = &node.kind else {
                    panic!("ink layer")
                };
                assert!(
                    raster.get(280, 25)[0] > 60000,
                    "stroke endpoint was completed"
                );
                let mut at_inspection = view.editor.doc.clone();
                at_inspection.node_mut(1).unwrap().visible = true;
                exec::view(&at_inspection, &args).unwrap()
            });
            let response = inspect_reply.join().unwrap();
            assert_eq!(response["isError"], false);
            let returned_image = response["content"]
                .as_array()
                .unwrap()
                .iter()
                .find(|block| block["type"] == "image")
                .unwrap();
            let expected_image = expected
                .content
                .iter()
                .find(|block| block["type"] == "image")
                .unwrap();
            assert_eq!(
                returned_image, expected_image,
                "inspection must show the entire prior stroke while the layer was still visible"
            );
            for response in [paint_reply.join().unwrap(), hide_reply.join().unwrap()] {
                assert_eq!(response["isError"], false, "{response}");
            }
        }
    }

    #[gpui_kit::test]
    fn brush_discovery_bypasses_playback_without_releasing_its_queue(cx: &mut TestAppContext) {
        let relay = Relay::start().unwrap();
        let view = painting(cx, true);
        let (paint, paint_reply) = call(&relay, "paint", stroke(25, "#ff0000"));
        let (inspect, inspect_reply) = call(&relay, "get_view", serde_json::json!({}));
        let (brushes, brushes_reply) = call(&relay, "list_brushes", serde_json::json!({}));
        view.update(cx, |view, cx| {
            view.run_tool_now(paint, cx);
            view.run_tool_now(inspect, cx);
            view.run_tool_now(brushes, cx);
            assert_eq!(view.assistant.tool_queue.len(), 1);
        });
        // No playback clock advances: only independent discovery can finish.
        cx.run_until_parked();
        assert_eq!(brushes_reply.join().unwrap()["isError"], false);
        view.update(cx, |view, cx| {
            assert!(view.assistant.playback.is_some());
            assert!(view.assistant.tool_busy);
            assert_eq!(view.assistant.tool_queue.len(), 1);
            view.complete_provider_turn(0.125, cx);
            view.stop_assistant(cx);
        });
        cx.run_until_parked();
        for response in [paint_reply.join().unwrap(), inspect_reply.join().unwrap()] {
            assert_eq!(
                response["isError"], true,
                "queued inspection must be cancelled: {response}"
            );
        }
        view.read_with(cx, |view, _| {
            assert!(!view.assistant.running);
            assert!(!view.assistant.tool_busy && view.assistant.tool_queue.is_empty());
            assert_eq!(view.assistant.cost, 0.125);
        });
    }

    #[gpui_kit::test]
    fn inspection_defers_provider_completion_and_stop_rejects_inflight_result(
        cx: &mut TestAppContext,
    ) {
        for stop in [false, true] {
            let relay = Relay::start().unwrap();
            let view = painting(cx, false);
            let (inspect, inspect_reply) = call(&relay, "get_view", serde_json::json!({}));
            let before = view.update(cx, |view, cx| {
                let before = view.editor.doc.clone();
                view.run_tool_now(inspect, cx);
                assert!(view.assistant.tool_busy);
                view.complete_provider_turn(0.125, cx);
                assert!(
                    view.assistant.running && view.assistant.completion_pending,
                    "the provider result must wait for its outstanding inspection"
                );
                if stop {
                    view.stop_assistant(cx);
                    assert!(!view.assistant.running);
                }
                before
            });
            cx.run_until_parked();
            assert_eq!(inspect_reply.join().unwrap()["isError"], stop);
            view.read_with(cx, |view, _| {
                assert!(!view.assistant.running);
                assert!(!view.assistant.tool_busy && view.assistant.tool_queue.is_empty());
                assert_eq!(view.assistant.cost, 0.125);
                assert_eq!(view.editor.doc, before);
                assert!(view.editor.history.is_empty());
            });
        }
    }

    #[gpui_kit::test]
    fn stop_rejects_queued_and_late_mutations_and_discards_heavy_work(cx: &mut TestAppContext) {
        let relay = Relay::start().unwrap();
        let view = painting(cx, false);
        let (paint, paint_reply) = call(&relay, "paint", stroke(25, "#ff0000"));
        let (queued, queued_reply) = call(
            &relay,
            "set_visibility",
            serde_json::json!({"node": 1, "visible": false}),
        );
        let (late, late_reply) = call(
            &relay,
            "set_visibility",
            serde_json::json!({"node": 1, "visible": false}),
        );
        let before = view.update(cx, |view, cx| {
            let before = view.editor.doc.clone();
            view.run_tool_now(paint, cx);
            view.run_tool_now(queued, cx);
            view.stop_assistant(cx);
            view.run_tool_now(late, cx);
            view.complete_provider_turn(0.125, cx);
            before
        });
        cx.run_until_parked();
        view.update(cx, |view, _| {
            assert_eq!(
                view.editor.doc, before,
                "a stopped background result must never apply"
            );
            assert!(view.editor.history.is_empty());
            assert_eq!(view.assistant.cost, 0.125);
            assert!(!view.assistant.tool_busy && view.assistant.tool_queue.is_empty());
        });
        for response in [
            paint_reply.join().unwrap(),
            queued_reply.join().unwrap(),
            late_reply.join().unwrap(),
        ] {
            assert_eq!(response["isError"], true, "{response}");
        }
    }

    #[gpui_kit::test]
    fn stop_freezes_visible_playback_and_keeps_partial_paint_undoable(cx: &mut TestAppContext) {
        let relay = Relay::start().unwrap();
        let view = painting(cx, true);
        let (first, first_reply) = call(&relay, "paint", stroke(25, "#ff0000"));
        let (queued, queued_reply) = call(&relay, "paint", stroke(75, "#0000ff"));
        let partial = view.update(cx, |view, cx| {
            view.run_tool_now(first, cx);
            view.run_tool_now(queued, cx);
            assert!(view.playback_tick(cx));
            let NodeKind::Raster { raster, .. } = &view.editor.doc.node(1).unwrap().kind else {
                panic!("ink layer");
            };
            assert!(raster.get(20, 25)[3] > 0);
            assert_eq!(raster.get(250, 25)[3], 0);
            let partial = view.editor.doc.clone();
            view.complete_provider_turn(0.125, cx);
            view.stop_assistant(cx);
            assert!(
                !view.assistant.running,
                "Stop completes a deferred provider result without waiting for another event"
            );
            partial
        });
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        view.update(cx, |view, _| {
            assert!(view.assistant.playback.is_none());
            assert_eq!(view.editor.doc, partial);
            assert_eq!(view.editor.history.len(), 1);
            assert!(view.editor.undo());
            let NodeKind::Raster { raster, .. } = &view.editor.doc.node(1).unwrap().kind else {
                panic!("ink layer");
            };
            assert_eq!(raster.get(20, 25)[3], 0);
        });
        for response in [first_reply.join().unwrap(), queued_reply.join().unwrap()] {
            assert_eq!(response["isError"], true, "{response}");
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::{EditorView, Turn, app_state, exec, launch};
    use crate::theme;
    use emulsion_assistant::session::{CliProcess, Launcher, LineSink};
    use emulsion_assistant::{Event, Session};
    use emulsion_core::Document;
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use serde_json::Value;
    use std::sync::Arc;
    use std::sync::Mutex;

    struct FakeLauncher(Arc<Mutex<Vec<String>>>);
    struct FakeProcess(Arc<Mutex<Vec<String>>>);

    impl Launcher for FakeLauncher {
        fn spawn(
            &self,
            _: &launch::LaunchSpec,
            _: LineSink,
        ) -> std::io::Result<Box<dyn CliProcess>> {
            Ok(Box::new(FakeProcess(self.0.clone())))
        }
    }

    impl CliProcess for FakeProcess {
        fn write_line(&mut self, line: &str) -> std::io::Result<()> {
            self.0.lock().unwrap().push(line.to_string());
            Ok(())
        }
        fn kill(&mut self) {}
    }

    fn drawing(cx: &mut TestAppContext) -> Entity<EditorView> {
        cx.update(|cx| {
            theme::install(cx);
            cx.set_global(app_state::AppSettings(emulsion_io::settings::Settings {
                suggestions: false,
                ..Default::default()
            }));
            cx.new(|cx| {
                let mut view = EditorView::new(Document::new(80, 60), None, None, None, "test".into(), cx);
                view.editor.begin("Assistant drawing");
                view.assistant.running = true;
                view.assistant.turn = Some(Turn {
                    prompt: "Draw a blue triangle; keep it centred.".into(),
                    ..Default::default()
                });
                let args = serde_json::json!({"name":"Triangle", "d":"M 10 50 L 40 10 L 70 50 Z", "fill":"#3344FF", "stroke":"none"});
                let result = exec::execute(&mut view.editor, "draw_path", &args);
                assert!(!result.is_error);
                view.observe_drawing_tool("draw_path", &args, &result, view.editor.revision, true);
                view
            })
        })
    }

    #[gpui_kit::test]
    fn final_drawing_review_preserves_brief_cost_and_single_undo(cx: &mut TestAppContext) {
        let view = drawing(cx);
        let written = Arc::new(Mutex::new(Vec::new()));
        let session = Session::start(
            &FakeLauncher(written.clone()),
            &launch::LaunchSpec {
                program: "unused".into(),
                args: vec![],
                env: vec![],
                cwd: ".".into(),
            },
        )
        .unwrap();
        view.update(cx, |view, cx| {
            view.assistant.session = Some(session);
            view.complete_provider_turn(0.25, cx);
            assert!(view.assistant.running);
            assert!(view.editor.in_transaction());
            assert!(view.editor.history.is_empty());
            let sent = written.lock().unwrap();
            let message: Value = serde_json::from_str(sent.last().unwrap()).unwrap();
            assert!(
                message
                    .to_string()
                    .contains("Draw a blue triangle; keep it centred.")
            );
            assert!(message.to_string().contains("skipped"));
            drop(sent);
            let args = serde_json::json!({});
            let preview = exec::execute(&mut view.editor, "get_view", &args);
            view.observe_drawing_tool("get_view", &args, &preview, view.editor.revision, false);
            view.complete_provider_turn(0.125, cx);
            assert!(!view.assistant.running);
            assert_eq!(view.assistant.cost, 0.375);
            assert_eq!(view.assistant.turn.as_ref().unwrap().done.unwrap().1, 0.375);
            assert_eq!(view.assistant.history.len(), 1);
            assert_eq!(view.editor.history.len(), 1);
            assert!(view.editor.undo());
            assert!(view.editor.doc.nodes.is_empty());
        });
    }

    #[gpui_kit::test]
    fn final_review_resumes_when_one_shot_exit_precedes_tool_completion(cx: &mut TestAppContext) {
        let view = drawing(cx);
        let attempts = Arc::new(Mutex::new(0));
        let count = attempts.clone();
        view.update(cx, |view, cx| {
            view.assistant.session = Some(Session::one_shot(
                emulsion_assistant::protocol::Flavor::Codex,
                Box::new(move |_, _| {
                    *count.lock().unwrap() += 1;
                    Err(std::io::Error::other("test resume failed"))
                }),
            ));
            view.assistant.tool_busy = true;
            view.complete_provider_turn(0.25, cx);
            view.on_event(Event::Exited(Some(0)), cx);
            assert_eq!(*attempts.lock().unwrap(), 0);
            assert!(view.assistant.running && view.assistant.completion_pending);
            view.complete_tool_work(view.assistant.tool_generation, cx);
            assert_eq!(
                *attempts.lock().unwrap(),
                1,
                "the prior Exited must not be lost while tools drain"
            );
            assert!(!view.assistant.running);
            assert_eq!(view.assistant.cost, 0.25);
        });
    }

    #[gpui_kit::test]
    fn final_drawing_review_waits_for_one_shot_exit_and_reports_resume_failure(
        cx: &mut TestAppContext,
    ) {
        let view = drawing(cx);
        let attempts = Arc::new(Mutex::new(0));
        let count = attempts.clone();
        view.update(cx, |view, cx| {
            view.assistant.session = Some(Session::one_shot(
                emulsion_assistant::protocol::Flavor::Codex,
                Box::new(move |_, _| {
                    *count.lock().unwrap() += 1;
                    Err(std::io::Error::other("test resume failed"))
                }),
            ));
            view.complete_provider_turn(0.25, cx);
            assert_eq!(
                *attempts.lock().unwrap(),
                0,
                "do not reset a still-running parser"
            );
            assert!(view.assistant.turn.as_ref().unwrap().review_pending);
            view.on_event(Event::Exited(Some(0)), cx);
            assert_eq!(*attempts.lock().unwrap(), 1);
            assert!(!view.assistant.running);
            assert_eq!(view.assistant.cost, 0.25);
            assert!(
                view.assistant
                    .turn
                    .as_ref()
                    .unwrap()
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("test resume failed")
            );
            view.on_event(Event::Exited(Some(0)), cx);
            assert_eq!(*attempts.lock().unwrap(), 1);
        });
    }

    #[gpui_kit::test]
    fn final_drawing_review_never_resumes_after_stop_or_failed_exit(cx: &mut TestAppContext) {
        for stop in [true, false] {
            let view = drawing(cx);
            view.update(cx, |view, cx| {
                view.assistant.session = Some(Session::one_shot(
                    emulsion_assistant::protocol::Flavor::Codex,
                    Box::new(|_, _| panic!("must not restart after Stop or process failure")),
                ));
                view.complete_provider_turn(0.25, cx);
                if stop {
                    view.stop_assistant(cx);
                    view.on_event(Event::Exited(Some(0)), cx);
                } else {
                    view.on_event(Event::Exited(Some(1)), cx);
                }
                assert!(!view.assistant.running);
                assert_eq!(view.assistant.cost, 0.25);
                assert_eq!(view.assistant.history.len(), 1);
            });
        }
    }
}
