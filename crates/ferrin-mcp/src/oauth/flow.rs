//! The authorization flow: registration, PKCE authorization, code exchange
//! and refresh.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use base64::Engine;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_spec::Headers;
use http::Method;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use sha2::Digest;
use url::Url;

use super::http::form_body;
use super::http::send;
use super::provider::OAuthClientProvider;
use super::types::AuthorizationServerMetadata;
use super::types::OAuthClientInformation;
use super::types::OAuthClientMetadata;
use super::types::OAuthTokens;
use crate::error::McpError;

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
pub fn start_authorization<'a>(
    metadata: &AuthorizationServerMetadata,
    client: &OAuthClientInformation,
    redirect_url: &Url,
    pkce: &Pkce,
    scope: Option<&str>,
    state: Option<&str>,
    resource: impl Into<Option<&'a Url>>,
) -> Result<Url, McpError> {
    let Some(endpoint) = &metadata.authorization_endpoint else {
        return Err(McpError::oauth(
            "authorization server publishes no authorization endpoint",
        ));
    };
    super::discovery::validate_metadata_url(endpoint)?;
    if !metadata
        .response_types_supported
        .as_ref()
        .is_some_and(|types| types.iter().any(|kind| kind == "code"))
    {
        return Err(McpError::oauth(
            "authorization server does not support the code response type",
        ));
    }
    if !metadata
        .code_challenge_methods_supported
        .as_ref()
        .is_some_and(|methods| methods.iter().any(|method| method == "S256"))
    {
        return Err(McpError::oauth(
            "authorization server does not support the S256 code challenge method",
        ));
    }
    let resource = resource.into();
    let mut url = endpoint.clone();
    let reserved = [
        "response_type",
        "client_id",
        "code_challenge",
        "code_challenge_method",
        "redirect_uri",
    ];
    let existing: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| {
            !reserved.contains(&name.as_ref())
                && !(name == "state" && state.is_some_and(|state| !state.is_empty()))
                && !(name == "scope" && scope.is_some_and(|scope| !scope.is_empty()))
                && !(name == "resource" && resource.is_some())
        })
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(existing);
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &client.client_id);
        query.append_pair("code_challenge", &pkce.challenge);
        query.append_pair("code_challenge_method", "S256");
        query.append_pair("redirect_uri", redirect_url.as_str());
        if let Some(state) = state.filter(|state| !state.is_empty()) {
            query.append_pair("state", state);
        }
        if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
            query.append_pair("scope", scope);
            if scope.contains("offline_access") {
                query.append_pair("prompt", "consent");
            }
        }
        if let Some(resource) = resource {
            query.append_pair("resource", resource_indicator(resource));
        }
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
    metadata: Option<&AuthorizationServerMetadata>,
) -> ClientAuth {
    if client.client_secret.is_none() {
        return ClientAuth::None;
    }
    match metadata.and_then(|metadata| metadata.token_endpoint_auth_methods_supported.as_ref()) {
        None => ClientAuth::Post,
        Some(methods) if methods.iter().any(|method| method == "client_secret_basic") => {
            ClientAuth::Basic
        }
        Some(methods) if methods.iter().any(|method| method == "client_secret_post") => {
            ClientAuth::Post
        }
        Some(methods) if methods.iter().any(|method| method == "none") => ClientAuth::None,
        Some(_) => ClientAuth::Post,
    }
}

pub(super) fn apply_client_auth(
    client: &OAuthClientInformation,
    metadata: Option<&AuthorizationServerMetadata>,
    form: &mut Vec<(String, String)>,
    headers: &mut Headers,
) -> Result<(), McpError> {
    match select_client_auth(client, metadata) {
        ClientAuth::Basic => {
            let secret = client
                .client_secret
                .as_ref()
                .map(ExposeSecret::expose_secret)
                .unwrap_or_default();
            if secret.is_empty() {
                return Err(McpError::oauth(
                    "basic client authentication requires a client secret",
                ));
            }
            let credentials = format!("{}:{secret}", client.client_id)
                .chars()
                .map(|ch| u8::try_from(u32::from(ch)))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| McpError::oauth("basic client credentials must fit in Latin-1"))?;
            let credentials = base64::engine::general_purpose::STANDARD.encode(credentials);
            let _ = headers.insert("authorization", &format!("Basic {credentials}"));
        }
        ClientAuth::Post => {
            form.push(("client_id".to_owned(), client.client_id.clone()));
            if let Some(secret) = client
                .client_secret
                .as_ref()
                .filter(|secret| !secret.expose_secret().is_empty())
            {
                form.push((
                    "client_secret".to_owned(),
                    secret.expose_secret().to_owned(),
                ));
            }
        }
        ClientAuth::None => form.push(("client_id".to_owned(), client.client_id.clone())),
    }
    Ok(())
}

