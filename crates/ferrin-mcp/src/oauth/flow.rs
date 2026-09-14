//! The authorization flow: registration, PKCE authorization, code exchange
//! and refresh.

use base64::Engine;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_spec::Headers;
use http::Method;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use sha2::Digest;
use url::Url;

use super::discovery::default_authorization_server_metadata;
use super::discovery::discover_authorization_server_metadata;
use super::discovery::discover_protected_resource_metadata;
use super::discovery::select_resource_url;
use super::http::form_body;
use super::http::send;
use super::provider::OAuthClientProvider;
use super::types::AuthResult;
use super::types::AuthorizationServerMetadata;
use super::types::InvalidateScope;
use super::types::OAuthClientInformation;
use super::types::OAuthClientMetadata;
use super::types::OAuthTokens;
use crate::error::McpError;

/// Inputs of [`auth`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthOptions {
    /// MCP server URL (the protected resource).
    pub server_url: Url,
    /// Authorization code returned to the redirect URL, when completing a
    /// pending authorization.
    pub authorization_code: Option<String>,
    /// Scope to request (default: the resource's `scopes_supported`, then the
    /// client metadata scope).
    pub scope: Option<String>,
    /// Protected resource metadata URL from the `WWW-Authenticate` challenge.
    pub resource_metadata_url: Option<Url>,
    /// Secure URL policy applied to every authorization server endpoint.
    pub url_policy: UrlPolicy,
}

impl AuthOptions {
    /// Options for `server_url` with the default URL policy.
    #[must_use]
    pub fn new(server_url: Url) -> Self {
        Self {
            server_url,
            authorization_code: None,
            scope: None,
            resource_metadata_url: None,
            url_policy: UrlPolicy::new(),
        }
    }

    /// Completes a pending authorization with `code`.
    #[must_use]
    pub fn authorization_code(mut self, code: impl Into<String>) -> Self {
        self.authorization_code = Some(code.into());
        self
    }

    /// Sets the requested scope.
    #[must_use]
    pub fn scope(mut self, scope: Option<String>) -> Self {
        self.scope = scope;
        self
    }

    /// Sets the resource metadata URL.
    #[must_use]
    pub fn resource_metadata_url(mut self, url: Option<Url>) -> Self {
        self.resource_metadata_url = url;
        self
    }

    /// Sets the URL policy.
    #[must_use]
    pub fn url_policy(mut self, policy: UrlPolicy) -> Self {
        self.url_policy = policy;
        self
    }
}

/// A PKCE verifier and its `S256` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    /// Code verifier (43 characters, base64url).
    pub verifier: String,
    /// Code challenge (`S256`).
    pub challenge: String,
}

/// Generates a random PKCE pair.
#[must_use]
pub fn generate_pkce() -> Pkce {
    let random: [u8; 32] = rand::random();
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random);
    Pkce {
        challenge: pkce_challenge(&verifier),
        verifier,
    }
}

/// The `S256` challenge of `verifier`.
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Builds the authorization URL.
///
/// # Errors
///
/// Returns [`McpError::OAuth`] when the server publishes no authorization
/// endpoint or does not support the `code` response type.
pub fn start_authorization(
    metadata: &AuthorizationServerMetadata,
    client: &OAuthClientInformation,
    redirect_url: &Url,
    pkce: &Pkce,
    scope: Option<&str>,
    state: Option<&str>,
    resource: &Url,
) -> Result<Url, McpError> {
    let Some(endpoint) = &metadata.authorization_endpoint else {
        return Err(McpError::oauth(
            "authorization server publishes no authorization endpoint",
        ));
    };
    if metadata
        .response_types_supported
        .as_ref()
        .is_some_and(|types| !types.iter().any(|kind| kind == "code"))
    {
        return Err(McpError::oauth(
            "authorization server does not support the code response type",
        ));
    }
    let mut url = endpoint.clone();
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &client.client_id);
        query.append_pair("code_challenge", &pkce.challenge);
        query.append_pair("code_challenge_method", "S256");
        query.append_pair("redirect_uri", redirect_url.as_str());
        if let Some(state) = state {
            query.append_pair("state", state);
        }
        if let Some(scope) = scope {
            query.append_pair("scope", scope);
            if scope
                .split_whitespace()
                .any(|entry| entry == "offline_access")
            {
                query.append_pair("prompt", "consent");
            }
        }
        query.append_pair("resource", resource.as_str());
    }
    Ok(url)
}

