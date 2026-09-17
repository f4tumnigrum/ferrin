//! HTTP helpers of the OAuth flow.

use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::RequestBody;
use ferrin_provider_util::http::read_body;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_provider_util::secure_url::validate_url;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use http::Method;
use http::StatusCode;
use url::Url;

use crate::error::McpError;

/// Maximum size of an OAuth response body.
pub(super) const MAX_BODY_BYTES: u64 = 1024 * 1024;

const USER_AGENT: &str = concat!("ferrin-mcp/", env!("CARGO_PKG_VERSION"));

/// A complete OAuth response.
#[derive(Debug)]
pub(super) struct OAuthResponse {
    pub(super) status: StatusCode,
    pub(super) body: String,
}

impl OAuthResponse {
    pub(super) fn json(&self) -> Option<JsonValue> {
        serde_json::from_str(&self.body).ok()
    }

    /// Converts a failed token or registration response into an error,
    /// preserving the OAuth `error` code when the body carries one.
    pub(super) fn into_error(self, what: &str) -> McpError {
        let json = self.json();
        let code = json
            .as_ref()
            .and_then(|json| json.get("error"))
            .and_then(JsonValue::as_str)
            .map(str::to_owned);
        let description = json
            .as_ref()
            .and_then(|json| json.get("error_description"))
            .and_then(JsonValue::as_str)
            .map(str::to_owned);
        match code {
            Some(code) => McpError::OAuth {
                message: match description {
                    Some(description) => format!("{what} failed: {code}: {description}"),
                    None => format!("{what} failed: {code}"),
                },
                error_code: Some(code),
            },
            None => McpError::oauth(format!("{what} failed (HTTP {})", self.status)),
        }
    }
}

pub(super) async fn send(
    http: &dyn HttpTransport,
    method: Method,
    url: &Url,
    policy: &UrlPolicy,
    headers: Headers,
    body: RequestBody,
) -> Result<OAuthResponse, McpError> {
    let validated = validate_url(url, policy).await?;
    let mut headers = headers;
    if !headers.contains("content-type")
        && let Some(content_type) = body.content_type()
    {
        let _ = headers.insert("content-type", &content_type);
    }
    let request = HttpRequest::new(method, url.clone())
        .with_headers(headers.with_user_agent_suffix([USER_AGENT]))
        .with_body(body)
        .with_pinned_addresses(validated.addresses);
    let response = http
        .execute(request)
        .await
        .map_err(|error| McpError::from_transport(error, url))?;
    let status = response.status;
    let bytes = read_body(&response.headers, response.body, MAX_BODY_BYTES)
        .await
        .map_err(|error| McpError::from_transport(error, url))?;
    Ok(OAuthResponse {
        status,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// `GET`s a JSON document; client errors allow discovery fallback.
pub(super) async fn get_json(
    http: &dyn HttpTransport,
    url: &Url,
    policy: &UrlPolicy,
    headers: Headers,
) -> Result<Option<JsonValue>, McpError> {
    let response = send(http, Method::GET, url, policy, headers, RequestBody::Empty).await?;
    if response.status.is_client_error() {
        return Ok(None);
    }
    if !response.status.is_success() {
        return Err(McpError::oauth(format!(
            "metadata request to {} failed (HTTP {})",
            url.host_str().unwrap_or_default(),
            response.status
        )));
    }
    response
        .json()
        .map(Some)
        .ok_or_else(|| McpError::oauth("metadata response is not valid JSON"))
}

/// Encodes `fields` as `application/x-www-form-urlencoded`.
pub(super) fn form_body(fields: &[(String, String)]) -> RequestBody {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in fields {
        serializer.append_pair(name, value);
    }
    RequestBody::Bytes {
        content_type: "application/x-www-form-urlencoded".to_owned(),
        data: bytes::Bytes::from(serializer.finish()),
    }
}
