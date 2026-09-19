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
use std::process::{Command, Stdio};
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
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &self.pid.to_string()])
                .status();
        }
    }
}

impl Launcher for ProdLauncher {
    fn spawn(&self, spec: &LaunchSpec, sink: LineSink) -> std::io::Result<Box<dyn CliProcess>> {
        let mut cmd = Command::new(&spec.program);
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
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
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

/// A running assistant conversation.
pub struct Session {
    process: Box<dyn CliProcess>,
    pub events: async_channel::Receiver<Event>,
    parser: Arc<Mutex<Parser>>,
    initialized: bool,
}

impl Session {
    pub fn start(launcher: &dyn Launcher, spec: &LaunchSpec) -> std::io::Result<Self> {
        let (tx, rx) = async_channel::unbounded();
        let parser = Arc::new(Mutex::new(Parser::default()));
        let p = parser.clone();
        let sink: LineSink = Box::new(move |line| {
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
                Line::Exit(code) => vec![Event::Exited(code)],
            };
            for e in events {
                let _ = tx.send_blocking(e);
            }
        });
        let process = launcher.spawn(spec, sink)?;
        Ok(Self {
            process,
            events: rx,
            parser,
            initialized: false,
        })
    }

    pub fn session_id(&self) -> Option<String> {
        self.parser.lock().session_id.clone()
    }

    fn write(&mut self, v: &Value) -> std::io::Result<()> {
        self.process.write_line(&v.to_string())
    }

    /// Send a user turn. The initialize request goes first, once.
    pub fn send(&mut self, text: &str, images: &[(String, String)]) -> std::io::Result<()> {
        if !self.initialized {
            self.write(&protocol::initialize())?;
            self.initialized = true;
        }
        let sid = self.session_id().unwrap_or_default();
        self.write(&protocol::user_message(&sid, text, images))
    }

    pub fn allow(
        &mut self,
        request_id: &str,
        tool_use_id: &str,
        input: &Value,
    ) -> std::io::Result<()> {
        self.write(&protocol::allow(request_id, tool_use_id, input))
    }

    pub fn deny(
        &mut self,
        request_id: &str,
        tool_use_id: &str,
        message: &str,
    ) -> std::io::Result<()> {
        self.write(&protocol::deny(request_id, tool_use_id, message))
    }

    pub fn interrupt(&mut self) -> std::io::Result<()> {
        self.write(&protocol::interrupt())
    }

    pub fn kill(&mut self) {
        self.process.kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.process.kill();
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
