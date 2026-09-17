//! Callback validation, credential binding and reference OAuth wire behavior.

use super::*;
use ferrin_mcp::oauth::AuthorizationCodeGrant;
use ferrin_mcp::oauth::discover_protected_resource_metadata;
use ferrin_mcp::oauth::exchange_authorization;
use ferrin_mcp::oauth::refresh_authorization;
use pretty_assertions::assert_eq;
use secrecy::SecretString;

fn pin(server: &FixtureServer) -> OAuthAuthorizationServerInformation {
    OAuthAuthorizationServerInformation {
        issuer: Some(server.url().to_string()),
        authorization_server_url: server.url(),
        token_endpoint: server.url().join("/token").unwrap(),
    }
}

fn options(server: &FixtureServer) -> AuthOptions {
    AuthOptions::new(server.url().join("/mcp").unwrap()).url_policy(local_policy())
}

fn client_provider(server: &FixtureServer) -> TestProvider {
    let provider = TestProvider::default();
    let mut client = OAuthClientInformation::public("client");
    client.authorization_server_information = Some(pin(server));
    *provider.client.lock().unwrap() = Some(client);
    *provider.verifier.lock().unwrap() = Some("stored-verifier".to_owned());
    provider
}

#[test]
fn credentials_preserve_the_authorization_server_when_persisted() {
    let token_json = json!({
        "access_token": "access", "token_type": "Bearer", "refresh_token": "refresh",
        "id_token": "id-secret", "expires_in": 12.5,
        "issuer": "https://issuer.example", "authorization_server": "https://issuer.example/tenant",
        "token_endpoint": "https://issuer.example/token"
    });
    let client_json = json!({
        "client_id": "client", "client_secret": "secret",
        "issuer": "https://issuer.example", "authorization_server": "https://issuer.example/tenant",
        "token_endpoint": "https://issuer.example/token"
    });
    assert_eq!(
        OAuthTokens::from_json(token_json.clone())
            .unwrap()
            .expose_to_json(),
        token_json
    );
    assert_eq!(
        OAuthClientInformation::from_json(client_json.clone())
            .unwrap()
            .expose_to_json(),
        client_json
    );
    assert!(!format!("{:?}", OAuthTokens::from_json(token_json).unwrap()).contains("id-secret"));
    assert!(OAuthTokens::from_json(json!({"access_token":"access"})).is_err());
}

#[tokio::test]
async fn callback_state_and_issuer_are_checked_before_exchanging_credentials() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    let provider = client_provider(&server);
    *provider.state.lock().unwrap() = Some("saved-state".to_owned());
    let http = default_transport().unwrap();
    for (callback, fragment) in [
        (options(&server).authorization_code("code"), "state"),
        (
            options(&server)
                .authorization_code("code")
                .callback_state("wrong-state"),
            "state",
        ),
        (
            options(&server)
                .authorization_code("code")
                .callback_state("saved-state")
                .callback_issuer("https://other.example"),
            "issuer",
        ),
    ] {
        let error = auth(&provider, http.as_ref(), callback).await.unwrap_err();
        assert!(matches!(error, McpError::OAuth { message, .. } if message.contains(fragment)));
    }
    assert!(
        !server
            .received()
            .iter()
            .any(|request| request.path == "/token")
    );
    server.mount(
        Method::POST,
        "/token",
        Fixture::json(&json!({"access_token":"access", "token_type":"Bearer"})),
    );
    let outcome = auth(
        &provider,
        http.as_ref(),
        options(&server)
            .authorization_code("code")
            .callback_state("saved-state")
            .callback_issuer(server.url().to_string()),
    )
    .await
    .unwrap();
    assert_eq!(outcome, AuthResult::Authorized);
    assert_eq!(
        provider
            .tokens_snapshot()
            .unwrap()
            .authorization_server_information,
        Some(pin(&server))
    );
}

#[tokio::test]
async fn callback_without_a_stored_authorization_server_is_rejected() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    let provider = TestProvider::default();
    *provider.client.lock().unwrap() = Some(OAuthClientInformation::public("client"));
    *provider.verifier.lock().unwrap() = Some("verifier".to_owned());
    let error = auth(
        &provider,
        default_transport().unwrap().as_ref(),
        options(&server).authorization_code("code"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error, McpError::OAuth { message, .. } if message.contains("stored authorization server"))
    );
    assert!(
        !server
            .received()
            .iter()
            .any(|request| request.path == "/token")
    );
}

