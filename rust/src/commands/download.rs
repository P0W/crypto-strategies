//! Download command - fetch historical data from Binance (default) or CoinDCX
//! Like Python's download_binance_data.py script

use anyhow::Result;
use crypto_strategies::data::{load_csv, BinanceDataFetcher, CoinDCXDataFetcher, DataSource};
use tracing::info;

pub fn run(
    pairs: String,
    timeframes: String,
    days: u32,
    output: String,
    source: DataSource,
) -> Result<()> {
    info!("Starting data download from {}", source);

    // Create a tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new()?;

    // Parse pairs and timeframes
    let symbols: Vec<&str> = pairs.split(',').map(|s| s.trim()).collect();
    let intervals: Vec<&str> = timeframes.split(',').map(|s| s.trim()).collect();

    let source_name = match source {
        DataSource::Binance => "BINANCE",
        DataSource::CoinDCX => "COINDCX",
    };

    println!("\n{}", "=".repeat(60));
    println!("DOWNLOADING HISTORICAL DATA FROM {}", source_name);
    println!("{}", "=".repeat(60));
    println!("  Symbols:    {:?}", symbols);
    println!("  Timeframes: {:?}", intervals);
    println!("  Days:       {}", days);
    println!("  Output:     {}", output);
    println!("{}\n", "=".repeat(60));

    let mut total_candles = 0;
    let mut success_count = 0;
    let mut total_downloads = 0;

    match source {
        DataSource::Binance => {
            let fetcher = BinanceDataFetcher::new(&output);
            
            for symbol in &symbols {
                println!("\n{}:", symbol);
                
                for interval in &intervals {
                    total_downloads += 1;
                    print!("  Downloading {} {}... ", symbol, interval);
                    
                    match rt.block_on(fetcher.download_pair(symbol, interval, days)) {
                        Ok(filepath) => {
                            if let Ok(candles) = load_csv(&filepath) {
                                total_candles += candles.len();
                                println!("✓ {} candles", candles.len());
                                success_count += 1;
                            }
                        }
                        Err(e) => {
                            println!("✗ Error: {}", e);
                        }
                    }
                }
            }
        }
        DataSource::CoinDCX => {
            let fetcher = CoinDCXDataFetcher::new(&output);
            
            for symbol in &symbols {
                println!("\n{}:", symbol);
                
                for interval in &intervals {
                    total_downloads += 1;
                    print!("  Downloading {} {}... ", symbol, interval);
                    
                    match rt.block_on(fetcher.download_pair(symbol, interval, days)) {
                        Ok(filepath) => {
                            if let Ok(candles) = load_csv(&filepath) {
                                total_candles += candles.len();
                                println!("✓ {} candles", candles.len());
                                success_count += 1;
                            }
                        }
                        Err(e) => {
                            println!("✗ Error: {}", e);
                        }
                    }
                }
            }
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("DOWNLOAD COMPLETE");
    println!("{}", "=".repeat(60));
    println!("  Successful: {}/{}", success_count, total_downloads);
    println!("  Total candles: {}", total_candles);
    println!("{}", "=".repeat(60));

    Ok(())
}
