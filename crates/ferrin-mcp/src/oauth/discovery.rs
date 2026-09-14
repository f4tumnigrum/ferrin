//! Metadata discovery: protected resource (RFC 9728) and authorization
//! server (RFC 8414 / OpenID Connect Discovery).

use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_spec::Headers;
use url::Url;

use super::http::get_json;
use super::types::AuthorizationServerMetadata;
use super::types::ProtectedResourceMetadata;
use crate::error::McpError;
use crate::protocol::LATEST_PROTOCOL_VERSION;

fn well_known(origin: &Url, suffix: &str) -> Option<Url> {
    origin.join(suffix).ok()
}

fn origin_url(url: &Url) -> Result<Url, McpError> {
    url.join("/")
        .map_err(|error| McpError::oauth(format!("invalid server url: {error}")))
}

/// Candidate URLs of the protected resource metadata document.
#[must_use]
pub fn protected_resource_metadata_urls(server_url: &Url) -> Vec<Url> {
    let Ok(origin) = origin_url(server_url) else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    let path = server_url.path();
    if path != "/" && !path.is_empty() {
        urls.extend(well_known(
            &origin,
            &format!("/.well-known/oauth-protected-resource{path}"),
        ));
    }
    urls.extend(well_known(&origin, "/.well-known/oauth-protected-resource"));
    urls
}

/// Fetches the protected resource metadata of `server_url` (or of
/// `explicit_url` when the `WWW-Authenticate` challenge named one).
///
/// # Errors
///
/// Returns the request or parse failure; `Ok(None)` when no document
/// exists.
pub async fn discover_protected_resource_metadata(
    http: &dyn HttpTransport,
    server_url: &Url,
    explicit_url: Option<Url>,
    policy: &UrlPolicy,
) -> Result<Option<ProtectedResourceMetadata>, McpError> {
    let candidates = match explicit_url {
        Some(url) => vec![url],
        None => protected_resource_metadata_urls(server_url),
    };
    let headers = Headers::new().with("mcp-protocol-version", LATEST_PROTOCOL_VERSION);
    for url in candidates {
        if let Some(json) = get_json(http, &url, policy, headers.clone()).await? {
            let metadata = serde_json::from_value(json).map_err(|error| {
                McpError::oauth(format!("invalid protected resource metadata: {error}"))
            })?;
            return Ok(Some(metadata));
        }
    }
    Ok(None)
}

/// A candidate authorization server metadata URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataUrl {
    /// Document location.
    pub url: Url,
    /// Whether the document is an OpenID Connect discovery document.
    pub openid: bool,
}

/// Candidate metadata URLs of an authorization server, in probe order.
#[must_use]
pub fn authorization_server_metadata_urls(authorization_server_url: &Url) -> Vec<MetadataUrl> {
    let Ok(origin) = origin_url(authorization_server_url) else {
        return Vec::new();
    };
    let path = authorization_server_url.path().trim_end_matches('/');
    let mut urls = Vec::new();
    let mut push = |suffix: String, openid: bool| {
        if let Some(url) = well_known(&origin, &suffix) {
            urls.push(MetadataUrl { url, openid });
        }
    };
    if path.is_empty() {
        push("/.well-known/oauth-authorization-server".to_owned(), false);
        push("/.well-known/openid-configuration".to_owned(), true);
    } else {
        push(
            format!("/.well-known/oauth-authorization-server{path}"),
            false,
        );
        push("/.well-known/oauth-authorization-server".to_owned(), false);
        push(format!("/.well-known/openid-configuration{path}"), true);
        push(format!("{path}/.well-known/openid-configuration"), true);
    }
    urls
}

fn supports_s256(metadata: &AuthorizationServerMetadata) -> Option<bool> {
    metadata
        .code_challenge_methods_supported
        .as_ref()
        .map(|methods| methods.iter().any(|method| method == "S256"))
}

/// Fetches the authorization server metadata.
///
/// # Errors
///
/// Returns the request or parse failure, or [`McpError::OAuth`] when the
/// server does not support the `S256` PKCE method; `Ok(None)` when no
/// document exists.
pub async fn discover_authorization_server_metadata(
    http: &dyn HttpTransport,
    authorization_server_url: &Url,
    policy: &UrlPolicy,
) -> Result<Option<AuthorizationServerMetadata>, McpError> {
    let headers = Headers::new().with("mcp-protocol-version", LATEST_PROTOCOL_VERSION);
    for candidate in authorization_server_metadata_urls(authorization_server_url) {
        let Some(json) = get_json(http, &candidate.url, policy, headers.clone()).await? else {
            continue;
        };
        let metadata: AuthorizationServerMetadata =
            serde_json::from_value(json).map_err(|error| {
                McpError::oauth(format!("invalid authorization server metadata: {error}"))
            })?;
        let s256 = supports_s256(&metadata);
        if (candidate.openid && s256 != Some(true)) || s256 == Some(false) {
            return Err(McpError::oauth(
                "authorization server does not support the S256 code challenge method required by MCP",
            ));
        }
        return Ok(Some(metadata));
    }
    Ok(None)
}

/// Endpoints assumed when the server publishes no metadata.
///
/// # Errors
///
/// Returns [`McpError::OAuth`] when `authorization_server_url` cannot be
/// joined with the endpoint paths.
pub fn default_authorization_server_metadata(
    authorization_server_url: &Url,
) -> Result<AuthorizationServerMetadata, McpError> {
    let origin = origin_url(authorization_server_url)?;
    let endpoint = |path: &str| {
        origin
            .join(path)
            .map_err(|error| McpError::oauth(format!("invalid authorization server url: {error}")))
    };
    Ok(AuthorizationServerMetadata {
        issuer: origin.to_string(),
        authorization_endpoint: Some(endpoint("/authorize")?),
        token_endpoint: endpoint("/token")?,
        registration_endpoint: Some(endpoint("/register")?),
        scopes_supported: None,
        response_types_supported: None,
        grant_types_supported: None,
        token_endpoint_auth_methods_supported: None,
        code_challenge_methods_supported: None,
        extra: ferrin_spec::JsonObject::new(),
    })
}

/// The `resource` indicator (RFC 8707) for `server_url`: the resource named
/// by the metadata when it covers the server URL, else the server URL
/// without fragment.
///
/// # Errors
///
/// Returns [`McpError::OAuth`] when the metadata names a resource that does
/// not cover `server_url`.
pub fn select_resource_url(
    server_url: &Url,
    metadata: Option<&ProtectedResourceMetadata>,
) -> Result<Url, McpError> {
    let mut canonical = server_url.clone();
    canonical.set_fragment(None);
    let Some(metadata) = metadata else {
        return Ok(canonical);
    };
    let resource = &metadata.resource;
    let same_origin = resource.origin() == canonical.origin();
    let resource_path = resource.path().trim_end_matches('/');
    let covers = same_origin
        && (resource_path.is_empty()
            || canonical.path() == resource_path
            || canonical.path().starts_with(&format!("{resource_path}/")));
    if !covers {
        return Err(McpError::oauth(format!(
            "protected resource metadata names {} which does not cover the server url",
            resource.host_str().unwrap_or_default()
        )));
    }
    Ok(resource.clone())
}
