//! HMAC signatures over approval requests.
//!
//! Payload: the JSON array `["ferrin-tool-approval-v1", approval_id,
//! tool_call_id, tool_name, input_digest]` where `input_digest` is the
//! canonical-JSON SHA-256 of the input (`ferrin_tool::fingerprint::hash_canonical`).
//! The signature is HMAC-SHA256 in unpadded base64url.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferrin_spec::ApprovalId;
use ferrin_spec::JsonValue;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolName;
use hmac::Hmac;
use hmac::KeyInit;
use hmac::Mac;
use secrecy::ExposeSecret;
use secrecy::SecretBox;
use sha2::Sha256;

/// Domain separator of the signature payload.
pub const SIGNATURE_DOMAIN: &str = "ferrin-tool-approval-v1";

type HmacSha256 = Hmac<Sha256>;

/// The fields covered by a signature.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SignedFields<'a> {
    pub(crate) approval_id: &'a ApprovalId,
    pub(crate) tool_call_id: &'a ToolCallId,
    pub(crate) tool_name: &'a ToolName,
    pub(crate) input: &'a JsonValue,
}

fn payload(fields: SignedFields<'_>) -> String {
    let digest = ferrin_tool::fingerprint::hash_canonical(fields.input);
    serde_json::to_string(&[
        SIGNATURE_DOMAIN,
        fields.approval_id.as_str(),
        fields.tool_call_id.as_str(),
        fields.tool_name.as_str(),
        digest.as_str(),
    ])
    .unwrap_or_default()
}

fn mac(secret: &SecretBox<[u8]>) -> HmacSha256 {
    #[allow(
        clippy::expect_used,
        reason = "HMAC accepts keys of any length; new_from_slice cannot fail"
    )]
    HmacSha256::new_from_slice(secret.expose_secret()).expect("HMAC accepts any key length")
}

/// Signs `fields`.
pub(crate) fn sign(secret: &SecretBox<[u8]>, fields: SignedFields<'_>) -> String {
    let mut mac = mac(secret);
    mac.update(payload(fields).as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

/// Verifies `signature` over `fields` in constant time.
pub(crate) fn verify(secret: &SecretBox<[u8]>, fields: SignedFields<'_>, signature: &str) -> bool {
    let Ok(decoded) = URL_SAFE_NO_PAD.decode(signature) else {
        return false;
    };
    let mut mac = mac(secret);
    mac.update(payload(fields).as_bytes());
    mac.verify_slice(&decoded).is_ok()
}
