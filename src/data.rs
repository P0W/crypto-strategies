//! Data loading and management
//!
//! Handles loading OHLCV data from CSV files and live data fetching from exchange APIs.
//! Supports both Binance (default) and CoinDCX data sources.
//! Similar to Python's data_fetcher.py and download_binance_data.py

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::binance::{self, BinanceClient};
use crate::coindcx::{self, CoinDCXClient};
use crate::{Candle, CandleValidationError, Symbol};

// =============================================================================
// Type Aliases
// =============================================================================

/// Result type for check_data_coverage: (missing_files, needs_earlier_data, needs_later_data)
pub type DataCoverageResult = (
    Vec<(Symbol, String)>,
    Vec<(Symbol, String, DateTime<Utc>)>,
    Vec<(Symbol, String, DateTime<Utc>)>,
);

// =============================================================================
// Constants
// =============================================================================

/// Valid intervals (compatible with both Binance and CoinDCX)
pub const INTERVALS: &[&str] = &[
    "1m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "1d", "3d", "1w", "1M",
];

pub fn timeframe_duration(timeframe: &str) -> Option<Duration> {
    match timeframe {
        "1m" => Some(Duration::minutes(1)),
        "5m" => Some(Duration::minutes(5)),
        "15m" => Some(Duration::minutes(15)),
        "30m" => Some(Duration::minutes(30)),
        "1h" => Some(Duration::hours(1)),
        "2h" => Some(Duration::hours(2)),
        "4h" => Some(Duration::hours(4)),
        "6h" => Some(Duration::hours(6)),
        "8h" => Some(Duration::hours(8)),
        "1d" => Some(Duration::days(1)),
        "3d" => Some(Duration::days(3)),
        "1w" => Some(Duration::weeks(1)),
        "1M" => Some(Duration::days(30)),
        _ => None,
    }
}

pub fn warmup_start(
    evaluation_start: DateTime<Utc>,
    timeframes: &[impl AsRef<str>],
    bars: i32,
) -> DateTime<Utc> {
    let longest = timeframes
        .iter()
        .filter_map(|timeframe| timeframe_duration(timeframe.as_ref()))
        .max()
        .unwrap_or_else(Duration::zero);
    evaluation_start - longest * bars
}

/// Validate that symbols have proper INR suffix for data files
/// Returns error message if any symbols are invalid
pub fn validate_symbol_names(symbols: &[Symbol]) -> Option<String> {
    let invalid: Vec<_> = symbols
        .iter()
        .filter(|s| !s.as_str().to_uppercase().ends_with("INR"))
        .map(|s| s.as_str())
        .collect();

    if invalid.is_empty() {
        None
    } else {
        Some(format!(
            "Invalid symbol names: {:?}. Use full names with INR suffix (e.g., BTCINR, ETHINR, SOLINR)",
            invalid
        ))
    }
}

/// Data source enum for selecting exchange
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataSource {
    #[default]
    Binance,
    CoinDCX,
}

impl std::str::FromStr for DataSource {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "binance" => Ok(DataSource::Binance),
            "coindcx" => Ok(DataSource::CoinDCX),
            _ => Err(format!(
                "Unknown data source: {}. Use 'binance' or 'coindcx'",
                s
            )),
        }
    }
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSource::Binance => write!(f, "binance"),
            DataSource::CoinDCX => write!(f, "coindcx"),
        }
    }
}

// =============================================================================
// Candle Conversion
// =============================================================================

/// Convert from CoinDCX API candle to internal Candle type with validation
impl TryFrom<coindcx::types::Candle> for Candle {
    type Error = CandleValidationError;

    fn try_from(c: coindcx::types::Candle) -> Result<Self, Self::Error> {
        Candle::new(
            DateTime::from_timestamp_millis(c.time).unwrap_or_else(Utc::now),
            c.open,
            c.high,
            c.low,
            c.close,
            c.volume,
        )
    }
}

/// Convert from Binance kline to internal Candle type with validation
impl TryFrom<binance::BinanceKline> for Candle {
    type Error = CandleValidationError;

    fn try_from(k: binance::BinanceKline) -> Result<Self, Self::Error> {
        Candle::new(
            DateTime::from_timestamp_millis(k.open_time).unwrap_or_else(Utc::now),
            k.open,
            k.high,
            k.low,
            k.close,
            k.volume,
        )
    }
}

// =============================================================================
// CSV Data Loading
// =============================================================================

/// Load OHLCV data from CSV file with validation
pub fn load_csv(path: impl AsRef<Path>) -> Result<Vec<Candle>> {
    let path = path.as_ref();
    let mut reader = csv::Reader::from_path(path).context("Failed to open CSV file")?;

    let mut candles = Vec::new();
    let mut invalid_count = 0;

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

        let open: f64 = record
            .get(1)
            .context("Missing open column")?
            .parse()
            .context("Failed to parse open")?;
        let high: f64 = record
            .get(2)
            .context("Missing high column")?
            .parse()
            .context("Failed to parse high")?;
        let low: f64 = record
            .get(3)
            .context("Missing low column")?
            .parse()
            .context("Failed to parse low")?;
        let close: f64 = record
            .get(4)
            .context("Missing close column")?
            .parse()
            .context("Failed to parse close")?;
        let volume: f64 = record
            .get(5)
            .context("Missing volume column")?
            .parse()
            .context("Failed to parse volume")?;

        match Candle::new(datetime, open, high, low, close, volume) {
            Ok(candle) => candles.push(candle),
            Err(e) => {
                invalid_count += 1;
                warn!(
                    "Skipping invalid candle at row {} in {:?}: {}",
                    row_idx + 2, // +2 for 1-indexed and header row
                    path.file_name().unwrap_or_default(),
                    e
                );
            }
        }
    }

    if invalid_count > 0 {
        warn!(
            "Skipped {} invalid candles out of {} in {:?}",
            invalid_count,
            invalid_count + candles.len(),
            path.file_name().unwrap_or_default()
        );
    }

    Ok(candles)
}

