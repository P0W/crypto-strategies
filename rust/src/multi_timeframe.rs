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

    aligned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_timeframe_data() {
        let mut mtf = MultiTimeframeData::new("5m");

        let candles_5m = vec![
            Candle::new(chrono::Utc::now(), 100.0, 110.0, 90.0, 105.0, 1000.0).unwrap(), // Unwrap the Result
        ];

        mtf.add_timeframe("5m", candles_5m);

        assert_eq!(mtf.len(), 1);
        assert!(mtf.has_timeframe("5m"));
        assert!(!mtf.has_timeframe("1d"));
        assert_eq!(mtf.primary().len(), 1);
    }
}
