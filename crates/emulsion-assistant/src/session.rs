//! The CLI child process.
//!
//! One persistent process per open document. stdout and stderr are read on
//! their own threads and parsed into [`Event`]s, delivered on an async
//! channel so the UI never blocks and the reader never back-pressures the
//! pipe. The child runs in its own process group and is killed with its
//! descendants when the session ends.

use crate::launch::LaunchSpec;
use crate::protocol::{self, Event, Parser};
use parking_lot::Mutex;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::Arc;

/// Output from the child, before parsing.
pub enum Line {
    Stdout(String),
    Stderr(String),
    Exit(Option<i32>),
}

pub type LineSink = Box<dyn Fn(Line) + Send + Sync>;

pub trait CliProcess: Send {
    fn write_line(&mut self, line: &str) -> std::io::Result<()>;
    /// Close stdin so a CLI that reads "additional input" from a pipe
    /// sees end-of-file at once.
    fn close_stdin(&mut self) {}
    fn kill(&mut self);
}

/// Starts processes. Tests substitute a fake.
pub trait Launcher {
    fn spawn(&self, spec: &LaunchSpec, sink: LineSink) -> std::io::Result<Box<dyn CliProcess>>;
}

pub struct ProdLauncher;

struct ProdProcess {
    stdin: Option<std::process::ChildStdin>,
    pid: u32,
    dead: Arc<std::sync::atomic::AtomicBool>,
}

impl CliProcess for ProdProcess {
    fn close_stdin(&mut self) {
        self.stdin.take();
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        let s = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("stdin closed"))?;
        s.write_all(line.as_bytes())?;
        s.write_all(b"\n")?;
        s.flush()
    }

    fn kill(&mut self) {
        self.stdin.take();
        if self.dead.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        #[cfg(unix)]
        unsafe {
            // Negative pid: the whole process group, including mcp-serve.
            libc::kill(-(self.pid as i32), libc::SIGTERM);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &self.pid.to_string()])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .status();
        }
    }
}

impl Launcher for ProdLauncher {
    fn spawn(&self, spec: &LaunchSpec, sink: LineSink) -> std::io::Result<Box<dyn CliProcess>> {
        let mut cmd = crate::provider::command(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn()?;
        let pid = child.id();
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let stdin = child.stdin.take();
        let sink = Arc::new(sink);
        let dead = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let s = sink.clone();
        let out = std::thread::Builder::new()
            .name("cli-stdout".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    s(Line::Stdout(line));
                }
            })?;
        let s = sink.clone();
        std::thread::Builder::new()
            .name("cli-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    s(Line::Stderr(line));
                }
            })?;
        let (s, d) = (sink, dead.clone());
        std::thread::Builder::new()
            .name("cli-wait".into())
            .spawn(move || {
                let code = child.wait().ok().and_then(|st| st.code());
                let _ = out.join(); // deliver all output before the exit
                d.store(true, std::sync::atomic::Ordering::Relaxed);
                s(Line::Exit(code));
            })?;
        Ok(Box::new(ProdProcess { stdin, pid, dead }))
    }
}

/// Builds the launch for one turn of a one-shot CLI: (prompt, session id
/// to resume) → spec.
pub type Respawn = Box<dyn Fn(&str, Option<String>) -> std::io::Result<LaunchSpec> + Send>;

/// A running assistant conversation.
pub struct Session {
    process: Option<Box<dyn CliProcess>>,
    pub events: async_channel::Receiver<Event>,
    tx: async_channel::Sender<Event>,
    parser: Arc<Mutex<Parser>>,
    initialized: bool,
    /// One process per turn (Codex, OpenCode, Kimi); `None` is a
    /// persistent stream-json process (Claude Code).
    respawn: Option<Respawn>,
}

