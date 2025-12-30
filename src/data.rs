//! Data loading and management
//!
//! Handles loading OHLCV data from CSV files and live data fetching from exchange APIs.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::path::Path;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::{Candle, Symbol};

// =============================================================================
// CSV Data Loading
// =============================================================================

/// Load OHLCV data from CSV file
pub fn load_csv(path: impl AsRef<Path>) -> Result<Vec<Candle>> {
    let mut reader = csv::Reader::from_path(path.as_ref())
        .context("Failed to open CSV file")?;
    
    let mut candles = Vec::new();
    
    for (row_idx, result) in reader.records().enumerate() {
        let record = result.context(format!("Failed to read row {}", row_idx + 1))?;
        
        let dt_str = record.get(0).context("Missing datetime column")?;
        let datetime = dt_str
            .parse::<DateTime<Utc>>()
            .or_else(|_| {
                // Try parsing without timezone and assume UTC
                chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S")
                    .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
            })
            .context(format!("Failed to parse datetime: {}", dt_str))?;
        
        let open: f64 = record.get(1)
            .context("Missing open column")?
            .parse()
            .context("Failed to parse open")?;
        let high: f64 = record.get(2)
            .context("Missing high column")?
            .parse()
            .context("Failed to parse high")?;
        let low: f64 = record.get(3)
            .context("Missing low column")?
            .parse()
            .context("Failed to parse low")?;
        let close: f64 = record.get(4)
            .context("Missing close column")?
            .parse()
            .context("Failed to parse close")?;
        let volume: f64 = record.get(5)
            .context("Missing volume column")?
            .parse()
            .context("Failed to parse volume")?;
        
        candles.push(Candle {
            datetime,
            open,
            high,
            low,
            close,
            volume,
        });
    }
    
    Ok(candles)
}

/// Load data for multiple symbols from CSV files
pub fn load_multi_symbol(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframe: &str,
) -> Result<HashMap<Symbol, Vec<Candle>>> {
    let mut data = HashMap::new();

    for symbol in symbols {
        let filename = format!("{}_{}.csv", symbol.as_str(), timeframe);
        let path = data_dir.as_ref().join(&filename);

        if !path.exists() {
            warn!("Data file not found: {}", path.display());
            continue;
        }

        let candles = load_csv(&path).context(format!("Failed to load data for {}", symbol))?;

        info!("Loaded {} candles for {}", candles.len(), symbol);
        data.insert(symbol.clone(), candles);
    }

    if data.is_empty() {
        anyhow::bail!("No data loaded for any symbol");
    }

    Ok(data)
}

// =============================================================================
// Live Data Fetching
// =============================================================================

/// Live data fetcher for real-time market data
pub struct LiveDataFetcher {
    client: reqwest::Client,
    base_url: String,
    rate_limit_delay: std::time::Duration,
}

impl LiveDataFetcher {
    /// Create a new live data fetcher
    pub fn new() -> Self {
        Self::with_config("https://api.coindcx.com", 100)
    }

    /// Create with custom configuration
    pub fn with_config(base_url: &str, rate_limit_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to build HTTP client");

        LiveDataFetcher {
            client,
            base_url: base_url.to_string(),
            rate_limit_delay: std::time::Duration::from_millis(rate_limit_ms),
        }
    }

    /// Fetch current ticker data for a symbol
    pub async fn fetch_ticker(&self, symbol: &str) -> Result<TickerData> {
        let url = format!("{}/exchange/ticker", self.base_url);

        let response: Vec<TickerResponse> = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch ticker")?
            .json()
            .await
            .context("Failed to parse ticker response")?;

        let ticker = response
            .into_iter()
            .find(|t| t.market == symbol)
            .context(format!("Ticker not found for {}", symbol))?;

        Ok(TickerData {
            symbol: symbol.to_string(),
            last_price: ticker.last_price.parse().unwrap_or(0.0),
            bid: ticker.bid.parse().unwrap_or(0.0),
            ask: ticker.ask.parse().unwrap_or(0.0),
            volume: ticker.volume.parse().unwrap_or(0.0),
            timestamp: Utc::now(),
        })
    }

