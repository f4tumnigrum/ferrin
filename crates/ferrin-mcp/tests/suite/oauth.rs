//! OAuth discovery, PKCE, the `auth` flow and the transport's `401` handling.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_mcp::McpError;
use ferrin_mcp::oauth::AuthOptions;
use ferrin_mcp::oauth::AuthResult;
use ferrin_mcp::oauth::AuthorizationServerMetadata;
use ferrin_mcp::oauth::InvalidateScope;
use ferrin_mcp::oauth::OAuthAuthorizationServerInformation;
use ferrin_mcp::oauth::OAuthClientInformation;
use ferrin_mcp::oauth::OAuthClientMetadata;
use ferrin_mcp::oauth::OAuthClientProvider;
use ferrin_mcp::oauth::OAuthTokens;
use ferrin_mcp::oauth::ProtectedResourceMetadata;
use ferrin_mcp::oauth::auth;
use ferrin_mcp::oauth::authorization_server_metadata_urls;
use ferrin_mcp::oauth::discover_authorization_server_metadata;
use ferrin_mcp::oauth::extract_www_authenticate_params;
use ferrin_mcp::oauth::generate_pkce;
use ferrin_mcp::oauth::pkce_challenge;
use ferrin_mcp::oauth::protected_resource_metadata_urls;
use ferrin_mcp::oauth::select_resource_url;
use ferrin_mcp::oauth::start_authorization;
use ferrin_mcp::protocol::JsonRpcMessage;
use ferrin_mcp::transport::HttpTransport;
use ferrin_mcp::transport::HttpTransportConfig;
use ferrin_mcp::transport::McpTransport;
use ferrin_mcp::transport::SendOptions;
use ferrin_mcp::transport::TransportEvent;
use ferrin_provider_util::http::default_transport;
use ferrin_spec::Headers;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use http::Method;
use http::StatusCode;
use pretty_assertions::assert_eq;
use secrecy::ExposeSecret;
use serde_json::json;
use url::Url;

use super::common::local_policy;

#[derive(Default)]
struct TestProvider {
    client: Mutex<Option<OAuthClientInformation>>,
    tokens: Mutex<Option<OAuthTokens>>,
    verifier: Mutex<Option<String>>,
    redirects: Mutex<Vec<Url>>,
    invalidations: Mutex<Vec<InvalidateScope>>,
    state: Mutex<Option<String>>,
}

impl TestProvider {
    fn with_tokens(mut tokens: OAuthTokens, server_url: &Url) -> Arc<Self> {
        let provider = Self::default();
        tokens.authorization_server_information = Some(OAuthAuthorizationServerInformation {
            issuer: Some(server_url.to_string()),
            authorization_server_url: server_url.clone(),
            token_endpoint: server_url.join("/token").unwrap(),
        });
        *provider.tokens.lock().unwrap() = Some(tokens);
        *provider.client.lock().unwrap() = Some(OAuthClientInformation::public("c1"));
        Arc::new(provider)
    }

    fn tokens_snapshot(&self) -> Option<OAuthTokens> {
        self.tokens.lock().unwrap().clone()
    }
}

impl OAuthClientProvider for TestProvider {
    fn redirect_url(&self) -> Option<Url> {
        Some(Url::parse("http://localhost:1/callback").unwrap())
    }

    fn client_metadata(&self) -> OAuthClientMetadata {
        OAuthClientMetadata {
            redirect_uris: vec![Url::parse("http://localhost:1/callback").unwrap()],
            client_name: Some("ferrin tests".to_owned()),
            scope: Some("fallback".to_owned()),
            ..OAuthClientMetadata::default()
        }
    }

