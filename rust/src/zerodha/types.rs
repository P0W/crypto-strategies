//! Zerodha API data structures

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Historical OHLCV candle data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub datetime: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Real-time quote data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    pub instrument_token: u64,
    pub last_price: f64,
    pub volume: f64,
    pub buy_quantity: u64,
    pub sell_quantity: u64,
    pub ohlc: OHLC,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OHLC {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Order data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub exchange: String,
    pub tradingsymbol: String,
    pub transaction_type: String, // BUY or SELL
    pub order_type: String, // MARKET, LIMIT, etc.
    pub quantity: i32,
    pub price: Option<f64>,
    pub trigger_price: Option<f64>,
    pub status: String,
    pub filled_quantity: i32,
    pub average_price: Option<f64>,
}

/// Position data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub exchange: String,
    pub tradingsymbol: String,
    pub quantity: i32,
    pub average_price: f64,
    pub last_price: f64,
    pub pnl: f64,
}

/// Holding data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Holding {
    pub tradingsymbol: String,
    pub exchange: String,
    pub quantity: i32,
    pub average_price: f64,
    pub last_price: f64,
    pub pnl: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candle_creation() {
        let candle = Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 105.0,
            low: 99.0,
            close: 103.0,
            volume: 1000.0,
        };
        assert_eq!(candle.open, 100.0);
    }
}