#[tokio::test]
async fn changed_issuer_server_or_endpoint_prevents_code_exchange_and_refresh() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    let http = default_transport().unwrap();
    for field in ["issuer", "server", "endpoint"] {
        let mut previous = pin(&server);
        match field {
            "issuer" => previous.issuer = Some("https://other.example".to_owned()),
            "server" => previous.authorization_server_url = server.url().join("/other").unwrap(),
            "endpoint" => previous.token_endpoint = server.url().join("/other-token").unwrap(),
            _ => unreachable!(),
        }
        let provider = client_provider(&server);
        provider
            .client
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .authorization_server_information = Some(previous.clone());
        let error = auth(
            &provider,
            http.as_ref(),
            options(&server).authorization_code("code"),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, McpError::OAuth { message, .. } if message.contains("does not match"))
        );
        let provider = client_provider(&server);
        let mut tokens = OAuthTokens::from_json(
            json!({"access_token":"access", "token_type":"Bearer", "refresh_token":"refresh"}),
        )
        .unwrap();
        tokens.authorization_server_information = Some(previous);
        *provider.tokens.lock().unwrap() = Some(tokens);
        let error = auth(&provider, http.as_ref(), options(&server))
            .await
            .unwrap_err();
        assert!(
            matches!(error, McpError::OAuth { message, .. } if message.contains("does not match"))
        );
    }
    assert!(
        !server
            .received()
            .iter()
            .any(|request| request.path == "/token")
    );
}

#[tokio::test]
async fn unbound_refresh_tokens_are_invalidated_before_starting_authorization() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    let provider = TestProvider::default();
    *provider.client.lock().unwrap() = Some(OAuthClientInformation::public("client"));
    *provider.tokens.lock().unwrap() = Some(
        OAuthTokens::from_json(
            json!({"access_token":"access", "token_type":"Bearer", "refresh_token":"refresh"}),
        )
        .unwrap(),
    );
    let outcome = auth(
        &provider,
        default_transport().unwrap().as_ref(),
        options(&server),
    )
    .await
    .unwrap();
    assert_eq!(outcome, AuthResult::Redirect);
    assert_eq!(
        *provider.invalidations.lock().unwrap(),
        vec![InvalidateScope::Tokens]
    );
    assert_eq!(
        provider
            .client
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .authorization_server_information,
        Some(pin(&server))
    );
    assert_eq!(
        *provider.state.lock().unwrap(),
        Some("state-xyz".to_owned())
    );
    assert!(
        !server
            .received()
            .iter()
            .any(|request| request.path == "/token")
    );
}

#[tokio::test]
async fn cross_origin_resource_metadata_is_rejected_before_any_request() {
    let server = FixtureServer::start().await.unwrap();
    let error = auth(
        &TestProvider::default(),
        default_transport().unwrap().as_ref(),
        options(&server)
            .resource_metadata_url(Some(Url::parse("https://other.example/metadata").unwrap())),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, McpError::OAuth { message, .. } if message.contains("same origin")));
    assert!(server.received().is_empty());
}

#[tokio::test]
async fn discovery_falls_back_on_client_errors_and_checks_the_selected_issuer() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server/tenant",
        Fixture::complete(StatusCode::FORBIDDEN, "text/plain", ""),
    );
    server.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server",
        Fixture::json(&serde_json::to_value(metadata(&server.url())).unwrap()),
    );
    let http = default_transport().unwrap();
    assert_eq!(
        discover_authorization_server_metadata(
            http.as_ref(),
            &server.url().join("/tenant").unwrap(),
            &local_policy()
        )
        .await
        .unwrap(),
        Some(metadata(&server.url()))
    );
    let other = FixtureServer::start().await.unwrap();
    let mut wrong = metadata(&other.url());
    wrong.issuer = "https://other.example".to_owned();
    other.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server",
        Fixture::json(&serde_json::to_value(wrong).unwrap()),
    );
    let error =
        discover_authorization_server_metadata(http.as_ref(), &other.url(), &local_policy())
            .await
            .unwrap_err();
    assert!(matches!(error, McpError::OAuth { message, .. } if message.contains("issuer")));
}