    fn client_information(
        &self,
    ) -> BoxFuture<'_, Result<Option<OAuthClientInformation>, McpError>> {
        Box::pin(std::future::ready(Ok(self.client.lock().unwrap().clone())))
    }

    fn save_client_information(
        &self,
        information: OAuthClientInformation,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        *self.client.lock().unwrap() = Some(information);
        Box::pin(std::future::ready(Ok(())))
    }

    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>, McpError>> {
        Box::pin(std::future::ready(Ok(self.tokens.lock().unwrap().clone())))
    }

    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<(), McpError>> {
        *self.tokens.lock().unwrap() = Some(tokens);
        Box::pin(std::future::ready(Ok(())))
    }

    fn redirect_to_authorization(
        &self,
        authorization_url: Url,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        self.redirects.lock().unwrap().push(authorization_url);
        Box::pin(std::future::ready(Ok(())))
    }

    fn save_code_verifier(&self, code_verifier: String) -> BoxFuture<'_, Result<(), McpError>> {
        *self.verifier.lock().unwrap() = Some(code_verifier);
        Box::pin(std::future::ready(Ok(())))
    }

    fn code_verifier(&self) -> BoxFuture<'_, Result<Option<String>, McpError>> {
        Box::pin(std::future::ready(Ok(self
            .verifier
            .lock()
            .unwrap()
            .clone())))
    }

    fn state(&self) -> BoxFuture<'_, Result<Option<String>, McpError>> {
        Box::pin(std::future::ready(Ok(Some("state-xyz".to_owned()))))
    }

    fn save_state(&self, state: String) -> BoxFuture<'_, Result<(), McpError>> {
        *self.state.lock().unwrap() = Some(state);
        Box::pin(std::future::ready(Ok(())))
    }

    fn stored_state(&self) -> BoxFuture<'_, Result<Option<String>, McpError>> {
        Box::pin(std::future::ready(Ok(self.state.lock().unwrap().clone())))
    }

    fn invalidate_credentials(
        &self,
        scope: InvalidateScope,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        self.invalidations.lock().unwrap().push(scope);
        match scope {
            InvalidateScope::All => {
                *self.client.lock().unwrap() = None;
                *self.tokens.lock().unwrap() = None;
                *self.verifier.lock().unwrap() = None;
            }
            InvalidateScope::Tokens => *self.tokens.lock().unwrap() = None,
            _ => {}
        }
        Box::pin(std::future::ready(Ok(())))
    }
}

fn metadata(base: &Url) -> AuthorizationServerMetadata {
    serde_json::from_value(json!({
        "issuer": base.as_str(),
        "authorization_endpoint": base.join("/authorize").unwrap(),
        "token_endpoint": base.join("/token").unwrap(),
        "registration_endpoint": base.join("/register").unwrap(),
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"]
    }))
    .unwrap()
}

/// Mounts protected-resource and authorization-server metadata plus a
/// registration endpoint on `server`.
fn mount_authorization_server(server: &FixtureServer) {
    let base = server.url();
    server.mount(
        Method::GET,
        "/.well-known/oauth-protected-resource/mcp",
        Fixture::complete(StatusCode::NOT_FOUND, "text/plain", ""),
    );
    server.mount(
        Method::GET,
        "/.well-known/oauth-protected-resource",
        Fixture::json(&json!({
            "resource": base.join("/mcp").unwrap(),
            "authorization_servers": [base.as_str()],
            "scopes_supported": ["mcp:read", "mcp:write"]
        })),
    );
    server.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server",
        Fixture::json(&serde_json::to_value(metadata(&base)).unwrap()),
    );
    server.mount(
        Method::POST,
        "/register",
        Fixture::json(&json!({"client_id": "c1", "client_secret": "s1", "client_id_issued_at": 1})),
    );
}

fn form_pairs(body: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect()
}

fn pair<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn www_authenticate_parameters_are_extracted() {
    let headers = Headers::new().with(
        "www-authenticate",
        r#"Bearer realm="mcp", resource_metadata="https://rs.example/.well-known/oauth-protected-resource", scope="read write", error="invalid_token""#,
    );
    let params = extract_www_authenticate_params(&headers);
    assert_eq!(
        params.resource_metadata_url.unwrap().as_str(),
        "https://rs.example/.well-known/oauth-protected-resource"
    );
    assert_eq!(params.scope.as_deref(), Some("read write"));
    let unquoted = Headers::new().with(
        "www-authenticate",
        "bearer scope=read,resource_metadata=https://rs.example/m",
    );
    let params = extract_www_authenticate_params(&unquoted);
    assert_eq!(params.scope.as_deref(), Some("read"));
    assert_eq!(
        params.resource_metadata_url.unwrap().as_str(),
        "https://rs.example/m"
    );
    assert_eq!(
        extract_www_authenticate_params(&Headers::new()),
        Default::default()
    );
}

