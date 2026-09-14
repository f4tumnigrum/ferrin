//! Handling of server-initiated requests (`ping`, `elicitation/create`).

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;

use super::ClientInner;
use crate::error::McpError;
use crate::protocol::INTERNAL_ERROR;
use crate::protocol::INVALID_PARAMS;
use crate::protocol::JsonRpcMessage;
use crate::protocol::JsonRpcRequest;
use crate::protocol::METHOD_NOT_FOUND;
use crate::transport::SendOptions;
use crate::transport::lock;

impl ClientInner {
    async fn elicit(&self, params: Option<JsonObject>) -> Result<JsonObject, (i64, String)> {
        let handler = lock(&self.elicitation).clone();
        let Some(handler) = handler else {
            return Err((
                METHOD_NOT_FOUND,
                "no elicitation handler registered on client".to_owned(),
            ));
        };
        let request = serde_json::from_value(JsonValue::Object(params.unwrap_or_default()))
            .map_err(|error| {
                (
                    INVALID_PARAMS,
                    format!("invalid elicitation params: {error}"),
                )
            })?;
        let result = handler.handle(request).await.map_err(|error| {
            (
                INTERNAL_ERROR,
                format!("elicitation handler failed: {error}"),
            )
        })?;
        match serde_json::to_value(result) {
            Ok(JsonValue::Object(object)) => Ok(object),
            _ => Err((
                INTERNAL_ERROR,
                "elicitation result is not an object".to_owned(),
            )),
        }
    }

    pub(super) async fn handle_server_request(&self, request: JsonRpcRequest) {
        let outcome = match request.method.as_str() {
            "ping" => Ok(JsonObject::new()),
            "elicitation/create" => self.elicit(request.params).await,
            other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
        };
        let message = match outcome {
            Ok(result) => JsonRpcMessage::response(request.id, result),
            Err((code, message)) => JsonRpcMessage::error(Some(request.id), code, message, None),
        };
        if let Err(error) = self.transport.send(message, SendOptions::default()).await {
            self.report(McpError::transport(format!(
                "failed to answer server request {}: {error}",
                request.method
            )));
        }
    }
}
