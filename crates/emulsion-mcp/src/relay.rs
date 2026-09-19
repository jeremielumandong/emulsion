//! Loopback relay between `emulsion mcp-serve` (spawned by the coding CLI)
//! and the running app.
//!
//! The app listens on 127.0.0.1 on a random port and hands the address and a
//! random token to the CLI through the MCP config's `env` block (never
//! argv). Each tool call is one JSON line each way:
//!
//! ```text
//! → {"token": "...", "name": "set_visibility", "arguments": {...}}
//! ← {"content": [...], "isError": false}
//! ```

use crate::server::{ToolDef, ToolHost, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::hash::{BuildHasher, RandomState};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const ENV_ADDR: &str = "EMULSION_RELAY";
pub const ENV_TOKEN: &str = "EMULSION_TOKEN";
const MAX_LINE: u64 = 8 << 20;
const CALL_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Serialize, Deserialize)]
struct Wire {
    token: String,
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// A tool call waiting for the app to run it.
pub struct RelayCall {
    pub name: String,
    pub arguments: Value,
    reply: std::sync::mpsc::SyncSender<ToolResult>,
}

impl RelayCall {
    pub fn reply(self, result: ToolResult) {
        let _ = self.reply.send(result);
    }
}

/// The app side: a listener plus a channel of incoming calls.
pub struct Relay {
    pub addr: SocketAddr,
    pub token: String,
    pub calls: async_channel::Receiver<RelayCall>,
    stop: Arc<AtomicBool>,
}

fn random_token() -> String {
    let a = RandomState::new().hash_one(std::time::SystemTime::now());
    let b = RandomState::new().hash_one(std::process::id());
    format!("{a:016x}{b:016x}")
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl Relay {
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        let token = random_token();
        let (tx, rx) = async_channel::unbounded::<RelayCall>();
        let stop = Arc::new(AtomicBool::new(false));
        let (tok, st) = (token.clone(), stop.clone());
        std::thread::Builder::new()
            .name("emulsion-relay".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if st.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    let (tx, tok) = (tx.clone(), tok.clone());
                    std::thread::spawn(move || {
                        if let Err(e) = serve_conn(conn, &tx, &tok) {
                            tracing::debug!(error = %e, "relay connection ended");
                        }
                    });
                }
            })?;
        Ok(Self {
            addr,
            token,
            calls: rx,
            stop,
        })
    }

    /// Environment for the `mcp-serve` child.
    pub fn env(&self) -> Vec<(String, String)> {
        vec![
            (ENV_ADDR.into(), self.addr.to_string()),
            (ENV_TOKEN.into(), self.token.clone()),
        ]
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

fn serve_conn(
    conn: TcpStream,
    tx: &async_channel::Sender<RelayCall>,
    token: &str,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(conn.try_clone()?).take(MAX_LINE);
    let mut writer = conn;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let result = match serde_json::from_str::<Wire>(&line) {
            Err(e) => ToolResult::error(format!("bad relay request: {e}")),
            Ok(w) if !same(&w.token, token) => ToolResult::error("relay token rejected"),
            Ok(w) => {
                let (rtx, rrx) = std::sync::mpsc::sync_channel(1);
                if tx
                    .send_blocking(RelayCall {
                        name: w.name,
                        arguments: w.arguments,
                        reply: rtx,
                    })
                    .is_err()
                {
                    ToolResult::error("the document was closed")
                } else {
                    rrx.recv_timeout(CALL_TIMEOUT)
                        .unwrap_or_else(|_| ToolResult::error("Emulsion did not answer in time"))
                }
            }
        };
        writeln!(
            writer,
            "{}",
            serde_json::to_string(&result).unwrap_or_default()
        )?;
        writer.flush()?;
        reader.set_limit(MAX_LINE);
    }
}

/// The `mcp-serve` side: forwards calls to the app.
pub struct RelayHost {
    addr: String,
    token: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
}

impl RelayHost {
    pub fn new(addr: String, token: String) -> Self {
        Self {
            addr,
            token,
            conn: None,
        }
    }

    pub fn from_env() -> Option<Self> {
        let addr = std::env::var(ENV_ADDR).ok()?;
        let token = std::env::var(ENV_TOKEN).ok()?;
        Some(Self::new(addr, token))
    }

    fn roundtrip(&mut self, name: &str, args: &Value) -> std::io::Result<ToolResult> {
        if self.conn.is_none() {
            let addr: SocketAddr = self.addr.parse().map_err(std::io::Error::other)?;
            let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
            self.conn = Some((BufReader::new(s.try_clone()?), s));
        }
        let (r, w) = self.conn.as_mut().expect("connected");
        let wire = Wire {
            token: self.token.clone(),
            name: name.into(),
            arguments: args.clone(),
        };
        writeln!(w, "{}", serde_json::to_string(&wire)?)?;
        w.flush()?;
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "relay closed",
            ));
        }
        serde_json::from_str(&line).map_err(std::io::Error::other)
    }
}

impl ToolHost for RelayHost {
    fn tools(&self) -> Vec<ToolDef> {
        crate::tools::definitions()
    }

    fn call(&mut self, name: &str, args: &Value) -> ToolResult {
        match self.roundtrip(name, args) {
            Ok(r) => r,
            Err(_) => {
                // One reconnect attempt: the app may have restarted the relay.
                self.conn = None;
                self.roundtrip(name, args).unwrap_or_else(|e| {
                    ToolResult::error(format!("Emulsion is not reachable: {e}"))
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn calls_reach_the_app_and_bad_tokens_are_rejected() {
        let relay = Relay::start().unwrap();
        let calls = relay.calls.clone();
        let app = std::thread::spawn(move || {
            while let Ok(call) = calls.recv_blocking() {
                let n = call.arguments["n"].as_i64().unwrap_or(0);
                let msg = format!("{}:{}", call.name, n * 2);
                call.reply(ToolResult::text(msg));
            }
        });
        let mut host = RelayHost::new(relay.addr.to_string(), relay.token.clone());
        let r = host.call("double", &json!({ "n": 21 }));
        assert_eq!(r.content[0]["text"], "double:42");
        let r = host.call("double", &json!({ "n": 2 }));
        assert_eq!(r.content[0]["text"], "double:4", "connection is reused");

        let mut evil = RelayHost::new(relay.addr.to_string(), "0".repeat(32));
        let r = evil.call("double", &json!({ "n": 1 }));
        assert!(r.is_error && r.content[0]["text"].as_str().unwrap().contains("token"));
        drop(relay);
        drop(app);
    }
}
