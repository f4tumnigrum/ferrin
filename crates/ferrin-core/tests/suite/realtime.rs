use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::Error;
use ferrin_core::realtime::ClientSecret;
use ferrin_core::realtime::RealtimeClientEvent;
use ferrin_core::realtime::RealtimeServerEvent;
use ferrin_core::realtime::RealtimeSession;
use ferrin_core::realtime::realtime_session;
use ferrin_core::realtime::realtime_tool_definitions;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::RealtimeModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::realtime_model::ClientSecretOptions;
use ferrin_spec::realtime_model::RealtimeSessionConfig;
use ferrin_spec::realtime_model::WebSocketConfig;
use ferrin_tool::ToolSet;
use futures_util::SinkExt;
use futures_util::StreamExt;
use http::header::SEC_WEBSOCKET_PROTOCOL;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::Request as HandshakeRequest;
use tokio_tungstenite::tungstenite::handshake::server::Response as HandshakeResponse;
use url::Url;

use super::common::weather_tools;

/// A local WebSocket server that records every JSON message from the client
/// and forwards scripted messages to it.
struct MockServer {
    url: Url,
    received: Arc<Mutex<Vec<JsonValue>>>,
    to_client: mpsc::Sender<JsonValue>,
    tasks: JoinSet<()>,
}

impl MockServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let (to_client, mut from_test) = mpsc::channel::<JsonValue>(32);
        let mut tasks = JoinSet::new();
        let sink = Arc::clone(&received);
        tasks.spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // Echo the first requested sub-protocol, as real servers do.
            #[allow(
                clippy::result_large_err,
                reason = "the callback signature is fixed by tungstenite"
            )]
            let mut ws = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &HandshakeRequest, mut response: HandshakeResponse| {
                    if let Some(protocols) = request.headers().get(SEC_WEBSOCKET_PROTOCOL) {
                        let first = protocols
                            .to_str()
                            .unwrap()
                            .split(',')
                            .next()
                            .unwrap()
                            .trim()
                            .to_owned();
                        response
                            .headers_mut()
                            .insert(SEC_WEBSOCKET_PROTOCOL, first.parse().unwrap());
                    }
                    Ok(response)
                },
            )
            .await
            .unwrap();
            loop {
                tokio::select! {
                    frame = ws.next() => match frame {
                        Some(Ok(Message::Text(text))) => {
                            let value: JsonValue = serde_json::from_str(text.as_str()).unwrap();
                            sink.lock().unwrap().push(value);
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(_)) => break,
                    },
                    outbound = from_test.recv() => match outbound {
                        Some(value) => ws.send(Message::text(value.to_string())).await.unwrap(),
                        None => {
                            let _ = ws.send(Message::Close(None)).await;
                            break;
                        }
                    },
                }
            }
        });
        Self {
            url: Url::parse(&format!("ws://127.0.0.1:{port}/realtime")).unwrap(),
            received,
            to_client,
            tasks,
        }
    }

    async fn push(&self, value: JsonValue) {
        self.to_client.send(value).await.unwrap();
    }

    /// Waits until the client sent `count` messages and returns them.
    async fn wait_for(&self, count: usize) -> Vec<JsonValue> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = self.received.lock().unwrap().clone();
            if snapshot.len() >= count {
                return snapshot;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {count} messages, got {snapshot:?}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn secret(&self) -> ClientSecret {
        ClientSecret {
            token: "ek_test".to_owned(),
            url: self.url.clone(),
            expires_at: None,
        }
    }

    async fn shutdown(mut self) {
        drop(self.to_client);
        while self.tasks.join_next().await.is_some() {}
    }
}

/// A realtime model whose wire format is the standardized event JSON.
struct MockRealtimeModel {
    provider: ProviderId,
    model_id: ModelId,
    secret_url: Option<Url>,
}

