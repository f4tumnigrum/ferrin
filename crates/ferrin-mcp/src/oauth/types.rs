//! OAuth 2.1 data types (RFC 6749, RFC 7591, RFC 8414, RFC 9728).

use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

/// Tokens issued by the authorization server.
#[derive(Clone)]
pub struct OAuthTokens {
    /// Access token.
    pub access_token: SecretString,
    /// Token type (`Bearer`).
    pub token_type: String,
    /// Lifetime in seconds.
    pub expires_in: Option<u64>,
    /// Granted scope.
    pub scope: Option<String>,
    /// Refresh token.
    pub refresh_token: Option<SecretString>,
}

impl std::fmt::Debug for OAuthTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokens")
            .field("access_token", &"<redacted>")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Deserialize)]
struct RawTokens {
    access_token: String,
    #[serde(default = "default_token_type")]
    token_type: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

fn default_token_type() -> String {
    "Bearer".to_owned()
}

impl OAuthTokens {
    /// Tokens with only an access token.
    #[must_use]
    pub fn bearer(access_token: impl Into<String>) -> Self {
        Self {
            access_token: SecretString::from(access_token.into()),
            token_type: default_token_type(),
            expires_in: None,
            scope: None,
            refresh_token: None,
        }
    }

    /// Parses a token endpoint response.
    ///
    /// # Errors
    ///
    /// Returns the deserialization error when `access_token` is missing.
    pub fn from_json(value: JsonValue) -> Result<Self, serde_json::Error> {
        let raw: RawTokens = serde_json::from_value(value)?;
        Ok(Self {
            access_token: SecretString::from(raw.access_token),
            token_type: raw.token_type,
            expires_in: raw.expires_in,
            scope: raw.scope,
            refresh_token: raw.refresh_token.map(SecretString::from),
        })
    }

    /// Serializes the tokens **including the secrets** for persistence.
    #[must_use]
    pub fn expose_to_json(&self) -> JsonValue {
        let mut object = JsonObject::new();
        object.insert(
            "access_token".to_owned(),
            JsonValue::from(self.access_token.expose_secret()),
        );
        object.insert(
            "token_type".to_owned(),
            JsonValue::from(self.token_type.as_str()),
        );
        if let Some(expires_in) = self.expires_in {
            object.insert("expires_in".to_owned(), JsonValue::from(expires_in));
        }
        if let Some(scope) = &self.scope {
            object.insert("scope".to_owned(), JsonValue::from(scope.as_str()));
        }
        if let Some(refresh_token) = &self.refresh_token {
            object.insert(
                "refresh_token".to_owned(),
                JsonValue::from(refresh_token.expose_secret()),
            );
        }
        JsonValue::Object(object)
    }
}

/// Client metadata used for dynamic registration (RFC 7591).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthClientMetadata {
    /// Redirect URIs.
    pub redirect_uris: Vec<Url>,
    /// Token endpoint authentication method (`none`, `client_secret_basic`,
    /// `client_secret_post`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,
    /// Grant types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    /// Response types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,
    /// Human-readable client name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// Client home page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<Url>,
    /// Requested scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Further fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Registered client credentials.
#[derive(Clone)]
pub struct OAuthClientInformation {
    /// Client id.
    pub client_id: String,
    /// Client secret, for confidential clients.
    pub client_secret: Option<SecretString>,
    /// Issue time (seconds since the epoch).
    pub client_id_issued_at: Option<u64>,
    /// Secret expiry (seconds since the epoch; `0` means never).
    pub client_secret_expires_at: Option<u64>,
}

impl std::fmt::Debug for OAuthClientInformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthClientInformation")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("client_id_issued_at", &self.client_id_issued_at)
            .field("client_secret_expires_at", &self.client_secret_expires_at)
            .finish()
    }
}

#[derive(Deserialize)]
struct RawClientInformation {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    client_id_issued_at: Option<u64>,
    #[serde(default)]
    client_secret_expires_at: Option<u64>,
}

