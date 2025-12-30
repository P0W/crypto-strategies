//! Data loading and management
//!
//! Handles loading OHLCV data from CSV files using Polars for efficient
//! data processing.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;

use crate::{Candle, Symbol};

/// Load OHLCV data from CSV file
pub fn load_csv(path: impl AsRef<Path>) -> Result<Vec<Candle>> {
    let df = CsvReadOptions::default()
        .with_has_header(true)
        .try_into_reader_with_file_path(Some(path.as_ref().to_path_buf()))?
        .finish()
        .context("Failed to read CSV")?;

    let datetime_col = df.column("datetime")
        .context("Missing datetime column")?
        .str()
        .context("Datetime column not string")?;
    let open_col = df.column("open")
        .context("Missing open column")?
        .f64()
        .context("Open column not f64")?;
    let high_col = df.column("high")
        .context("Missing high column")?
        .f64()
        .context("High column not f64")?;
    let low_col = df.column("low")
        .context("Missing low column")?
        .f64()
        .context("Low column not f64")?;
    let close_col = df.column("close")
        .context("Missing close column")?
        .f64()
        .context("Close column not f64")?;
    let volume_col = df.column("volume")
        .context("Missing volume column")?
        .f64()
        .context("Volume column not f64")?;

    let mut candles = Vec::new();

    for i in 0..df.height() {
        // Get datetime string - for StringChunked, get() returns Option<&str>
        let dt_str = datetime_col.get(i)
            .context("Failed to get datetime string")?;
        
        let datetime = dt_str.parse::<DateTime<Utc>>()
            .or_else(|_| {
                // Try parsing without timezone and assume UTC
                chrono::NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S")
                    .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
            })
            .context(format!("Failed to parse datetime: {}", dt_str))?;

        let open = open_col.get(i).context("Missing open value")?;
        let high = high_col.get(i).context("Missing high value")?;
        let low = low_col.get(i).context("Missing low value")?;
        let close = close_col.get(i).context("Missing close value")?;
        let volume = volume_col.get(i).context("Missing volume value")?;

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

/// Load data for multiple symbols
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
            log::warn!("Data file not found: {}", path.display());
            continue;
        }

        let candles = load_csv(&path)
            .context(format!("Failed to load data for {}", symbol))?;
        
        log::info!("Loaded {} candles for {}", candles.len(), symbol);
        data.insert(symbol.clone(), candles);
    }

    if data.is_empty() {
        anyhow::bail!("No data loaded for any symbol");
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_load_csv() {
        // This would need actual test data
        // Just ensuring the module compiles for now
    }
}