impl Session {
    /// A persistent Claude Code session.
    pub fn start(launcher: &dyn Launcher, spec: &LaunchSpec) -> std::io::Result<Self> {
        let (tx, rx) = async_channel::unbounded();
        let parser = Arc::new(Mutex::new(Parser::default()));
        let process = launcher.spawn(spec, Self::sink(&parser, &tx))?;
        Ok(Self {
            process: Some(process),
            events: rx,
            tx,
            parser,
            initialized: false,
            respawn: None,
        })
    }

    /// A one-shot session: nothing runs until the first `send`.
    pub fn one_shot(flavor: protocol::Flavor, respawn: Respawn) -> Self {
        let (tx, rx) = async_channel::unbounded();
        Self {
            process: None,
            events: rx,
            tx,
            parser: Arc::new(Mutex::new(Parser::with_flavor(flavor))),
            initialized: true,
            respawn: Some(respawn),
        }
    }

    pub fn is_one_shot(&self) -> bool {
        self.respawn.is_some()
    }

    fn sink(parser: &Arc<Mutex<Parser>>, tx: &async_channel::Sender<Event>) -> LineSink {
        let p = parser.clone();
        let tx = tx.clone();
        Box::new(move |line| {
            let events = match line {
                Line::Stdout(l) => p.lock().feed(&l),
                Line::Stderr(l) => {
                    let t = l.trim();
                    if t.is_empty() {
                        vec![]
                    } else {
                        vec![Event::Stderr(t.to_string())]
                    }
                }
                Line::Exit(code) => {
                    // One-shot CLIs end a turn by exiting; if nothing said
                    // "done", a clean exit is a result and a failure an error.
                    let mut out = Vec::new();
                    let mut parser = p.lock();
                    if parser.flavor != protocol::Flavor::Claude && !parser.saw_result {
                        parser.saw_result = true;
                        let (input_tokens, output_tokens, cost_usd) = parser.usage;
                        out.push(match code {
                            Some(0) | None => Event::Result {
                                text: String::new(),
                                cost_usd,
                                duration_ms: 0,
                                turns: 1,
                                input_tokens,
                                output_tokens,
                            },
                            Some(c) => Event::Error(format!("the CLI exited with status {c}")),
                        });
                    }
                    out.push(Event::Exited(code));
                    out
                }
            };
            for e in events {
                let _ = tx.send_blocking(e);
            }
        })
    }

    pub fn session_id(&self) -> Option<String> {
        self.parser.lock().session_id.clone()
    }

    fn write(&mut self, v: &Value) -> std::io::Result<()> {
        match self.process.as_mut() {
            Some(p) => p.write_line(&v.to_string()),
            None => Err(std::io::Error::other("no process")),
        }
    }

    /// Send a user turn. For a persistent CLI the initialize request goes
    /// first, once; a one-shot CLI starts a fresh process for the turn,
    /// resuming its previous conversation when it has an id for it.
    pub fn send_with(
        &mut self,
        launcher: &dyn Launcher,
        text: &str,
        images: &[(String, String)],
    ) -> std::io::Result<()> {
        if let Some(respawn) = &self.respawn {
            if let Some(mut old) = self.process.take() {
                old.kill();
            }
            self.parser.lock().begin_turn();
            let spec = respawn(text, self.session_id())?;
            let mut process = launcher.spawn(&spec, Self::sink(&self.parser, &self.tx))?;
            // The prompt travels in argv; nothing more is coming on stdin.
            process.close_stdin();
            self.process = Some(process);
            return Ok(());
        }
        if !self.initialized {
            self.write(&protocol::initialize())?;
            self.initialized = true;
        }
        let sid = self.session_id().unwrap_or_default();
        self.write(&protocol::user_message(&sid, text, images))
    }

    /// `send_with` using the production launcher.
    pub fn send(&mut self, text: &str, images: &[(String, String)]) -> std::io::Result<()> {
        self.send_with(&ProdLauncher, text, images)
    }

