//! Protocol versions and version-dependent constants.

/// Latest protocol version (stateless, per-request metadata).
pub const LATEST_PROTOCOL_VERSION: &str = "2026-07-28";

/// Latest protocol version with the `initialize` handshake.
pub const LATEST_LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

/// Every protocol version the client accepts from an `initialize` result.
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 5] = [
    LATEST_PROTOCOL_VERSION,
    LATEST_LEGACY_PROTOCOL_VERSION,
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

/// JSON-RPC error codes only modern (2026-07-28) servers produce: header
/// mismatch, missing required client capability, unsupported protocol version.
pub const MODERN_PROTOCOL_ERROR_CODES: [i64; 3] = [-32020, -32021, -32022];

/// `_meta` key carrying the protocol version of a request.
pub const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// `_meta` key carrying the client implementation of a request.
pub const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";

/// `_meta` key carrying the client capabilities of a request.
pub const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// `_meta` key carrying the server implementation of a result.
pub const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// Generation of the protocol a server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProtocolEra {
    /// `initialize` handshake, sessions, inbound streams (2025-11-25 and
    /// earlier).
    Legacy,
    /// Stateless per-request metadata (2026-07-28).
    Modern,
}

impl ProtocolEra {
    /// The era a version string belongs to.
    #[must_use]
    pub fn of_version(version: &str) -> Self {
        if version == LATEST_PROTOCOL_VERSION {
            Self::Modern
        } else {
            Self::Legacy
        }
    }

    /// Whether this is the modern era.
    #[must_use]
    pub fn is_modern(self) -> bool {
        matches!(self, Self::Modern)
    }
}

/// Whether `version` is in [`SUPPORTED_PROTOCOL_VERSIONS`].
#[must_use]
pub fn is_supported_version(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}