/// How the client authenticates at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientAuth {
    Basic,
    Post,
    None,
}

fn select_client_auth(
    client: &OAuthClientInformation,
    metadata: &AuthorizationServerMetadata,
) -> ClientAuth {
    if client.client_secret.is_none() {
        return ClientAuth::None;
    }
    match &metadata.token_endpoint_auth_methods_supported {
        None => ClientAuth::Basic,
        Some(methods) if methods.iter().any(|method| method == "client_secret_basic") => {
            ClientAuth::Basic
        }
        Some(methods) if methods.iter().any(|method| method == "client_secret_post") => {
            ClientAuth::Post
        }
        Some(_) => ClientAuth::Basic,
    }
}

fn apply_client_auth(
    client: &OAuthClientInformation,
    metadata: &AuthorizationServerMetadata,
    form: &mut Vec<(String, String)>,
    headers: &mut Headers,
) {
    match select_client_auth(client, metadata) {
        ClientAuth::Basic => {
            let secret = client
                .client_secret
                .as_ref()
                .map(ExposeSecret::expose_secret)
                .unwrap_or_default();
            // Percent-encoding per RFC 6749 section 2.3.1 (spaces as `%20`).
            let encode = |value: &str| {
                url::form_urlencoded::byte_serialize(value.as_bytes())
                    .collect::<String>()
                    .replace('+', "%20")
            };
            let credentials = base64::engine::general_purpose::STANDARD.encode(format!(
                "{}:{}",
                encode(&client.client_id),
                encode(secret)
            ));
            let _ = headers.insert("authorization", &format!("Basic {credentials}"));
        }
        ClientAuth::Post => {
            form.push(("client_id".to_owned(), client.client_id.clone()));
            form.push((
                "client_secret".to_owned(),
                client
                    .client_secret
                    .as_ref()
                    .map(ExposeSecret::expose_secret)
                    .unwrap_or_default()
                    .to_owned(),
            ));
        }
        ClientAuth::None => form.push(("client_id".to_owned(), client.client_id.clone())),
    }
}

async fn token_request(
    http: &dyn HttpTransport,
    metadata: &AuthorizationServerMetadata,
    client: &OAuthClientInformation,
    mut form: Vec<(String, String)>,
    policy: &UrlPolicy,
    what: &str,
) -> Result<OAuthTokens, McpError> {
    let mut headers = Headers::new().with("accept", "application/json");
    apply_client_auth(client, metadata, &mut form, &mut headers);
    let response = send(
        http,
        Method::POST,
        &metadata.token_endpoint,
        policy,
        headers,
        form_body(&form),
    )
    .await?;
    if !response.status.is_success() {
        return Err(response.into_error(what));
    }
    let json = response
        .json()
        .ok_or_else(|| McpError::oauth(format!("{what} response is not valid JSON")))?;
    OAuthTokens::from_json(json)
        .map_err(|error| McpError::oauth(format!("invalid {what} response: {error}")))
}

/// An authorization code grant to exchange.
#[derive(Debug, Clone)]
pub struct AuthorizationCodeGrant {
    /// Code returned to the redirect URL.
    pub code: String,
    /// PKCE verifier of the pending authorization.
    pub code_verifier: String,
    /// Redirect URL the code was sent to.
    pub redirect_url: Url,
    /// Resource indicator.
    pub resource: Url,
}

