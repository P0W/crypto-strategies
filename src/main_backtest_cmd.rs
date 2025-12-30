//! Backtest command implementation

use anyhow::Result;
use crypto_strategies::strategies::volatility_regime;
use crypto_strategies::{backtest::Backtester, data, Config, Strategy};
use tracing::{debug, info};

pub fn run(
    config_path: String,
    strategy_override: Option<String>,
    capital_override: Option<f64>,
    start_override: Option<String>,
    end_override: Option<String>,
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

    if let Some(start) = start_override {
        info!("Overriding start date to: {}", start);
        config.backtest.start_date = Some(start);
    }

    if let Some(end) = end_override {
        info!("Overriding end date to: {}", end);
        config.backtest.end_date = Some(end);
    }

    // Load data
    info!("Loading data from: {}", config.backtest.data_dir);
    let symbols = config.trading.symbols();
    debug!("Symbols: {:?}", symbols);

    let data = data::load_multi_symbol(
        &config.backtest.data_dir,
        &symbols,
        &config.trading.timeframe,
    )?;

    info!("Loaded data for {} symbols", data.len());

    // Create strategy based on config
    info!("Creating strategy: {}", config.strategy_name);
    let strategy: Box<dyn Strategy> = match config.strategy_name.as_str() {
        "volatility_regime" => Box::new(volatility_regime::create_strategy_from_config(&config)?),
        other => {
            anyhow::bail!(
                "Unknown strategy: {}. Available strategies: volatility_regime",
                other
            )
        }
    };

    let mut backtester = Backtester::new(config.clone(), strategy);

    info!("Running backtest...");
    let result = backtester.run(data);

    // Print results
    println!("\n{}", "=".repeat(60));
    println!("BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    println!("Initial Capital:    ₹{:.2}", config.trading.initial_capital);
    println!("Total Return:       {:.2}%", result.metrics.total_return);
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
    println!("{}", "=".repeat(60));

    info!("Backtest completed successfully");

    Ok(())
}
