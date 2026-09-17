//! Whole-request deadlines, including send and cancellation cleanup.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_mcp::McpClient;
use ferrin_mcp::McpClientConfig;
use ferrin_mcp::McpError;
use ferrin_mcp::RequestOptions;
use ferrin_mcp::elicitation_handler;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::transport::CloseOptions;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::SendOptions;
use ferrin_mcp::transport::TransportCapabilities;
use ferrin_mcp::transport::TransportConfig;
use ferrin_mcp::transport::TransportEvent;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::common::MockTransport;
use super::common::config;
use super::common::discover_result;
use super::common::discovery_capabilities;
use super::common::modern_transport;
use super::common::reply;

struct BlockingTransport {
    inner: Arc<MockTransport>,
    method: &'static str,
    entered: Notify,
    token: Mutex<Option<CancellationToken>>,
    release: Option<Notify>,
    delivered: Notify,
    fail_start: bool,
}

impl BlockingTransport {
    fn new(method: &'static str) -> Arc<Self> {
        Arc::new(Self {
            inner: MockTransport::new(discovery_capabilities(), |message| {
                Ok(if message.method() == Some("server/discover") {
                    vec![reply(message, discover_result())]
                } else {
                    Vec::new()
                })
            }),
            method,
            entered: Notify::new(),
            token: Mutex::new(None),
            release: None,
            delivered: Notify::new(),
            fail_start: false,
        })
    }
}

impl McpTransport for BlockingTransport {
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>> {
        if self.fail_start {
            Box::pin(async { Err(McpError::transport("injected startup failure")) })
        } else {
            self.inner.start()
        }
    }
    fn incoming(&self) -> BoxStream<'static, TransportEvent> {
        self.inner.incoming()
    }
    fn close(&self, options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>> {
        if self.fail_start {
            *self.token.lock().unwrap() = options.cancellation;
            Box::pin(std::future::pending())
        } else {
            self.inner.close(options)
        }
    }
    fn protocol_version(&self) -> Option<String> {
        self.inner.protocol_version()
    }
    fn set_protocol_version(&self, version: Option<&str>) {
        self.inner.set_protocol_version(version);
    }
    fn capabilities(&self) -> TransportCapabilities {
        self.inner.capabilities()
    }
    fn send(
        &self,
        message: JsonRpcMessage,
        options: SendOptions,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            if message.method() == Some(self.method) {
                *self.token.lock().unwrap() = options.cancellation;
                self.entered.notify_one();
                if let Some(release) = &self.release {
                    release.notified().await;
                    self.delivered.notify_one();
                    Ok(())
                } else {
                    std::future::pending().await
                }
            } else {
                self.inner.send(message, options).await
            }
        })
    }
}

