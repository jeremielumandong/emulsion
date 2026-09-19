//! TypeSafe's Jev: typed questions in, calibrated answers out.
//!
//! `POST https://api.typesafe.ai/v1/systemone` with a state and a map of
//! Choice / Noul / Score questions. Jev reads text only; no pixels are sent.

use crate::decide::{ClauseDecision, Decide, Intent, NodeInfo};
use serde_json::{Map, Value, json};
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
pub const MODEL: &str = "jev-latest";
/// Nodes beyond this are not offered as targets, to bound the request.
const MAX_NODES: usize = 60;

pub struct Jev {
    key: String,
    endpoint: String,
    model: String,
    agent: ureq::Agent,
}

#[derive(Debug, thiserror::Error)]
pub enum JevError {
    #[error("Jev request failed: {0}")]
    Http(String),
    #[error("Jev returned status {0}: {1}")]
    Status(u16, String),
    #[error("Jev answer is missing {0}")]
    Missing(String),
}

impl Jev {
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_endpoint(key, ENDPOINT)
    }

    pub fn with_endpoint(key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build();
        Self {
            key: key.into(),
            endpoint: endpoint.into(),
            model: MODEL.into(),
            agent: config.into(),
        }
    }

    /// Evaluate `questions` against `state`; returns the `answers` object.
    pub fn evaluate(
        &self,
        state: &Value,
        questions: Map<String, Value>,
    ) -> Result<Map<String, Value>, JevError> {
        let body = json!({ "state": state, "model": self.model, "questions": questions });
        let mut attempt = 0;
        loop {
            let resp = self
                .agent
                .post(&self.endpoint)
                .header("Authorization", format!("Bearer {}", self.key))
                .send_json(&body)
                .map_err(|e| JevError::Http(e.to_string()))?;
            let status = resp.status().as_u16();
            if status == 429 && attempt < 2 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(10);
                std::thread::sleep(Duration::from_secs(wait));
                attempt += 1;
                continue;
            }
            let mut resp = resp;
            let v: Value = resp
                .body_mut()
                .read_json()
                .map_err(|e| JevError::Http(e.to_string()))?;
            if !(200..300).contains(&status) {
                return Err(JevError::Status(
                    status,
                    v.to_string().chars().take(300).collect(),
                ));
            }
            return v["answers"]
                .as_object()
                .cloned()
                .ok_or_else(|| JevError::Missing("answers".into()));
        }
    }
}

pub struct JevDecider {
    pub jev: Jev,
}

impl JevDecider {
    /// The questions for a request: one Choice per clause for the operation,
    /// one Noul per clause and node for targeting. All in one call.
    pub fn questions(clauses: &[String], nodes: &[NodeInfo]) -> (Value, Map<String, Value>) {
        let nodes = &nodes[..nodes.len().min(MAX_NODES)];
        let state = json!({
            "request_clauses": clauses,
            "document_nodes": nodes.iter().map(|n| json!({ "row_from_top": n.row, "name": n.name, "kind": n.kind })).collect::<Vec<_>>(),
            "context": "An image editor. Nodes are listed from the top of the stack (row 1) down.",
        });
        let criteria: Map<String, Value> = Intent::ALL
            .iter()
            .map(|i| (i.key().to_string(), json!(i.describe())))
            .collect();
        let mut q = Map::new();
        for (ci, c) in clauses.iter().enumerate() {
            q.insert(
                format!("intent_{ci}"),
                json!({ "type": "choice", "instructions": format!("Which editor operation does clause {ci} ask for: \"{c}\"?"), "criteria": criteria }),
            );
            for n in nodes {
                q.insert(
                    format!("target_{ci}_{}", n.id),
                    json!({
                        "type": "noul",
                        "instructions": format!(
                            "Does clause {ci} (\"{c}\") refer to the node \"{}\" (row {} from the top, a {} node)? Ordinals like 'the third' or 'the top two' count rows from the top.",
                            n.name, n.row, n.kind
                        ),
                    }),
                );
            }
        }
        (state, q)
    }

