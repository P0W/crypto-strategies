//! Download command - fetch historical data from CoinDCX
//! Like Python's data_fetcher.py main() function

use anyhow::Result;
use crypto_strategies::data::{load_csv, CoinDCXDataFetcher};
use tracing::info;

pub fn run(pairs: String, interval: String, days: u32, output: String) -> Result<()> {
    info!("Starting data download");

    // Create a tokio runtime for async operations
    let rt = tokio::runtime::Runtime::new()?;

    let fetcher = CoinDCXDataFetcher::new(&output);

    // Parse pairs
    let symbols: Vec<&str> = pairs.split(',').map(|s| s.trim()).collect();

    println!("\n{}", "=".repeat(60));
    println!("DOWNLOADING HISTORICAL DATA FROM COINDCX");
    println!("{}", "=".repeat(60));
    println!("  Pairs:      {:?}", symbols);
    println!("  Interval:   {}", interval);
    println!("  Days:       {}", days);
    println!("  Output:     {}", output);
    println!("{}\n", "=".repeat(60));

    let mut total_candles = 0;
    let mut success_count = 0;

    for symbol in &symbols {
        println!("Downloading {}...", symbol);

        match rt.block_on(fetcher.download_pair(symbol, &interval, days)) {
            Ok(filepath) => {
                if let Ok(candles) = load_csv(&filepath) {
                    total_candles += candles.len();
                    println!(
                        "  ✓ {} candles saved to {}",
                        candles.len(),
                        filepath.display()
                    );
                    success_count += 1;
                }
            }
            Err(e) => {
                println!("  ✗ Error: {}", e);
            }
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("DOWNLOAD COMPLETE");
    println!("{}", "=".repeat(60));
    println!("  Successful: {}/{}", success_count, symbols.len());
    println!("  Total candles: {}", total_candles);
    println!("{}", "=".repeat(60));

    Ok(())
}
