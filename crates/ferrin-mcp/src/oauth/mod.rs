//! OAuth 2.1 authorization for HTTP transports (feature `oauth`).
//!
//! The flow follows the MCP authorization specification: protected resource
//! metadata discovery, authorization server metadata discovery, optional
//! dynamic client registration, PKCE (`S256`) authorization code grant with
//! resource indicators, and token refresh.

mod discovery;
mod flow;
mod http;
mod provider;
mod types;

pub use discovery::MetadataUrl;
pub use discovery::authorization_server_metadata_urls;
pub use discovery::default_authorization_server_metadata;
pub use discovery::discover_authorization_server_metadata;
pub use discovery::discover_protected_resource_metadata;
pub use discovery::protected_resource_metadata_urls;
pub use discovery::select_resource_url;
pub use flow::AuthOptions;
pub use flow::AuthorizationCodeGrant;
pub use flow::Pkce;
pub use flow::auth;
pub use flow::exchange_authorization;
pub use flow::generate_pkce;
pub use flow::pkce_challenge;
pub use flow::refresh_authorization;
pub use flow::register_client;
pub use flow::start_authorization;
pub use provider::OAuthClientProvider;
pub use types::AuthResult;
pub use types::AuthorizationServerMetadata;
pub use types::InvalidateScope;
pub use types::OAuthClientInformation;
pub use types::OAuthClientMetadata;
pub use types::OAuthTokens;
pub use types::ProtectedResourceMetadata;
pub use types::WwwAuthenticateParams;
pub use types::extract_www_authenticate_params;
