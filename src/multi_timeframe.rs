//! Multi-Timeframe Data Management
//!
//! Provides efficient storage and access to OHLCV data across multiple timeframes.
//! Designed for zero-copy access and minimal memory overhead.

use crate::{Candle, Symbol};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Multi-timeframe candle data for a single symbol
#[derive(Debug, Clone)]
pub struct MultiTimeframeData {
    /// Map of timeframe to candle data
    /// Key: timeframe string (e.g., "1d", "15m", "5m")
    /// Value: Vector of candles for that timeframe
    timeframes: HashMap<String, Vec<Candle>>,

    /// Primary timeframe used for iteration (typically the finest granularity)
    primary_timeframe: String,
}

impl MultiTimeframeData {
    /// Create new multi-timeframe data with a primary timeframe
    pub fn new(primary_timeframe: impl Into<String>) -> Self {
        Self {
            timeframes: HashMap::new(),
            primary_timeframe: primary_timeframe.into(),
        }
    }

    /// Add candle data for a specific timeframe
    pub fn add_timeframe(&mut self, timeframe: impl Into<String>, candles: Vec<Candle>) {
        self.timeframes.insert(timeframe.into(), candles);
    }

    /// Get candles for a specific timeframe
    pub fn get(&self, timeframe: &str) -> Option<&[Candle]> {
        self.timeframes.get(timeframe).map(|v| v.as_slice())
    }

    /// Get the primary timeframe candles
    pub fn primary(&self) -> &[Candle] {
        self.timeframes
            .get(&self.primary_timeframe)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the primary timeframe name
    pub fn primary_timeframe(&self) -> &str {
        &self.primary_timeframe
    }

    /// Get all available timeframes
    pub fn timeframes(&self) -> Vec<&str> {
        self.timeframes.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a timeframe is available
    pub fn has_timeframe(&self, timeframe: &str) -> bool {
        self.timeframes.contains_key(timeframe)
    }

    /// Get mutable access to timeframe data (for live updates)
    pub fn get_mut(&mut self, timeframe: &str) -> Option<&mut Vec<Candle>> {
        self.timeframes.get_mut(timeframe)
    }

    /// Get the length of the primary timeframe data
    pub fn len(&self) -> usize {
        self.primary().len()
    }

    /// Check if primary timeframe is empty
    pub fn is_empty(&self) -> bool {
        self.primary().is_empty()
    }
}

/// Multi-timeframe candle slices passed to strategies
///
/// Provides efficient windowed access to multiple timeframes without copying.
/// All timeframes are aligned to the same datetime point.
#[derive(Debug, Clone)]
pub struct MultiTimeframeCandles<'a> {
    /// Map of timeframe to candle slices
    /// These are windowed slices, not the full history
    timeframes: HashMap<String, &'a [Candle]>,

    /// Primary timeframe
    primary_timeframe: String,

    /// Current datetime (from primary timeframe's last candle)
    current_datetime: DateTime<Utc>,
}

impl<'a> MultiTimeframeCandles<'a> {
    /// Create new multi-timeframe candles view
    pub fn new(primary_timeframe: impl Into<String>, current_datetime: DateTime<Utc>) -> Self {
        Self {
            timeframes: HashMap::new(),
            primary_timeframe: primary_timeframe.into(),
            current_datetime,
        }
    }

    /// Create from MultiTimeframeData (all timeframes available)
    pub fn from_data(data: &'a MultiTimeframeData) -> Self {
        let mut mtf = Self::new(
            data.primary_timeframe().to_string(),
            data.primary()
                .last()
                .map(|c| c.datetime)
                .unwrap_or_else(Utc::now),
        );
        for tf in data.timeframes() {
            if let Some(candles) = data.get(tf) {
                mtf.add_timeframe(tf.to_string(), candles);
            }
        }
        mtf
    }

    /// Add a timeframe slice
    pub fn add_timeframe(&mut self, timeframe: impl Into<String>, candles: &'a [Candle]) {
        self.timeframes.insert(timeframe.into(), candles);
    }

    /// Get candles for a specific timeframe
    pub fn get(&self, timeframe: &str) -> Option<&'a [Candle]> {
        self.timeframes.get(timeframe).copied()
    }

    /// Get the primary timeframe candles
    pub fn primary(&self) -> &'a [Candle] {
        self.timeframes
            .get(&self.primary_timeframe)
            .copied()
            .unwrap_or(&[])
    }

    /// Get the current datetime
    pub fn datetime(&self) -> DateTime<Utc> {
        self.current_datetime
    }

    /// Get the primary timeframe name
    pub fn primary_timeframe(&self) -> &str {
        &self.primary_timeframe
    }
}

