//! Download 15-minute OHLCV data from CoinDCX
//!
//! This binary downloads actual 15m candle data from the exchange
//! instead of generating it from 5m data.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use csv::Writer;
use std::path::Path;
use std::thread;
use std::time::Duration;

#[derive(Debug, serde::Deserialize)]
struct CandleData {
    #[serde(rename = "time")]
    timestamp: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

async fn download_candles(
    symbol: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
) -> Result<Vec<CandleData>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    // CoinDCX public API endpoint for candles
    // TODO: Update URL based on actual CoinDCX API documentation
    // Current endpoint is placeholder - verify at https://docs.coindcx.com/
    let url = format!(
        "https://public.coindcx.com/market_data/candles?pair={}&interval=15m&startTime={}&endTime={}",
        symbol,
        start_time.timestamp_millis(),
        end_time.timestamp_millis()
    );

    println!("Downloading {} 15m candles...", symbol);
    
    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to send request")?;

    if !response.status().is_success() {
        anyhow::bail!("API returned status: {}", response.status());
    }

    let candles: Vec<CandleData> = response
        .json()
        .await
        .context("Failed to parse JSON response")?;

    Ok(candles)
}

fn save_to_csv(candles: &[CandleData], output_path: &Path) -> Result<()> {
    let mut wtr = Writer::from_path(output_path)?;
    
    // Write header
    wtr.write_record(&["datetime", "open", "high", "low", "close", "volume"])?;

    // Write data
    for candle in candles {
        let dt = DateTime::from_timestamp(candle.timestamp / 1000, 0)
            .context("Invalid timestamp")?;
        wtr.write_record(&[
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            candle.open.to_string(),
            candle.high.to_string(),
            candle.low.to_string(),
            candle.close.to_string(),
            candle.volume.to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Symbols to download
    let symbols = vec!["BTCINR", "ETHINR", "SOLINR", "XRPINR", "BNBINR"];
    
    // Date range (adjust based on your needs)
    let end_time = Utc::now();
    let start_time = end_time - chrono::Duration::days(365); // Last 1 year

    let data_dir = Path::new("../data");
    if !data_dir.exists() {
        std::fs::create_dir_all(data_dir)?;
    }

    for symbol in symbols {
        println!("\nProcessing {}...", symbol);
        
        match download_candles(symbol, start_time, end_time).await {
            Ok(candles) => {
                let output_path = data_dir.join(format!("{}_15m.csv", symbol));
                save_to_csv(&candles, &output_path)?;
                println!("✓ Saved {} candles to {:?}", candles.len(), output_path);
            }
            Err(e) => {
                eprintln!("✗ Failed to download {}: {}", symbol, e);
                eprintln!("  Attempting to generate from 5m data as fallback...");
                
                // Fallback: generate from 5m if download fails
                // This will be handled by the existing generate_15m binary
            }
        }

        // Rate limiting: wait between requests
        thread::sleep(Duration::from_secs(2));
    }

    println!("\n✓ Download complete!");
    println!("Note: If any downloads failed, run 'cargo run --release --bin generate_15m' to generate from 5m data");

    Ok(())
}