#[tokio::test]
async fn protected_resource_discovery_preserves_query_and_falls_back_on_forbidden() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::GET,
        "/.well-known/oauth-protected-resource/mcp",
        Fixture::complete(StatusCode::FORBIDDEN, "text/plain", ""),
    );
    let expected = json!({"resource": server.url().join("/mcp").unwrap()});
    server.mount(
        Method::GET,
        "/.well-known/oauth-protected-resource",
        Fixture::json(&expected),
    );
    let resource = server.url().join("/mcp?tenant=one").unwrap();
    let urls = protected_resource_metadata_urls(&resource);
    assert_eq!(urls[0].query(), Some("tenant=one"));
    assert_eq!(urls[1].query(), None);
    let found = discover_protected_resource_metadata(
        default_transport().unwrap().as_ref(),
        &resource,
        None,
        &local_policy(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(serde_json::to_value(found).unwrap(), expected);
}

#[tokio::test]
async fn missing_resource_metadata_omits_the_resource_and_registers_selected_scope() {
    let server = FixtureServer::start().await.unwrap();
    server.mount(
        Method::GET,
        "/.well-known/oauth-authorization-server",
        Fixture::json(&serde_json::to_value(metadata(&server.url())).unwrap()),
    );
    server.mount(
        Method::POST,
        "/register",
        Fixture::json(&json!({"client_id":"client"})),
    );
    let provider = TestProvider::default();
    let result = auth(
        &provider,
        default_transport().unwrap().as_ref(),
        options(&server).scope(Some("selected".to_owned())),
    )
    .await
    .unwrap();
    assert_eq!(result, AuthResult::Redirect);
    let redirect = provider.redirects.lock().unwrap()[0].clone();
    assert!(!redirect.query_pairs().any(|(name, _)| name == "resource"));
    let registration = server
        .received()
        .into_iter()
        .find(|request| request.path == "/register")
        .unwrap();
    assert_eq!(
        registration.body_json().unwrap()["scope"],
        json!("selected")
    );
}

#[test]
fn authorization_urls_replace_existing_parameters_and_preserve_resource_paths() {
    let base = Url::parse("https://issuer.example/").unwrap();
    let mut meta = metadata(&base);
    meta.authorization_endpoint = Some(
        base.join("/authorize?client_id=old&state=old&prompt=login&extra=keep")
            .unwrap(),
    );
    let client = OAuthClientInformation::public("new");
    let redirect = base.join("/callback").unwrap();
    for (resource, expected) in [
        ("https://resource.example/", "https://resource.example"),
        (
            "https://resource.example/path/",
            "https://resource.example/path/",
        ),
    ] {
        let resource = Url::parse(resource).unwrap();
        let url = start_authorization(
            &meta,
            &client,
            &redirect,
            &generate_pkce(),
            Some("offline_access"),
            Some("new-state"),
            &resource,
        )
        .unwrap();
        let pairs = url.query_pairs().into_owned().collect::<Vec<_>>();
        assert_eq!(
            pairs.iter().filter(|(name, _)| name == "client_id").count(),
            1
        );
        assert_eq!(pair(&pairs, "client_id"), Some("new"));
        assert_eq!(pair(&pairs, "state"), Some("new-state"));
        assert_eq!(pair(&pairs, "resource"), Some(expected));
        assert_eq!(
            pairs
                .iter()
                .filter(|(name, _)| name == "prompt")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            vec!["login", "consent"]
        );
    }
    meta.code_challenge_methods_supported = None;
    assert!(
        start_authorization(
            &meta,
            &client,
            &redirect,
            &generate_pkce(),
            None,
            None,
            None
        )
        .is_err()
    );
    meta.code_challenge_methods_supported = Some(vec!["S256".to_owned()]);
    meta.authorization_endpoint = Some(Url::parse("javascript:alert(1)").unwrap());
    assert!(
        start_authorization(
            &meta,
            &client,
            &redirect,
            &generate_pkce(),
            None,
            None,
            None
        )
        .is_err()
    );
}

#[tokio::test]
async fn unsupported_grants_fail_without_token_requests() {
    let server = FixtureServer::start().await.unwrap();
    let mut meta = metadata(&server.url());
    meta.grant_types_supported = Some(vec!["client_credentials".to_owned()]);
    let client = OAuthClientInformation::public("client");
    let http = default_transport().unwrap();
    let grant = AuthorizationCodeGrant {
        code: "code".to_owned(),
        code_verifier: "verifier".to_owned(),
        redirect_url: server.url(),
        resource: None,
    };
    assert!(
        exchange_authorization(http.as_ref(), &meta, &client, &grant, &local_policy())
            .await
            .is_err()
    );
    assert!(
        refresh_authorization(
            http.as_ref(),
            &meta,
            &client,
            &SecretString::from("refresh"),
            None,
            &local_policy()
        )
        .await
        .is_err()
    );
    assert!(server.received().is_empty());
}

struct CustomAuthentication(TestProvider);

impl OAuthClientProvider for CustomAuthentication {
    fn redirect_url(&self) -> Option<Url> {
        self.0.redirect_url()
    }
    fn client_metadata(&self) -> OAuthClientMetadata {
        self.0.client_metadata()
    }
    fn client_information(
        &self,
    ) -> BoxFuture<'_, Result<Option<OAuthClientInformation>, McpError>> {
        self.0.client_information()
    }
    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>, McpError>> {
        self.0.tokens()
    }
    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<(), McpError>> {
        self.0.save_tokens(tokens)
    }
    fn save_code_verifier(&self, code: String) -> BoxFuture<'_, Result<(), McpError>> {
        self.0.save_code_verifier(code)
    }
    fn code_verifier(&self) -> BoxFuture<'_, Result<Option<String>, McpError>> {
        self.0.code_verifier()
    }
    fn redirect_to_authorization(&self, url: Url) -> BoxFuture<'_, Result<(), McpError>> {
        self.0.redirect_to_authorization(url)
    }
    fn add_client_authentication<'a>(
        &'a self,
        headers: &'a mut Headers,
        params: &'a mut Vec<(String, String)>,
        server: &'a Url,
        metadata: Option<&'a AuthorizationServerMetadata>,
        _client: &'a OAuthClientInformation,
    ) -> BoxFuture<'a, Result<(), McpError>> {
        assert_eq!(metadata.unwrap().issuer, server.as_str());
        headers.insert("authorization", "Custom assertion").unwrap();
        params.push(("client_assertion".to_owned(), "signed-assertion".to_owned()));
        Box::pin(std::future::ready(Ok(())))
    }
}

