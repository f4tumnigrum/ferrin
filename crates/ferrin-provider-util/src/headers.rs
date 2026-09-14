//! Header helpers for outbound requests.

use ferrin_spec::Headers;
use url::Url;

/// Headers never forwarded on downloads (hop-by-hop, proxy, metadata and
/// credential headers).
pub const BLOCKED_DOWNLOAD_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "forwarded",
    "proxy-authorization",
    "via",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
    "metadata",
    "metadata-flavor",
    "x-aws-ec2-metadata-token",
    "x-metadata-token",
    "cookie",
    "set-cookie",
];

/// Removes [`BLOCKED_DOWNLOAD_HEADERS`].
#[must_use]
pub fn sanitize_download_headers(mut headers: Headers) -> Headers {
    for name in BLOCKED_DOWNLOAD_HEADERS {
        headers.remove(name);
    }
    headers
}

/// Returns `true` when both URLs share scheme, host and port.
#[must_use]
pub fn is_same_origin(a: &Url, b: &Url) -> bool {
    a.origin() == b.origin()
}

/// Keeps only `user-agent` and `accept`, used when following a redirect to
/// another origin.
#[must_use]
pub fn strip_to_public_headers(headers: &Headers) -> Headers {
    let mut kept = Headers::new();
    for name in ["user-agent", "accept"] {
        if let Some(value) = headers.get_str(name) {
            kept = kept.with(name, value);
        }
    }
    kept
}
