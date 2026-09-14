//! URL policy and validation.

use std::net::IpAddr;
use std::net::SocketAddr;

use url::Host;
use url::Origin;
use url::Url;

use super::private_network::is_private_hostname;
use super::private_network::is_private_ip;

/// Default body limit for downloads (100 MiB).
pub const DEFAULT_MAX_BODY_BYTES: u64 = 100 * 1024 * 1024;

/// Default maximum number of redirects followed.
pub const DEFAULT_MAX_REDIRECTS: u8 = 5;

/// URL schemes a policy can allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Scheme {
    /// `https`.
    Https,
    /// `http` (opt in; plain text).
    Http,
}

impl Scheme {
    fn matches(self, scheme: &str) -> bool {
        match self {
            Self::Https => scheme.eq_ignore_ascii_case("https"),
            Self::Http => scheme.eq_ignore_ascii_case("http"),
        }
    }
}

/// Rules applied to URLs before they are fetched.
#[derive(Debug, Clone)]
pub struct UrlPolicy {
    /// Allowed schemes (default `[Https]`).
    pub allowed_schemes: Vec<Scheme>,
    /// Allow private, loopback and link-local destinations (default `false`).
    pub allow_private_networks: bool,
    /// Origins exempt from the private-network check.
    pub trusted_origins: Vec<Origin>,
    /// Origins that may receive `Authorization` and other caller headers.
    pub credentialed_origins: Vec<Origin>,
    /// Maximum redirects followed (default 5).
    pub max_redirects: u8,
    /// Maximum response body size in bytes (default 100 MiB).
    pub max_body_bytes: u64,
}

impl Default for UrlPolicy {
    fn default() -> Self {
        Self {
            allowed_schemes: vec![Scheme::Https],
            allow_private_networks: false,
            trusted_origins: Vec::new(),
            credentialed_origins: Vec::new(),
            max_redirects: DEFAULT_MAX_REDIRECTS,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

impl UrlPolicy {
    /// The default policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Also allows `http`.
    #[must_use]
    pub fn allow_http(mut self) -> Self {
        if !self.allowed_schemes.contains(&Scheme::Http) {
            self.allowed_schemes.push(Scheme::Http);
        }
        self
    }

    /// Allows private-network destinations (local development, tests).
    #[must_use]
    pub fn allow_private_networks(mut self) -> Self {
        self.allow_private_networks = true;
        self
    }

    /// Adds a trusted origin.
    #[must_use]
    pub fn trust_origin(mut self, origin: &Url) -> Self {
        self.trusted_origins.push(origin.origin());
        self
    }

    /// Adds an origin that may receive caller credentials.
    #[must_use]
    pub fn credential_origin(mut self, origin: &Url) -> Self {
        self.credentialed_origins.push(origin.origin());
        self
    }

    /// Sets the redirect limit.
    #[must_use]
    pub fn max_redirects(mut self, max_redirects: u8) -> Self {
        self.max_redirects = max_redirects;
        self
    }

    /// Sets the body limit.
    #[must_use]
    pub fn max_body_bytes(mut self, max_body_bytes: u64) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }

    /// Returns `true` when `url`'s origin is trusted.
    #[must_use]
    pub fn is_trusted(&self, url: &Url) -> bool {
        let origin = url.origin();
        self.trusted_origins.contains(&origin)
    }

    /// Returns `true` when `url`'s origin may receive credentials.
    #[must_use]
    pub fn is_credentialed(&self, url: &Url) -> bool {
        let origin = url.origin();
        self.credentialed_origins.contains(&origin)
    }
}

/// Why a URL was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UrlValidationError {
    /// The scheme is not allowed.
    #[error("url scheme \"{scheme}\" is not allowed")]
    SchemeNotAllowed {
        /// The scheme found.
        scheme: String,
    },
    /// The URL embeds a username or password.
    #[error("url must not contain credentials")]
    EmbeddedCredentials,
    /// The URL has no host.
    #[error("url has no host")]
    MissingHost,
    /// The host names a private or local destination.
    #[error("host \"{host}\" is not allowed (private, loopback or reserved address)")]
    PrivateHost {
        /// The offending host.
        host: String,
    },
    /// DNS resolution failed.
    #[error("could not resolve host \"{host}\": {message}")]
    Resolution {
        /// The host.
        host: String,
        /// Resolver message.
        message: String,
    },
    /// The host resolved to a blocked address.
    #[error("host \"{host}\" resolves to blocked address {address}")]
    PrivateAddress {
        /// The host.
        host: String,
        /// The blocked address.
        address: IpAddr,
    },
}

/// A URL that passed validation, with the addresses to connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedUrl {
    /// The validated URL.
    pub url: Url,
    /// Addresses the host resolved to (empty when validation skipped
    /// resolution, for example for trusted origins).
    pub addresses: Vec<SocketAddr>,
}

/// Validates `url` against `policy`, resolving the hostname when needed.
///
/// Trusted origins skip the private-network checks (and resolution);
/// `allow_private_networks` skips only the address checks but still resolves
/// so the connection can be pinned.
///
/// # Errors
///
/// Returns [`UrlValidationError`] describing the first failed rule.
pub async fn validate_url(
    url: &Url,
    policy: &UrlPolicy,
) -> Result<ValidatedUrl, UrlValidationError> {
    if !policy
        .allowed_schemes
        .iter()
        .any(|scheme| scheme.matches(url.scheme()))
    {
        return Err(UrlValidationError::SchemeNotAllowed {
            scheme: url.scheme().to_owned(),
        });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlValidationError::EmbeddedCredentials);
    }
    let host = url.host().ok_or(UrlValidationError::MissingHost)?;
    let host_string = url.host_str().unwrap_or_default().to_owned();
    if policy.is_trusted(url) {
        return Ok(ValidatedUrl {
            url: url.clone(),
            addresses: Vec::new(),
        });
    }
    let check_private = !policy.allow_private_networks;
    let port = url
        .port_or_known_default()
        .unwrap_or(if url.scheme() == "http" { 80 } else { 443 });
    let addresses = match host {
        Host::Ipv4(ip) => vec![SocketAddr::new(IpAddr::V4(ip), port)],
        Host::Ipv6(ip) => vec![SocketAddr::new(IpAddr::V6(ip), port)],
        Host::Domain(domain) => {
            if check_private && is_private_hostname(domain) {
                return Err(UrlValidationError::PrivateHost { host: host_string });
            }
            let resolved = tokio::net::lookup_host((domain, port))
                .await
                .map_err(|error| UrlValidationError::Resolution {
                    host: host_string.clone(),
                    message: error.to_string(),
                })?;
            let addresses: Vec<SocketAddr> = resolved.collect();
            if addresses.is_empty() {
                return Err(UrlValidationError::Resolution {
                    host: host_string,
                    message: "no addresses".to_owned(),
                });
            }
            addresses
        }
    };
    if check_private && let Some(blocked) = addresses.iter().find(|addr| is_private_ip(addr.ip())) {
        return Err(UrlValidationError::PrivateAddress {
            host: host_string,
            address: blocked.ip(),
        });
    }
    Ok(ValidatedUrl {
        url: url.clone(),
        addresses,
    })
}