impl RealtimeModel for MockRealtimeModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn do_create_client_secret(
        &self,
        options: ClientSecretOptions,
    ) -> impl Future<Output = Result<ClientSecret, ProviderError>> + Send {
        let url = self.secret_url.clone();
        async move {
            let url = url.ok_or_else(|| ProviderError::unsupported("client secrets"))?;
            assert!(options.session_config.is_some());
            Ok(ClientSecret {
                token: "ek_created".to_owned(),
                url,
                expires_at: Some(1_900_000_000),
            })
        }
    }

    fn websocket_config(&self, token: &str, url: &Url) -> WebSocketConfig {
        let mut url = url.clone();
        url.query_pairs_mut().append_pair("token", token);
        WebSocketConfig {
            url,
            protocols: vec!["mock.realtime.v1".to_owned()],
        }
    }

    fn parse_server_event(
        &self,
        raw: JsonValue,
    ) -> Result<Vec<RealtimeServerEvent>, ProviderError> {
        if raw.get("type").and_then(JsonValue::as_str) == Some("broken") {
            return Err(ProviderError::Other("broken message".into()));
        }
        Ok(vec![serde_json::from_value(raw).map_err(|error| {
            ProviderError::Other(error.to_string().into())
        })?])
    }

    fn serialize_client_event(
        &self,
        event: RealtimeClientEvent,
    ) -> impl Future<Output = Result<JsonValue, ProviderError>> + Send {
        std::future::ready(
            serde_json::to_value(event)
                .map_err(|error| ProviderError::Other(error.to_string().into())),
        )
    }

    fn build_session_config(
        &self,
        config: &RealtimeSessionConfig,
    ) -> Result<JsonValue, ProviderError> {
        serde_json::to_value(config).map_err(|error| ProviderError::Other(error.to_string().into()))
    }

    fn health_check_response(&self, raw: &JsonValue) -> Option<JsonValue> {
        (raw.get("type")?.as_str()? == "ping").then(|| json!({ "type": "pong" }))
    }
}

fn model() -> Arc<MockRealtimeModel> {
    Arc::new(MockRealtimeModel {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("realtime-mock"),
        secret_url: None,
    })
}

async fn next(session: &mut RealtimeSession) -> Result<RealtimeServerEvent, Error> {
    tokio::time::timeout(Duration::from_secs(5), session.next_event())
        .await
        .expect("timed out waiting for an event")
        .expect("stream ended")
}

fn created(session_id: &str) -> JsonValue {
    json!({ "type": "session-created", "session_id": session_id, "raw": {} })
}

