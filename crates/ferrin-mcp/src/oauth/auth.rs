//! OAuth orchestration with callback validation and credential issuer binding.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! translated from TypeScript to Rust and modified; see `NOTICE`.

use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::secure_url::UrlPolicy;
use url::Url;

use super::discovery::default_authorization_server_metadata;
use super::discovery::discover_authorization_server_metadata;
use super::discovery::discover_protected_resource_metadata;
use super::flow::AuthorizationCodeGrant;
use super::flow::TokenRequest;
use super::flow::generate_pkce;
use super::flow::register_client;
use super::flow::start_authorization;
use super::provider::OAuthClientProvider;
use super::types::AuthResult;
use super::types::InvalidateScope;
use super::types::OAuthAuthorizationServerInformation;
use super::types::OAuthClientInformation;
use crate::error::McpError;

/// Inputs of [`auth`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthOptions {
    /// MCP server URL (the protected resource).
    pub server_url: Url,
    /// Authorization code returned to the redirect URL.
    pub authorization_code: Option<String>,
    /// State returned to the redirect URL.
    pub callback_state: Option<String>,
    /// Issuer returned to the redirect URL.
    pub callback_issuer: Option<String>,
    /// Requested scope, ahead of resource scopes and client metadata scope.
    pub scope: Option<String>,
    /// Protected resource metadata URL from the authentication challenge.
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
            callback_state: None,
            callback_issuer: None,
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

    /// Sets the state returned by the authorization server.
    #[must_use]
    pub fn callback_state(mut self, state: impl Into<String>) -> Self {
        self.callback_state = Some(state.into());
        self
    }

    /// Sets the issuer returned by the authorization server.
    #[must_use]
    pub fn callback_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.callback_issuer = Some(issuer.into());
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

fn assert_server_matches(
    stored: &OAuthAuthorizationServerInformation,
    current: &OAuthAuthorizationServerInformation,
) -> Result<(), McpError> {
    if (stored.issuer.is_some() && current.issuer.is_some() && stored.issuer != current.issuer)
        || stored.authorization_server_url != current.authorization_server_url
        || stored.token_endpoint != current.token_endpoint
    {
        return Err(McpError::oauth(
            "authorization server metadata does not match the server that issued the stored credentials",
        ));
    }
    Ok(())
}

async fn stored_server(
    provider: &dyn OAuthClientProvider,
    client: &OAuthClientInformation,
) -> Result<Option<OAuthAuthorizationServerInformation>, McpError> {
    Ok(provider
        .authorization_server_information()
        .await?
        .or_else(|| client.authorization_server_information.clone()))
}

fn is_transient(error: &McpError) -> bool {
    match error {
        McpError::OAuth {
            error_code: Some(code),
            ..
        } => !matches!(
            code.as_str(),
            "invalid_client" | "invalid_grant" | "unauthorized_client"
        ),
        McpError::OAuth {
            error_code: None, ..
        }
        | McpError::Transport(_) => true,
        _ => false,
    }
}