#[test]
fn discovery_urls_follow_the_specified_order() {
    let with_path = Url::parse("https://as.example/tenant/a").unwrap();
    let urls: Vec<(String, bool)> = authorization_server_metadata_urls(&with_path)
        .into_iter()
        .map(|candidate| (candidate.url.to_string(), candidate.openid))
        .collect();
    assert_eq!(
        urls,
        vec![
            (
                "https://as.example/.well-known/oauth-authorization-server/tenant/a".to_owned(),
                false
            ),
            (
                "https://as.example/.well-known/oauth-authorization-server".to_owned(),
                false
            ),
            (
                "https://as.example/.well-known/openid-configuration/tenant/a".to_owned(),
                true
            ),
            (
                "https://as.example/tenant/a/.well-known/openid-configuration".to_owned(),
                true
            ),
        ]
    );
    let root = Url::parse("https://as.example/").unwrap();
    assert_eq!(authorization_server_metadata_urls(&root).len(), 2);
    let resource_urls: Vec<String> =
        protected_resource_metadata_urls(&Url::parse("https://mcp.example/api/mcp").unwrap())
            .into_iter()
            .map(|url| url.to_string())
            .collect();
    assert_eq!(
        resource_urls,
        vec![
            "https://mcp.example/.well-known/oauth-protected-resource/api/mcp",
            "https://mcp.example/.well-known/oauth-protected-resource",
        ]
    );
}

