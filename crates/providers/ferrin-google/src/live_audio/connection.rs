//! DNS-pinned and size-limited Live API WebSocket transport.

use ferrin_provider_util::secure_url::validate_url;
use ferrin_spec::Headers;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::client_async_tls_with_config;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_util::sync::CancellationToken;

use crate::config::API_KEY_HEADER;
use crate::config::SharedConfig;

pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const SERVICE: &str = "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

pub(super) async fn connect(
    config: &SharedConfig,
    headers: &Headers,
    cancellation: &CancellationToken,
) -> Result<(Socket, Headers), ProviderError> {
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProviderError::Cancelled),
        result = connect_inner(config, headers) => result,
    }
}

async fn connect_inner(
    config: &SharedConfig,
    headers: &Headers,
) -> Result<(Socket, Headers), ProviderError> {
    let limit = usize::try_from(config.url_policy.max_body_bytes).unwrap_or(usize::MAX);
    if limit == 0 {
        return Err(
            InvalidArgumentError::new("url_policy", "message size limit must be positive").into(),
        );
    }
    let mut url = config.websocket_url(SERVICE);
    let mut policy_url = url.clone();
    let scheme = if url.scheme() == "ws" {
        "http"
    } else {
        "https"
    };
    policy_url
        .set_scheme(scheme)
        .map_err(|()| InvalidArgumentError::new("base_url", "invalid Live API URL scheme"))?;
    let validated = validate_url(&policy_url, &config.url_policy)
        .await
        .map_err(|_| {
            InvalidArgumentError::new("base_url", "Live API URL rejected by url_policy")
        })?;
    let mut headers = config.headers(headers)?;
    let key = headers
        .get_str(API_KEY_HEADER)
        .ok_or_else(|| InvalidArgumentError::new("api_key", "missing Live API key"))?;
    url.query_pairs_mut().append_pair("key", key);
    headers.as_map_mut().remove(API_KEY_HEADER);
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| InvalidArgumentError::new("base_url", "invalid Live API WebSocket URL"))?;
    // Caller headers may customize authentication, but never handshake framing.
    for (name, value) in headers.as_map() {
        if !matches!(name.as_str(), "host" | "connection" | "upgrade")
            && !name.as_str().starts_with("sec-websocket-")
        {
            request.headers_mut().insert(name.clone(), value.clone());
        }
    }
    let tcp = if validated.addresses.is_empty() {
        // Explicitly trusted origins skip DNS validation by UrlPolicy contract.
        let host = policy_url
            .host_str()
            .ok_or_else(|| InvalidArgumentError::new("base_url", "Live API URL has no host"))?;
        TcpStream::connect((host, policy_url.port_or_known_default().unwrap_or(443))).await
    } else {
        TcpStream::connect(validated.addresses.as_slice()).await
    }
    .map_err(|_| ProviderError::message("could not connect to Google Live API"))?;
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit))
        .write_buffer_size(0)
        .max_write_buffer_size(128 * 1024);
    // TLS retains the original hostname for SNI and certificate verification.
    // Never expose handshake errors: they may include the authenticated URI.
    let (socket, response) = client_async_tls_with_config(request, tcp, Some(ws_config), None)
        .await
        .map_err(|_| ProviderError::message("Google Live API WebSocket handshake failed"))?;
    Ok((socket, Headers::from_map(response.headers().clone())))
}