    pub fn interpret(
        answers: &Map<String, Value>,
        clauses: &[String],
        nodes: &[NodeInfo],
    ) -> Vec<ClauseDecision> {
        let nodes = &nodes[..nodes.len().min(MAX_NODES)];
        (0..clauses.len())
            .map(|ci| {
                let a = &answers
                    .get(&format!("intent_{ci}"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let intent = Intent::from_key(a["choice"].as_str().unwrap_or("other"));
                let confidence = a["confidence"].as_f64().unwrap_or(0.0) as f32;
                let targets = nodes
                    .iter()
                    .filter_map(|n| {
                        let p =
                            answers.get(&format!("target_{ci}_{}", n.id))?["noul"].as_f64()? as f32;
                        Some((n.id, p))
                    })
                    .collect();
                ClauseDecision {
                    intent,
                    confidence,
                    targets,
                }
            })
            .collect()
    }
}

impl Decide for JevDecider {
    fn name(&self) -> &'static str {
        "Jev"
    }

    fn decide(
        &self,
        clauses: &[String],
        nodes: &[NodeInfo],
    ) -> anyhow::Result<Vec<ClauseDecision>> {
        if clauses.is_empty() {
            return Ok(vec![]);
        }
        let (state, q) = Self::questions(clauses, nodes);
        let answers = self.jev.evaluate(&state, q)?;
        Ok(Self::interpret(&answers, clauses, nodes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};

    fn nodes() -> Vec<NodeInfo> {
        vec![
            NodeInfo {
                id: 3,
                row: 1,
                name: "Clouds".into(),
                kind: "pixels",
            },
            NodeInfo {
                id: 2,
                row: 2,
                name: "Sun".into(),
                kind: "pixels",
            },
            NodeInfo {
                id: 1,
                row: 3,
                name: "Grass".into(),
                kind: "pixels",
            },
        ]
    }

    /// A one-shot HTTP server that records the request and answers as Jev would.
    fn mock(answer: Value) -> (String, std::thread::JoinHandle<(String, Value)>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}/v1/systemone", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut auth = String::new();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let low = line.to_lowercase();
                if low.starts_with("authorization:") {
                    auth = line.trim().to_string();
                }
                if let Some(v) = low.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let reply =
                serde_json::to_vec(&json!({ "model": "jev-1.13.0", "answers": answer })).unwrap();
            let mut w = s;
            write!(
                w,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                reply.len()
            )
            .unwrap();
            w.write_all(&reply).unwrap();
            (auth, serde_json::from_slice(&body).unwrap())
        });
        (addr, h)
    }

    #[test]
    fn one_call_with_typed_questions() {
        let clauses = vec![
            "hide the top two nodes".to_string(),
            "rename the third to Sky".to_string(),
        ];
        let answer = json!({
            "intent_0": { "type": "choice", "choice": "hide", "probabilities": {}, "confidence": 0.93 },
            "intent_1": { "type": "choice", "choice": "rename", "probabilities": {}, "confidence": 0.88 },
            "target_0_3": { "type": "noul", "noul": 0.97 }, "target_0_2": { "type": "noul", "noul": 0.95 },
            "target_0_1": { "type": "noul", "noul": 0.03 }, "target_1_3": { "type": "noul", "noul": 0.02 },
            "target_1_2": { "type": "noul", "noul": 0.04 }, "target_1_1": { "type": "noul", "noul": 0.96 },
        });
        let (url, server) = mock(answer);
        let d = JevDecider {
            jev: Jev::with_endpoint("test-key", url),
        };
        let out = d.decide(&clauses, &nodes()).unwrap();
        let (auth, body) = server.join().unwrap();
        assert_eq!(
            auth,
            "authorization: Bearer test-key".replace("authorization", &auth[..13])
        );
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["questions"]["intent_0"]["type"], "choice");
        assert_eq!(body["questions"]["target_1_1"]["type"], "noul");
        assert_eq!(
            body["questions"].as_object().unwrap().len(),
            2 + 2 * 3,
            "fan-out in one request"
        );
        assert_eq!(out[0].intent, Intent::Hide);
        let t0: Vec<u64> = out[0]
            .targets
            .iter()
            .filter(|(_, p)| *p > 0.5)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(t0, vec![3, 2]);
        assert_eq!(out[1].intent, Intent::Rename);
    }
}
