//! Zerodha Kite API Integration
//!
//! Production-grade client for Zerodha Kite Connect API

pub mod auth;
pub mod client;
pub mod error;
pub mod types;

pub use auth::{sign_request, Credentials};
pub use client::{ClientConfig, ZerodhaClient};
pub use error::{ZerodhaError, ZerodhaResult};
pub use types::*;

pub const API_BASE_URL: &str = "https://api.kite.trade";

pub fn to_kite_interval(timeframe: &str) -> Option<&'static str> {
    match timeframe {
        "1m" => Some("minute"),
        "5m" => Some("5minute"),
        "15m" => Some("15minute"),
        "30m" => Some("30minute"),
        "1h" => Some("60minute"),
        "1d" => Some("day"),
        _ => None,
    }
}
