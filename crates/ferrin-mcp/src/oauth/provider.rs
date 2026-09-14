//! The storage and user-interaction side of the OAuth flow.

use futures_util::future::BoxFuture;
use url::Url;

use super::types::InvalidateScope;
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
