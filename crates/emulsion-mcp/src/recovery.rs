//! Explicit recovery information for failures with known transport provenance.
//!
//! Keep the original text first for existing clients. The second content block
//! is machine-readable without requiring a newer MCP protocol version.

use crate::server::ToolResult;
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    AppUnavailable,
    ConnectionLost,
    ToolTimeout,
    DocumentClosed,
    AuthenticationRejected,
    InvalidRequest,
    UnknownTool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    NotStarted,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryPolicy {
    Reconnect,
    InspectBeforeRetry,
    DoNotRetry,
    CorrectRequest,
}

pub fn tool_error(
    code: ErrorCode,
    message: impl Into<String>,
    execution_state: ExecutionState,
    retry_policy: RetryPolicy,
) -> ToolResult {
    let message = message.into();
    let recovery = match retry_policy {
        RetryPolicy::Reconnect => json!({
            "action": "Start Emulsion and restore this session's app connection, then inspect the document before retrying. The request was not submitted.",
            "inspection_tools": ["describe_document", "get_view"],
        }),
        RetryPolicy::InspectBeforeRetry => json!({
            "action": "Do not repeat the operation automatically. Allow any pending work to finish, reconnect if needed, then inspect the document and preview before deciding what remains. For library or file operations, verify the relevant library state or output too; document inspection alone cannot confirm them. If the outcome cannot be verified, report that uncertainty instead of replaying it.",
            "inspection_tools": ["describe_document", "get_view"],
        }),
        RetryPolicy::DoNotRetry => json!({
            "action": match code {
                ErrorCode::DocumentClosed => "Do not retry this request. The target document was closed; establish a session with the intended open document before issuing new calls.",
                ErrorCode::AuthenticationRejected => "Do not retry this request. Start a fresh MCP connection from Emulsion because this connection was rejected.",
                ErrorCode::InvalidRequest => "Do not repeat this malformed relay request. Correct the relay message before reconnecting.",
                _ => "Do not retry this request until the reported failure is resolved.",
            },
        }),
        RetryPolicy::CorrectRequest => json!({
            "action": "Check tools/list and correct the tool name or arguments before submitting a new request.",
        }),
    };
    let mut result = ToolResult::error(&message);
    result.content.push(json!({
        "type": "text",
        "text": json!({
            "ok": false,
            "code": code,
            "message": message,
            "execution_state": execution_state,
            "retry_policy": retry_policy,
            "recovery": recovery,
        }).to_string(),
    }));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_text_and_reports_uncertain_execution_without_replay() {
        let result = tool_error(
            ErrorCode::ConnectionLost,
            "connection broke",
            ExecutionState::Unknown,
            RetryPolicy::InspectBeforeRetry,
        );
        assert!(result.is_error);
        assert_eq!(result.content[0]["text"], "connection broke");
        let envelope: serde_json::Value =
            serde_json::from_str(result.content[1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["code"], "connection_lost");
        assert_eq!(envelope["execution_state"], "unknown");
        assert_eq!(envelope["retry_policy"], "inspect_before_retry");
        assert_eq!(
            envelope["recovery"]["inspection_tools"],
            json!(["describe_document", "get_view"])
        );
    }
}
