//! Data loading and management
//!
//! Handles loading OHLCV data from CSV files and live data fetching from exchange APIs.
//! Similar to Python's data_fetcher.py

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration as StdDuration;
use tracing::{info, warn};

use crate::{Candle, Symbol};

// =============================================================================
// Constants
// =============================================================================

const COINDCX_CANDLES_URL: &str = "https://public.coindcx.com/market_data/candles";
const COINDCX_MARKETS_URL: &str = "https://api.coindcx.com/exchange/v1/markets_details";
const REQUEST_DELAY_MS: u64 = 500;

/// Valid intervals for CoinDCX
pub const INTERVALS: &[&str] = &[
    "1m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "1d", "3d", "1w", "1M",
];

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
// CoinDCX Data Fetcher (like Python's CoinDCXDataFetcher class)
// =============================================================================

/// Fetch historical OHLCV data from CoinDCX public API
/// Equivalent to Python's CoinDCXDataFetcher class
pub struct CoinDCXDataFetcher {
    client: reqwest::blocking::Client,
    pub data_dir: PathBuf,
    request_delay: StdDuration,
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

#[derive(Debug, serde::Deserialize)]
struct MarketDetails {
    pair: Option<String>,
    base_currency_short_name: Option<String>,
    status: Option<String>,
}

impl CoinDCXDataFetcher {
    /// Create a new data fetcher
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).ok();

