//! Backtest command implementation

use anyhow::Result;
use chrono::{DateTime, Utc};
use crypto_strategies::monthly_pnl::MonthlyPnLMatrix;
use crypto_strategies::strategies;
use crypto_strategies::{backtest::Backtester, data, Config};
use tracing::{debug, info};

pub fn run(
    config_path: String,
    strategy_override: Option<String>,
    capital_override: Option<f64>,
    start_override: Option<String>,
    end_override: Option<String>,
    no_risk_limits: bool,
) -> Result<()> {
    info!("Starting backtest");

    // Load configuration
    let mut config = Config::from_file(&config_path)?;
    info!("Loaded configuration from: {}", config_path);

    // Apply overrides
    if let Some(strategy) = strategy_override {
        info!("Overriding strategy to: {}", strategy);
        // Update the strategy name in the strategy object
        if let Some(obj) = config.strategy.as_object_mut() {
            obj.insert("name".to_string(), serde_json::json!(strategy));
        }
    }

    if let Some(capital) = capital_override {
        info!("Overriding initial capital to: ₹{:.2}", capital);
        config.trading.initial_capital = capital;
    }

    // Disable risk limits if requested
    if no_risk_limits {
        info!("Risk limits DISABLED - no drawdown halt, unlimited positions");
        config.trading.max_drawdown = 1.0; // 100% - effectively no limit
        config.trading.max_positions = 100; // Effectively unlimited
        config.trading.max_portfolio_heat = 1.0; // 100% - no limit
    }

    // Parse date range filters
    let start_date: Option<DateTime<Utc>> = start_override
        .as_ref()
        .map(|s| data::parse_date(s))
        .transpose()?;
    let end_date: Option<DateTime<Utc>> = end_override
        .as_ref()
        .map(|s| data::parse_date(s))
        .transpose()?;

    if let Some(ref start) = start_date {
        info!("Filtering data from: {}", start);
    }
    if let Some(ref end) = end_date {
        info!("Filtering data until: {}", end);
    }

    // Load data
    info!("Loading data from: {}", config.backtest.data_dir);
    let symbols = config.trading.symbols();
    let timeframe = config.timeframe();
    debug!("Symbols: {:?}", symbols);

    // Check for missing data and fetch if needed (including date range coverage)
    let timeframes = vec![timeframe.clone()];
    data::check_and_fetch_data(
        &config.backtest.data_dir,
        &symbols,
        &timeframes,
        start_date,
        end_date,
    )?;

    // Create strategy based on config
    info!("Creating strategy: {}", config.strategy_name());
    let strategy = strategies::create_strategy(&config)?;

    // Check if strategy requires multiple timeframes
    let required_tfs = strategy.required_timeframes();

    let mut backtester = Backtester::new(config.clone(), strategy);

    info!("Running backtest...");
    let result = if !required_tfs.is_empty() {
        // Multi-timeframe strategy
        info!(
            "Multi-timeframe strategy detected, loading timeframes: {:?}",
            required_tfs
        );

        // Build timeframes list: required TFs + primary timeframe
        let mut all_timeframes: Vec<&str> = required_tfs.clone();
        if !all_timeframes.contains(&timeframe.as_str()) {
            all_timeframes.push(&timeframe);
        }

        // Load multi-timeframe data
        let mtf_data = data::load_multi_timeframe(
            &config.backtest.data_dir,
            &symbols,
            &all_timeframes,
            &timeframe,
            start_date,
            end_date,
        )?;

        info!("Loaded multi-timeframe data for {} symbols", mtf_data.len());
        backtester.run_multi_timeframe(mtf_data)
    } else {
        // Single-timeframe strategy (backward compatibility)
        let data = data::load_multi_symbol_with_range(
            &config.backtest.data_dir,
            &symbols,
            &timeframe,
            start_date,
            end_date,
        )?;

        info!("Loaded data for {} symbols", data.len());
        backtester.run(data)
    };

    // Print results
    println!("\n{}", "=".repeat(60));
    println!("BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    if let Some(ref start) = start_date {
        println!("Start Date:         {}", start.format("%Y-%m-%d %H:%M:%S"));
    }
    if let Some(ref end) = end_date {
        println!("End Date:           {}", end.format("%Y-%m-%d %H:%M:%S"));
    }
    println!("Initial Capital:    ₹{:.2}", config.trading.initial_capital);
    println!("Total Return:       {:.2}%", result.metrics.total_return);
    println!("Post-Tax Return:    {:.2}%", result.metrics.post_tax_return);
    println!("Sharpe Ratio:       {:.2}", result.metrics.sharpe_ratio);
    println!("Calmar Ratio:       {:.2}", result.metrics.calmar_ratio);
    println!("Max Drawdown:       {:.2}%", result.metrics.max_drawdown);
    println!("Win Rate:           {:.2}%", result.metrics.win_rate);
    println!("Profit Factor:      {:.2}", result.metrics.profit_factor);
    println!("Expectancy:         ₹{:.2}", result.metrics.expectancy);
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

    // Generate and display monthly P&L matrix
    let monthly_matrix = MonthlyPnLMatrix::from_trades(&result.trades);

    // Use colored output for terminal display
    print!("{}", monthly_matrix.render_colored());

    info!("Backtest completed successfully");

    Ok(())
}
