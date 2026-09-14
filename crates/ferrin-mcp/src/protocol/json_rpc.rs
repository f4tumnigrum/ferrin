//! JSON-RPC 2.0 message types.

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use serde::Deserialize;
use serde::Serialize;

use crate::error::McpError;

/// JSON-RPC protocol version string.
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC `parse error`.
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC `invalid request`.
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC `method not found`.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC `invalid params`.
pub const INVALID_PARAMS: i64 = -32602;
/// JSON-RPC `internal error`.
pub const INTERNAL_ERROR: i64 = -32603;

/// Request identifier (string or integer).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// Numeric id.
    Number(i64),
    /// String id.
    String(String),
}

impl From<i64> for RequestId {
    fn from(value: i64) -> Self {
        Self::Number(value)
    }
}

impl From<&str> for RequestId {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(number) => write!(f, "{number}"),
            Self::String(text) => f.write_str(text),
        }
    }
}

/// A request expecting a response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// Request id.
    pub id: RequestId,
    /// Method name.
    pub method: String,
    /// Parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<JsonObject>,
}

/// A notification (no response expected).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// Method name.
    pub method: String,
    /// Parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<JsonObject>,
}

/// A successful response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Id of the request answered.
    pub id: RequestId,
    /// Result object.
    pub result: JsonObject,
}

/// Error payload of an error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcErrorObject {
    /// Error code.
    pub code: i64,
    /// Error message.
    pub message: String,
    /// Additional data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
}

/// An error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Id of the request answered (`None` when the request id could not be
    /// determined).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    /// The error.
    pub error: JsonRpcErrorObject,
}

/// Any JSON-RPC message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JsonRpcMessage {
    /// A request.
    Request(JsonRpcRequest),
    /// A notification.
    Notification(JsonRpcNotification),
    /// A success response.
    Response(JsonRpcResponse),
    /// An error response.
    Error(JsonRpcError),
}

#[derive(Deserialize)]
struct Envelope {
    jsonrpc: String,
    #[serde(default)]
    id: Option<RequestId>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<JsonObject>,
    #[serde(default)]
    result: Option<JsonObject>,
    #[serde(default)]
    error: Option<JsonRpcErrorObject>,
}

impl JsonRpcMessage {
    /// Creates a request message.
    #[must_use]
    pub fn request(
        id: impl Into<RequestId>,
        method: impl Into<String>,
        params: Option<JsonObject>,
    ) -> Self {
        Self::Request(JsonRpcRequest {
            id: id.into(),
            method: method.into(),
            params,
        })
    }

    /// Creates a notification message.
    #[must_use]
    pub fn notification(method: impl Into<String>, params: Option<JsonObject>) -> Self {
        Self::Notification(JsonRpcNotification {
            method: method.into(),
            params,
        })
    }

    /// Creates a success response.
    #[must_use]
    pub fn response(id: RequestId, result: JsonObject) -> Self {
        Self::Response(JsonRpcResponse { id, result })
    }

    /// Creates an error response.
    #[must_use]
    pub fn error(
        id: Option<RequestId>,
        code: i64,
        message: impl Into<String>,
        data: Option<JsonValue>,
    ) -> Self {
        Self::Error(JsonRpcError {
            id,
            error: JsonRpcErrorObject {
                code,
                message: message.into(),
                data,
            },
        })
    }

