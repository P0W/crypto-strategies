//! Binance API client for fetching historical kline (candlestick) data
//!
//! No API key required for public market data endpoints.
//!
//! # Example
//! ```no_run
//! use crypto_strategies::binance::BinanceClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = BinanceClient::new();
//!     let klines = client.get_klines("BTCUSDT", "1h", None, None, Some(100)).await?;
//!     println!("Fetched {} klines", klines.len());
//!     Ok(())
//! }
//! ```

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use std::time::Duration as StdDuration;
use tracing::{debug, info, warn};

use super::types::{BinanceKline, SymbolMapping};

/// Base URL for Binance API
const BINANCE_API_BASE: &str = "https://api.binance.com/api/v3";

/// Maximum klines per request (Binance limit)
const MAX_KLINES_PER_REQUEST: u32 = 1000;

/// Rate limit delay between requests (ms)
const RATE_LIMIT_DELAY_MS: u64 = 100;

/// Binance API client
#[derive(Debug, Clone)]
pub struct BinanceClient {
    client: Client,
    symbol_mapping: SymbolMapping,
}

impl Default for BinanceClient {
    fn default() -> Self {
        Self::new()
    }
}

impl BinanceClient {
    /// Create a new Binance client
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(StdDuration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        BinanceClient {
            client,
            symbol_mapping: SymbolMapping::default(),
        }
    }

    /// Create with custom symbol mapping
    pub fn with_mapping(symbol_mapping: SymbolMapping) -> Self {
        let client = Client::builder()
            .timeout(StdDuration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        BinanceClient {
            client,
            symbol_mapping,
        }
    }

    /// Get the symbol mapping
    pub fn symbol_mapping(&self) -> &SymbolMapping {
        &self.symbol_mapping
    }

    /// Convert symbol to Binance pair format
    pub fn to_binance_pair(&self, symbol: &str) -> String {
        self.symbol_mapping.to_binance_pair(symbol)
    }

    /// Fetch klines (candlestick data) from Binance
    ///
    /// # Arguments
    /// * `symbol` - Binance trading pair (e.g., "BTCUSDT")
    /// * `interval` - Timeframe (e.g., "1h", "4h", "1d")
    /// * `start_time` - Optional start time in milliseconds
    /// * `end_time` - Optional end time in milliseconds
    /// * `limit` - Optional number of klines to fetch (max 1000)
    pub async fn get_klines(
        &self,
        symbol: &str,
        interval: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<BinanceKline>> {
        let url = format!("{}/klines", BINANCE_API_BASE);

        let mut params = vec![
            ("symbol", symbol.to_string()),
            ("interval", interval.to_string()),
        ];

        if let Some(start) = start_time {
            params.push(("startTime", start.to_string()));
        }

        if let Some(end) = end_time {
            params.push(("endTime", end.to_string()));
        }

        let limit = limit
            .unwrap_or(MAX_KLINES_PER_REQUEST)
            .min(MAX_KLINES_PER_REQUEST);
        params.push(("limit", limit.to_string()));

        debug!(
            "Fetching klines: symbol={}, interval={}, limit={}",
            symbol, interval, limit
        );

        let response = self
            .client
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("Failed to send request to Binance")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Binance API error {}: {}", status, body);
        }

        let raw_data: Vec<Vec<serde_json::Value>> = response
            .json()
            .await
            .context("Failed to parse Binance response")?;

        let klines: Vec<BinanceKline> = raw_data
            .iter()
            .filter_map(|row| BinanceKline::from_raw(row))
            .collect();

        Ok(klines)
    }

    /// Fetch full historical data by paginating through multiple requests
    ///
    /// # Arguments
    /// * `symbol` - Trading pair (will be converted to Binance format)
    /// * `interval` - Timeframe (e.g., "1h", "4h", "1d")
    /// * `days_back` - Number of days of history to fetch
    pub async fn fetch_full_history(
        &self,
        symbol: &str,
        interval: &str,
        days_back: u32,
    ) -> Result<Vec<BinanceKline>> {
        let binance_symbol = self.to_binance_pair(symbol);

        let end_time = Utc::now().timestamp_millis();
        let start_time = (Utc::now() - Duration::days(days_back as i64)).timestamp_millis();

        info!(
            "Fetching {} {} data from Binance ({} days back)",
            binance_symbol, interval, days_back
        );

        let mut all_klines = Vec::new();
        let mut current_start = start_time;

        while current_start < end_time {
            match self
                .get_klines(
                    &binance_symbol,
                    interval,
                    Some(current_start),
                    Some(end_time),
                    Some(MAX_KLINES_PER_REQUEST),
                )
                .await
            {
                Ok(klines) => {
                    if klines.is_empty() {
                        break;
                    }

                    // Move start time to after last candle
                    if let Some(last) = klines.last() {
                        current_start = last.open_time + 1;
                    }

                    all_klines.extend(klines);

                    // Rate limiting
                    tokio::time::sleep(StdDuration::from_millis(RATE_LIMIT_DELAY_MS)).await;
                }
                Err(e) => {
                    warn!("Error fetching klines: {}", e);
                    break;
                }
            }
        }

        // Sort and deduplicate
        all_klines.sort_by_key(|k| k.open_time);
        all_klines.dedup_by_key(|k| k.open_time);

        info!(
            "Fetched {} candles for {} {}",
            all_klines.len(),
            binance_symbol,
            interval
        );

        Ok(all_klines)
    }

    /// Check server connectivity
    pub async fn ping(&self) -> Result<bool> {
        let url = format!("{}/ping", BINANCE_API_BASE);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }

    /// Get server time
    pub async fn get_server_time(&self) -> Result<DateTime<Utc>> {
        let url = format!("{}/time", BINANCE_API_BASE);
        let response = self.client.get(&url).send().await?;

        #[derive(serde::Deserialize)]
        struct TimeResponse {
            #[serde(rename = "serverTime")]
            server_time: i64,
        }

        let time_resp: TimeResponse = response.json().await?;
        DateTime::from_timestamp_millis(time_resp.server_time).context("Invalid server time")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = BinanceClient::new();
        assert_eq!(client.to_binance_pair("BTC"), "BTCUSDT");
        assert_eq!(client.to_binance_pair("BTCINR"), "BTCUSDT");
    }
}
