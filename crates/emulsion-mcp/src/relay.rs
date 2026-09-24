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

use crate::recovery::{ErrorCode, ExecutionState, RetryPolicy, tool_error};
use crate::server::{ToolDef, ToolHost, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

pub const ENV_ADDR: &str = "EMULSION_RELAY";
pub const ENV_TOKEN: &str = "EMULSION_TOKEN";
const MAX_LINE: u64 = 8 << 20;
/// Longest a single tool call may run, as advertised to the coding CLIs:
/// painting plays at a hand's pace and answers only when it has finished.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(900);
/// How long the relay waits for the app. Longer than [`TOOL_TIMEOUT`], so
/// the CLI gives up first instead of seeing a timeout while the app is
/// still running the call (and retrying it).
const CALL_TIMEOUT: Duration = TOOL_TIMEOUT.saturating_add(Duration::from_secs(30));
/// A new connection must authenticate within this long.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Concurrent connections; the CLI needs one per `mcp-serve`.
const MAX_CONNECTIONS: usize = 16;

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

/// 32 bytes from the operating system's CSPRNG, as hex.
fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
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
        let token = random_token()?;
        let (tx, rx) = async_channel::unbounded::<RelayCall>();
        let stop = Arc::new(AtomicBool::new(false));
        let (tok, st) = (token.clone(), stop.clone());
        let open = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new()
            .name("emulsion-relay".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    if st.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    if open.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                        open.fetch_sub(1, Ordering::Relaxed);
                        tracing::debug!("relay connection refused: too many open");
                        continue;
                    }
                    let (tx, tok, open) = (tx.clone(), tok.clone(), open.clone());
                    std::thread::spawn(move || {
                        if let Err(e) = serve_conn(conn, &tx, &tok) {
                            tracing::debug!(error = %e, "relay connection ended");
                        }
                        open.fetch_sub(1, Ordering::Relaxed);
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
    // Until it authenticates, a peer gets a short read timeout; after that
    // the CLI may sit idle between turns for as long as it likes.
    conn.set_read_timeout(Some(AUTH_TIMEOUT))?;
    let mut reader = BufReader::new(conn.try_clone()?).take(MAX_LINE);
    let mut writer = conn;
    let mut line = String::new();
    let mut authenticated = false;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let w = match serde_json::from_str::<Wire>(&line) {
            Ok(w) if same(&w.token, token) => w,
            // Reply, then hang up: no second guess on the same connection.
            rejected => {
                let (code, why) = match rejected {
                    Err(e) => (ErrorCode::InvalidRequest, format!("bad relay request: {e}")),
                    Ok(_) => (
                        ErrorCode::AuthenticationRejected,
                        "relay token rejected".into(),
                    ),
                };
                return reply(
                    &mut writer,
                    &tool_error(
                        code,
                        why,
                        ExecutionState::NotStarted,
                        RetryPolicy::DoNotRetry,
                    ),
                );
            }
        };
        if !authenticated {
            authenticated = true;
            writer.set_read_timeout(None)?;
        }
        let (rtx, rrx) = std::sync::mpsc::sync_channel(1);
        let result = if tx
            .send_blocking(RelayCall {
                name: w.name,
                arguments: w.arguments,
                reply: rtx,
            })
            .is_err()
        {
            tool_error(
                ErrorCode::DocumentClosed,
                "the document was closed",
                ExecutionState::NotStarted,
                RetryPolicy::DoNotRetry,
            )
        } else {
            wait_for_reply(&rrx, CALL_TIMEOUT)
        };
        reply(&mut writer, &result)?;
        reader.set_limit(MAX_LINE);
    }
}

fn wait_for_reply(
    receiver: &std::sync::mpsc::Receiver<ToolResult>,
    timeout: Duration,
) -> ToolResult {
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => tool_error(
            ErrorCode::ToolTimeout,
            "Emulsion did not answer in time; the call may still be running. Do not repeat it before checking the document.",
            ExecutionState::Unknown,
            RetryPolicy::InspectBeforeRetry,
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => tool_error(
            ErrorCode::ConnectionLost,
            "Emulsion stopped answering this call; it may or may not have run. Check the document before repeating it.",
            ExecutionState::Unknown,
            RetryPolicy::InspectBeforeRetry,
        ),
    }
}

fn reply(writer: &mut TcpStream, result: &ToolResult) -> std::io::Result<()> {
    writeln!(
        writer,
        "{}",
        serde_json::to_string(result).unwrap_or_default()
    )?;
    writer.flush()
}

/// How far a relay round trip got before it failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// No connection: the request never left.
    Connect,
    /// The request was (or may have been) written.
    Exchange,
}