/// Exchanges an authorization code for tokens.
///
/// # Errors
///
/// Returns [`McpError::OAuth`] with the server's `error` code on rejection,
/// or the transport failure.
pub async fn exchange_authorization(
    http: &dyn HttpTransport,
    metadata: &AuthorizationServerMetadata,
    client: &OAuthClientInformation,
    grant: &AuthorizationCodeGrant,
    policy: &UrlPolicy,
) -> Result<OAuthTokens, McpError> {
    let form = vec![
        ("grant_type".to_owned(), "authorization_code".to_owned()),
        ("code".to_owned(), grant.code.clone()),
        ("code_verifier".to_owned(), grant.code_verifier.clone()),
        ("redirect_uri".to_owned(), grant.redirect_url.to_string()),
        ("resource".to_owned(), grant.resource.to_string()),
    ];
    token_request(http, metadata, client, form, policy, "token exchange").await
}

/// Refreshes tokens; the previous refresh token is kept when the server
/// does not rotate it.
///
/// # Errors
///
/// See [`exchange_authorization`].
pub async fn refresh_authorization(
    http: &dyn HttpTransport,
    metadata: &AuthorizationServerMetadata,
    client: &OAuthClientInformation,
    refresh_token: &SecretString,
    resource: &Url,
    policy: &UrlPolicy,
) -> Result<OAuthTokens, McpError> {
    let form = vec![
        ("grant_type".to_owned(), "refresh_token".to_owned()),
        (
            "refresh_token".to_owned(),
            refresh_token.expose_secret().to_owned(),
        ),
        ("resource".to_owned(), resource.to_string()),
    ];
    let mut tokens = token_request(http, metadata, client, form, policy, "token refresh").await?;
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token.clone());
    }
    Ok(tokens)
}

/// Registers the client dynamically (RFC 7591).
///
/// # Errors
///
/// Returns [`McpError::OAuth`] when the server publishes no registration
/// endpoint or rejects the registration.
pub async fn register_client(
    http: &dyn HttpTransport,
    metadata: &AuthorizationServerMetadata,
    client_metadata: &OAuthClientMetadata,
    policy: &UrlPolicy,
) -> Result<OAuthClientInformation, McpError> {
    let Some(endpoint) = &metadata.registration_endpoint else {
        return Err(McpError::oauth(
            "authorization server does not support dynamic client registration",
        ));
    };
    let body = serde_json::to_vec(client_metadata)
        .map_err(|error| McpError::oauth(format!("invalid client metadata: {error}")))?;
    let headers = Headers::new().with("accept", "application/json");
    let response = send(
        http,
        Method::POST,
        endpoint,
        policy,
        headers,
        ferrin_provider_util::http::RequestBody::json(bytes::Bytes::from(body)),
    )
    .await?;
    if !response.status.is_success() {
        return Err(response.into_error("client registration"));
    }
    let json = response
        .json()
        .ok_or_else(|| McpError::oauth("client registration response is not valid JSON"))?;
    OAuthClientInformation::from_json(json)
        .map_err(|error| McpError::oauth(format!("invalid client registration response: {error}")))
}

fn is_transient(error: &McpError) -> bool {
    match error {
        McpError::OAuth {
            error_code: Some(code),
            ..
        } => code == "server_error" || code == "temporarily_unavailable",
        McpError::OAuth {
            error_code: None, ..
        } => true,
        McpError::Transport(_) => true,
        _ => false,
    }
}