/// Filter candles by date range
pub fn filter_candles_by_date(
    candles: Vec<Candle>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Vec<Candle> {
    candles
        .into_iter()
        .filter(|c| {
            let after_start = start.is_none_or(|s| c.datetime >= s);
            let before_end = end.is_none_or(|e| c.datetime <= e);
            after_start && before_end
        })
        .collect()
}

/// Parse a date string (YYYY-MM-DD or YYYY-MM-DD HH:MM:SS) to DateTime<Utc>
pub fn parse_date(date_str: &str) -> Result<DateTime<Utc>> {
    // Try full datetime format first
    if let Ok(dt) = date_str.parse::<DateTime<Utc>>() {
        return Ok(dt);
    }

    // Try YYYY-MM-DD HH:MM:SS format
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }

    // Try YYYY-MM-DD format (assume start of day)
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        let ndt = nd.and_hms_opt(0, 0, 0).unwrap();
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }

    anyhow::bail!(
        "Failed to parse date: {}. Use YYYY-MM-DD or YYYY-MM-DD HH:MM:SS format",
        date_str
    )
}

/// Load data for multiple symbols from CSV files
pub fn load_multi_symbol(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframe: &str,
) -> Result<HashMap<Symbol, Vec<Candle>>> {
    load_multi_symbol_with_range(data_dir, symbols, timeframe, None, None)
}

/// Load data for multiple symbols from CSV files with optional date range filtering
pub fn load_multi_symbol_with_range(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframe: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
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
        let original_len = candles.len();

        // Apply date filtering
        let candles = filter_candles_by_date(candles, start, end);

        if start.is_some() || end.is_some() {
            info!(
                "Loaded {} candles for {} (filtered from {} total)",
                candles.len(),
                symbol,
                original_len
            );
        } else {
            info!("Loaded {} candles for {}", candles.len(), symbol);
        }

        if !candles.is_empty() {
            data.insert(symbol.clone(), candles);
        }
    }

    if data.is_empty() {
        anyhow::bail!("No data loaded for any symbol");
    }

    Ok(data)
}