#[tokio::test]
async fn custom_authentication_replaces_standard_credentials_for_both_token_grants() {
    let server = FixtureServer::start().await.unwrap();
    mount_authorization_server(&server);
    server.mount(
        Method::POST,
        "/token",
        Fixture::json(
            &json!({"access_token":"access", "token_type":"Bearer", "refresh_token":"refresh"}),
        ),
    );
    let provider = CustomAuthentication(client_provider(&server));
    let http = default_transport().unwrap();
    assert_eq!(
        auth(
            &provider,
            http.as_ref(),
            options(&server).authorization_code("code")
        )
        .await
        .unwrap(),
        AuthResult::Authorized
    );
    assert_eq!(
        auth(&provider, http.as_ref(), options(&server))
            .await
            .unwrap(),
        AuthResult::Authorized
    );
    let requests = server
        .received()
        .into_iter()
        .filter(|request| request.path == "/token")
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request.header("authorization"), Some("Custom assertion"));
        let pairs = form_pairs(&request.body_text());
        assert_eq!(pair(&pairs, "client_assertion"), Some("signed-assertion"));
        assert_eq!(pair(&pairs, "client_id"), None);
        assert_eq!(pair(&pairs, "client_secret"), None);
    }
}

#[tokio::test]
async fn client_authentication_uses_reference_fallbacks_and_preference_order() {
    for (methods, expected_header, expected_secret) in [
        (None, None, Some("secret")),
        (Some(vec![]), None, Some("secret")),
        (Some(vec!["unknown".to_owned()]), None, Some("secret")),
        (Some(vec!["none".to_owned()]), None, None),
        (
            Some(vec![
                "client_secret_post".to_owned(),
                "client_secret_basic".to_owned(),
            ]),
            Some("Basic Y2xpZW50OnNlY3JldA=="),
            None,
        ),
    ] {
        let server = FixtureServer::start().await.unwrap();
        server.mount(
            Method::POST,
            "/token",
            Fixture::json(&json!({"access_token":"access", "token_type":"Bearer"})),
        );
        let mut meta = metadata(&server.url());
        meta.token_endpoint_auth_methods_supported = methods;
        let client = OAuthClientInformation::from_json(
            json!({"client_id":"client", "client_secret":"secret"}),
        )
        .unwrap();
        refresh_authorization(
            default_transport().unwrap().as_ref(),
            &meta,
            &client,
            &SecretString::from("refresh"),
            None,
            &local_policy(),
        )
        .await
        .unwrap();
        let request = server.received().remove(0);
        let pairs = form_pairs(&request.body_text());
        assert_eq!(
            (
                request.header("authorization"),
                pair(&pairs, "client_secret")
            ),
            (expected_header, expected_secret)
        );
        assert_eq!(
            pair(&pairs, "client_id"),
            expected_header.is_none().then_some("client")
        );
    }
}
