//! Secure handling of application- and model-supplied URLs.
//!
//! Every URL is validated against a [`UrlPolicy`] before any connection is
//! opened: scheme allow-list, no embedded credentials, no hostnames or
//! resolved addresses in private, loopback, link-local, multicast or
//! reserved ranges. DNS is resolved once and the connection is pinned to the
//! validated addresses, so a rebinding resolver cannot redirect the request
//! after validation. Redirects are followed manually, each hop is validated
//! again, and cross-origin hops drop every header except `User-Agent` and
//! `Accept`. Bodies are read with a byte limit.

mod download;
mod policy;
mod private_network;

pub use download::DownloadError;
pub use download::DownloadErrorKind;
pub use download::Downloaded;
pub use download::fetch;
pub use download::fetch_with_headers;
pub use policy::DEFAULT_MAX_BODY_BYTES;
pub use policy::DEFAULT_MAX_REDIRECTS;
pub use policy::Scheme;
pub use policy::UrlPolicy;
pub use policy::UrlValidationError;
pub use policy::ValidatedUrl;
pub use policy::validate_url;
pub use private_network::is_private_hostname;
pub use private_network::is_private_ip;
