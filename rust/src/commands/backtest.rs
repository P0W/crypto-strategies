//! Backtest command implementation

use anyhow::Result;
use crypto_strategies::strategies;
use crypto_strategies::{backtest::Backtester, data, Config};
use tracing::{debug, info};

pub fn run(
    config_path: String,
    strategy_override: Option<String>,
    capital_override: Option<f64>,
    _start_override: Option<String>,
    _end_override: Option<String>,
) -> Result<()> {
    info!("Starting backtest");

    // Load configuration
    let mut config = Config::from_file(&config_path)?;
    info!("Loaded configuration from: {}", config_path);

    // Apply overrides
    if let Some(strategy) = strategy_override {
        info!("Overriding strategy to: {}", strategy);
        config.strategy_name = strategy;
    }

    if let Some(capital) = capital_override {
        info!("Overriding initial capital to: ₹{:.2}", capital);
        config.trading.initial_capital = capital;
    }

    // Note: start/end date filtering not yet implemented in data loader

    // Load data
    info!("Loading data from: {}", config.backtest.data_dir);
    let symbols = config.trading.symbols();
    let timeframe = config.timeframe();
    debug!("Symbols: {:?}", symbols);

    // Check for missing data and fetch if needed
    let timeframes = vec![timeframe.clone()];
    let missing = data::find_missing_data(&config.backtest.data_dir, &symbols, &timeframes);
    
    if !missing.is_empty() {
        println!("\n{}", "=".repeat(60));
        println!("FETCHING MISSING DATA");
        println!("{}", "=".repeat(60));
        println!("  Missing files: {}", missing.len());
        for (sym, tf) in &missing {
            println!("    - {}_{}.csv", sym.as_str(), tf);
        }
        println!("{}\n", "-".repeat(60));

        // Fetch missing data (default 365 days)
        match data::ensure_data_available_sync(
            &config.backtest.data_dir,
            &symbols,
            &timeframes,
            365,
        ) {
            Ok(failed) => {
                if !failed.is_empty() {
                    println!("  ⚠ Could not fetch {} files:", failed.len());
                    for (sym, tf) in &failed {
                        println!("    - {}_{}.csv", sym.as_str(), tf);
                    }
                } else {
                    println!("  ✓ All missing data fetched successfully");
                }
            }
            Err(e) => {
                println!("  ⚠ Error fetching data: {}", e);
            }
        }
        println!("{}\n", "=".repeat(60));
    }

    let data = data::load_multi_symbol(
        &config.backtest.data_dir,
        &symbols,
        &timeframe,
    )?;

    info!("Loaded data for {} symbols", data.len());

    // Create strategy based on config
    info!("Creating strategy: {}", config.strategy_name);
    let strategy = strategies::create_strategy(&config)?;

    let mut backtester = Backtester::new(config.clone(), strategy);

    info!("Running backtest...");
    let result = backtester.run(data);

    // Print results
    println!("\n{}", "=".repeat(60));
    println!("BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    println!("Initial Capital:    ₹{:.2}", config.trading.initial_capital);
    println!("Total Return:       {:.2}%", result.metrics.total_return);
    println!("Post-Tax Return:    {:.2}%", result.metrics.post_tax_return);
    println!("Sharpe Ratio:       {:.2}", result.metrics.sharpe_ratio);
    println!("Calmar Ratio:       {:.2}", result.metrics.calmar_ratio);
    println!("Max Drawdown:       {:.2}%", result.metrics.max_drawdown);
    println!("Win Rate:           {:.2}%", result.metrics.win_rate);
    println!("Profit Factor:      {:.2}", result.metrics.profit_factor);
    println!("Total Trades:       {}", result.metrics.total_trades);
    println!("Winning Trades:     {}", result.metrics.winning_trades);
    println!("Losing Trades:      {}", result.metrics.losing_trades);
    println!("Average Win:        ₹{:.2}", result.metrics.avg_win);
    println!("Average Loss:       ₹{:.2}", result.metrics.avg_loss);
    println!("Largest Win:        ₹{:.2}", result.metrics.largest_win);
    println!("Largest Loss:       ₹{:.2}", result.metrics.largest_loss);
    println!("{}", "-".repeat(60));
    println!(
        "Total Commission:   ₹{:.2}",
        result.metrics.total_commission
    );
    println!("Tax (30%):          ₹{:.2}", result.metrics.tax_amount);
    println!("{}", "=".repeat(60));

    info!("Backtest completed successfully");

    Ok(())
}