    /// Fetch current tickers for multiple symbols
    pub async fn fetch_tickers(&self, symbols: &[&str]) -> Result<HashMap<String, TickerData>> {
        let url = format!("{}/exchange/ticker", self.base_url);

        let response: Vec<TickerResponse> = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch tickers")?
            .json()
            .await
            .context("Failed to parse tickers response")?;

        let symbol_set: std::collections::HashSet<&str> = symbols.iter().copied().collect();

        let mut result = HashMap::new();
        for ticker in response {
            if symbol_set.contains(ticker.market.as_str()) {
                result.insert(
                    ticker.market.clone(),
                    TickerData {
                        symbol: ticker.market,
                        last_price: ticker.last_price.parse().unwrap_or(0.0),
                        bid: ticker.bid.parse().unwrap_or(0.0),
                        ask: ticker.ask.parse().unwrap_or(0.0),
                        volume: ticker.volume.parse().unwrap_or(0.0),
                        timestamp: Utc::now(),
                    },
                );
            }
        }

        Ok(result)
    }

    /// Fetch historical candles (if available from API)
    pub async fn fetch_candles(
        &self,
        symbol: &str,
        interval: &str,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        // CoinDCX candles endpoint
        let url = format!(
            "{}/exchange/v1/markets/{}/candles?interval={}&limit={}",
            self.base_url, symbol, interval, limit
        );

        let response: Vec<CandleResponse> = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch candles")?
            .json()
            .await
            .context("Failed to parse candles response")?;

        let candles: Vec<Candle> = response
            .into_iter()
            .map(|c| Candle {
                datetime: DateTime::from_timestamp(c.time / 1000, 0).unwrap_or_else(Utc::now),
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect();

        Ok(candles)
    }

    /// Apply rate limiting
    pub async fn rate_limit(&self) {
        sleep(self.rate_limit_delay).await;
    }
}

impl Default for LiveDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Data Cache
// =============================================================================

/// In-memory cache for candle data with TTL
pub struct CandleCache {
    data: HashMap<Symbol, CachedCandles>,
    max_candles: usize,
    ttl: Duration,
}

struct CachedCandles {
    candles: Vec<Candle>,
    last_updated: DateTime<Utc>,
}

impl CandleCache {
    /// Create a new candle cache
    pub fn new(max_candles: usize, ttl_seconds: i64) -> Self {
        CandleCache {
            data: HashMap::new(),
            max_candles,
            ttl: Duration::seconds(ttl_seconds),
        }
    }

    /// Get candles for a symbol (returns None if stale or missing)
    pub fn get(&self, symbol: &Symbol) -> Option<&Vec<Candle>> {
        self.data.get(symbol).and_then(|cached| {
            if Utc::now() - cached.last_updated < self.ttl {
                Some(&cached.candles)
            } else {
                None
            }
        })
    }

    /// Get candles even if stale
    pub fn get_stale(&self, symbol: &Symbol) -> Option<&Vec<Candle>> {
        self.data.get(symbol).map(|c| &c.candles)
    }

    /// Update candles for a symbol
    pub fn update(&mut self, symbol: Symbol, candles: Vec<Candle>) {
        let mut candles = candles;

        // Keep only the most recent candles
        if candles.len() > self.max_candles {
            candles = candles.split_off(candles.len() - self.max_candles);
        }

        self.data.insert(
            symbol,
            CachedCandles {
                candles,
                last_updated: Utc::now(),
            },
        );
    }

    /// Append a single candle
    pub fn append(&mut self, symbol: &Symbol, candle: Candle) {
        if let Some(cached) = self.data.get_mut(symbol) {
            // Check if this is a new candle or update to existing
            if let Some(last) = cached.candles.last_mut() {
                if last.datetime == candle.datetime {
                    // Update existing candle
                    *last = candle;
                } else {
                    // New candle
                    cached.candles.push(candle);
                    if cached.candles.len() > self.max_candles {
                        cached.candles.remove(0);
                    }
                }
            } else {
                cached.candles.push(candle);
            }
            cached.last_updated = Utc::now();
        } else {
            self.data.insert(
                symbol.clone(),
                CachedCandles {
                    candles: vec![candle],
                    last_updated: Utc::now(),
                },
            );
        }
    }

    /// Check if data needs refresh
    pub fn needs_refresh(&self, symbol: &Symbol) -> bool {
        match self.data.get(symbol) {
            Some(cached) => Utc::now() - cached.last_updated >= self.ttl,
            None => true,
        }
    }

    /// Clear all cached data
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let total_candles: usize = self.data.values().map(|c| c.candles.len()).sum();
        let stale_count = self
            .data
            .values()
            .filter(|c| Utc::now() - c.last_updated >= self.ttl)
            .count();

        CacheStats {
            symbol_count: self.data.len(),
            total_candles,
            stale_count,
        }
    }
}

// =============================================================================
// Data Types
// =============================================================================

/// Ticker data from exchange
#[derive(Debug, Clone)]
pub struct TickerData {
    pub symbol: String,
    pub last_price: f64,
    pub bid: f64,
    pub ask: f64,
    pub volume: f64,
    pub timestamp: DateTime<Utc>,
}

impl TickerData {
    /// Convert ticker to a candle (single point)
    pub fn to_candle(&self) -> Candle {
        Candle {
            datetime: self.timestamp,
            open: self.last_price,
            high: self.last_price,
            low: self.last_price,
            close: self.last_price,
            volume: self.volume,
        }
    }

