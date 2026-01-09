//! Zerodha-specific error types

use std::fmt;

#[derive(Debug)]
pub enum ZerodhaError {
    ApiError(String),
    AuthError(String),
    NetworkError(String),
    ParseError(String),
    RateLimitExceeded,
    CircuitBreakerOpen,
}

impl fmt::Display for ZerodhaError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ApiError(msg) => write!(f, "API error: {}", msg),
            Self::AuthError(msg) => write!(f, "Auth error: {}", msg),
            Self::NetworkError(msg) => write!(f, "Network error: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::RateLimitExceeded => write!(f, "Rate limit exceeded"),
            Self::CircuitBreakerOpen => write!(f, "Circuit breaker is open"),
        }
    }
}

impl std::error::Error for ZerodhaError {}

impl From<reqwest::Error> for ZerodhaError {
    fn from(err: reqwest::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

pub type ZerodhaResult<T> = Result<T, ZerodhaError>;
