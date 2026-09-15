//! Shared helpers: a recording policy client and tool call fixtures.

use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::generate_text::ApprovalContext;
use ferrin_core::generate_text::ParsedToolCall;
use ferrin_policy::PolicyClient;
use ferrin_policy::PolicyError;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonValue;
use serde_json::json;

/// Records every evaluation and answers with a fixed result.
pub(crate) struct RecordingClient {
    response: Result<JsonValue, PolicyError>,
    calls: Mutex<Vec<(String, JsonValue)>>,
}

impl RecordingClient {
    pub(crate) fn returning(value: JsonValue) -> Arc<Self> {
        Arc::new(Self {
            response: Ok(value),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn failing(error: PolicyError) -> Arc<Self> {
        Arc::new(Self {
            response: Err(error),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn calls(&self) -> Vec<(String, JsonValue)> {
        self.calls.lock().unwrap().clone()
    }
}

impl PolicyClient for RecordingClient {
    fn evaluate<'a>(
        &'a self,
        path: &'a str,
        input: JsonValue,
    ) -> BoxFuture<'a, Result<JsonValue, PolicyError>> {
        self.calls.lock().unwrap().push((path.to_owned(), input));
        let response = self.response.clone();
        Box::pin(async move { response })
    }
}

pub(crate) fn engine_error() -> PolicyError {
    PolicyError::Engine {
        message: "boom".to_owned(),
    }
}

pub(crate) fn delete_call() -> ParsedToolCall {
    ParsedToolCall::new("call-1", "delete_file", json!({ "path": "/tmp/x" }))
}

pub(crate) fn empty_context() -> ApprovalContext<'static> {
    ApprovalContext {
        messages: &[],
        tools_context: None,
    }
}