impl Stage {
    /// Only a call that never reached the app is sent again: once the
    /// request is written the app may already be running it, and a
    /// mutating tool (paint, export) must not run twice.
    fn retryable(self) -> bool {
        self == Stage::Connect
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

    fn connect(&mut self) -> std::io::Result<()> {
        if self.conn.is_none() {
            let addr: SocketAddr = self.addr.parse().map_err(std::io::Error::other)?;
            let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
            self.conn = Some((BufReader::new(s.try_clone()?), s));
        }
        Ok(())
    }

    fn roundtrip(
        &mut self,
        name: &str,
        args: &Value,
    ) -> Result<ToolResult, (Stage, std::io::Error)> {
        self.connect().map_err(|e| (Stage::Connect, e))?;
        self.exchange(name, args).map_err(|e| {
            // The connection's state is unknown; start fresh next call.
            self.conn = None;
            (Stage::Exchange, e)
        })
    }

    fn exchange(&mut self, name: &str, args: &Value) -> std::io::Result<ToolResult> {
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
        let result = match self.roundtrip(name, args) {
            // One more attempt, only if the call never reached the app.
            Err((stage, _)) if stage.retryable() => self.roundtrip(name, args),
            other => other,
        };
        result.unwrap_or_else(|(stage, e)| match stage {
            Stage::Connect => tool_error(ErrorCode::AppUnavailable, format!("Emulsion is not reachable: {e}"), ExecutionState::NotStarted, RetryPolicy::Reconnect),
            Stage::Exchange => tool_error(ErrorCode::ConnectionLost, format!(
                "the connection to Emulsion broke during the call, so it may or may not have run; check before repeating it: {e}"
            ), ExecutionState::Unknown, RetryPolicy::InspectBeforeRetry),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope(result: &ToolResult) -> Value {
        assert!(result.is_error);
        serde_json::from_str(result.content[1]["text"].as_str().unwrap()).unwrap()
    }

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
        assert_eq!(envelope(&r)["code"], "authentication_rejected");
        assert_eq!(envelope(&r)["execution_state"], "not_started");
        assert_eq!(envelope(&r)["retry_policy"], "do_not_retry");
        drop(relay);
        drop(app);
    }

    #[test]
    fn relay_outwaits_the_advertised_tool_timeout() {
        assert!(CALL_TIMEOUT > TOOL_TIMEOUT);
        assert!(CALL_TIMEOUT - TOOL_TIMEOUT <= Duration::from_secs(60));
    }

    #[test]
    fn tokens_are_32_random_bytes() {
        let (a, b) = (random_token().unwrap(), random_token().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn a_bad_token_closes_the_connection() {
        let relay = Relay::start().unwrap();
        let mut conn = TcpStream::connect(relay.addr).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let wire = Wire {
            token: "0".repeat(64),
            name: "double".into(),
            arguments: json!({}),
        };
        writeln!(conn, "{}", serde_json::to_string(&wire).unwrap()).unwrap();
        let mut reader = BufReader::new(conn);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("token rejected"), "{line}");
        line.clear();
        assert_eq!(reader.read_line(&mut line).unwrap(), 0, "hung up");
        assert!(relay.calls.try_recv().is_err(), "nothing reached the app");
    }

    #[test]
    fn only_undelivered_calls_are_retried() {
        assert!(Stage::Connect.retryable());
        assert!(!Stage::Exchange.retryable());

        // A relay that takes each request and hangs up without answering.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let received = Arc::new(AtomicUsize::new(0));
        let count = received.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let mut line = String::new();
                if BufReader::new(conn).read_line(&mut line).unwrap_or(0) > 0 {
                    count.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        let mut host = RelayHost::new(addr.to_string(), "t".into());
        let r = host.call("paint", &json!({}));
        assert!(r.is_error);
        assert!(
            r.content[0]["text"]
                .as_str()
                .unwrap()
                .contains("may or may not"),
            "{:?}",
            r.content
        );
        assert_eq!(received.load(Ordering::SeqCst), 1, "sent once");
        assert_eq!(envelope(&r)["code"], "connection_lost");
        assert_eq!(envelope(&r)["execution_state"], "unknown");
        assert_eq!(envelope(&r)["retry_policy"], "inspect_before_retry");

        // Nothing listening: the call never left, so it is tried again.
        let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let dead = closed.local_addr().unwrap();
        drop(closed);
        let r = RelayHost::new(dead.to_string(), "t".into()).call("paint", &json!({}));
        assert!(
            r.content[0]["text"]
                .as_str()
                .unwrap()
                .contains("not reachable")
        );
        assert_eq!(envelope(&r)["code"], "app_unavailable");
        assert_eq!(envelope(&r)["execution_state"], "not_started");
        assert_eq!(envelope(&r)["retry_policy"], "reconnect");
    }

    #[test]
    fn timed_out_reply_is_unknown_and_does_not_cancel_the_work() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let result = wait_for_reply(&receiver, Duration::ZERO);
        assert_eq!(envelope(&result)["code"], "tool_timeout");
        assert_eq!(envelope(&result)["execution_state"], "unknown");
        assert_eq!(envelope(&result)["retry_policy"], "inspect_before_retry");
        // A wait timeout does not mean the app stopped or rolled back the edit.
        sender.send(ToolResult::text("paint completed")).unwrap();
        assert_eq!(
            receiver.recv().unwrap().content[0]["text"],
            "paint completed"
        );
    }

    #[test]
    fn dropped_reply_is_disconnection_not_timeout() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        drop(sender);
        let result = wait_for_reply(&receiver, Duration::ZERO);
        assert_eq!(envelope(&result)["code"], "connection_lost");
        assert_eq!(envelope(&result)["execution_state"], "unknown");
        assert_eq!(envelope(&result)["retry_policy"], "inspect_before_retry");
    }

    #[test]
    fn closed_document_rejects_submission() {
        let relay = Relay::start().unwrap();
        relay.calls.close();
        let result =
            RelayHost::new(relay.addr.to_string(), relay.token.clone()).call("paint", &json!({}));
        assert_eq!(envelope(&result)["code"], "document_closed");
        assert_eq!(envelope(&result)["execution_state"], "not_started");
        assert_eq!(envelope(&result)["retry_policy"], "do_not_retry");
        assert!(relay.calls.try_recv().is_err());
    }
}
