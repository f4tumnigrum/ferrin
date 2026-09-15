//! Request-scoped SSE responses must terminate with a matching JSON-RPC reply.

use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::sse::decode_stream;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use super::EventChannel;
use super::TransportEvent;
use crate::error::McpError;
use crate::protocol::JsonRpcMessage;
use crate::protocol::RequestId;

pub(super) async fn pump_response(
    response: HttpResponse,
    max_event_bytes: usize,
    cancellation: &CancellationToken,
    events: &EventChannel,
    request_id: &RequestId,
) -> Result<(), McpError> {
    let mut stream = std::pin::pin!(decode_stream(response.body, max_event_bytes));
    loop {
        let next = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Ok(()),
            next = stream.next() => next,
        };
        let event = next
            .ok_or_else(|| McpError::protocol("request event stream ended without a response"))?
            .map_err(|error| {
                McpError::transport(format!("request event stream failed: {error}"))
            })?;
        if event.event.as_deref().is_some_and(|name| name != "message") {
            continue;
        }
        let messages = JsonRpcMessage::parse_one_or_many(&event.data)?;
        let mut complete = false;
        for message in messages {
            complete |= match &message {
                JsonRpcMessage::Response(response) => response.id == *request_id,
                JsonRpcMessage::Error(error) => error.id.as_ref() == Some(request_id),
                _ => false,
            };
            events.emit(TransportEvent::Message(message));
        }
        if complete {
            return Ok(());
        }
    }
}