#[test]
fn pkce_uses_s256() {
    let pkce = generate_pkce();
    assert_eq!(pkce.verifier.len(), 43);
    assert_eq!(pkce.challenge, pkce_challenge(&pkce.verifier));
    assert_ne!(generate_pkce().verifier, pkce.verifier);
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn authorization_urls_carry_pkce_state_scope_and_resource() {
    let base = Url::parse("https://as.example/").unwrap();
    let pkce = generate_pkce();
    let url = start_authorization(
        &metadata(&base),
        &OAuthClientInformation::public("c1"),
        &Url::parse("http://localhost:1/callback").unwrap(),
        &pkce,
        Some("read offline_access"),
        Some("st"),
        &Url::parse("https://mcp.example/mcp").unwrap(),
    )
    .unwrap();
    assert_eq!(url.origin(), base.origin());
    assert_eq!(url.path(), "/authorize");
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    assert_eq!(pair(&pairs, "response_type"), Some("code"));
    assert_eq!(pair(&pairs, "client_id"), Some("c1"));
    assert_eq!(
        pair(&pairs, "code_challenge"),
        Some(pkce.challenge.as_str())
    );
    assert_eq!(pair(&pairs, "code_challenge_method"), Some("S256"));
    assert_eq!(
        pair(&pairs, "redirect_uri"),
        Some("http://localhost:1/callback")
    );
    assert_eq!(pair(&pairs, "state"), Some("st"));
    assert_eq!(pair(&pairs, "scope"), Some("read offline_access"));
    assert_eq!(pair(&pairs, "prompt"), Some("consent"));
    assert_eq!(pair(&pairs, "resource"), Some("https://mcp.example/mcp"));
    let mut no_code = metadata(&base);
    no_code.response_types_supported = Some(vec!["token".to_owned()]);
    assert!(matches!(
        start_authorization(
            &no_code,
            &OAuthClientInformation::public("c1"),
            &url,
            &pkce,
            None,
            None,
            &url
        ),
        Err(McpError::OAuth { .. })
    ));
}

#[test]
fn resource_selection_requires_coverage() {
    let server = Url::parse("https://mcp.example/api/mcp#frag").unwrap();
    assert_eq!(select_resource_url(&server, None).unwrap(), None);
    let covering: ProtectedResourceMetadata =
        serde_json::from_value(json!({"resource": "https://mcp.example/api"})).unwrap();
    assert_eq!(
        select_resource_url(&server, Some(&covering))
            .unwrap()
            .unwrap()
            .as_str(),
        "https://mcp.example/api"
    );
    let foreign: ProtectedResourceMetadata =
        serde_json::from_value(json!({"resource": "https://other.example/api"})).unwrap();
    assert!(matches!(
        select_resource_url(&server, Some(&foreign)),
        Err(McpError::OAuth { .. })
    ));
}

#[tokio::test]
async fn the_flow_registers_redirects_exchanges_and_refreshes() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    let http = default_transport().unwrap();
    let provider = Arc::new(TestProvider::default());
    let options =
        || AuthOptions::new(server.url().join("/mcp").unwrap()).url_policy(local_policy());

    let outcome = auth(provider.as_ref(), http.as_ref(), options())
        .await
        .unwrap();
    assert_eq!(outcome, AuthResult::Redirect);
    let client = provider.client.lock().unwrap().clone().unwrap();
    assert_eq!(client.client_id, "c1");
    let registration = server
        .received()
        .into_iter()
        .find(|request| request.path == "/register")
        .unwrap();
    assert_eq!(
        registration.body_json().unwrap()["client_name"],
        json!("ferrin tests")
    );
    let redirect = provider.redirects.lock().unwrap().remove(0);
    let pairs: Vec<(String, String)> = redirect.query_pairs().into_owned().collect();
    let verifier = provider.verifier.lock().unwrap().clone().unwrap();
    assert_eq!(
        pair(&pairs, "code_challenge"),
        Some(pkce_challenge(&verifier).as_str())
    );
    assert_eq!(pair(&pairs, "scope"), Some("mcp:read mcp:write"));
    assert_eq!(pair(&pairs, "state"), Some("state-xyz"));
    assert_eq!(
        pair(&pairs, "resource"),
        Some(server.url().join("/mcp").unwrap().as_str())
    );

    server.mount_once(
        Method::POST,
        "/token",
        Fixture::json(&json!({"access_token": "at-1", "token_type": "Bearer", "refresh_token": "rt-1", "expires_in": 3600})),
    );
    let outcome = auth(
        provider.as_ref(),
        http.as_ref(),
        options()
            .authorization_code("code-1")
            .callback_state("state-xyz"),
    )
    .await
    .unwrap();
    assert_eq!(outcome, AuthResult::Authorized);
    let tokens = provider.tokens_snapshot().unwrap();
    assert_eq!(tokens.access_token.expose_secret(), "at-1");
    assert_eq!(tokens.expires_in, Some(3600.0));
    let exchange = server
        .received()
        .into_iter()
        .find(|request| request.path == "/token")
        .unwrap();
    assert_eq!(
        exchange.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    let body = form_pairs(&exchange.body_text());
    assert_eq!(pair(&body, "grant_type"), Some("authorization_code"));
    assert_eq!(pair(&body, "code"), Some("code-1"));
    assert_eq!(pair(&body, "code_verifier"), Some(verifier.as_str()));
    assert_eq!(
        pair(&body, "redirect_uri"),
        Some("http://localhost:1/callback")
    );
    assert_eq!(pair(&body, "client_id"), Some("c1"));
    assert_eq!(pair(&body, "client_secret"), Some("s1"));
    assert_eq!(exchange.header("authorization"), None);

    server.mount_once(
        Method::POST,
        "/token",
        Fixture::json(&json!({"access_token": "at-2", "token_type": "Bearer"})),
    );
    let outcome = auth(provider.as_ref(), http.as_ref(), options())
        .await
        .unwrap();
    assert_eq!(outcome, AuthResult::Authorized);
    let tokens = provider.tokens_snapshot().unwrap();
    assert_eq!(tokens.access_token.expose_secret(), "at-2");
    assert_eq!(tokens.refresh_token.unwrap().expose_secret(), "rt-1");
    let refresh = server
        .received()
        .into_iter()
        .filter(|request| request.path == "/token")
        .nth(1)
        .unwrap();
    let body = form_pairs(&refresh.body_text());
    assert_eq!(pair(&body, "grant_type"), Some("refresh_token"));
    assert_eq!(pair(&body, "refresh_token"), Some("rt-1"));

    server.mount_once(
        Method::POST,
        "/token",
        Fixture::json_status(
            StatusCode::BAD_REQUEST,
            &json!({"error": "invalid_grant", "error_description": "expired"}),
        ),
    );
    let outcome = auth(provider.as_ref(), http.as_ref(), options())
        .await
        .unwrap();
    assert_eq!(outcome, AuthResult::Redirect);
    assert_eq!(
        *provider.invalidations.lock().unwrap(),
        vec![InvalidateScope::Tokens]
    );
    assert!(provider.tokens_snapshot().is_none());
    assert_eq!(provider.redirects.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn basic_client_authentication_is_used_when_post_is_unsupported() {
    let server = FixtureServer::start().await.unwrap();
    let base = server.url();
    let mut as_metadata = metadata(&base);
    as_metadata.token_endpoint_auth_methods_supported =
        Some(vec!["client_secret_basic".to_owned()]);
    server.mount(
        Method::GET,
        "/.well-known/oauth-protected-resource",
        Fixture::complete(StatusCode::NOT_FOUND, "text/plain", ""),
    );
    server.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server",
        Fixture::json(&serde_json::to_value(as_metadata).unwrap()),
    );
    server.mount_once(
        Method::POST,
        "/token",
        Fixture::json(&json!({"access_token": "at-3", "token_type": "Bearer"})),
    );
    let provider = TestProvider::with_tokens(
        OAuthTokens::from_json(json!({
            "access_token": "old", "token_type": "Bearer", "refresh_token": "rt-9"
        }))
        .unwrap(),
        &base,
    );
    *provider.client.lock().unwrap() = Some(
        OAuthClientInformation::from_json(json!({"client_id": "c 1", "client_secret": "s/1"}))
            .unwrap(),
    );
    let http = default_transport().unwrap();
    let outcome = auth(
        provider.as_ref(),
        http.as_ref(),
        AuthOptions::new(base.join("/").unwrap()).url_policy(local_policy()),
    )
    .await
    .unwrap();
    assert_eq!(outcome, AuthResult::Authorized);
    let token_request = server
        .received()
        .into_iter()
        .find(|request| request.path == "/token")
        .unwrap();
    assert_eq!(
        token_request.header("authorization"),
        Some("Basic YyAxOnMvMQ==")
    );
    let body = form_pairs(&token_request.body_text());
    assert_eq!(pair(&body, "client_id"), None);
    assert_eq!(pair(&body, "resource"), None);
}

#[tokio::test]
async fn openid_metadata_without_s256_is_rejected() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server",
        Fixture::complete(StatusCode::NOT_FOUND, "text/plain", ""),
    );
    server.mount(
        Method::GET,
        "/.well-known/openid-configuration",
        Fixture::json(&json!({
            "issuer": server.url().as_str(),
            "token_endpoint": server.url().join("/token").unwrap(),
            "authorization_endpoint": server.url().join("/authorize").unwrap(),
            "jwks_uri": server.url().join("/jwks").unwrap(),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"]
        })),
    );
    let http = default_transport().unwrap();
    let error =
        discover_authorization_server_metadata(http.as_ref(), &server.url(), &local_policy())
            .await
            .unwrap_err();
    assert!(matches!(error, McpError::OAuth { ref message, .. } if message.contains("S256")));
    let strict = discover_authorization_server_metadata(
        http.as_ref(),
        &server.url(),
        &ferrin_provider_util::secure_url::UrlPolicy::new(),
    )
    .await;
    assert!(matches!(strict, Err(McpError::Url(_))));
}

