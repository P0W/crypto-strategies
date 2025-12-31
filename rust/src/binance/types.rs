//! Binance API types for klines (candlestick) data

use serde::{Deserialize, Serialize};

/// Binance kline/candlestick data
/// API returns an array: [open_time, open, high, low, close, volume, close_time,
///                        quote_volume, trades, taker_buy_base, taker_buy_quote, ignore]
#[derive(Debug, Clone)]
pub struct BinanceKline {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub close_time: i64,
    pub quote_volume: f64,
    pub trades: u64,
    pub taker_buy_base: f64,
    pub taker_buy_quote: f64,
}

impl BinanceKline {
    /// Parse from raw JSON array returned by Binance API
    pub fn from_raw(raw: &[serde_json::Value]) -> Option<Self> {
        if raw.len() < 11 {
            return None;
        }

        Some(BinanceKline {
            open_time: raw[0].as_i64()?,
            open: raw[1].as_str()?.parse().ok()?,
            high: raw[2].as_str()?.parse().ok()?,
            low: raw[3].as_str()?.parse().ok()?,
            close: raw[4].as_str()?.parse().ok()?,
            volume: raw[5].as_str()?.parse().ok()?,
            close_time: raw[6].as_i64()?,
            quote_volume: raw[7].as_str()?.parse().ok()?,
            trades: raw[8].as_u64()?,
            taker_buy_base: raw[9].as_str()?.parse().ok()?,
            taker_buy_quote: raw[10].as_str()?.parse().ok()?,
        })
    }
}

/// Symbol mapping from common names to Binance trading pairs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMapping {
    pub mappings: std::collections::HashMap<String, String>,
}

impl Default for SymbolMapping {
    fn default() -> Self {
        let mut mappings = std::collections::HashMap::new();

        // Common crypto symbols mapped to USDT pairs
        mappings.insert("BTC".to_string(), "BTCUSDT".to_string());
        mappings.insert("ETH".to_string(), "ETHUSDT".to_string());
        mappings.insert("SOL".to_string(), "SOLUSDT".to_string());
        mappings.insert("BNB".to_string(), "BNBUSDT".to_string());
        mappings.insert("XRP".to_string(), "XRPUSDT".to_string());
        mappings.insert("DOGE".to_string(), "DOGEUSDT".to_string());
        mappings.insert("ADA".to_string(), "ADAUSDT".to_string());
        mappings.insert("AVAX".to_string(), "AVAXUSDT".to_string());
        mappings.insert("DOT".to_string(), "DOTUSDT".to_string());
        mappings.insert("MATIC".to_string(), "MATICUSDT".to_string());
        mappings.insert("LINK".to_string(), "LINKUSDT".to_string());
        mappings.insert("UNI".to_string(), "UNIUSDT".to_string());
        mappings.insert("ATOM".to_string(), "ATOMUSDT".to_string());
        mappings.insert("LTC".to_string(), "LTCUSDT".to_string());
        mappings.insert("SHIB".to_string(), "SHIBUSDT".to_string());
        mappings.insert("TRX".to_string(), "TRXUSDT".to_string());
        mappings.insert("NEAR".to_string(), "NEARUSDT".to_string());
        mappings.insert("APT".to_string(), "APTUSDT".to_string());
        mappings.insert("ARB".to_string(), "ARBUSDT".to_string());
        mappings.insert("OP".to_string(), "OPUSDT".to_string());

        SymbolMapping { mappings }
    }
}

impl SymbolMapping {
    /// Get Binance pair from symbol (e.g., "BTC" -> "BTCUSDT", "BTCINR" -> "BTCUSDT")
    pub fn to_binance_pair(&self, symbol: &str) -> String {
        let base = symbol
            .trim()
            .to_uppercase()
            .replace("INR", "")
            .replace("USDT", "");

        self.mappings
            .get(&base)
            .cloned()
            .unwrap_or_else(|| format!("{}USDT", base))
    }
}

/// Valid Binance intervals
pub const BINANCE_INTERVALS: &[&str] = &[
    "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "3d", "1w", "1M",
];

/// Check if interval is valid for Binance
pub fn is_valid_interval(interval: &str) -> bool {
    BINANCE_INTERVALS.contains(&interval)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_mapping() {
        let mapping = SymbolMapping::default();

        assert_eq!(mapping.to_binance_pair("BTC"), "BTCUSDT");
        assert_eq!(mapping.to_binance_pair("BTCINR"), "BTCUSDT");
        assert_eq!(mapping.to_binance_pair("eth"), "ETHUSDT");
        assert_eq!(mapping.to_binance_pair("SOLINR"), "SOLUSDT");
        assert_eq!(mapping.to_binance_pair("UNKNOWN"), "UNKNOWNUSDT");
    }

    #[test]
    fn test_valid_intervals() {
        assert!(is_valid_interval("1h"));
        assert!(is_valid_interval("4h"));
        assert!(is_valid_interval("1d"));
        assert!(!is_valid_interval("2d"));
    }
}