#[tokio::test]
async fn connects_sends_session_update_and_streams_events() {
    let server = MockServer::start().await;
    let mut session = realtime_session(model())
        .client_secret(server.secret())
        .instructions("Be brief.")
        .voice("alloy")
        .tools(weather_tools())
        .connect()
        .await
        .unwrap();

    let sent = server.wait_for(1).await;
    assert_eq!(sent[0]["type"], "session-update");
    assert_eq!(sent[0]["config"]["instructions"], "Be brief.");
    assert_eq!(sent[0]["config"]["voice"], "alloy");
    assert_eq!(sent[0]["config"]["tools"][0]["name"], "get_weather");
    assert_eq!(
        sent[0]["config"]["tools"][0]["description"],
        "Get the weather for a city."
    );

    server.push(created("sess_1")).await;
    let event = next(&mut session).await.unwrap();
    assert!(
        matches!(&event, RealtimeServerEvent::SessionCreated { session_id: Some(id), .. } if id == "sess_1"),
        "{event:?}"
    );

    session.send_text("hello").await.unwrap();
    let sent = server.wait_for(3).await;
    assert_eq!(sent[1]["type"], "conversation-item-create");
    assert_eq!(sent[1]["item"]["type"], "text-message");
    assert_eq!(sent[1]["item"]["text"], "hello");
    assert_eq!(sent[2]["type"], "response-create");

    session.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn executes_local_tools_and_requests_one_follow_up_response() {
    let server = MockServer::start().await;
    let mut session = realtime_session(model())
        .client_secret(server.secret())
        .tools(weather_tools())
        .connect()
        .await
        .unwrap();
    server.wait_for(1).await;

    for (call_id, city) in [("call_1", "Rome"), ("call_2", "Oslo")] {
        server
            .push(json!({
                "type": "function-call-arguments-done",
                "response_id": "resp_1",
                "item_id": format!("item_{call_id}"),
                "call_id": call_id,
                "name": "get_weather",
                "arguments": json!({ "city": city }).to_string(),
                "raw": {}
            }))
            .await;
        let event = next(&mut session).await.unwrap();
        assert!(
            matches!(event, RealtimeServerEvent::FunctionCallArgumentsDone { .. }),
            "{event:?}"
        );
    }
    // Both outputs are submitted; no response is requested until the
    // tool-bearing response is done.
    let sent = server.wait_for(3).await;
    let outputs: Vec<&JsonValue> = sent[1..]
        .iter()
        .filter(|message| message["item"]["type"] == "function-call-output")
        .collect();
    assert_eq!(outputs.len(), 2);
    for output in &outputs {
        assert_eq!(output["item"]["name"], "get_weather");
        let payload: JsonValue =
            serde_json::from_str(output["item"]["output"].as_str().unwrap()).unwrap();
        assert_eq!(payload["temperature"], 21);
    }
    assert!(
        sent.iter()
            .all(|message| message["type"] != "response-create")
    );

    server
        .push(json!({ "type": "response-done", "response_id": "resp_1", "status": "completed", "raw": {} }))
        .await;
    let event = next(&mut session).await.unwrap();
    assert!(matches!(event, RealtimeServerEvent::ResponseDone { .. }));
    let sent = server.wait_for(4).await;
    assert_eq!(sent[3]["type"], "response-create");
    assert_eq!(sent.len(), 4);

    session.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn unknown_tools_and_broken_messages_surface_as_errors() {
    let server = MockServer::start().await;
    let mut session = realtime_session(model())
        .client_secret(server.secret())
        .tools(weather_tools())
        .connect()
        .await
        .unwrap();
    server.wait_for(1).await;

    server
        .push(json!({
            "type": "function-call-arguments-done",
            "response_id": "resp_1",
            "item_id": "item_1",
            "call_id": "call_1",
            "name": "launch_rockets",
            "arguments": "{}",
            "raw": {}
        }))
        .await;
    next(&mut session).await.unwrap();
    let error = next(&mut session).await.unwrap_err();
    assert!(matches!(error, Error::NoSuchTool { .. }), "{error}");

    server.push(json!({ "type": "broken" })).await;
    let error = next(&mut session).await.unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");

    // Malformed arguments for a known tool are reported without an output.
    server
        .push(json!({
            "type": "function-call-arguments-done",
            "response_id": "resp_1",
            "item_id": "item_2",
            "call_id": "call_2",
            "name": "get_weather",
            "arguments": "{ not json",
            "raw": {}
        }))
        .await;
    next(&mut session).await.unwrap();
    let error = next(&mut session).await.unwrap_err();
    assert!(matches!(error, Error::InvalidToolInput(_)), "{error}");

    session.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn health_checks_are_answered_and_unknown_events_pass_through() {
    let server = MockServer::start().await;
    let mut session = realtime_session(model())
        .client_secret(server.secret())
        .connect()
        .await
        .unwrap();
    server.wait_for(1).await;

    server.push(json!({ "type": "ping" })).await;
    let sent = server.wait_for(2).await;
    assert_eq!(sent[1]["type"], "pong");
    // The ping is still parsed; the mock maps unknown types through serde,
    // which fails, so an error item follows.
    let error = next(&mut session).await.unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");

    server
        .push(json!({ "type": "custom", "raw_type": "rate_limits.updated", "raw": { "x": 1 } }))
        .await;
    let event = next(&mut session).await.unwrap();
    assert!(
        matches!(event, RealtimeServerEvent::Custom { raw_type, .. } if raw_type == "rate_limits.updated")
    );

    session.close().await.unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn creates_a_client_secret_when_none_is_given() {
    let server = MockServer::start().await;
    let issuing = Arc::new(MockRealtimeModel {
        provider: ProviderId::new("mock"),
        model_id: ModelId::new("realtime-mock"),
        secret_url: Some(server.url.clone()),
    });
    let session = realtime_session(issuing)
        .expires_after_seconds(600)
        .connect()
        .await
        .unwrap();
    assert!(!session.is_closed());
    server.wait_for(1).await;
    session.close().await.unwrap();
    server.shutdown().await;

    let error = realtime_session(model()).connect().await.unwrap_err();
    assert!(error.as_provider().is_some(), "{error}");
}

#[tokio::test]
async fn server_close_ends_the_stream() {
    let server = MockServer::start().await;
    let mut session = realtime_session(model())
        .client_secret(server.secret())
        .connect()
        .await
        .unwrap();
    server.wait_for(1).await;
    server.shutdown().await;
    let ended = tokio::time::timeout(Duration::from_secs(5), session.next_event())
        .await
        .unwrap();
    assert!(ended.is_none());
    assert!(session.is_closed());
    let error = session.send_text("late").await.unwrap_err();
    assert!(error.to_string().contains("closed"), "{error}");
}

#[tokio::test]
async fn tool_definitions_skip_provider_tools() {
    let definitions = realtime_tool_definitions(&weather_tools(), None)
        .await
        .unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "get_weather");
    assert_eq!(definitions[0].parameters["type"], "object");
    assert!(
        realtime_tool_definitions(&ToolSet::new(), None)
            .await
            .unwrap()
            .is_empty()
    );
}