    /// The request id, for requests and responses.
    #[must_use]
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            Self::Request(request) => Some(&request.id),
            Self::Response(response) => Some(&response.id),
            Self::Error(error) => error.id.as_ref(),
            Self::Notification(_) => None,
        }
    }

    /// The method, for requests and notifications.
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        match self {
            Self::Request(request) => Some(&request.method),
            Self::Notification(notification) => Some(&notification.method),
            Self::Response(_) | Self::Error(_) => None,
        }
    }

    /// The parameters, for requests and notifications.
    #[must_use]
    pub fn params(&self) -> Option<&JsonObject> {
        match self {
            Self::Request(request) => request.params.as_ref(),
            Self::Notification(notification) => notification.params.as_ref(),
            Self::Response(_) | Self::Error(_) => None,
        }
    }

    /// Serializes the message to a JSON value.
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        let mut object = JsonObject::new();
        object.insert("jsonrpc".to_owned(), JsonValue::from(JSONRPC_VERSION));
        match self {
            Self::Request(request) => {
                object.insert(
                    "id".to_owned(),
                    serde_json::to_value(&request.id).unwrap_or(JsonValue::Null),
                );
                object.insert(
                    "method".to_owned(),
                    JsonValue::from(request.method.as_str()),
                );
                if let Some(params) = &request.params {
                    object.insert("params".to_owned(), JsonValue::Object(params.clone()));
                }
            }
            Self::Notification(notification) => {
                object.insert(
                    "method".to_owned(),
                    JsonValue::from(notification.method.as_str()),
                );
                if let Some(params) = &notification.params {
                    object.insert("params".to_owned(), JsonValue::Object(params.clone()));
                }
            }
            Self::Response(response) => {
                object.insert(
                    "id".to_owned(),
                    serde_json::to_value(&response.id).unwrap_or(JsonValue::Null),
                );
                object.insert(
                    "result".to_owned(),
                    JsonValue::Object(response.result.clone()),
                );
            }
            Self::Error(error) => {
                object.insert(
                    "id".to_owned(),
                    error.id.as_ref().map_or(JsonValue::Null, |id| {
                        serde_json::to_value(id).unwrap_or(JsonValue::Null)
                    }),
                );
                object.insert(
                    "error".to_owned(),
                    serde_json::to_value(&error.error).unwrap_or(JsonValue::Null),
                );
            }
        }
        JsonValue::Object(object)
    }

    /// Serializes the message to compact JSON text.
    #[must_use]
    pub fn to_json_string(&self) -> String {
        self.to_json().to_string()
    }

    /// Parses a message from a JSON value.
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Protocol`] when the value is not a JSON-RPC 2.0
    /// message.
    pub fn from_json(value: JsonValue) -> Result<Self, McpError> {
        let envelope: Envelope = serde_json::from_value(value)
            .map_err(|error| McpError::protocol(format!("invalid JSON-RPC message: {error}")))?;
        if envelope.jsonrpc != JSONRPC_VERSION {
            return Err(McpError::protocol(format!(
                "unsupported JSON-RPC version '{}'",
                envelope.jsonrpc
            )));
        }
        match (
            envelope.method,
            envelope.id,
            envelope.result,
            envelope.error,
        ) {
            (Some(method), Some(id), None, None) => Ok(Self::Request(JsonRpcRequest {
                id,
                method,
                params: envelope.params,
            })),
            (Some(method), None, None, None) => Ok(Self::Notification(JsonRpcNotification {
                method,
                params: envelope.params,
            })),
            (None, Some(id), Some(result), None) => {
                Ok(Self::Response(JsonRpcResponse { id, result }))
            }
            (None, id, None, Some(error)) => Ok(Self::Error(JsonRpcError { id, error })),
            _ => Err(McpError::protocol(
                "invalid JSON-RPC message: expected a request, notification, result or error",
            )),
        }
    }

    /// Parses a message from JSON text.
    ///
    /// # Errors
    ///
    /// See [`JsonRpcMessage::from_json`].
    pub fn parse(text: &str) -> Result<Self, McpError> {
        let value: JsonValue = serde_json::from_str(text)
            .map_err(|error| McpError::protocol(format!("invalid JSON-RPC message: {error}")))?;
        Self::from_json(value)
    }

    /// Parses one message or a batch (JSON array) of messages.
    ///
    /// # Errors
    ///
    /// See [`JsonRpcMessage::from_json`].
    pub fn parse_one_or_many(text: &str) -> Result<Vec<Self>, McpError> {
        let value: JsonValue = serde_json::from_str(text)
            .map_err(|error| McpError::protocol(format!("invalid JSON-RPC message: {error}")))?;
        match value {
            JsonValue::Array(items) => items.into_iter().map(Self::from_json).collect(),
            other => Ok(vec![Self::from_json(other)?]),
        }
    }
}
