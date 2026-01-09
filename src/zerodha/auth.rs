//! Authentication utilities for Zerodha Kite API

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct Credentials {
    pub api_key: String,
    pub api_secret: String,
    pub access_token: Option<String>,
}

impl Credentials {
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
            access_token: None,
        }
    }

    pub fn with_access_token(mut self, token: String) -> Self {
        self.access_token = Some(token);
        self
    }
}

/// Generate checksum for login/session APIs
pub fn generate_checksum(api_key: &str, request_token: &str, api_secret: &str) -> String {
    let data = format!("{}{}", api_key, request_token);
    let mut mac =
        HmacSha256::new_from_slice(api_secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(data.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Sign request for authenticated endpoints
pub fn sign_request(method: &str, path: &str, body: &str, api_secret: &str) -> String {
    let message = format!("{}{}{}", method, path, body);
    let mut mac =
        HmacSha256::new_from_slice(api_secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_creation() {
        let creds = Credentials::new("test_key", "test_secret");
        assert_eq!(creds.api_key, "test_key");
        assert_eq!(creds.api_secret, "test_secret");
        assert!(creds.access_token.is_none());
    }

    #[test]
    fn test_credentials_with_token() {
        let creds = Credentials::new("key", "secret").with_access_token("token123".to_string());
        assert_eq!(creds.access_token, Some("token123".to_string()));
    }
}