impl OAuthClientInformation {
    /// A public client.
    #[must_use]
    pub fn public(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            client_id_issued_at: None,
            client_secret_expires_at: None,
        }
    }

    /// Parses a registration response.
    ///
    /// # Errors
    ///
    /// Returns the deserialization error when `client_id` is missing.
    pub fn from_json(value: JsonValue) -> Result<Self, serde_json::Error> {
        let raw: RawClientInformation = serde_json::from_value(value)?;
        Ok(Self {
            client_id: raw.client_id,
            client_secret: raw.client_secret.map(SecretString::from),
            client_id_issued_at: raw.client_id_issued_at,
            client_secret_expires_at: raw.client_secret_expires_at,
        })
    }

    /// Serializes the information **including the secret** for persistence.
    #[must_use]
    pub fn expose_to_json(&self) -> JsonValue {
        let mut object = JsonObject::new();
        object.insert(
            "client_id".to_owned(),
            JsonValue::from(self.client_id.as_str()),
        );
        if let Some(secret) = &self.client_secret {
            object.insert(
                "client_secret".to_owned(),
                JsonValue::from(secret.expose_secret()),
            );
        }
        if let Some(issued) = self.client_id_issued_at {
            object.insert("client_id_issued_at".to_owned(), JsonValue::from(issued));
        }
        if let Some(expires) = self.client_secret_expires_at {
            object.insert(
                "client_secret_expires_at".to_owned(),
                JsonValue::from(expires),
            );
        }
        JsonValue::Object(object)
    }
}

/// Authorization server metadata (RFC 8414 / OpenID Connect Discovery).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationServerMetadata {
    /// Issuer identifier.
    pub issuer: String,
    /// Authorization endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<Url>,
    /// Token endpoint.
    pub token_endpoint: Url,
    /// Dynamic registration endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<Url>,
    /// Supported scopes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    /// Supported response types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_types_supported: Option<Vec<String>>,
    /// Supported grant types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<String>>,
    /// Supported token endpoint authentication methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    /// Supported PKCE methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_challenge_methods_supported: Option<Vec<String>>,
    /// Further fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Protected resource metadata (RFC 9728).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedResourceMetadata {
    /// Resource identifier.
    pub resource: Url,
    /// Authorization servers protecting the resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_servers: Option<Vec<Url>>,
    /// Scopes the resource understands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    /// Further fields.
    #[serde(flatten)]
    pub extra: JsonObject,
}

/// Outcome of [`super::auth`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthResult {
    /// Valid tokens are stored; the request can be retried.
    Authorized,
    /// The user was redirected to the authorization server; call
    /// [`super::auth`] again with the authorization code.
    Redirect,
}

/// Which stored credentials to discard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InvalidateScope {
    /// Client information, tokens and code verifier.
    All,
    /// Client information only.
    Client,
    /// Tokens only.
    Tokens,
    /// Code verifier only.
    Verifier,
}

/// Parameters of a `WWW-Authenticate: Bearer` challenge.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WwwAuthenticateParams {
    /// `resource_metadata` URL.
    pub resource_metadata_url: Option<Url>,
    /// `scope` the resource requires.
    pub scope: Option<String>,
}

/// Parses the `resource_metadata` and `scope` parameters of a
/// `WWW-Authenticate` header.
#[must_use]
pub fn extract_www_authenticate_params(headers: &ferrin_spec::Headers) -> WwwAuthenticateParams {
    let Some(value) = headers.get_str("www-authenticate") else {
        return WwwAuthenticateParams::default();
    };
    let mut params = WwwAuthenticateParams::default();
    for (name, value) in challenge_params(value) {
        match name.as_str() {
            "resource_metadata" => params.resource_metadata_url = Url::parse(&value).ok(),
            "scope" => params.scope = Some(value),
            _ => {}
        }
    }
    params
}

/// Splits `Bearer a="x", b=y` into `(name, value)` pairs, handling quoted
/// values with escapes.
fn challenge_params(header: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut rest = header.trim_start();
    if let Some(after_scheme) = rest
        .strip_prefix("Bearer")
        .or_else(|| rest.strip_prefix("bearer"))
        && after_scheme.starts_with(|c: char| c.is_whitespace())
    {
        rest = after_scheme.trim_start();
    }
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
            chars.next();
        }
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() || c == ',' {
                break;
            }
            name.push(c);
            chars.next();
        }
        if name.is_empty() {
            break;
        }
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek() != Some(&'=') {
            params.push((name.to_ascii_lowercase(), String::new()));
            continue;
        }
        chars.next();
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(escaped) = chars.next() {
                            value.push(escaped);
                        }
                    }
                    '"' => break,
                    other => value.push(other),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' || c.is_whitespace() {
                    break;
                }
                value.push(c);
                chars.next();
            }
        }
        params.push((name.to_ascii_lowercase(), value));
    }
    params
}