async fn auth_internal(
    provider: &dyn OAuthClientProvider,
    http: &dyn HttpTransport,
    options: &AuthOptions,
) -> Result<AuthResult, McpError> {
    if options
        .resource_metadata_url
        .as_ref()
        .is_some_and(|url| url.origin() != options.server_url.origin())
    {
        return Err(McpError::oauth(
            "protected resource metadata must have the same origin as the server",
        ));
    }
    let policy = &options.url_policy;
    let resource_metadata = discover_protected_resource_metadata(
        http,
        &options.server_url,
        options.resource_metadata_url.clone(),
        policy,
    )
    .await
    .unwrap_or(None);
    let authorization_server_url = resource_metadata
        .as_ref()
        .and_then(|metadata| metadata.authorization_servers.as_ref())
        .and_then(|servers| servers.first().cloned())
        .unwrap_or_else(|| options.server_url.clone());
    let mut canonical_resource = options.server_url.clone();
    canonical_resource.set_fragment(None);
    let resource = provider
        .validate_resource_url(
            &canonical_resource,
            resource_metadata
                .as_ref()
                .map(|metadata| &metadata.resource),
        )
        .await?;
    provider
        .validate_authorization_server_url(&options.server_url, &authorization_server_url)
        .await?;
    let discovered =
        discover_authorization_server_metadata(http, &authorization_server_url, policy).await?;
    let has_metadata = discovered.is_some();
    let metadata = match discovered {
        Some(metadata) => metadata,
        None => default_authorization_server_metadata(&authorization_server_url)?,
    };
    let current_server = OAuthAuthorizationServerInformation {
        issuer: Some(metadata.issuer.clone()),
        authorization_server_url: authorization_server_url.clone(),
        token_endpoint: metadata.token_endpoint.clone(),
    };
    let mut client_metadata = provider.client_metadata();
    let scope = options
        .scope
        .clone()
        .filter(|scope| !scope.is_empty())
        .or_else(|| {
            resource_metadata
                .as_ref()
                .and_then(|metadata| metadata.scopes_supported.as_ref())
                .map(|scopes| scopes.join(" "))
                .filter(|scope| !scope.is_empty())
        })
        .or_else(|| client_metadata.scope.clone());
    let client = match provider.client_information().await? {
        Some(client) => {
            if client
                .authorization_server_information
                .as_ref()
                .is_some_and(|info| info.issuer.is_some())
                && let Some(stored) = stored_server(provider, &client).await?
            {
                assert_server_matches(&stored, &current_server)?;
            }
            client
        }
        None => {
            if options.authorization_code.is_some() {
                return Err(McpError::oauth(
                    "existing client information is required when exchanging an authorization code",
                ));
            }
            client_metadata.scope = scope.clone();
            let mut registered = register_client(http, &metadata, &client_metadata, policy).await?;
            registered.authorization_server_information = Some(current_server.clone());
            provider.save_client_information(registered.clone()).await?;
            registered
        }
    };
    let request = TokenRequest {
        http,
        metadata: &metadata,
        client: &client,
        policy,
        authentication: Some((
            provider,
            &authorization_server_url,
            has_metadata.then_some(&metadata),
        )),
    };
    if let Some(code) = &options.authorization_code {
        if let Some(expected) = provider.stored_state().await?
            && options.callback_state.as_ref() != Some(&expected)
        {
            return Err(McpError::oauth(
                "authorization callback state does not match the stored state",
            ));
        }
        let stored = stored_server(provider, &client).await?.ok_or_else(|| {
            McpError::oauth("stored authorization server information is required when exchanging an authorization code")
        })?;
        let expected_issuer = stored.issuer.as_deref().unwrap_or(&metadata.issuer);
        if options
            .callback_issuer
            .as_ref()
            .is_some_and(|issuer| issuer != expected_issuer)
        {
            return Err(McpError::oauth(
                "authorization callback issuer does not match the stored issuer",
            ));
        }
        assert_server_matches(&stored, &current_server)?;
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
        let mut tokens = request.exchange(&grant).await?;
        tokens.authorization_server_information = Some(current_server);
        provider.save_tokens(tokens).await?;
        return Ok(AuthResult::Authorized);
    }
    if let Some(tokens) = provider.tokens().await?
        && let Some(refresh_token) = &tokens.refresh_token
    {
        let stored = match tokens.authorization_server_information {
            Some(information) => Some(information),
            None => stored_server(provider, &client).await?,
        };
        if let Some(stored) = stored {
            assert_server_matches(&stored, &current_server)?;
            match request.refresh(refresh_token, resource.as_ref()).await {
                Ok(mut refreshed) => {
                    refreshed.authorization_server_information = Some(current_server);
                    provider.save_tokens(refreshed).await?;
                    return Ok(AuthResult::Authorized);
                }
                Err(error) if is_transient(&error) => {}
                Err(error) => return Err(error),
            }
        } else {
            provider
                .invalidate_credentials(InvalidateScope::Tokens)
                .await?;
        }
    }
    let redirect_url = provider
        .redirect_url()
        .ok_or_else(|| McpError::oauth("a redirect url is required to start authorization"))?;
    let pkce = generate_pkce();
    let state = provider.state().await?;
    if let Some(state) = state.as_ref().filter(|state| !state.is_empty()) {
        provider.save_state(state.clone()).await?;
    }
    let url = start_authorization(
        &metadata,
        &client,
        &redirect_url,
        &pkce,
        scope.as_deref(),
        state.as_deref(),
        resource.as_ref(),
    )?;
    provider
        .save_authorization_server_information(current_server)
        .await?;
    provider.save_code_verifier(pkce.verifier).await?;
    provider.redirect_to_authorization(url).await?;
    Ok(AuthResult::Redirect)
}

/// Discovers OAuth metadata and exchanges, refreshes, or starts authorization.
///
/// Rejected client credentials are invalidated and retried once; invalid
/// grants discard only stored tokens. Callback state, issuer and stored
/// authorization server identity are checked before credentials are sent.
///
/// # Errors
///
/// Returns [`McpError::OAuth`] for protocol failures, [`McpError::Url`] for URL
/// policy violations, or the provider's storage errors.
#[tracing::instrument(skip_all, fields(server = %options.server_url.host_str().unwrap_or_default()))]
pub async fn auth(
    provider: &dyn OAuthClientProvider,
    http: &dyn HttpTransport,
    options: AuthOptions,
) -> Result<AuthResult, McpError> {
    match Box::pin(auth_internal(provider, http, &options)).await {
        Err(McpError::OAuth {
            error_code: Some(code),
            ..
        }) if code == "invalid_client" || code == "unauthorized_client" => {
            provider
                .invalidate_credentials(InvalidateScope::All)
                .await?;
            Box::pin(auth_internal(provider, http, &options)).await
        }
        Err(McpError::OAuth {
            error_code: Some(code),
            ..
        }) if code == "invalid_grant" => {
            provider
                .invalidate_credentials(InvalidateScope::Tokens)
                .await?;
            Box::pin(auth_internal(provider, http, &options)).await
        }
        other => other,
    }
}
