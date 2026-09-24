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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// How long a CLI has to exit after SIGTERM before it gets SIGKILL.
#[cfg(unix)]
const KILL_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

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
    dead: Arc<AtomicBool>,
}

/// Signal the process group `pgid`. A group that is already gone is not an
/// error.
#[cfg(unix)]
fn signal_group(pgid: u32, signal: libc::c_int) {
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return;
    };
    // SAFETY: `kill` takes plain integers and touches no memory of ours. The
    // negative pid names the child's process group (it was spawned with
    // `process_group(0)`, so the group id is its pid): the CLI and its
    // `mcp-serve`, nothing else.
    if unsafe { libc::kill(-pgid, signal) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(error = %e, pgid, signal, "could not signal the assistant CLI");
        }
    }
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
        if self.dead.load(Ordering::Relaxed) {
            return;
        }
        #[cfg(unix)]
        {
            signal_group(self.pid, libc::SIGTERM);
            // A CLI that ignores SIGTERM would keep the relay token; force
            // it after a grace period, off the UI thread. The `cli-wait`
            // thread reaps it either way.
            let (pid, dead) = (self.pid, self.dead.clone());
            let _ = std::thread::Builder::new()
                .name("cli-kill".into())
                .spawn(move || {
                    std::thread::sleep(KILL_GRACE);
                    if !dead.load(Ordering::Relaxed) {
                        signal_group(pid, libc::SIGKILL);
                    }
                });
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
        let dead = Arc::new(AtomicBool::new(false));

        let s = sink.clone();
        let directory = spec.directory.clone();
        let out = std::thread::Builder::new()
            .name("cli-stdout".into())
            .spawn(move || {
                let _directory = directory;
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    s(Line::Stdout(line));
                }
            })?;
        let s = sink.clone();
        let directory = spec.directory.clone();
        std::thread::Builder::new()
            .name("cli-stderr".into())
            .spawn(move || {
                let _directory = directory;
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    s(Line::Stderr(line));
                }
            })?;
        let (s, d) = (sink, dead.clone());
        let directory = spec.directory.clone();
        std::thread::Builder::new()
            .name("cli-wait".into())
            .spawn(move || {
                let _directory = directory;
                let code = child.wait().ok().and_then(|st| st.code());
                // Reaped: its pid may be reused, so never signal it again.
                d.store(true, Ordering::Relaxed);
                let _ = out.join(); // deliver all output before the exit
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
    /// Which spawn is current. Each process's output is tagged with the
    /// generation it was started in; a replaced process's late output and
    /// exit are dropped instead of landing in the next turn.
    generation: Arc<AtomicU64>,
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
        let generation = Arc::new(AtomicU64::new(0));
        let process = launcher.spawn(spec, Self::sink(&parser, &tx, &generation))?;
        Ok(Self {
            process: Some(process),
            events: rx,
            tx,
            parser,
            generation,
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
            generation: Arc::new(AtomicU64::new(0)),
            initialized: true,
            respawn: Some(respawn),
        }
    }

    pub fn is_one_shot(&self) -> bool {
        self.respawn.is_some()
    }

    /// Parses one process's output into events. The parser lock is held
    /// while checking the generation, so a respawn (which bumps it under the
    /// same lock) cleanly separates the old process's lines from the new.
    fn sink(
        parser: &Arc<Mutex<Parser>>,
        tx: &async_channel::Sender<Event>,
        generation: &Arc<AtomicU64>,
    ) -> LineSink {
        let p = parser.clone();
        let tx = tx.clone();
        let current = generation.clone();
        let mine = generation.load(Ordering::SeqCst);
        Box::new(move |line| {
            let mut parser = p.lock();
            if current.load(Ordering::SeqCst) != mine {
                return;
            }
            let events = match line {
                Line::Stdout(l) => parser.feed(&l),
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
                    // "done", a clean exit is a result, a failure an error,
                    // and a signal (stopped by the person) a cancelled turn.
                    let mut out = Vec::new();
                    if parser.flavor != protocol::Flavor::Claude && !parser.saw_result {
                        parser.saw_result = true;
                        let (input_tokens, output_tokens, cost_usd) = parser.usage;
                        out.push(match code {
                            Some(0) => Event::Result {
                                text: String::new(),
                                cost_usd,
                                duration_ms: 0,
                                turns: 1,
                                input_tokens,
                                output_tokens,
                            },
                            Some(c) => Event::Error(format!("the CLI exited with status {c}")),
                            None => Event::Error("the turn was cancelled".into()),
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
            {
                // Retire the previous process first: nothing it still says
                // (its exit included) belongs to this turn.
                let mut parser = self.parser.lock();
                self.generation.fetch_add(1, Ordering::SeqCst);
                parser.begin_turn();
            }
            if let Some(mut old) = self.process.take() {
                old.kill();
            }
            let spec = respawn(text, self.session_id())?;
            let sink = Self::sink(&self.parser, &self.tx, &self.generation);
            let mut process = launcher.spawn(&spec, sink)?;
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
            directory: None,
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

    /// One-shot processes that stay quiet until the test speaks for them;
    /// `kill` reports a signal exit, as SIGTERM does.
    #[derive(Default)]
    struct OneShotLauncher {
        sinks: Arc<StdMutex<Vec<Arc<LineSink>>>>,
    }

    struct OneShotProcess {
        sink: Arc<LineSink>,
    }

    impl CliProcess for OneShotProcess {
        fn write_line(&mut self, _: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn kill(&mut self) {
            (self.sink)(Line::Exit(None));
        }
    }

    impl Launcher for OneShotLauncher {
        fn spawn(&self, _: &LaunchSpec, sink: LineSink) -> std::io::Result<Box<dyn CliProcess>> {
            let sink = Arc::new(sink);
            self.sinks.lock().unwrap().push(sink.clone());
            Ok(Box::new(OneShotProcess { sink }))
        }
    }

    fn one_shot_session() -> Session {
        Session::one_shot(
            protocol::Flavor::Codex,
            Box::new(|_, _| {
                Ok(LaunchSpec {
                    program: "codex".into(),
                    args: vec![],
                    env: vec![],
                    cwd: ".".into(),
                    directory: None,
                })
            }),
        )
    }

    #[test]
    fn a_replaced_process_cannot_end_the_next_turn() {
        let launcher = OneShotLauncher::default();
        let mut s = one_shot_session();
        s.send_with(&launcher, "first", &[]).unwrap();
        s.send_with(&launcher, "second", &[]).unwrap();
        let old = launcher.sinks.lock().unwrap()[0].clone();
        // Late output from the first process, after its (dropped) exit.
        old(Line::Stdout(
            r#"{"type":"turn.completed","usage":{}}"#.into(),
        ));
        old(Line::Exit(Some(0)));
        assert!(s.events.try_recv().is_err(), "nothing from the old process");

        let new = launcher.sinks.lock().unwrap()[1].clone();
        new(Line::Stdout(
            r#"{"type":"turn.completed","usage":{}}"#.into(),
        ));
        assert!(matches!(s.events.try_recv(), Ok(Event::Result { .. })));
    }

    #[test]
    fn a_signal_exit_cancels_the_turn() {
        let launcher = OneShotLauncher::default();
        let mut s = one_shot_session();
        s.send_with(&launcher, "paint", &[]).unwrap();
        s.interrupt().unwrap();
        assert!(matches!(s.events.try_recv(), Ok(Event::Error(e)) if e.contains("cancelled")));
        assert!(matches!(s.events.try_recv(), Ok(Event::Exited(None))));
    }

    #[test]
    fn managed_directory_survives_until_child_exit() {
        use crate::storage::SessionDirectory;
        use std::time::{Duration, Instant};

        let root =
            std::env::temp_dir().join(format!("emulsion-child-storage-{}", std::process::id()));
        let directory = SessionDirectory::create(&root).unwrap();
        let path = directory.path().to_path_buf();
        #[cfg(windows)]
        let (program, args) = (
            "cmd.exe",
            vec!["/D", "/C", "echo ready & set /p input= & echo done"],
        );
        #[cfg(not(windows))]
        let (program, args) = ("sh", vec!["-c", "echo ready; read input; echo done"]);
        let spec = LaunchSpec {
            program: program.into(),
            args: args.into_iter().map(String::from).collect(),
            env: vec![],
            cwd: path.clone(),
            directory: Some(directory),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let mut process = ProdLauncher
            .spawn(
                &spec,
                Box::new(move |line| {
                    let _ = tx.send(line);
                }),
            )
            .unwrap();
        drop(spec);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Line::Stdout(_)
        ));
        assert!(path.is_dir(), "running child retains the workspace");
        process.close_stdin();
        while !matches!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Line::Exit(_)
        ) {}
        let deadline = Instant::now() + Duration::from_secs(5);
        while path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!path.exists(), "last process lease releases the workspace");
        std::fs::remove_dir(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_cli_ignoring_sigterm_is_killed() {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = StdMutex::new(tx);
        let spec = LaunchSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "trap '' TERM; echo ready; sleep 30".into()],
            env: vec![],
            cwd: ".".into(),
            directory: None,
        };
        let sink: LineSink = Box::new(move |line| {
            let tag = match line {
                Line::Stdout(l) => l,
                Line::Stderr(_) => return,
                Line::Exit(code) => format!("exit {code:?}"),
            };
            let _ = tx.lock().unwrap().send(tag);
        });
        let mut p = ProdLauncher.spawn(&spec, sink).unwrap();
        let wait = std::time::Duration::from_secs(10);
        assert_eq!(rx.recv_timeout(wait).unwrap(), "ready");
        let start = std::time::Instant::now();
        p.kill();
        assert_eq!(rx.recv_timeout(wait).unwrap(), "exit None", "reaped");
        assert!(start.elapsed() >= KILL_GRACE, "SIGTERM was ignored");
    }
}