#[tokio::test]
async fn the_http_transport_refreshes_tokens_after_a_401() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    let challenge = format!(
        r#"Bearer resource_metadata="{}""#,
        server
            .url()
            .join("/.well-known/oauth-protected-resource")
            .unwrap()
    );
    server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::UNAUTHORIZED, "text/plain", "")
            .with_header("www-authenticate", &challenge),
    );
    server.mount_once(
        Method::POST,
        "/token",
        Fixture::json(&json!({"access_token": "at-fresh", "token_type": "Bearer"})),
    );
    server.mount_once(
        Method::POST,
        "/mcp",
        Fixture::json(&json!({"jsonrpc": "2.0", "id": 1, "result": {"ok": true}})),
    );
    let provider = TestProvider::with_tokens(
        OAuthTokens::from_json(
            json!({"access_token": "at-old", "token_type": "Bearer", "refresh_token": "rt-old"}),
        )
        .unwrap(),
        &server.url(),
    );
    let config = HttpTransportConfig::new(server.url().join("/mcp").unwrap())
        .url_policy(local_policy())
        .auth_provider(provider.clone());
    let transport = HttpTransport::new(config).unwrap();
    transport.start().await.unwrap();
    let mut incoming = transport.incoming();
    transport
        .send(
            JsonRpcMessage::request(1, "ping", None),
            SendOptions::default(),
        )
        .await
        .unwrap();
    let event = tokio::time::timeout(Duration::from_secs(5), incoming.next())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        event,
        TransportEvent::Message(JsonRpcMessage::Response(_))
    ));
    let posts: Vec<_> = server
        .received()
        .into_iter()
        .filter(|request| request.path == "/mcp")
        .collect();
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].header("authorization"), Some("Bearer at-old"));
    assert_eq!(posts[1].header("authorization"), Some("Bearer at-fresh"));
    assert_eq!(
        provider
            .tokens_snapshot()
            .unwrap()
            .access_token
            .expose_secret(),
        "at-fresh"
    );
}