#[tokio::test(start_paused = true)]
async fn request_deadline_covers_hanging_send() {
    let transport = BlockingTransport::new("ping");
    let client = McpClient::connect(McpClientConfig::new(TransportConfig::Custom(
        transport.clone(),
    )))
    .await
    .unwrap();
    let timeout = Duration::from_millis(10);
    let started = tokio::time::Instant::now();
    let result = client.ping(RequestOptions::with_timeout(timeout)).await;
    assert!(matches!(result, Err(McpError::Timeout(value)) if value == timeout));
    assert_eq!(started.elapsed(), timeout);
    assert!(
        transport
            .token
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
}

#[tokio::test(start_paused = true)]
async fn initialization_deadline_includes_protocol_discovery() {
    let transport = BlockingTransport::new("server/discover");
    let timeout = Duration::from_millis(10);
    let started = tokio::time::Instant::now();
    let result = McpClient::connect(
        McpClientConfig::new(TransportConfig::Custom(transport.clone()))
            .initialization_timeout(timeout),
    )
    .await;
    assert!(matches!(result, Err(McpError::Timeout(value)) if value == timeout));
    assert_eq!(started.elapsed(), timeout);
    assert!(transport.inner.is_closed());
    assert!(
        transport
            .token
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
}

#[tokio::test(start_paused = true)]
async fn hanging_cancellation_notification_does_not_delay_timeout() {
    let transport = BlockingTransport::new("notifications/cancelled");
    let client = McpClient::connect(
        McpClientConfig::new(TransportConfig::Custom(transport.clone()))
            .send_cancel_notifications(true),
    )
    .await
    .unwrap();
    let timeout = Duration::from_millis(10);
    let started = tokio::time::Instant::now();
    assert!(matches!(
        client.ping(RequestOptions::with_timeout(timeout)).await,
        Err(McpError::Timeout(_))
    ));
    assert_eq!(started.elapsed(), timeout);
    transport.entered.notified().await;
}

#[tokio::test(start_paused = true)]
async fn caller_cancellation_interrupts_hanging_send() {
    let transport = BlockingTransport::new("ping");
    let client = McpClient::connect(McpClientConfig::new(TransportConfig::Custom(
        transport.clone(),
    )))
    .await
    .unwrap();
    let cancellation = CancellationToken::new();
    let (outcome, ()) = tokio::join!(
        client.ping(RequestOptions::default().cancellation(cancellation.clone())),
        async {
            transport.entered.notified().await;
            cancellation.cancel();
        }
    );
    assert!(matches!(outcome, Err(McpError::Cancelled)));
}

#[tokio::test(start_paused = true)]
async fn request_deadline_covers_input_handler() {
    let transport = modern_transport(|_| {
        Ok(json!({
            "resultType": "input_required",
            "inputRequests": {"answer": {"method": "elicitation/create", "params": {"message": "Answer", "requestedSchema": {"type": "object"}}}}
        }))
    });
    let client = McpClient::connect(config(transport).max_input_rounds(8).elicitation_handler(
        elicitation_handler(|_| async { std::future::pending().await }),
    ))
    .await
    .unwrap();
    let timeout = Duration::from_millis(10);
    assert!(
        matches!(client.ping(RequestOptions::with_timeout(timeout)).await, Err(McpError::Timeout(value)) if value == timeout)
    );
}

#[tokio::test(start_paused = true)]
async fn asynchronous_cancellation_notification_is_delivered_after_timeout_returns() {
    let mut transport = BlockingTransport::new("notifications/cancelled");
    Arc::get_mut(&mut transport).unwrap().release = Some(Notify::new());
    let client = McpClient::connect(
        McpClientConfig::new(TransportConfig::Custom(transport.clone()))
            .send_cancel_notifications(true),
    )
    .await
    .unwrap();
    let timeout = Duration::from_millis(10);
    let started = tokio::time::Instant::now();
    assert!(matches!(
        client.ping(RequestOptions::with_timeout(timeout)).await,
        Err(McpError::Timeout(_))
    ));
    assert_eq!(started.elapsed(), timeout);
    transport.entered.notified().await;
    transport.release.as_ref().unwrap().notify_one();
    transport.delivered.notified().await;
}

#[tokio::test(start_paused = true)]
async fn cancellation_cleanup_has_a_separate_bound() {
    let transport = BlockingTransport::new("notifications/cancelled");
    let client = McpClient::connect(
        McpClientConfig::new(TransportConfig::Custom(transport.clone()))
            .send_cancel_notifications(true),
    )
    .await
    .unwrap();
    assert!(matches!(
        client
            .ping(RequestOptions::with_timeout(Duration::from_millis(10)))
            .await,
        Err(McpError::Timeout(_))
    ));
    transport.entered.notified().await;
    let token = transport.token.lock().unwrap().clone().unwrap();
    let started = tokio::time::Instant::now();
    token.cancelled().await;
    assert_eq!(started.elapsed(), Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn final_client_drop_cancels_pending_cleanup() {
    let transport = BlockingTransport::new("notifications/cancelled");
    let client = McpClient::connect(
        McpClientConfig::new(TransportConfig::Custom(transport.clone()))
            .send_cancel_notifications(true),
    )
    .await
    .unwrap();
    assert!(matches!(
        client
            .ping(RequestOptions::with_timeout(Duration::from_millis(10)))
            .await,
        Err(McpError::Timeout(_))
    ));
    transport.entered.notified().await;
    let token = transport.token.lock().unwrap().clone().unwrap();
    let started = tokio::time::Instant::now();
    drop(client);
    token.cancelled().await;
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn failed_start_does_not_wait_indefinitely_for_custom_transport_cleanup() {
    let mut transport = BlockingTransport::new("unused");
    Arc::get_mut(&mut transport).unwrap().fail_start = true;
    let started = tokio::time::Instant::now();
    let error = McpClient::connect(McpClientConfig::new(TransportConfig::Custom(
        transport.clone(),
    )))
    .await
    .unwrap_err();
    assert!(error.to_string().contains("injected startup failure"));
    assert_eq!(started.elapsed(), Duration::from_secs(1));
    assert!(
        transport
            .token
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .is_cancelled()
    );
}