async fn auth_internal(
    provider: &dyn OAuthClientProvider,
    http: &dyn HttpTransport,
    options: &AuthOptions,
) -> Result<AuthResult, McpError> {
    let policy = &options.url_policy;
    let resource_metadata = match discover_protected_resource_metadata(
        http,
        &options.server_url,
        options.resource_metadata_url.clone(),
        policy,
    )
    .await
    {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::debug!(error = %error, "protected resource metadata discovery failed");
            None
        }
    };
    let authorization_server_url = resource_metadata
        .as_ref()
        .and_then(|metadata| metadata.authorization_servers.as_ref())
        .and_then(|servers| servers.first().cloned())
        .map_or_else(
            || {
                options
                    .server_url
                    .join("/")
                    .map_err(|error| McpError::oauth(format!("invalid server url: {error}")))
            },
            Ok,
        )?;
    let resource = select_resource_url(&options.server_url, resource_metadata.as_ref())?;
    let metadata =
        match discover_authorization_server_metadata(http, &authorization_server_url, policy)
            .await?
        {
            Some(metadata) => metadata,
            None => default_authorization_server_metadata(&authorization_server_url)?,
        };
    let client_metadata = provider.client_metadata();
    let client = match provider.client_information().await? {
        Some(client) => client,
        None => {
            if options.authorization_code.is_some() {
                return Err(McpError::oauth(
                    "existing client information is required when exchanging an authorization code",
                ));
            }
            let registered = register_client(http, &metadata, &client_metadata, policy).await?;
            provider.save_client_information(registered.clone()).await?;
            registered
        }
    };
    let scope = options
        .scope
        .clone()
        .or_else(|| {
            resource_metadata
                .as_ref()
                .and_then(|metadata| metadata.scopes_supported.as_ref())
                .filter(|scopes| !scopes.is_empty())
                .map(|scopes| scopes.join(" "))
        })
        .or_else(|| client_metadata.scope.clone());
    if let Some(code) = &options.authorization_code {
        let verifier = provider.code_verifier().await?.ok_or_else(|| {
            McpError::oauth("no code verifier is stored for the pending authorization")
        })?;
        let redirect_url = provider.redirect_url().ok_or_else(|| {
            McpError::oauth("a redirect url is required to exchange an authorization code")
        })?;
        let grant = AuthorizationCodeGrant {
            code: code.clone(),
            code_verifier: verifier,
            redirect_url,
            resource,
        };
        let tokens = exchange_authorization(http, &metadata, &client, &grant, policy).await?;
        provider.save_tokens(tokens).await?;
        return Ok(AuthResult::Authorized);
    }
    if let Some(tokens) = provider.tokens().await?
        && let Some(refresh_token) = &tokens.refresh_token
    {
        match refresh_authorization(http, &metadata, &client, refresh_token, &resource, policy)
            .await
        {
            Ok(refreshed) => {
                provider.save_tokens(refreshed).await?;
                return Ok(AuthResult::Authorized);
            }
            Err(error) if is_transient(&error) => {
                tracing::debug!(error = %error, "token refresh failed; starting a new authorization");
            }
            Err(error) => return Err(error),
        }
    }
    let redirect_url = provider
        .redirect_url()
        .ok_or_else(|| McpError::oauth("a redirect url is required to start authorization"))?;
    let pkce = generate_pkce();
    let state = provider.state().await?;
    let url = start_authorization(
        &metadata,
        &client,
        &redirect_url,
        &pkce,
        scope.as_deref(),
        state.as_deref(),
        &resource,
    )?;
    provider.save_code_verifier(pkce.verifier).await?;
    provider.redirect_to_authorization(url).await?;
    Ok(AuthResult::Redirect)
}

/// Runs the authorization flow: discovers metadata, registers the client if
/// needed, then exchanges an authorization code, refreshes stored tokens or
/// redirects the user to the authorization server.
///
/// Rejected credentials are invalidated through the provider and the flow is
/// retried once (`invalid_client`/`unauthorized_client` discard everything,
/// `invalid_grant` discards the tokens).
///
/// # Errors
///
/// Returns [`McpError::OAuth`] for protocol failures, [`McpError::Url`] when
/// an endpoint violates the URL policy, or the provider's storage errors.
#[tracing::instrument(skip_all, fields(server = %options.server_url.host_str().unwrap_or_default()))]
pub async fn auth(
    provider: &dyn OAuthClientProvider,
    http: &dyn HttpTransport,
    options: AuthOptions,
) -> Result<AuthResult, McpError> {
    match auth_internal(provider, http, &options).await {
        Err(McpError::OAuth {
            error_code: Some(code),
            ..
        }) if code == "invalid_client" || code == "unauthorized_client" => {
            provider
                .invalidate_credentials(InvalidateScope::All)
                .await?;
            auth_internal(provider, http, &options).await
        }
        Err(McpError::OAuth {
            error_code: Some(code),
            ..
        }) if code == "invalid_grant" => {
            provider
                .invalidate_credentials(InvalidateScope::Tokens)
                .await?;
            auth_internal(provider, http, &options).await
        }
        other => other,
    }
}