#[tokio::test]
async fn a_401_without_stored_tokens_redirects_and_reports_unauthorized() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    server.mount(
        Method::POST,
        "/mcp",
        Fixture::complete(StatusCode::UNAUTHORIZED, "text/plain", ""),
    );
    let provider = Arc::new(TestProvider::default());
    let config = HttpTransportConfig::new(server.url().join("/mcp").unwrap())
        .url_policy(local_policy())
        .auth_provider(provider.clone());
    let transport = HttpTransport::new(config).unwrap();
    transport.start().await.unwrap();
    let error = transport
        .send(
            JsonRpcMessage::request(1, "ping", None),
            SendOptions::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::Unauthorized));
    assert_eq!(provider.redirects.lock().unwrap().len(), 1);
    assert!(provider.client.lock().unwrap().is_some());
}

struct InterruptedMetadata {
    attempts: std::sync::atomic::AtomicUsize,
    entered: tokio::sync::Notify,
}

impl ferrin_provider_util::http::HttpTransport for InterruptedMetadata {
    fn execute(
        &self,
        request: ferrin_provider_util::http::HttpRequest,
    ) -> BoxFuture<
        '_,
        Result<
            ferrin_provider_util::http::HttpResponse,
            ferrin_provider_util::http::TransportError,
        >,
    > {
        use ferrin_provider_util::http::HttpResponse;
        use std::sync::atomic::Ordering;
        Box::pin(async move {
            if request.url.path() == "/mcp" {
                return Ok(HttpResponse::from_bytes(
                    StatusCode::UNAUTHORIZED,
                    Headers::new(),
                    bytes::Bytes::new(),
                ));
            }
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                self.entered.notify_one();
                return std::future::pending().await;
            }
            Ok(HttpResponse::from_bytes(
                StatusCode::BAD_REQUEST,
                Headers::new(),
                bytes::Bytes::new(),
            ))
        })
    }
}

#[tokio::test]
async fn cancelled_authentication_does_not_block_later_flows() {
    use std::sync::atomic::Ordering;
    use tokio_util::sync::CancellationToken;
    let http = Arc::new(InterruptedMetadata {
        attempts: std::sync::atomic::AtomicUsize::new(0),
        entered: tokio::sync::Notify::new(),
    });
    let config = HttpTransportConfig::new(Url::parse("http://127.0.0.1/mcp").unwrap())
        .url_policy(local_policy())
        .auth_provider(Arc::new(TestProvider::default()))
        .transport(http.clone());
    let transport = HttpTransport::new(config).unwrap();
    transport.start().await.unwrap();
    let token = CancellationToken::new();
    let (first, ()) = tokio::join!(
        transport.send(
            JsonRpcMessage::request(1, "ping", None),
            SendOptions {
                cancellation: Some(token.clone()),
                ..SendOptions::default()
            }
        ),
        async {
            http.entered.notified().await;
            token.cancel();
        }
    );
    assert!(matches!(first, Err(McpError::Cancelled)));
    let second = tokio::time::timeout(
        Duration::from_secs(5),
        transport.send(
            JsonRpcMessage::request(2, "ping", None),
            SendOptions::default(),
        ),
    )
    .await
    .expect("later authentication must start a new flow");
    assert!(matches!(second, Err(McpError::OAuth { .. })));
    let after_second = http.attempts.load(Ordering::SeqCst);
    assert!(after_second > 1);
    let third = transport
        .send(
            JsonRpcMessage::request(3, "ping", None),
            SendOptions::default(),
        )
        .await;
    assert!(matches!(third, Err(McpError::OAuth { .. })));
    assert!(http.attempts.load(Ordering::SeqCst) > after_second);
}

#[path = "oauth_parity.rs"]
mod parity;
