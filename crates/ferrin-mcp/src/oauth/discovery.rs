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
        let mut candidate = well_known(
            &origin,
            &format!("/.well-known/oauth-protected-resource{path}"),
        );
        if let Some(url) = &mut candidate {
            url.set_query(server_url.query());
        }
        urls.extend(candidate);
    }
    let mut root = well_known(&origin, "/.well-known/oauth-protected-resource");
    if urls.is_empty()
        && let Some(url) = &mut root
    {
        url.set_query(server_url.query());
    }
    urls.extend(root);
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
    /// Expected issuer of the document at this URL.
    pub expected_issuer: String,
}

/// Candidate metadata URLs of an authorization server, in probe order.
#[must_use]
pub fn authorization_server_metadata_urls(authorization_server_url: &Url) -> Vec<MetadataUrl> {
    let Ok(origin) = origin_url(authorization_server_url) else {
        return Vec::new();
    };
    let path = authorization_server_url.path().trim_end_matches('/');
    let mut urls = Vec::new();
    let origin_issuer = authorization_server_url.origin().ascii_serialization();
    let mut push = |suffix: String, openid: bool, issuer_path: &str| {
        if let Some(url) = well_known(&origin, &suffix) {
            urls.push(MetadataUrl {
                url,
                openid,
                expected_issuer: format!("{origin_issuer}{issuer_path}"),
            });
        }
    };
    if path.is_empty() {
        push(
            "/.well-known/oauth-authorization-server".to_owned(),
            false,
            "",
        );
        push("/.well-known/openid-configuration".to_owned(), true, "");
    } else {
        push(
            format!("/.well-known/oauth-authorization-server{path}"),
            false,
            path,
        );
        push(
            "/.well-known/oauth-authorization-server".to_owned(),
            false,
            "",
        );
        push(
            format!("/.well-known/openid-configuration{path}"),
            true,
            path,
        );
        push(
            format!("{path}/.well-known/openid-configuration"),
            true,
            path,
        );
    }
    urls
}

fn supports_s256(metadata: &AuthorizationServerMetadata) -> Option<bool> {
    metadata
        .code_challenge_methods_supported
        .as_ref()
        .map(|methods| methods.iter().any(|method| method == "S256"))
}

fn validate_metadata(metadata: &AuthorizationServerMetadata, openid: bool) -> Result<(), McpError> {
    if metadata.authorization_endpoint.is_none() || metadata.response_types_supported.is_none() {
        return Err(McpError::oauth(
            "authorization server metadata requires authorization_endpoint and response_types_supported",
        ));
    }
    for url in metadata
        .authorization_endpoint
        .iter()
        .chain(std::iter::once(&metadata.token_endpoint))
        .chain(metadata.registration_endpoint.iter())
    {
        validate_metadata_url(url)?;
    }
    if openid {
        let jwks = metadata
            .extra
            .get("jwks_uri")
            .and_then(ferrin_spec::JsonValue::as_str)
            .and_then(|value| Url::parse(value).ok())
            .ok_or_else(|| McpError::oauth("openid metadata requires a valid jwks_uri"))?;
        validate_metadata_url(&jwks)?;
        for name in [
            "subject_types_supported",
            "id_token_signing_alg_values_supported",
        ] {
            if !metadata
                .extra
                .get(name)
                .and_then(ferrin_spec::JsonValue::as_array)
                .is_some_and(|values| values.iter().all(ferrin_spec::JsonValue::is_string))
            {
                return Err(McpError::oauth(format!("openid metadata requires {name}")));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_metadata_url(url: &Url) -> Result<(), McpError> {
    if matches!(url.scheme(), "javascript" | "data" | "vbscript") {
        return Err(McpError::oauth("oauth metadata url uses an unsafe scheme"));
    }
    Ok(())
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
        validate_metadata(&metadata, candidate.openid)?;
        let issuer_matches = metadata.issuer == candidate.expected_issuer
            || (candidate.expected_issuer
                == authorization_server_url.origin().ascii_serialization()
                && metadata.issuer == format!("{}/", candidate.expected_issuer));
        if !issuer_matches {
            return Err(McpError::oauth(
                "authorization server metadata issuer does not match the discovery issuer",
            ));
        }
        if candidate.openid && supports_s256(&metadata) != Some(true) {
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
        issuer: authorization_server_url.to_string(),
        authorization_endpoint: Some(endpoint("/authorize")?),
        token_endpoint: endpoint("/token")?,
        registration_endpoint: Some(endpoint("/register")?),
        scopes_supported: None,
        response_types_supported: Some(vec!["code".to_owned()]),
        grant_types_supported: None,
        token_endpoint_auth_methods_supported: None,
        code_challenge_methods_supported: Some(vec!["S256".to_owned()]),
        extra: ferrin_spec::JsonObject::new(),
    })
}

/// The `resource` indicator (RFC 8707) for `server_url`: the resource named
/// by the metadata when it covers the server URL; absent without metadata.
///
/// # Errors
///
/// Returns [`McpError::OAuth`] when the metadata names a resource that does
/// not cover `server_url`.
pub fn select_resource_url(
    server_url: &Url,
    metadata: Option<&ProtectedResourceMetadata>,
) -> Result<Option<Url>, McpError> {
    validate_resource_url(server_url, metadata.map(|metadata| &metadata.resource))
}

pub(super) fn validate_resource_url(
    server_url: &Url,
    resource: Option<&Url>,
) -> Result<Option<Url>, McpError> {
    let mut canonical = server_url.clone();
    canonical.set_fragment(None);
    let Some(resource) = resource else {
        return Ok(None);
    };
    let same_origin = resource.origin() == canonical.origin();
    let resource_path = resource.path().trim_end_matches('/');
    let covers = same_origin
        && canonical.path().len() >= resource.path().len()
        && (resource_path.is_empty()
            || canonical.path() == resource_path
            || canonical.path().starts_with(&format!("{resource_path}/")));
    if !covers {
        return Err(McpError::oauth(format!(
            "protected resource metadata names {} which does not cover the server url",
            resource.host_str().unwrap_or_default()
        )));
    }
    Ok(Some(resource.clone()))
}
