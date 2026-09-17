//! The storage and user-interaction side of the OAuth flow.

use ferrin_spec::Headers;
use futures_util::future::BoxFuture;
use url::Url;

use super::types::AuthorizationServerMetadata;
use super::types::InvalidateScope;
use super::types::OAuthAuthorizationServerInformation;
use super::types::OAuthClientInformation;
use super::types::OAuthClientMetadata;
use super::types::OAuthTokens;
use crate::error::McpError;

/// Application hooks used by [`super::auth`]: credential storage, redirect
/// handling and PKCE state.
pub trait OAuthClientProvider: Send + Sync {
    /// Redirect URL registered for this client, when it can receive
    /// authorization codes.
    fn redirect_url(&self) -> Option<Url>;

    /// Client metadata used for dynamic registration.
    fn client_metadata(&self) -> OAuthClientMetadata;

    /// Stored client credentials.
    fn client_information(&self)
    -> BoxFuture<'_, Result<Option<OAuthClientInformation>, McpError>>;

    /// Stores credentials obtained by dynamic registration. The default
    /// rejects registration.
    fn save_client_information(
        &self,
        information: OAuthClientInformation,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        let _ = information;
        Box::pin(std::future::ready(Err(McpError::oauth(
            "provider does not store client information; dynamic client registration is unavailable",
        ))))
    }

    /// Stored tokens.
    fn tokens(&self) -> BoxFuture<'_, Result<Option<OAuthTokens>, McpError>>;

    /// Stores tokens.
    fn save_tokens(&self, tokens: OAuthTokens) -> BoxFuture<'_, Result<(), McpError>>;

    /// Sends the user to the authorization URL.
    fn redirect_to_authorization(
        &self,
        authorization_url: Url,
    ) -> BoxFuture<'_, Result<(), McpError>>;

    /// Stores the PKCE code verifier for the pending authorization.
    fn save_code_verifier(&self, code_verifier: String) -> BoxFuture<'_, Result<(), McpError>>;

    /// The PKCE code verifier of the pending authorization.
    fn code_verifier(&self) -> BoxFuture<'_, Result<Option<String>, McpError>>;

    /// `state` parameter for the authorization request (default: none).
    fn state(&self) -> BoxFuture<'_, Result<Option<String>, McpError>> {
        Box::pin(std::future::ready(Ok(None)))
    }

    /// Stores the authorization request state for callback validation.
    fn save_state(&self, state: String) -> BoxFuture<'_, Result<(), McpError>> {
        let _ = state;
        Box::pin(std::future::ready(Ok(())))
    }

    /// Previously stored authorization request state (default: none).
    fn stored_state(&self) -> BoxFuture<'_, Result<Option<String>, McpError>> {
        Box::pin(std::future::ready(Ok(None)))
    }

    /// Stored authorization server identity, ahead of the client credential pin.
    fn authorization_server_information(
        &self,
    ) -> BoxFuture<'_, Result<Option<OAuthAuthorizationServerInformation>, McpError>> {
        Box::pin(std::future::ready(Ok(None)))
    }

    /// Stores the authorization server identity before redirecting.
    ///
    /// The default attaches it to stored client information and delegates to
    /// [`Self::save_client_information`].
    fn save_authorization_server_information(
        &self,
        information: OAuthAuthorizationServerInformation,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let mut client = self.client_information().await?.ok_or_else(|| {
                McpError::oauth("client information is required to save the authorization server")
            })?;
            client.authorization_server_information = Some(information);
            self.save_client_information(client).await
        })
    }

    /// Validates a discovered authorization server before fetching its metadata.
    fn validate_authorization_server_url<'a>(
        &'a self,
        server_url: &'a Url,
        authorization_server_url: &'a Url,
    ) -> BoxFuture<'a, Result<(), McpError>> {
        let _ = (server_url, authorization_server_url);
        Box::pin(std::future::ready(Ok(())))
    }

    /// Selects the optional resource indicator after validating its coverage.
    ///
    /// Override to choose a different indicator or omit it. The default omits
    /// the indicator when no protected resource metadata is available.
    fn validate_resource_url<'a>(
        &'a self,
        server_url: &'a Url,
        resource: Option<&'a Url>,
    ) -> BoxFuture<'a, Result<Option<Url>, McpError>> {
        Box::pin(std::future::ready(super::discovery::validate_resource_url(
            server_url, resource,
        )))
    }

    /// Adds client authentication to a token exchange or refresh request.
    ///
    /// Override to replace the standard Basic, POST or public-client method.
    fn add_client_authentication<'a>(
        &'a self,
        headers: &'a mut Headers,
        params: &'a mut Vec<(String, String)>,
        authorization_server_url: &'a Url,
        metadata: Option<&'a AuthorizationServerMetadata>,
        client: &'a OAuthClientInformation,
    ) -> BoxFuture<'a, Result<(), McpError>> {
        let _ = authorization_server_url;
        Box::pin(std::future::ready(super::flow::apply_client_auth(
            client, metadata, params, headers,
        )))
    }

    /// Discards stored credentials after the server rejected them (default:
    /// no-op).
    fn invalidate_credentials(
        &self,
        scope: InvalidateScope,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        let _ = scope;
        Box::pin(std::future::ready(Ok(())))
    }
}