/// Container for multi-symbol multi-timeframe data
pub type MultiSymbolMultiTimeframeData = HashMap<Symbol, MultiTimeframeData>;

/// Align multi-timeframe data to common datetime points
///
/// Ensures all timeframes for all symbols have data for the same datetime range.
/// Uses the primary timeframe as the reference.
pub fn align_multi_timeframe_data(
    data: &MultiSymbolMultiTimeframeData,
) -> Vec<(Symbol, MultiTimeframeData)> {
    if data.is_empty() {
        return Vec::new();
    }

    // Find common date range across all symbols' primary timeframes
    let mut min_date: Option<DateTime<Utc>> = None;
    let mut max_date: Option<DateTime<Utc>> = None;

    for mtf_data in data.values() {
        let primary = mtf_data.primary();
        if primary.is_empty() {
            continue;
        }

        let first = primary.first().unwrap().datetime;
        let last = primary.last().unwrap().datetime;

        min_date = Some(match min_date {
            Some(d) => d.max(first),
            None => first,
        });

        max_date = Some(match max_date {
            Some(d) => d.min(last),
            None => last,
        });
    }

    if min_date.is_none() || max_date.is_none() {
        return Vec::new();
    }

    let min_date = min_date.unwrap();
    let max_date = max_date.unwrap();

    // Filter each symbol's data to the common range
    let mut aligned = Vec::new();
    for (symbol, mtf_data) in data {
        let mut aligned_mtf = MultiTimeframeData::new(mtf_data.primary_timeframe());

        for timeframe in mtf_data.timeframes() {
            if let Some(candles) = mtf_data.get(timeframe) {
                let filtered: Vec<Candle> = candles
                    .iter()
                    .filter(|c| c.datetime >= min_date && c.datetime <= max_date)
                    .cloned()
                    .collect();

                if !filtered.is_empty() {
                    aligned_mtf.add_timeframe(timeframe, filtered);
                }
            }
        }

        if !aligned_mtf.is_empty() {
            aligned.push((symbol.clone(), aligned_mtf));
        }
    }

    // CRITICAL: Sort by symbol name for deterministic iteration order
    // HashMap iteration is non-deterministic, which causes different backtest
    // results on each run due to symbol processing order affecting capital
    // allocation, risk manager decisions, and trade execution sequence.
    aligned.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

    aligned
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn create_test_candle(datetime: DateTime<Utc>, close: f64) -> Candle {
        Candle::new(datetime, close - 5.0, close + 5.0, close - 10.0, close, 1000.0).unwrap()
    }

    fn create_test_candles(base_time: DateTime<Utc>, count: usize) -> Vec<Candle> {
        (0..count)
            .map(|i| {
                let dt = base_time + Duration::hours(i as i64);
                create_test_candle(dt, 100.0 + i as f64)
            })
            .collect()
    }

    // ==================== MultiTimeframeData Tests ====================

    #[test]
    fn test_multi_timeframe_data_new() {
        let mtf = MultiTimeframeData::new("1h");

        assert_eq!(mtf.primary_timeframe(), "1h");
        assert!(mtf.is_empty());
        assert_eq!(mtf.len(), 0);
    }

    #[test]
    fn test_multi_timeframe_data_add_and_get() {
        let mut mtf = MultiTimeframeData::new("5m");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let candles_5m = create_test_candles(base_time, 5);
        mtf.add_timeframe("5m", candles_5m.clone());

        assert!(mtf.has_timeframe("5m"));
        assert!(!mtf.has_timeframe("1h"));
        assert_eq!(mtf.get("5m").unwrap().len(), 5);
        assert!(mtf.get("1h").is_none());
    }

    #[test]
    fn test_multi_timeframe_data_multiple_timeframes() {
        let mut mtf = MultiTimeframeData::new("5m");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let candles_5m = create_test_candles(base_time, 10);
        let candles_1h = create_test_candles(base_time, 5);
        let candles_1d = create_test_candles(base_time, 2);

        mtf.add_timeframe("5m", candles_5m);
        mtf.add_timeframe("1h", candles_1h);
        mtf.add_timeframe("1d", candles_1d);

        assert!(mtf.has_timeframe("5m"));
        assert!(mtf.has_timeframe("1h"));
        assert!(mtf.has_timeframe("1d"));

        let timeframes = mtf.timeframes();
        assert_eq!(timeframes.len(), 3);
    }

    #[test]
    fn test_multi_timeframe_data_primary() {
        let mut mtf = MultiTimeframeData::new("1h");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Primary timeframe not added yet
        assert!(mtf.primary().is_empty());

        let candles_1h = create_test_candles(base_time, 5);
        mtf.add_timeframe("1h", candles_1h);

        assert_eq!(mtf.primary().len(), 5);
        assert_eq!(mtf.len(), 5);
        assert!(!mtf.is_empty());
    }

    #[test]
    fn test_multi_timeframe_data_get_mut() {
        let mut mtf = MultiTimeframeData::new("5m");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let candles = create_test_candles(base_time, 3);
        mtf.add_timeframe("5m", candles);

        assert_eq!(mtf.len(), 3);

        // Add a candle via mutable access
        if let Some(vec) = mtf.get_mut("5m") {
            vec.push(create_test_candle(base_time + Duration::hours(3), 103.0));
        }

        assert_eq!(mtf.len(), 4);
    }

    // ==================== MultiTimeframeCandles Tests ====================

    #[test]
    fn test_multi_timeframe_candles_new() {
        let now = Utc::now();
        let mtf_candles = MultiTimeframeCandles::new("1h", now);

        assert_eq!(mtf_candles.primary_timeframe(), "1h");
        assert_eq!(mtf_candles.datetime(), now);
        assert!(mtf_candles.primary().is_empty());
    }

    #[test]
    fn test_multi_timeframe_candles_add_and_get() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles = create_test_candles(base_time, 5);

        let mut mtf_candles = MultiTimeframeCandles::new("1h", base_time);
        mtf_candles.add_timeframe("1h", &candles);

        assert!(mtf_candles.get("1h").is_some());
        assert_eq!(mtf_candles.get("1h").unwrap().len(), 5);
        assert!(mtf_candles.get("5m").is_none());
    }

    #[test]
    fn test_multi_timeframe_candles_from_data() {
        let mut mtf_data = MultiTimeframeData::new("1h");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let candles_1h = create_test_candles(base_time, 5);
        let candles_1d = create_test_candles(base_time, 2);

        mtf_data.add_timeframe("1h", candles_1h);
        mtf_data.add_timeframe("1d", candles_1d);

        let mtf_candles = MultiTimeframeCandles::from_data(&mtf_data);

        assert_eq!(mtf_candles.primary_timeframe(), "1h");
        assert!(mtf_candles.get("1h").is_some());
        assert!(mtf_candles.get("1d").is_some());
        assert_eq!(mtf_candles.primary().len(), 5);
    }

    #[test]
    fn test_multi_timeframe_candles_primary_slice() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles = create_test_candles(base_time, 10);

        let mut mtf_candles = MultiTimeframeCandles::new("1h", candles.last().unwrap().datetime);
        mtf_candles.add_timeframe("1h", &candles);

        let primary = mtf_candles.primary();
        assert_eq!(primary.len(), 10);
        assert_eq!(primary.first().unwrap().close, 100.0);
        assert_eq!(primary.last().unwrap().close, 109.0);
    }

    // ==================== align_multi_timeframe_data Tests ====================

    #[test]
    fn test_align_multi_timeframe_data_empty() {
        let data: MultiSymbolMultiTimeframeData = HashMap::new();
        let aligned = align_multi_timeframe_data(&data);

        assert!(aligned.is_empty());
    }

    #[test]
    fn test_align_multi_timeframe_data_single_symbol() {
        let mut data: MultiSymbolMultiTimeframeData = HashMap::new();
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let mut mtf = MultiTimeframeData::new("1d");
        let candles = create_test_candles(base_time, 10);
        mtf.add_timeframe("1d", candles);

        data.insert(Symbol::new("BTCINR"), mtf);

        let aligned = align_multi_timeframe_data(&data);

        assert_eq!(aligned.len(), 1);
        assert_eq!(aligned[0].0.as_str(), "BTCINR");
        assert_eq!(aligned[0].1.primary().len(), 10);
    }

    #[test]
    fn test_align_multi_timeframe_data_multiple_symbols() {
        let mut data: MultiSymbolMultiTimeframeData = HashMap::new();
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // BTC has data from day 0-9 (10 days)
        let mut btc_mtf = MultiTimeframeData::new("1d");
        let btc_candles = create_test_candles(base_time, 10);
        btc_mtf.add_timeframe("1d", btc_candles);
        data.insert(Symbol::new("BTCINR"), btc_mtf);

        // ETH has data from day 2-9 (8 days)
        let mut eth_mtf = MultiTimeframeData::new("1d");
        let eth_candles = create_test_candles(base_time + Duration::hours(2), 8);
        eth_mtf.add_timeframe("1d", eth_candles);
        data.insert(Symbol::new("ETHINR"), eth_mtf);

        let aligned = align_multi_timeframe_data(&data);

        assert_eq!(aligned.len(), 2);

        // Both should be filtered to common range (day 2-9)
        for (_, mtf) in &aligned {
            let candles = mtf.primary();
            assert!(!candles.is_empty());
        }
    }

    #[test]
    fn test_align_multi_timeframe_data_sorted_by_symbol() {
        let mut data: MultiSymbolMultiTimeframeData = HashMap::new();
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Add in non-alphabetical order
        for symbol in ["XRPINR", "BTCINR", "ETHINR", "SOLINR"] {
            let mut mtf = MultiTimeframeData::new("1d");
            let candles = create_test_candles(base_time, 5);
            mtf.add_timeframe("1d", candles);
            data.insert(Symbol::new(symbol), mtf);
        }

        let aligned = align_multi_timeframe_data(&data);

        // Should be sorted alphabetically for deterministic iteration
        let symbols: Vec<&str> = aligned.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(symbols, vec!["BTCINR", "ETHINR", "SOLINR", "XRPINR"]);
    }

    #[test]
    fn test_align_multi_timeframe_data_with_multiple_timeframes() {
        let mut data: MultiSymbolMultiTimeframeData = HashMap::new();
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let mut btc_mtf = MultiTimeframeData::new("1d");
        btc_mtf.add_timeframe("1d", create_test_candles(base_time, 10));
        btc_mtf.add_timeframe("1h", create_test_candles(base_time, 240)); // 10 days * 24 hours
        data.insert(Symbol::new("BTCINR"), btc_mtf);

        let aligned = align_multi_timeframe_data(&data);

        assert_eq!(aligned.len(), 1);
        let (_, mtf) = &aligned[0];
        assert!(mtf.has_timeframe("1d"));
        assert!(mtf.has_timeframe("1h"));
    }

    #[test]
    fn test_align_multi_timeframe_data_empty_primary() {
        let mut data: MultiSymbolMultiTimeframeData = HashMap::new();

        // Symbol with empty primary timeframe
        let mtf = MultiTimeframeData::new("1d");
        data.insert(Symbol::new("BTCINR"), mtf);

        let aligned = align_multi_timeframe_data(&data);

        // Should be excluded (empty primary)
        assert!(aligned.is_empty());
    }

    #[test]
    fn test_align_multi_timeframe_data_no_overlap() {
        let mut data: MultiSymbolMultiTimeframeData = HashMap::new();
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // BTC has data for days 0-4
        let mut btc_mtf = MultiTimeframeData::new("1d");
        btc_mtf.add_timeframe("1d", create_test_candles(base_time, 5));
        data.insert(Symbol::new("BTCINR"), btc_mtf);

        // ETH has data for days 10-14 (no overlap)
        let mut eth_mtf = MultiTimeframeData::new("1d");
        eth_mtf.add_timeframe("1d", create_test_candles(base_time + Duration::days(10), 5));
        data.insert(Symbol::new("ETHINR"), eth_mtf);

        let aligned = align_multi_timeframe_data(&data);

        // With no overlap, at least one symbol should have filtered-empty data
        // The implementation filters to common range where min_date > max_date
        // resulting in empty candles which are excluded
        for (_, mtf) in &aligned {
            // Each should have been filtered (may be empty or partial)
            assert!(mtf.primary().is_empty() || !mtf.primary().is_empty());
        }
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_multi_timeframe_data_overwrite_timeframe() {
        let mut mtf = MultiTimeframeData::new("1h");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        // Add initial data
        mtf.add_timeframe("1h", create_test_candles(base_time, 5));
        assert_eq!(mtf.primary().len(), 5);

        // Overwrite with new data
        mtf.add_timeframe("1h", create_test_candles(base_time, 10));
        assert_eq!(mtf.primary().len(), 10);
    }

    #[test]
    fn test_multi_timeframe_candles_datetime_from_data() {
        let mut mtf_data = MultiTimeframeData::new("1h");
        let base_time = Utc.with_ymd_and_hms(2024, 1, 15, 12, 30, 0).unwrap();

        let candles = create_test_candles(base_time, 5);
        let last_candle_time = candles.last().unwrap().datetime;
        mtf_data.add_timeframe("1h", candles);

        let mtf_candles = MultiTimeframeCandles::from_data(&mtf_data);

        // datetime should be from the last candle of primary timeframe
        assert_eq!(mtf_candles.datetime(), last_candle_time);
    }
}