        let client = reqwest::blocking::Client::builder()
            .timeout(StdDuration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            data_dir,
            request_delay: StdDuration::from_millis(REQUEST_DELAY_MS),
        }
    }

    /// Convert symbol to CoinDCX pair format: BTCINR -> I-BTC_INR
    pub fn to_pair(symbol: &str) -> String {
        if symbol.ends_with("INR") {
            let base = &symbol[..symbol.len() - 3];
            format!("I-{}_INR", base)
        } else {
            format!("I-{}_INR", symbol)
        }
    }

    /// Get list of available INR trading pairs
    pub fn list_inr_pairs(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(COINDCX_MARKETS_URL)
            .send()
            .context("Failed to fetch markets")?;

        let markets: Vec<MarketDetails> = response.json().context("Failed to parse markets")?;

        let pairs: Vec<String> = markets
            .into_iter()
            .filter(|m| {
                m.base_currency_short_name.as_deref() == Some("INR")
                    && m.status.as_deref() == Some("active")
            })
            .filter_map(|m| m.pair)
            .collect();

        Ok(pairs)
    }

    /// Fetch candles from CoinDCX API
    pub fn fetch_candles(
        &self,
        pair: &str,
        interval: &str,
        end_time: Option<DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<Candle>> {
        let mut url = format!(
            "{}?pair={}&interval={}&limit={}",
            COINDCX_CANDLES_URL, pair, interval, limit.min(1000)
        );

        if let Some(end) = end_time {
            url.push_str(&format!("&endTime={}", end.timestamp_millis()));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .context("Failed to send request")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned status: {}", response.status());
        }

        let candles: Vec<CandleResponse> = response.json().context("Failed to parse response")?;

        let candles: Vec<Candle> = candles
            .into_iter()
            .map(|c| Candle {
                datetime: DateTime::from_timestamp_millis(c.time).unwrap_or_else(Utc::now),
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
            })
            .collect();

        Ok(candles)
    }

    /// Fetch full historical data by making multiple API calls
    pub fn fetch_full_history(
        &self,
        pair: &str,
        interval: &str,
        days_back: u32,
    ) -> Result<Vec<Candle>> {
        let end_time = Utc::now();
        let start_time = end_time - Duration::days(days_back as i64);

        info!(
            "Fetching {} {} data from {} to {}",
            pair, interval, start_time, end_time
        );

        let mut all_candles: Vec<Candle> = Vec::new();
        let mut current_end = end_time;
        let mut last_oldest_time: Option<i64> = None;

        while current_end > start_time {
            let candles = self.fetch_candles(pair, interval, Some(current_end), 1000)?;

            if candles.is_empty() {
                warn!("No more data available before {}", current_end);
                break;
            }

            // Find oldest candle time
            let oldest_time = candles.iter().map(|c| c.datetime.timestamp_millis()).min();

            // If oldest time hasn't changed, we've reached the limit of available data
            if last_oldest_time == oldest_time {
                break;
            }
            last_oldest_time = oldest_time;

            let oldest_dt = candles
                .iter()
                .map(|c| c.datetime)
                .min()
                .unwrap_or(start_time);

            info!(
                "  Fetched {} candles, oldest: {}",
                candles.len(),
                oldest_dt.format("%Y-%m-%d %H:%M")
            );

            all_candles.extend(candles);

            // Move window back
            current_end = oldest_dt - Duration::minutes(1);

            // Rate limiting
            sleep(self.request_delay);

            if oldest_dt <= start_time {
                break;
            }
        }

        if all_candles.is_empty() {
            return Ok(all_candles);
        }

        // Sort by time (oldest first) and deduplicate
        all_candles.sort_by_key(|c| c.datetime);
        all_candles.dedup_by_key(|c| c.datetime);

        // Filter to requested date range
        all_candles.retain(|c| c.datetime >= start_time);

        info!("Total candles fetched: {}", all_candles.len());

        Ok(all_candles)
    }

    /// Save candles to CSV file
    pub fn save_to_csv(&self, candles: &[Candle], filename: &str) -> Result<PathBuf> {
        let filepath = self.data_dir.join(filename);
        let mut file = File::create(&filepath).context("Failed to create output file")?;

        // Write header
        writeln!(file, "datetime,open,high,low,close,volume")?;

        // Write data
        for candle in candles {
            writeln!(
                file,
                "{},{},{},{},{},{}",
                candle.datetime.format("%Y-%m-%d %H:%M:%S"),
                candle.open,
                candle.high,
                candle.low,
                candle.close,
                candle.volume
            )?;
        }

        info!("Saved {} rows to {}", candles.len(), filepath.display());
        Ok(filepath)
    }

    /// Download historical data for a symbol and save to CSV
    pub fn download_pair(
        &self,
        symbol: &str,
        interval: &str,
        days_back: u32,
    ) -> Result<PathBuf> {
        let pair = Self::to_pair(symbol);
        let candles = self.fetch_full_history(&pair, interval, days_back)?;

        if candles.is_empty() {
            anyhow::bail!("No data fetched for {}", symbol);
        }

        // Normalize symbol for filename
        let symbol_name = if symbol.ends_with("INR") {
            symbol.to_string()
        } else {
            format!("{}INR", symbol)
        };

        let filename = format!("{}_{}.csv", symbol_name, interval);
        self.save_to_csv(&candles, &filename)
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
            if let Some(last) = cached.candles.last_mut() {
                if last.datetime == candle.datetime {
                    *last = candle;
                } else {
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
        if candle.high < candle.low {
            errors.push(format!(
                "Candle {}: high ({}) < low ({})",
                i, candle.high, candle.low
            ));
        }
        if candle.close <= 0.0 {
            errors.push(format!(
                "Candle {}: invalid close price ({})",
                i, candle.close
            ));
        }
        if candle.volume < 0.0 {
            errors.push(format!("Candle {}: negative volume ({})", i, candle.volume));
        }
        if i > 0 && candle.datetime <= candles[i - 1].datetime {
            warnings.push(format!(
                "Candle {}: not chronological",
                i
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
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_to_pair() {
        assert_eq!(CoinDCXDataFetcher::to_pair("BTCINR"), "I-BTC_INR");
        assert_eq!(CoinDCXDataFetcher::to_pair("BTC"), "I-BTC_INR");
        assert_eq!(CoinDCXDataFetcher::to_pair("ETHINR"), "I-ETH_INR");
    }
}