/// Load data for multiple symbols and multiple timeframes from CSV files
///
/// # Arguments
/// * `data_dir` - Directory containing CSV files
/// * `symbols` - Symbols to load
/// * `timeframes` - Timeframes to load (e.g., ["1d", "15m", "5m"])
/// * `primary_timeframe` - The primary timeframe for iteration
/// * `start` - Optional start date filter
/// * `end` - Optional end date filter
///
/// # Returns
/// HashMap of Symbol to MultiTimeframeData, where each entry contains all requested timeframes
pub fn load_multi_timeframe(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[&str],
    primary_timeframe: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<crate::MultiSymbolMultiTimeframeData> {
    use crate::MultiTimeframeData;
    use rayon::prelude::*;

    if timeframes.is_empty() {
        anyhow::bail!("At least one timeframe must be specified");
    }

    if !timeframes.contains(&primary_timeframe) {
        anyhow::bail!(
            "Primary timeframe '{}' must be in timeframes list: {:?}",
            primary_timeframe,
            timeframes
        );
    }

    let data_path = data_dir.as_ref().to_path_buf();
    let timeframes: Vec<String> = timeframes.iter().map(|s| s.to_string()).collect();
    let primary_tf = primary_timeframe.to_string();

    // Load data for all symbol-timeframe combinations in parallel
    let results: Vec<_> = symbols
        .par_iter()
        .map(|symbol| {
            let mut mtf_data = MultiTimeframeData::new(&primary_tf);
            let mut loaded_any = false;

            for timeframe in &timeframes {
                let filename = format!("{}_{}.csv", symbol.as_str(), timeframe);
                let path = data_path.join(&filename);

                if !path.exists() {
                    warn!(
                        "Data file not found: {} (symbol: {}, timeframe: {})",
                        path.display(),
                        symbol,
                        timeframe
                    );
                    continue;
                }

                match load_csv(&path) {
                    Ok(candles) => {
                        let original_len = candles.len();
                        let candles = filter_candles_by_date(candles, start, end);

                        if !candles.is_empty() {
                            if start.is_some() || end.is_some() {
                                info!(
                                    "Loaded {} candles for {} {} (filtered from {} total)",
                                    candles.len(),
                                    symbol,
                                    timeframe,
                                    original_len
                                );
                            } else {
                                info!(
                                    "Loaded {} candles for {} {}",
                                    candles.len(),
                                    symbol,
                                    timeframe
                                );
                            }
                            mtf_data.add_timeframe(timeframe, candles);
                            loaded_any = true;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to load {} {}: {}", symbol, timeframe, e);
                    }
                }
            }

            if loaded_any {
                Some((symbol.clone(), mtf_data))
            } else {
                None
            }
        })
        .collect();

    // Filter out None values and collect into HashMap
    let data: crate::MultiSymbolMultiTimeframeData = results.into_iter().flatten().collect();

    if data.is_empty() {
        anyhow::bail!("No data loaded for any symbol-timeframe combination");
    }

    Ok(data)
}

/// Get the date range covered by a data file
pub fn get_data_date_range(
    path: impl AsRef<Path>,
) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>> {
    let candles = load_csv(path.as_ref())?;
    if candles.is_empty() {
        return Ok(None);
    }

    let min_date = candles.iter().map(|c| c.datetime).min().unwrap();
    let max_date = candles.iter().map(|c| c.datetime).max().unwrap();

    Ok(Some((min_date, max_date)))
}

/// Check which symbols need data for a given date range
/// Returns: (missing_files, files_needing_earlier_data, files_needing_later_data)
/// Note: files_needing_earlier_data only includes files where there's a significant gap (>7 days)
/// to avoid repeatedly trying to fetch data that Binance doesn't have
pub fn check_data_coverage(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> DataCoverageResult {
    let mut missing_files = Vec::new();
    let mut needs_earlier = Vec::new();
    let mut needs_later = Vec::new();
    let data_dir = data_dir.as_ref();

    for symbol in symbols {
        for timeframe in timeframes {
            let filename = format!("{}_{}.csv", symbol.as_str(), timeframe);
            let path = data_dir.join(&filename);

            if !path.exists() {
                missing_files.push((symbol.clone(), timeframe.clone()));
                continue;
            }

            // Check date range
            if let Ok(Some((data_start, data_end))) = get_data_date_range(&path) {
                if let Some(req_start) = start {
                    // Only flag as needing earlier data if there's a significant gap (>7 days)
                    // This prevents repeated attempts when Binance simply doesn't have older data
                    let gap_days = (data_start - req_start).num_days();
                    if gap_days > 7 {
                        needs_earlier.push((symbol.clone(), timeframe.clone(), req_start));
                    }
                }
                if let Some(req_end) = end {
                    if data_end < req_end {
                        needs_later.push((symbol.clone(), timeframe.clone(), req_end));
                    }
                }
            }
        }
    }

    (missing_files, needs_earlier, needs_later)
}

/// Check which data files are missing for given symbols and timeframes
pub fn find_missing_data(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
) -> Vec<(Symbol, String)> {
    let mut missing = Vec::new();
    let data_dir = data_dir.as_ref();

    for symbol in symbols {
        for timeframe in timeframes {
            let filename = format!("{}_{}.csv", symbol.as_str(), timeframe);
            let path = data_dir.join(&filename);

            if !path.exists() {
                missing.push((symbol.clone(), timeframe.clone()));
            }
        }
    }

    missing
}

/// Ensure data exists for all symbols and timeframes, fetching missing data if needed
/// Returns list of symbols/timeframes that couldn't be fetched
pub async fn ensure_data_available(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
    days_back: u32,
) -> Result<Vec<(Symbol, String)>> {
    let data_dir = data_dir.as_ref();
    let missing = find_missing_data(data_dir, symbols, timeframes);

    if missing.is_empty() {
        info!("All data files present");
        return Ok(Vec::new());
    }

    info!(
        "Found {} missing data files, fetching from CoinDCX...",
        missing.len()
    );

    let fetcher = CoinDCXDataFetcher::new(data_dir);
    let mut failed = Vec::new();

    for (symbol, timeframe) in &missing {
        info!("Fetching {} {}...", symbol.as_str(), timeframe);
        match fetcher
            .download_pair(symbol.as_str(), timeframe, days_back)
            .await
        {
            Ok(path) => {
                info!("  Downloaded to {}", path.display());
            }
            Err(e) => {
                warn!("  Failed to fetch {} {}: {}", symbol.as_str(), timeframe, e);
                failed.push((symbol.clone(), timeframe.clone()));
            }
        }
    }

    Ok(failed)
}

/// Synchronous wrapper for ensure_data_available
pub fn ensure_data_available_sync(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
    days_back: u32,
) -> Result<Vec<(Symbol, String)>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(ensure_data_available(
        data_dir, symbols, timeframes, days_back,
    ))
}

/// Ensure data exists for date range, downloading and merging if needed
/// Returns list of symbols/timeframes that couldn't be fetched
pub async fn ensure_data_for_range(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<Vec<(Symbol, String)>> {
    let data_dir = data_dir.as_ref();

    // Check what data we need
    let (missing_files, needs_earlier, needs_later) =
        check_data_coverage(data_dir, symbols, timeframes, start, end);

    let mut failed = Vec::new();

    // Download completely missing files
    if !missing_files.is_empty() {
        info!("Found {} missing data files", missing_files.len());
        let fetcher = BinanceDataFetcher::new(data_dir);

        for (symbol, timeframe) in &missing_files {
            // Calculate days needed from start date to now
            let days_back = start
                .map(|s| (Utc::now() - s).num_days() as u32 + 1)
                .unwrap_or(365);

            println!(
                "    Downloading {}_{}.csv ({} days)...",
                symbol.as_str(),
                timeframe,
                days_back
            );
            match fetcher
                .download_pair(symbol.as_str(), timeframe, days_back)
                .await
            {
                Ok(path) => {
                    println!("    Downloaded to {}", path.display());
                }
                Err(e) => {
                    println!("    FAILED: {}", e);
                    failed.push((symbol.clone(), timeframe.clone()));
                }
            }
        }
    }

    // Extend files that need earlier data
    if !needs_earlier.is_empty() {
        info!("Found {} files needing earlier data", needs_earlier.len());
        let fetcher = BinanceDataFetcher::new(data_dir);

        for (symbol, timeframe, needed_start) in &needs_earlier {
            let filename = format!("{}_{}.csv", symbol.as_str(), timeframe);
            let path = data_dir.join(&filename);

            // Load existing data
            let existing_candles = match load_csv(&path) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to load existing data for {}: {}", symbol, e);
                    failed.push((symbol.clone(), timeframe.clone()));
                    continue;
                }
            };

            let existing_start = existing_candles
                .iter()
                .map(|c| c.datetime)
                .min()
                .unwrap_or(Utc::now());

            // Skip if we already have data at or before the needed start
            if existing_start <= *needed_start {
                continue;
            }

            // Calculate days to fetch from NOW to needed_start
            // (The Binance client fetches data from (now - days_back) to now)
            let days_from_now_to_needed = (Utc::now() - *needed_start).num_days();
            let days_back = days_from_now_to_needed as u32 + 30; // Extra buffer

            println!(
                "    Extending {}_{}.csv with earlier data (fetching back to {})...",
                symbol.as_str(),
                timeframe,
                needed_start.format("%Y-%m-%d")
            );

            // Fetch historical data
            match fetcher
                .fetch_full_history(symbol.as_str(), timeframe, days_back)
                .await
            {
                Ok(new_candles) => {
                    if new_candles.is_empty() {
                        println!(
                            "    No earlier data available from Binance (earliest: {})",
                            existing_start.format("%Y-%m-%d")
                        );
                        continue;
                    }

                    // Check if we actually got earlier data
                    let fetched_start = new_candles.iter().map(|c| c.datetime).min().unwrap();

                    if fetched_start >= existing_start {
                        println!(
                            "    Binance data only goes back to {} (requested: {})",
                            existing_start.format("%Y-%m-%d"),
                            needed_start.format("%Y-%m-%d")
                        );
                        continue;
                    }

                    // Merge: new candles + existing candles, deduplicate
                    let mut all_candles: Vec<Candle> =
                        new_candles.into_iter().chain(existing_candles).collect();

                    // Sort by datetime and deduplicate
                    all_candles.sort_by_key(|c| c.datetime);
                    all_candles.dedup_by_key(|c| c.datetime);

                    // Save merged data
                    match fetcher.save_to_csv(&all_candles, &filename) {
                        Ok(_) => {
                            let new_start = all_candles.iter().map(|c| c.datetime).min().unwrap();
                            println!(
                                "    Extended: data now starts from {} ({} total candles)",
                                new_start.format("%Y-%m-%d"),
                                all_candles.len()
                            );
                            // Warn if data doesn't go back far enough
                            if new_start > *needed_start {
                                println!(
                                    "    WARNING: Data starts at {} but {} was requested.",
                                    new_start.format("%Y-%m-%d"),
                                    needed_start.format("%Y-%m-%d")
                                );
                                println!(
                                    "             Binance may not have earlier data for this symbol."
                                );
                            }
                        }
                        Err(e) => {
                            println!("    FAILED to save merged data: {}", e);
                            failed.push((symbol.clone(), timeframe.clone()));
                        }
                    }
                }
                Err(e) => {
                    println!("    FAILED to fetch earlier data: {}", e);
                    failed.push((symbol.clone(), timeframe.clone()));
                }
            }
        }
    }

    // Extend files that need later data (files that are stale and need refresh)
    // Note: Skip files already handled by needs_earlier (they got fresh data up to now)
    let earlier_symbols: std::collections::HashSet<_> = needs_earlier
        .iter()
        .map(|(s, tf, _)| (s.clone(), tf.clone()))
        .collect();

    let needs_later_only: Vec<_> = needs_later
        .into_iter()
        .filter(|(s, tf, _)| !earlier_symbols.contains(&(s.clone(), tf.clone())))
        .collect();

    if !needs_later_only.is_empty() {
        info!(
            "Found {} files needing only later data",
            needs_later_only.len()
        );
        let fetcher = BinanceDataFetcher::new(data_dir);

        for (symbol, timeframe, needed_end) in &needs_later_only {
            let filename = format!("{}_{}.csv", symbol.as_str(), timeframe);
            let path = data_dir.join(&filename);

            // Load existing data to get the start date
            let existing_candles = match load_csv(&path) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to load existing data for {}: {}", symbol, e);
                    failed.push((symbol.clone(), timeframe.clone()));
                    continue;
                }
            };

            let existing_start = existing_candles
                .iter()
                .map(|c| c.datetime)
                .min()
                .unwrap_or(Utc::now());

            // Fetch from existing start to now (this will get all newer data)
            let days_back = (Utc::now() - existing_start).num_days() as u32 + 30;

            println!(
                "    Refreshing {}_{}.csv with latest data...",
                symbol.as_str(),
                timeframe,
            );

            match fetcher
                .fetch_full_history(symbol.as_str(), timeframe, days_back)
                .await
            {
                Ok(new_candles) => {
                    if new_candles.is_empty() {
                        println!("    No data available from Binance");
                        continue;
                    }

                    // Merge and deduplicate
                    let mut all_candles: Vec<Candle> =
                        existing_candles.into_iter().chain(new_candles).collect();

                    all_candles.sort_by_key(|c| c.datetime);
                    all_candles.dedup_by_key(|c| c.datetime);

                    match fetcher.save_to_csv(&all_candles, &filename) {
                        Ok(_) => {
                            let new_end = all_candles.iter().map(|c| c.datetime).max().unwrap();
                            println!(
                                "    Extended: data now ends at {} ({} total candles)",
                                new_end.format("%Y-%m-%d"),
                                all_candles.len()
                            );
                            // Warn if data doesn't cover the requested end date
                            if new_end < *needed_end {
                                println!(
                                    "    WARNING: Data ends at {} but {} was requested.",
                                    new_end.format("%Y-%m-%d"),
                                    needed_end.format("%Y-%m-%d")
                                );
                                println!(
                                    "             Symbol may be delisted or rebranded on Binance."
                                );
                            }
                        }
                        Err(e) => {
                            println!("    FAILED to save data: {}", e);
                            failed.push((symbol.clone(), timeframe.clone()));
                        }
                    }
                }
                Err(e) => {
                    println!("    FAILED to fetch data: {}", e);
                    failed.push((symbol.clone(), timeframe.clone()));
                }
            }
        }
    }

    Ok(failed)
}