    /// Calculate spread percentage
    pub fn spread_pct(&self) -> f64 {
        if self.bid > 0.0 {
            ((self.ask - self.bid) / self.bid) * 100.0
        } else {
            0.0
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub symbol_count: usize,
    pub total_candles: usize,
    pub stale_count: usize,
}

// API response types
#[derive(Debug, serde::Deserialize)]
struct TickerResponse {
    market: String,
    last_price: String,
    bid: String,
    ask: String,
    volume: String,
}

#[derive(Debug, serde::Deserialize)]
struct CandleResponse {
    time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

// =============================================================================
// Data Validation
// =============================================================================

/// Validate candle data for consistency
pub fn validate_candles(candles: &[Candle]) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if candles.is_empty() {
        errors.push("No candles provided".to_string());
        return ValidationResult { errors, warnings };
    }

    for (i, candle) in candles.iter().enumerate() {
        // Check OHLC consistency
        if candle.high < candle.low {
            errors.push(format!(
                "Candle {}: high ({}) < low ({})",
                i, candle.high, candle.low
            ));
        }
        if candle.high < candle.open || candle.high < candle.close {
            warnings.push(format!("Candle {}: high ({}) not highest", i, candle.high));
        }
        if candle.low > candle.open || candle.low > candle.close {
            warnings.push(format!("Candle {}: low ({}) not lowest", i, candle.low));
        }

        // Check for zero/negative values
        if candle.close <= 0.0 {
            errors.push(format!(
                "Candle {}: invalid close price ({})",
                i, candle.close
            ));
        }
        if candle.volume < 0.0 {
            errors.push(format!("Candle {}: negative volume ({})", i, candle.volume));
        }

        // Check chronological order
        if i > 0 && candle.datetime <= candles[i - 1].datetime {
            warnings.push(format!(
                "Candle {}: not chronological ({} <= {})",
                i,
                candle.datetime,
                candles[i - 1].datetime
            ));
        }
    }

    ValidationResult { errors, warnings }
}

/// Result of data validation
#[derive(Debug)]
pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn log(&self) {
        for error in &self.errors {
            tracing::error!("Data validation error: {}", error);
        }
        for warning in &self.warnings {
            tracing::warn!("Data validation warning: {}", warning);
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_csv() {
        // This would need actual test data
        // Just ensuring the module compiles for now
    }

    #[test]
    fn test_candle_cache() {
        let mut cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        let candle = Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 102.0,
            volume: 1000.0,
        };

        cache.append(&symbol, candle.clone());

        assert!(cache.get(&symbol).is_some());
        assert_eq!(cache.get(&symbol).unwrap().len(), 1);
    }

    #[test]
    fn test_validate_candles() {
        let candles = vec![Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 102.0,
            volume: 1000.0,
        }];

        let result = validate_candles(&candles);
        assert!(result.is_valid());
    }

    #[test]
    fn test_validate_candles_invalid() {
        let candles = vec![Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 90.0, // Invalid: high < low
            low: 95.0,
            close: 102.0,
            volume: 1000.0,
        }];

        let result = validate_candles(&candles);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_ticker_spread() {
        let ticker = TickerData {
            symbol: "BTCINR".to_string(),
            last_price: 100.0,
            bid: 99.0,
            ask: 101.0,
            volume: 1000.0,
            timestamp: Utc::now(),
        };

        let spread = ticker.spread_pct();
        assert!((spread - 2.02).abs() < 0.01); // ~2% spread
    }
}
