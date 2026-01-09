//! Authentication utilities for CoinDCX API
//!
//! Implements HMAC-SHA256 signature generation as per
//! the official CoinDCX API documentation.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Generate HMAC-SHA256 signature for API authentication
///
/// The signature is computed over the JSON body of the request
/// using the API secret as the key.
///
/// # Example
///
/// ```
/// use crypto_strategies::coindcx::auth::sign_request;
///
/// let secret = "your-api-secret";
/// let body = r#"{"timestamp":1234567890}"#;
/// let signature = sign_request(body, secret);
/// ```
pub fn sign_request(body: &str, secret: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(body.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Verify a signature against the expected value
///
/// This is useful for testing or webhook verification.
pub fn verify_signature(body: &str, secret: &str, signature: &str) -> bool {
    let computed = sign_request(body, secret);
    // Use constant-time comparison to prevent timing attacks
    constant_time_eq(computed.as_bytes(), signature.as_bytes())
}

/// Constant-time byte comparison to prevent timing attacks
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// API credentials container
#[derive(Debug, Clone)]
pub struct Credentials {
    api_key: String,
    api_secret: String,
}

impl Credentials {
    /// Create new credentials from API key and secret
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
        }
    }

    /// Create credentials from environment variables
    ///
    /// Looks for `COINDCX_API_KEY` and `COINDCX_API_SECRET`
    pub fn from_env() -> Result<Self, std::env::VarError> {
        let api_key = std::env::var("COINDCX_API_KEY")?;
        let api_secret = std::env::var("COINDCX_API_SECRET")?;
        Ok(Self::new(api_key, api_secret))
    }

    /// Get the API key
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the API secret
    pub fn api_secret(&self) -> &str {
        &self.api_secret
    }

    /// Sign a request body
    pub fn sign(&self, body: &str) -> String {
        sign_request(body, &self.api_secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_request() {
        // Test vector - you can verify this with the CoinDCX API
        let secret = "test_secret";
        let body = r#"{"timestamp":1234567890}"#;

        let signature = sign_request(body, secret);

        // Signature should be a hex string
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
        // SHA256 produces 32 bytes = 64 hex characters
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_sign_consistency() {
        let secret = "test_secret";
        let body = r#"{"timestamp":1234567890}"#;

        let sig1 = sign_request(body, secret);
        let sig2 = sign_request(body, secret);

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_different_secrets_produce_different_signatures() {
        let body = r#"{"timestamp":1234567890}"#;

        let sig1 = sign_request(body, "secret1");
        let sig2 = sign_request(body, "secret2");

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_different_bodies_produce_different_signatures() {
        let secret = "test_secret";

        let sig1 = sign_request(r#"{"timestamp":1234567890}"#, secret);
        let sig2 = sign_request(r#"{"timestamp":1234567891}"#, secret);

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_verify_signature_valid() {
        let secret = "test_secret";
        let body = r#"{"timestamp":1234567890}"#;

        let signature = sign_request(body, secret);
        assert!(verify_signature(body, secret, &signature));
    }

    #[test]
    fn test_verify_signature_invalid() {
        let secret = "test_secret";
        let body = r#"{"timestamp":1234567890}"#;

        let signature = "invalid_signature_that_wont_match";
        assert!(!verify_signature(body, secret, signature));
    }

    #[test]
    fn test_verify_signature_wrong_secret() {
        let body = r#"{"timestamp":1234567890}"#;

        let signature = sign_request(body, "secret1");
        assert!(!verify_signature(body, "secret2", &signature));
    }

    #[test]
    fn test_credentials_new() {
        let creds = Credentials::new("my_key", "my_secret");
        assert_eq!(creds.api_key(), "my_key");
        assert_eq!(creds.api_secret(), "my_secret");
    }

    #[test]
    fn test_credentials_sign() {
        let creds = Credentials::new("my_key", "my_secret");
        let body = r#"{"timestamp":1234567890}"#;

        let sig1 = creds.sign(body);
        let sig2 = sign_request(body, "my_secret");

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_empty_body() {
        let secret = "test_secret";
        let signature = sign_request("", secret);

        // Should still produce a valid signature
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_unicode_body() {
        let secret = "test_secret";
        let body = r#"{"name":"日本語"}"#;

        let signature = sign_request(body, secret);
        assert_eq!(signature.len(), 64);
    }
}