/// Synchronous wrapper for ensure_data_for_range
/// Handles both cases: called from async context or sync context
fn ensure_data_for_range_sync(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<Vec<(Symbol, String)>> {
    let data_dir = data_dir.as_ref().to_path_buf();
    let symbols = symbols.to_vec();
    let timeframes = timeframes.to_vec();

    // Check if we're already in a Tokio runtime
    if tokio::runtime::Handle::try_current().is_ok() {
        // We're inside a runtime - use spawn_blocking to avoid nested runtime panic
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(ensure_data_for_range(
                    &data_dir,
                    &symbols,
                    &timeframes,
                    start,
                    end,
                ))
            })
            .join()
            .unwrap()
        })
    } else {
        // No runtime - create one
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(ensure_data_for_range(
            data_dir,
            &symbols,
            &timeframes,
            start,
            end,
        ))
    }
}

/// Check data availability and fetch missing/incomplete data from Binance.
///
/// This is the primary entry point for ensuring data is available before backtesting.
/// Checks for missing files, files needing earlier data (--start), and files needing
/// later data (--end), then fetches as needed.
///
/// Returns Ok(()) on success. Prints a summary of actions taken.
pub fn check_and_fetch_data(
    data_dir: impl AsRef<Path>,
    symbols: &[Symbol],
    timeframes: &[String],
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<()> {
    let data_dir = data_dir.as_ref();

    let (missing_files, needs_earlier, needs_later) =
        check_data_coverage(data_dir, symbols, timeframes, start, end);

    let needs_download =
        !missing_files.is_empty() || !needs_earlier.is_empty() || !needs_later.is_empty();

    if !needs_download {
        info!("All required data files are available");
        return Ok(());
    }

    // Automatic fetching currently uses Binance and therefore only supports
    // crypto symbols with the repository's INR filename convention.
    if let Some(err) = validate_symbol_names(symbols) {
        anyhow::bail!(
            "{} Automatic fetching is only available for crypto symbols; provide local CSV files for equity symbols.",
            err
        );
    }

    println!("\n{}", "=".repeat(60));
    println!("DATA AVAILABILITY CHECK");
    println!("{}", "=".repeat(60));

    if !missing_files.is_empty() {
        println!("  Missing files: {}", missing_files.len());
        for (sym, tf) in &missing_files {
            println!("    - {}_{}.csv", sym.as_str(), tf);
        }
    }

    if !needs_earlier.is_empty() {
        println!("  Files needing earlier data: {}", needs_earlier.len());
        for (sym, tf, needed_start) in &needs_earlier {
            println!(
                "    - {}_{}.csv (need data from {})",
                sym.as_str(),
                tf,
                needed_start.format("%Y-%m-%d")
            );
        }
    }

    if !needs_later.is_empty() {
        println!("  Files needing later data: {}", needs_later.len());
        for (sym, tf, needed_end) in &needs_later {
            println!(
                "    - {}_{}.csv (need data until {})",
                sym.as_str(),
                tf,
                needed_end.format("%Y-%m-%d")
            );
        }
    }

    println!("{}", "-".repeat(60));
    println!("  Fetching data from Binance...\n");

    let failed = ensure_data_for_range_sync(data_dir, symbols, timeframes, start, end)?;

    println!();
    if failed.is_empty() {
        println!("  All data fetched successfully");
    } else {
        println!("  WARNING: Could not fetch {} file(s):", failed.len());
        for (sym, tf) in &failed {
            println!("    - {}_{}.csv", sym.as_str(), tf);
        }
        println!();
        println!("  Backtest will proceed with available data.");
        println!("  Results may be limited to the available date range.");
    }

    println!("{}\n", "=".repeat(60));

    Ok(())
}

// =============================================================================
// CoinDCX Data Fetcher (uses coindcx client library)
// =============================================================================

/// Fetch historical OHLCV data from CoinDCX API
///
/// This fetcher uses the full-featured `CoinDCXClient` with rate limiting,
/// circuit breaker, and retry logic.
pub struct CoinDCXDataFetcher {
    client: CoinDCXClient,
    pub data_dir: PathBuf,
}

impl CoinDCXDataFetcher {
    /// Create a new data fetcher
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).ok();

        // Create client without credentials (for public endpoints only)
        let client = CoinDCXClient::new("", "");

        Self { client, data_dir }
    }

    /// Create with a custom client configuration
    pub fn with_client(client: CoinDCXClient, data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).ok();

        Self { client, data_dir }
    }

    /// Convert symbol to CoinDCX pair format: BTCINR -> I-BTC_INR
    pub fn to_pair(symbol: &str) -> String {
        if let Some(base) = symbol.strip_suffix("INR") {
            format!("I-{}_INR", base)
        } else {
            format!("I-{}_INR", symbol)
        }
    }

    /// Fetch candles using the CoinDCX client
    pub async fn fetch_candles(
        &self,
        pair: &str,
        interval: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Candle>> {
        let api_candles = self.client.get_candles(pair, interval, limit).await?;
        let mut candles = Vec::with_capacity(api_candles.len());
        let mut invalid_count = 0;

        for api_candle in api_candles {
            match Candle::try_from(api_candle) {
                Ok(candle) => candles.push(candle),
                Err(e) => {
                    invalid_count += 1;
                    warn!("Skipping invalid candle for {}: {}", pair, e);
                }
            }
        }

        if invalid_count > 0 {
            warn!(
                "Skipped {} invalid candles out of {} for {}",
                invalid_count,
                invalid_count + candles.len(),
                pair
            );
        }

        Ok(candles)
    }

    /// Get list of available markets
    pub async fn list_markets(&self) -> Result<Vec<coindcx::types::MarketDetails>> {
        self.client.get_markets_details().await
    }

    /// Get list of available INR trading pairs
    pub async fn list_inr_pairs(&self) -> Result<Vec<String>> {
        let markets = self.client.get_markets_details().await?;
        let pairs: Vec<String> = markets
            .into_iter()
            .filter(|m| m.base_currency_short_name == "INR" && m.status == "active")
            .filter_map(|m| m.pair)
            .collect();
        Ok(pairs)
    }

    /// Fetch full historical data
    pub async fn fetch_full_history(
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

        // Fetch candles (CoinDCX public API returns up to 1000 most recent)
        let candles = self.fetch_candles(pair, interval, Some(1000)).await?;

        if candles.is_empty() {
            info!("No candles available for {}", pair);
            return Ok(Vec::new());
        }

        let oldest_dt = candles
            .iter()
            .map(|c| c.datetime)
            .min()
            .unwrap_or(start_time);
        let newest_dt = candles.iter().map(|c| c.datetime).max().unwrap_or(end_time);

        info!(
            "  Fetched {} candles, range: {} to {}",
            candles.len(),
            oldest_dt.format("%Y-%m-%d %H:%M"),
            newest_dt.format("%Y-%m-%d %H:%M")
        );

        // Filter to requested date range, sort and deduplicate
        let mut all_candles: Vec<Candle> = candles
            .into_iter()
            .filter(|c| c.datetime >= start_time)
            .collect();

        all_candles.sort_by_key(|c| c.datetime);
        all_candles.dedup_by_key(|c| c.datetime);

        info!("Total candles after filtering: {}", all_candles.len());
        Ok(all_candles)
    }

    /// Save candles to CSV file
    pub fn save_to_csv(&self, candles: &[Candle], filename: &str) -> Result<PathBuf> {
        let filepath = self.data_dir.join(filename);
        let mut file = File::create(&filepath).context("Failed to create output file")?;

        writeln!(file, "datetime,open,high,low,close,volume")?;

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
    pub async fn download_pair(
        &self,
        symbol: &str,
        interval: &str,
        days_back: u32,
    ) -> Result<PathBuf> {
        let pair = Self::to_pair(symbol);
        let candles = self.fetch_full_history(&pair, interval, days_back).await?;

        if candles.is_empty() {
            anyhow::bail!("No data fetched for {}", symbol);
        }

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
// Binance Data Fetcher (default, no API key required)
// =============================================================================

/// Fetch historical OHLCV data from Binance API
///
/// This fetcher uses the public Binance API which doesn't require authentication.
/// Data is downloaded in USDT pairs and saved with INR suffix to maintain
/// compatibility with the existing file naming convention.
pub struct BinanceDataFetcher {
    client: BinanceClient,
    pub data_dir: PathBuf,
}

impl BinanceDataFetcher {
    /// Create a new Binance data fetcher
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).ok();

        BinanceDataFetcher {
            client: BinanceClient::new(),
            data_dir,
        }
    }

    /// Convert symbol to Binance pair format
    /// E.g., "BTC" or "BTCINR" -> "BTCUSDT"
    pub fn to_pair(&self, symbol: &str) -> String {
        self.client.to_binance_pair(symbol)
    }

    /// Fetch candles from Binance
    pub async fn fetch_candles(
        &self,
        symbol: &str,
        interval: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Candle>> {
        let binance_pair = self.to_pair(symbol);
        let klines = self
            .client
            .get_klines(&binance_pair, interval, None, None, limit)
            .await?;

        let mut candles = Vec::with_capacity(klines.len());
        let mut invalid_count = 0;

        for kline in klines {
            match Candle::try_from(kline) {
                Ok(candle) => candles.push(candle),
                Err(e) => {
                    invalid_count += 1;
                    warn!("Skipping invalid candle for {}: {}", symbol, e);
                }
            }
        }

        if invalid_count > 0 {
            warn!(
                "Skipped {} invalid candles out of {} for {}",
                invalid_count,
                invalid_count + candles.len(),
                symbol
            );
        }

        Ok(candles)
    }

    /// Fetch full historical data
    pub async fn fetch_full_history(
        &self,
        symbol: &str,
        interval: &str,
        days_back: u32,
    ) -> Result<Vec<Candle>> {
        let klines = self
            .client
            .fetch_full_history(symbol, interval, days_back)
            .await?;

        let mut candles = Vec::with_capacity(klines.len());
        let mut invalid_count = 0;

        for kline in klines {
            match Candle::try_from(kline) {
                Ok(candle) => candles.push(candle),
                Err(e) => {
                    invalid_count += 1;
                    warn!("Skipping invalid candle for {}: {}", symbol, e);
                }
            }
        }

        if invalid_count > 0 {
            warn!(
                "Skipped {} invalid candles out of {} for {}",
                invalid_count,
                invalid_count + candles.len(),
                symbol
            );
        }

        Ok(candles)
    }

    /// Save candles to CSV file
    pub fn save_to_csv(&self, candles: &[Candle], filename: &str) -> Result<PathBuf> {
        let filepath = self.data_dir.join(filename);
        let mut file = File::create(&filepath).context("Failed to create output file")?;

        writeln!(file, "datetime,open,high,low,close,volume")?;

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
    /// Uses INR suffix in filename to maintain compatibility with existing data files
    pub async fn download_pair(
        &self,
        symbol: &str,
        interval: &str,
        days_back: u32,
    ) -> Result<PathBuf> {
        let candles = self.fetch_full_history(symbol, interval, days_back).await?;

        if candles.is_empty() {
            anyhow::bail!("No data fetched for {}", symbol);
        }

        // Extract base symbol and add INR suffix for filename compatibility
        let base = symbol
            .trim()
            .to_uppercase()
            .replace("INR", "")
            .replace("USDT", "");
        let symbol_name = format!("{}INR", base);

        let filename = format!("{}_{}.csv", symbol_name, interval);
        self.save_to_csv(&candles, &filename)
    }

    /// Download multiple timeframes for a symbol
    pub async fn download_symbol(
        &self,
        symbol: &str,
        timeframes: &[&str],
        days_back: u32,
    ) -> Vec<Result<PathBuf>> {
        let mut results = Vec::new();

        for tf in timeframes {
            results.push(self.download_pair(symbol, tf, days_back).await);
        }

        results
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
            warnings.push(format!("Candle {}: not chronological", i));
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
    use chrono::{Datelike, Duration, TimeZone, Timelike};
    use tempfile::TempDir;

    fn create_test_candle(datetime: DateTime<Utc>, close: f64) -> Candle {
        Candle {
            datetime,
            open: close - 5.0,
            high: close + 5.0,
            low: close - 10.0,
            close,
            volume: 1000.0,
        }
    }

    #[test]
    fn test_warmup_start_uses_longest_timeframe() {
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let warmup = warmup_start(start, &["15m", "1d"], 300);
        assert_eq!(warmup, start - Duration::days(300));
    }

    // ==================== DataSource Tests ====================

    #[test]
    fn test_data_source_from_str() {
        assert_eq!(
            "binance".parse::<DataSource>().unwrap(),
            DataSource::Binance
        );
        assert_eq!(
            "BINANCE".parse::<DataSource>().unwrap(),
            DataSource::Binance
        );
        assert_eq!(
            "coindcx".parse::<DataSource>().unwrap(),
            DataSource::CoinDCX
        );
        assert_eq!(
            "CoinDCX".parse::<DataSource>().unwrap(),
            DataSource::CoinDCX
        );
    }

    #[test]
    fn test_data_source_from_str_invalid() {
        assert!("invalid".parse::<DataSource>().is_err());
        assert!("".parse::<DataSource>().is_err());
    }

    #[test]
    fn test_data_source_display() {
        assert_eq!(format!("{}", DataSource::Binance), "binance");
        assert_eq!(format!("{}", DataSource::CoinDCX), "coindcx");
    }

    #[test]
    fn test_data_source_default() {
        assert_eq!(DataSource::default(), DataSource::Binance);
    }

    // ==================== validate_symbol_names Tests ====================

    #[test]
    fn test_validate_symbol_names_valid() {
        let symbols = vec![
            Symbol::new("BTCINR"),
            Symbol::new("ETHINR"),
            Symbol::new("SOLINR"),
        ];
        assert!(validate_symbol_names(&symbols).is_none());
    }

    #[test]
    fn test_validate_symbol_names_invalid() {
        let symbols = vec![Symbol::new("BTC"), Symbol::new("ETH")];
        let result = validate_symbol_names(&symbols);
        assert!(result.is_some());
        assert!(result.unwrap().contains("BTC"));
    }

    #[test]
    fn test_validate_symbol_names_mixed() {
        let symbols = vec![Symbol::new("BTCINR"), Symbol::new("ETH")];
        let result = validate_symbol_names(&symbols);
        assert!(result.is_some());
        let err_msg = result.unwrap();
        // Check that ETH is listed as invalid but BTCINR is not in the invalid list
        // Note: The message contains BTCINR as an example, so we check the debug format
        assert!(err_msg.contains(r#""ETH""#)); // ETH should be in the invalid list
                                               // The actual invalid symbols list is formatted as ["ETH"], not containing BTCINR
        assert!(err_msg.starts_with("Invalid symbol names: [\"ETH\"]"));
    }

    #[test]
    fn test_validate_symbol_names_empty() {
        let symbols: Vec<Symbol> = vec![];
        assert!(validate_symbol_names(&symbols).is_none());
    }

    // ==================== parse_date Tests ====================

    #[test]
    fn test_parse_date_yyyy_mm_dd() {
        let result = parse_date("2024-01-15").unwrap();
        assert_eq!(result.year(), 2024);
        assert_eq!(result.month(), 1);
        assert_eq!(result.day(), 15);
        assert_eq!(result.hour(), 0);
        assert_eq!(result.minute(), 0);
    }

    #[test]
    fn test_parse_date_full_datetime() {
        let result = parse_date("2024-01-15 14:30:00").unwrap();
        assert_eq!(result.year(), 2024);
        assert_eq!(result.month(), 1);
        assert_eq!(result.day(), 15);
        assert_eq!(result.hour(), 14);
        assert_eq!(result.minute(), 30);
    }

    #[test]
    fn test_parse_date_iso8601() {
        let result = parse_date("2024-01-15T14:30:00Z").unwrap();
        assert_eq!(result.year(), 2024);
        assert_eq!(result.hour(), 14);
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("invalid").is_err());
        assert!(parse_date("2024/01/15").is_err());
        assert!(parse_date("15-01-2024").is_err());
    }

    // ==================== filter_candles_by_date Tests ====================

    #[test]
    fn test_filter_candles_by_date_no_filter() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles: Vec<Candle> = (0..5)
            .map(|i| create_test_candle(base_time + Duration::days(i), 100.0 + i as f64))
            .collect();

        let filtered = filter_candles_by_date(candles.clone(), None, None);
        assert_eq!(filtered.len(), 5);
    }

    #[test]
    fn test_filter_candles_by_date_start_only() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles: Vec<Candle> = (0..10)
            .map(|i| create_test_candle(base_time + Duration::days(i), 100.0 + i as f64))
            .collect();

        let start = base_time + Duration::days(5);
        let filtered = filter_candles_by_date(candles, Some(start), None);

        assert_eq!(filtered.len(), 5);
        assert!(filtered.iter().all(|c| c.datetime >= start));
    }

    #[test]
    fn test_filter_candles_by_date_end_only() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles: Vec<Candle> = (0..10)
            .map(|i| create_test_candle(base_time + Duration::days(i), 100.0 + i as f64))
            .collect();

        let end = base_time + Duration::days(4);
        let filtered = filter_candles_by_date(candles, None, Some(end));

        assert_eq!(filtered.len(), 5);
        assert!(filtered.iter().all(|c| c.datetime <= end));
    }

    #[test]
    fn test_filter_candles_by_date_both() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles: Vec<Candle> = (0..10)
            .map(|i| create_test_candle(base_time + Duration::days(i), 100.0 + i as f64))
            .collect();

        let start = base_time + Duration::days(2);
        let end = base_time + Duration::days(6);
        let filtered = filter_candles_by_date(candles, Some(start), Some(end));

        assert_eq!(filtered.len(), 5);
        assert!(filtered
            .iter()
            .all(|c| c.datetime >= start && c.datetime <= end));
    }

    // ==================== CandleCache Tests ====================

    #[test]
    fn test_candle_cache_new() {
        let cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        assert!(cache.get(&symbol).is_none());
        assert!(cache.needs_refresh(&symbol));
    }

    #[test]
    fn test_candle_cache_append() {
        let mut cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        let candle = create_test_candle(Utc::now(), 100.0);
        cache.append(&symbol, candle);

        assert!(cache.get(&symbol).is_some());
        assert_eq!(cache.get(&symbol).unwrap().len(), 1);
    }

    #[test]
    fn test_candle_cache_update() {
        let mut cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        let base_time = Utc::now();
        let candles: Vec<Candle> = (0..5)
            .map(|i| create_test_candle(base_time + Duration::hours(i), 100.0 + i as f64))
            .collect();

        cache.update(symbol.clone(), candles);

        assert_eq!(cache.get(&symbol).unwrap().len(), 5);
    }

    #[test]
    fn test_candle_cache_update_truncates() {
        let mut cache = CandleCache::new(3, 60); // Max 3 candles
        let symbol = Symbol::new("BTCINR");

        let base_time = Utc::now();
        let candles: Vec<Candle> = (0..10)
            .map(|i| create_test_candle(base_time + Duration::hours(i), 100.0 + i as f64))
            .collect();

        cache.update(symbol.clone(), candles);

        // Should only keep last 3
        assert_eq!(cache.get(&symbol).unwrap().len(), 3);
    }

    #[test]
    fn test_candle_cache_append_same_datetime() {
        let mut cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        let now = Utc::now();
        let candle1 = create_test_candle(now, 100.0);
        let candle2 = create_test_candle(now, 105.0); // Same datetime, different price

        cache.append(&symbol, candle1);
        cache.append(&symbol, candle2);

        // Should replace, not add
        let candles = cache.get(&symbol).unwrap();
        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].close, 105.0);
    }

    #[test]
    fn test_candle_cache_append_different_datetime() {
        let mut cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        let base_time = Utc::now();
        let candle1 = create_test_candle(base_time, 100.0);
        let candle2 = create_test_candle(base_time + Duration::hours(1), 105.0);

        cache.append(&symbol, candle1);
        cache.append(&symbol, candle2);

        assert_eq!(cache.get(&symbol).unwrap().len(), 2);
    }

    #[test]
    fn test_candle_cache_append_truncates() {
        let mut cache = CandleCache::new(3, 60); // Max 3 candles
        let symbol = Symbol::new("BTCINR");

        let base_time = Utc::now();
        for i in 0..5 {
            let candle = create_test_candle(base_time + Duration::hours(i), 100.0 + i as f64);
            cache.append(&symbol, candle);
        }

        // Should only keep last 3
        assert_eq!(cache.get(&symbol).unwrap().len(), 3);
    }

    #[test]
    fn test_candle_cache_clear() {
        let mut cache = CandleCache::new(100, 60);
        let symbol = Symbol::new("BTCINR");

        let candle = create_test_candle(Utc::now(), 100.0);
        cache.append(&symbol, candle);

        assert!(cache.get(&symbol).is_some());

        cache.clear();

        assert!(cache.get(&symbol).is_none());
    }

    #[test]
    fn test_candle_cache_needs_refresh() {
        let mut cache = CandleCache::new(100, 1); // 1 second TTL
        let symbol = Symbol::new("BTCINR");

        // Non-existent symbol needs refresh
        assert!(cache.needs_refresh(&symbol));

        let candle = create_test_candle(Utc::now(), 100.0);
        cache.append(&symbol, candle);

        // Just added, should not need refresh
        assert!(!cache.needs_refresh(&symbol));

        // Note: Testing actual TTL expiry would require sleeping, which we avoid in unit tests
    }

    // ==================== validate_candles Tests ====================

    #[test]
    fn test_validate_candles_valid() {
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
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validate_candles_empty() {
        let candles: Vec<Candle> = vec![];
        let result = validate_candles(&candles);

        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.contains("No candles")));
    }

    #[test]
    fn test_validate_candles_high_less_than_low() {
        let candles = vec![Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 90.0, // Invalid: high < low
            low: 95.0,
            close: 92.0,
            volume: 1000.0,
        }];

        let result = validate_candles(&candles);
        assert!(!result.is_valid());
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("high") && e.contains("low")));
    }

    #[test]
    fn test_validate_candles_invalid_close() {
        let candles = vec![Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 0.0, // Invalid: close <= 0
            volume: 1000.0,
        }];

        let result = validate_candles(&candles);
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.contains("close")));
    }

    #[test]
    fn test_validate_candles_negative_volume() {
        let candles = vec![Candle {
            datetime: Utc::now(),
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 102.0,
            volume: -1000.0, // Invalid: negative volume
        }];

        let result = validate_candles(&candles);
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.contains("volume")));
    }

    #[test]
    fn test_validate_candles_not_chronological() {
        let base_time = Utc::now();
        let candles = vec![
            Candle {
                datetime: base_time + Duration::hours(1),
                open: 100.0,
                high: 105.0,
                low: 95.0,
                close: 102.0,
                volume: 1000.0,
            },
            Candle {
                datetime: base_time, // Earlier than previous
                open: 100.0,
                high: 105.0,
                low: 95.0,
                close: 102.0,
                volume: 1000.0,
            },
        ];

        let result = validate_candles(&candles);
        assert!(result.is_valid()); // Only a warning, not an error
        assert!(result.warnings.iter().any(|w| w.contains("chronological")));
    }

    // ==================== ValidationResult Tests ====================

    #[test]
    fn test_validation_result_is_valid() {
        let valid = ValidationResult {
            errors: vec![],
            warnings: vec!["some warning".to_string()],
        };
        assert!(valid.is_valid());

        let invalid = ValidationResult {
            errors: vec!["some error".to_string()],
            warnings: vec![],
        };
        assert!(!invalid.is_valid());
    }

    // ==================== CoinDCXDataFetcher Tests ====================

    #[test]
    fn test_to_pair() {
        assert_eq!(CoinDCXDataFetcher::to_pair("BTCINR"), "I-BTC_INR");
        assert_eq!(CoinDCXDataFetcher::to_pair("BTC"), "I-BTC_INR");
        assert_eq!(CoinDCXDataFetcher::to_pair("ETHINR"), "I-ETH_INR");
        assert_eq!(CoinDCXDataFetcher::to_pair("SOL"), "I-SOL_INR");
    }

    #[test]
    fn test_coindcx_fetcher_new() {
        let temp_dir = TempDir::new().unwrap();
        let _fetcher = CoinDCXDataFetcher::new(temp_dir.path());

        assert!(temp_dir.path().exists());
    }

    // ==================== BinanceDataFetcher Tests ====================

    #[test]
    fn test_binance_fetcher_to_pair() {
        let temp_dir = TempDir::new().unwrap();
        let fetcher = BinanceDataFetcher::new(temp_dir.path());

        // BinanceDataFetcher delegates to BinanceClient::to_binance_pair
        // which converts INR symbols to USDT
        assert_eq!(fetcher.to_pair("BTCINR"), "BTCUSDT");
        assert_eq!(fetcher.to_pair("BTC"), "BTCUSDT");
    }

    #[test]
    fn test_binance_fetcher_new() {
        let temp_dir = TempDir::new().unwrap();
        let _fetcher = BinanceDataFetcher::new(temp_dir.path());

        assert!(temp_dir.path().exists());
    }

    // ==================== CSV Loading Tests ====================

    #[test]
    fn test_load_csv_valid() {
        let temp_dir = TempDir::new().unwrap();
        let csv_path = temp_dir.path().join("test.csv");

        let csv_content = "datetime,open,high,low,close,volume
2024-01-01 00:00:00,100.0,105.0,95.0,102.0,1000.0
2024-01-02 00:00:00,102.0,108.0,100.0,106.0,1500.0";

        std::fs::write(&csv_path, csv_content).unwrap();

        let candles = load_csv(&csv_path).unwrap();

        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].close, 102.0);
        assert_eq!(candles[1].close, 106.0);
    }

    #[test]
    fn test_load_csv_missing_file() {
        let result = load_csv("nonexistent.csv");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_csv_with_timezone() {
        let temp_dir = TempDir::new().unwrap();
        let csv_path = temp_dir.path().join("test.csv");

        // ISO 8601 format with timezone
        let csv_content = "datetime,open,high,low,close,volume
2024-01-01T00:00:00Z,100.0,105.0,95.0,102.0,1000.0";

        std::fs::write(&csv_path, csv_content).unwrap();

        let candles = load_csv(&csv_path).unwrap();
        assert_eq!(candles.len(), 1);
    }

    // ==================== find_missing_data Tests ====================

    #[test]
    fn test_find_missing_data() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path();

        // Create one file
        std::fs::write(
            data_dir.join("BTCINR_1d.csv"),
            "datetime,open,high,low,close,volume",
        )
        .unwrap();

        let symbols = vec![Symbol::new("BTCINR"), Symbol::new("ETHINR")];
        let timeframes = vec!["1d".to_string()];

        let missing = find_missing_data(data_dir, &symbols, &timeframes);

        // ETHINR_1d.csv is missing
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].0.as_str(), "ETHINR");
        assert_eq!(missing[0].1, "1d");
    }

    #[test]
    fn test_find_missing_data_none_missing() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path();

        // Create all files
        std::fs::write(
            data_dir.join("BTCINR_1d.csv"),
            "datetime,open,high,low,close,volume",
        )
        .unwrap();
        std::fs::write(
            data_dir.join("ETHINR_1d.csv"),
            "datetime,open,high,low,close,volume",
        )
        .unwrap();

        let symbols = vec![Symbol::new("BTCINR"), Symbol::new("ETHINR")];
        let timeframes = vec!["1d".to_string()];

        let missing = find_missing_data(data_dir, &symbols, &timeframes);
        assert!(missing.is_empty());
    }

    // ==================== INTERVALS constant Tests ====================

    #[test]
    fn test_intervals_contains_common_timeframes() {
        assert!(INTERVALS.contains(&"1m"));
        assert!(INTERVALS.contains(&"5m"));
        assert!(INTERVALS.contains(&"15m"));
        assert!(INTERVALS.contains(&"1h"));
        assert!(INTERVALS.contains(&"4h"));
        assert!(INTERVALS.contains(&"1d"));
        assert!(INTERVALS.contains(&"1w"));
    }
}