    pub fn allow(
        &mut self,
        request_id: &str,
        tool_use_id: &str,
        input: &Value,
    ) -> std::io::Result<()> {
        if self.respawn.is_some() {
            return Ok(());
        }
        self.write(&protocol::allow(request_id, tool_use_id, input))
    }

    pub fn deny(
        &mut self,
        request_id: &str,
        tool_use_id: &str,
        message: &str,
    ) -> std::io::Result<()> {
        if self.respawn.is_some() {
            return Ok(());
        }
        self.write(&protocol::deny(request_id, tool_use_id, message))
    }

    pub fn interrupt(&mut self) -> std::io::Result<()> {
        if self.respawn.is_some() {
            if let Some(p) = self.process.as_mut() {
                p.kill();
            }
            return Ok(());
        }
        self.write(&protocol::interrupt())
    }

    pub fn kill(&mut self) {
        if let Some(p) = self.process.as_mut() {
            p.kill();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Replies to each user message with a scripted turn.
    struct FakeLauncher {
        written: Arc<StdMutex<Vec<String>>>,
    }

    struct FakeProcess {
        sink: Arc<LineSink>,
        written: Arc<StdMutex<Vec<String>>>,
    }

    impl CliProcess for FakeProcess {
        fn write_line(&mut self, line: &str) -> std::io::Result<()> {
            self.written.lock().unwrap().push(line.to_string());
            let v: Value = serde_json::from_str(line).unwrap();
            if v["type"] == "user" {
                for l in [
                    r#"{"type":"system","subtype":"init","session_id":"fake-1","tools":[]}"#,
                    r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"mcp__emulsion__rename_node","input":{"node":1,"name":"Sky"},"tool_use_id":"t1"}}"#,
                ] {
                    (self.sink)(Line::Stdout(l.into()));
                }
            }
            if v["type"] == "control_response" {
                (self.sink)(Line::Stdout(r#"{"type":"result","subtype":"success","result":"Renamed.","total_cost_usd":0.001,"session_id":"fake-1"}"#.into()));
            }
            Ok(())
        }
        fn kill(&mut self) {
            (self.sink)(Line::Exit(Some(0)));
        }
    }

    impl Launcher for FakeLauncher {
        fn spawn(&self, _: &LaunchSpec, sink: LineSink) -> std::io::Result<Box<dyn CliProcess>> {
            Ok(Box::new(FakeProcess {
                sink: Arc::new(sink),
                written: self.written.clone(),
            }))
        }
    }

    #[test]
    fn session_flow_with_fake_cli() {
        let written = Arc::new(StdMutex::new(Vec::new()));
        let spec = LaunchSpec {
            program: "claude".into(),
            args: vec![],
            env: vec![],
            cwd: ".".into(),
        };
        let mut s = Session::start(
            &FakeLauncher {
                written: written.clone(),
            },
            &spec,
        )
        .unwrap();
        s.send("rename the bottom node to Sky", &[]).unwrap();
        let init = s.events.recv_blocking().unwrap();
        assert!(matches!(init, Event::Init { .. }));
        let Event::Permission {
            request_id,
            tool_use_id,
            input,
            ..
        } = s.events.recv_blocking().unwrap()
        else {
            panic!()
        };
        s.allow(&request_id, &tool_use_id, &input).unwrap();
        assert!(matches!(
            s.events.recv_blocking().unwrap(),
            Event::Result { .. }
        ));
        assert_eq!(s.session_id().as_deref(), Some("fake-1"));
        let w = written.lock().unwrap();
        assert!(w[0].contains("\"initialize\""), "initialize first");
        assert!(w[1].contains("\"type\":\"user\""));
        assert!(w[2].contains("\"behavior\":\"allow\""));
        drop(w);
        s.send("again", &[]).unwrap();
        assert_eq!(
            written
                .lock()
                .unwrap()
                .iter()
                .filter(|l| l.contains("initialize"))
                .count(),
            1,
            "initialize once"
        );
    }
}
