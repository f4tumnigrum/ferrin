//! Size-limited download with manual, validated redirects.

use bytes::Bytes;
use ferrin_spec::Headers;
use ferrin_spec::MediaType;
use http::Method;
use http::StatusCode;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::policy::UrlPolicy;
use super::policy::UrlValidationError;
use super::policy::validate_url;
use crate::headers::is_same_origin;
use crate::headers::sanitize_download_headers;
use crate::headers::strip_to_public_headers;
use crate::http::HttpRequest;
use crate::http::HttpTransport;
use crate::http::TransportError;
use crate::http::read_body;

/// A downloaded resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    /// The URL the bytes came from (after redirects).
    pub url: Url,
    /// Body bytes.
    pub data: Bytes,
    /// `Content-Type` without parameters, if the server sent one.
    pub media_type: Option<MediaType>,
    /// Response headers.
    pub headers: Headers,
}

/// Download failure: the URL concerned plus the reason.
#[derive(Debug, thiserror::Error)]
#[error("failed to download {url}: {kind}")]
pub struct DownloadError {
    url: Box<Url>,
    #[source]
    kind: DownloadErrorKind,
}

impl DownloadError {
    /// Creates an error.
    #[must_use]
    pub fn new(url: Url, kind: DownloadErrorKind) -> Self {
        Self {
            url: Box::new(url),
            kind,
        }
    }

    /// The URL the error concerns (the current hop when redirects were
    /// followed).
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The reason.
    #[must_use]
    pub fn kind(&self) -> &DownloadErrorKind {
        &self.kind
    }

    /// Consumes the error, returning the reason.
    #[must_use]
    pub fn into_kind(self) -> DownloadErrorKind {
        self.kind
    }

    /// Returns `true` when the download was cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self.kind, DownloadErrorKind::Cancelled)
    }
}

/// Why a download failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DownloadErrorKind {
    /// The URL (initial or redirect target) failed validation.
    #[error("url rejected: {0}")]
    Validation(#[source] UrlValidationError),
    /// The transport failed.
    #[error(transparent)]
    Transport(TransportError),
    /// The server answered with a non-2xx status (after redirects).
    #[error("status {status}")]
    Status {
        /// The status code.
        status: StatusCode,
        /// The response headers.
        headers: Box<Headers>,
    },
    /// A redirect response had no usable `Location`.
    #[error("redirect has an invalid location")]
    InvalidRedirect,
    /// More redirects than the policy allows.
    #[error("too many redirects (limit {limit})")]
    TooManyRedirects {
        /// The limit.
        limit: u8,
    },
    /// The download was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// Downloads `url` with the default headers.
///
/// # Errors
///
/// See [`DownloadError`].
pub async fn fetch(
    transport: &dyn HttpTransport,
    url: Url,
    policy: &UrlPolicy,
    cancellation: CancellationToken,
) -> Result<Downloaded, DownloadError> {
    fetch_with_headers(transport, url, Headers::new(), policy, cancellation).await
}

/// Downloads `url`, sending `headers` (minus hop-by-hop and credential
/// headers; `Authorization` only to credentialed origins).
///
/// Redirect hops to another origin keep only `User-Agent` and `Accept`.
///
/// # Errors
///
/// See [`DownloadError`].
pub async fn fetch_with_headers(
    transport: &dyn HttpTransport,
    url: Url,
    headers: Headers,
    policy: &UrlPolicy,
    cancellation: CancellationToken,
) -> Result<Downloaded, DownloadError> {
    let initial = url.clone();
    let mut headers = sanitize_download_headers(headers);
    if !headers.contains("user-agent") {
        headers = headers.with("user-agent", &format!("ferrin/{}", crate::VERSION));
    }
    if !policy.is_credentialed(&url) {
        headers.remove("authorization");
    }
    let mut current = url;
    let mut redirects: u8 = 0;
    loop {
        let validated = validate_url(&current, policy).await.map_err(|source| {
            DownloadError::new(current.clone(), DownloadErrorKind::Validation(source))
        })?;
        let request = HttpRequest::new(Method::GET, validated.url.clone())
            .with_headers(headers.clone())
            .with_cancellation(cancellation.clone())
            .with_pinned_addresses(validated.addresses);
        let response = transport
            .execute(request)
            .await
            .map_err(|source| transport_error(&current, source))?;
        if response.status.is_redirection() {
            let Some(location) = response
                .headers
                .get_str("location")
                .and_then(|location| current.join(location).ok())
            else {
                return Err(DownloadError::new(
                    current,
                    DownloadErrorKind::InvalidRedirect,
                ));
            };
            redirects += 1;
            if redirects > policy.max_redirects {
                return Err(DownloadError::new(
                    initial,
                    DownloadErrorKind::TooManyRedirects {
                        limit: policy.max_redirects,
                    },
                ));
            }
            if !is_same_origin(&current, &location) {
                headers = strip_to_public_headers(&headers);
            }
            current = location;
            continue;
        }
        if !response.status.is_success() {
            return Err(DownloadError::new(
                current,
                DownloadErrorKind::Status {
                    status: response.status,
                    headers: Box::new(response.headers),
                },
            ));
        }
        let media_type = response
            .headers
            .get_str("content-type")
            .map(|value| value.split(';').next().unwrap_or(value).trim())
            .filter(|value| !value.is_empty())
            .map(MediaType::new);
        let response_headers = response.headers.clone();
        let data = read_body(&response.headers, response.body, policy.max_body_bytes)
            .await
            .map_err(|source| transport_error(&current, source))?;
        return Ok(Downloaded {
            url: current,
            data,
            media_type,
            headers: response_headers,
        });
    }
}

fn transport_error(url: &Url, source: TransportError) -> DownloadError {
    let kind = if source.is_cancelled() {
        DownloadErrorKind::Cancelled
    } else {
        DownloadErrorKind::Transport(source)
    };
    DownloadError::new(url.clone(), kind)
}