fn resource_indicator(resource: &Url) -> &str {
    if resource.path() == "/" && resource.as_str().ends_with('/') {
        resource.as_str().trim_end_matches('/')
    } else {
        resource.as_str()
    }
}

pub(super) struct TokenRequest<'a> {
    pub(super) http: &'a dyn HttpTransport,
    pub(super) metadata: &'a AuthorizationServerMetadata,
    pub(super) client: &'a OAuthClientInformation,
    pub(super) policy: &'a UrlPolicy,
    pub(super) authentication: Option<(
        &'a dyn OAuthClientProvider,
        &'a Url,
        Option<&'a AuthorizationServerMetadata>,
    )>,
}

impl TokenRequest<'_> {
    async fn send(
        &self,
        mut form: Vec<(String, String)>,
        resource: Option<&Url>,
        what: &str,
    ) -> Result<OAuthTokens, McpError> {
        let mut headers = Headers::new()
            .with("accept", "application/json")
            .with("content-type", "application/x-www-form-urlencoded");
        if let Some((provider, server, metadata)) = self.authentication {
            provider
                .add_client_authentication(&mut headers, &mut form, server, metadata, self.client)
                .await?;
        } else {
            apply_client_auth(self.client, Some(self.metadata), &mut form, &mut headers)?;
        }
        if let Some(resource) = resource {
            form.retain(|(name, _)| name != "resource");
            form.push((
                "resource".to_owned(),
                resource_indicator(resource).to_owned(),
            ));
        }
        let response = send(
            self.http,
            Method::POST,
            &self.metadata.token_endpoint,
            self.policy,
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

    fn check_grant(&self, grant: &str) -> Result<(), McpError> {
        if self
            .metadata
            .grant_types_supported
            .as_ref()
            .is_some_and(|grants| !grants.iter().any(|candidate| candidate == grant))
        {
            return Err(McpError::oauth(format!(
                "authorization server does not support the {grant} grant type"
            )));
        }
        Ok(())
    }

    pub(super) async fn exchange(
        &self,
        grant: &AuthorizationCodeGrant,
    ) -> Result<OAuthTokens, McpError> {
        self.check_grant("authorization_code")?;
        let form = vec![
            ("grant_type".to_owned(), "authorization_code".to_owned()),
            ("code".to_owned(), grant.code.clone()),
            ("code_verifier".to_owned(), grant.code_verifier.clone()),
            ("redirect_uri".to_owned(), grant.redirect_url.to_string()),
        ];
        self.send(form, grant.resource.as_ref(), "token exchange")
            .await
    }

    pub(super) async fn refresh(
        &self,
        refresh_token: &SecretString,
        resource: Option<&Url>,
    ) -> Result<OAuthTokens, McpError> {
        self.check_grant("refresh_token")?;
        let form = vec![
            ("grant_type".to_owned(), "refresh_token".to_owned()),
            (
                "refresh_token".to_owned(),
                refresh_token.expose_secret().to_owned(),
            ),
        ];
        let mut tokens = self.send(form, resource, "token refresh").await?;
        if tokens.refresh_token.is_none() {
            tokens.refresh_token = Some(refresh_token.clone());
        }
        Ok(tokens)
    }
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
    /// Optional resource indicator.
    pub resource: Option<Url>,
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
    TokenRequest {
        http,
        metadata,
        client,
        policy,
        authentication: None,
    }
    .exchange(grant)
    .await
}

/// Refreshes tokens; the previous refresh token is kept when the server
/// does not rotate it.
///
/// # Errors
///
/// See [`exchange_authorization`].
pub async fn refresh_authorization<'a>(
    http: &dyn HttpTransport,
    metadata: &AuthorizationServerMetadata,
    client: &OAuthClientInformation,
    refresh_token: &SecretString,
    resource: impl Into<Option<&'a Url>>,
    policy: &UrlPolicy,
) -> Result<OAuthTokens, McpError> {
    TokenRequest {
        http,
        metadata,
        client,
        policy,
        authentication: None,
    }
    .refresh(refresh_token, resource.into())
    .await
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
