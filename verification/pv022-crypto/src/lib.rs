//! PV-022: hmac 0.13 / sha2 0.11 / subtle 2.6 / secrecy 0.10 interplay for
//! tool approval signatures.

use hmac::Hmac;
use hmac::KeyInit;
use hmac::Mac;
use secrecy::ExposeSecret;
use secrecy::SecretBox;
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub struct ApprovalSigner {
    key: SecretBox<[u8]>,
}

impl ApprovalSigner {
    pub fn new(key: Vec<u8>) -> Self {
        Self { key: SecretBox::from(key.into_boxed_slice()) }
    }

    pub fn sign(&self, payload: &[u8]) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(self.key.expose_secret()).expect("hmac accepts any key length");
        mac.update(payload);
        mac.finalize().into_bytes().into()
    }

    /// Constant-time verification through `Mac::verify_slice` (hmac's own
    /// constant-time path).
    pub fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(self.key.expose_secret()).expect("hmac accepts any key length");
        mac.update(payload);
        mac.verify_slice(signature).is_ok()
    }

    /// Constant-time comparison via `subtle` for pre-computed digests.
    pub fn digests_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
        a.ct_eq(b).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let signer = ApprovalSigner::new(b"secret-key".to_vec());
        let sig = signer.sign(b"ferrin-tool-approval-v1\0call-1\0{\"a\":1}");
        assert!(signer.verify(b"ferrin-tool-approval-v1\0call-1\0{\"a\":1}", &sig));
        assert!(!signer.verify(b"ferrin-tool-approval-v1\0call-2\0{\"a\":1}", &sig));
        assert!(ApprovalSigner::digests_equal(&sig, &sig));
        println!("hmac-sha256 ok, key type = SecretBox<[u8]>, verify_slice available");
    }
}
