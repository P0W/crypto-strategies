//! Backtest command implementation

use anyhow::Result;
use chrono::{DateTime, Utc};
use crypto_strategies::monthly_pnl::MonthlyPnLMatrix;
use crypto_strategies::multi_timeframe::MultiTimeframeData;
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
        if let Some(obj) = config.strategy.as_object_mut() {
            obj.insert("name".to_string(), serde_json::json!(strategy));
        }
    }

    if let Some(capital) = capital_override {
        info!("Overriding initial capital to: ₹{:.2}", capital);
        config.trading.initial_capital = capital;
    }

    if no_risk_limits {
        info!("Risk limits DISABLED");
        config.trading.max_drawdown = 1.0;
        config.trading.max_positions = 100;
        config.trading.max_portfolio_heat = 1.0;
    }

    // Parse date filters
    let start_date: Option<DateTime<Utc>> = start_override
        .as_ref()
        .map(|s| data::parse_date(s))
        .transpose()?;
    let end_date: Option<DateTime<Utc>> = end_override
        .as_ref()
        .map(|s| data::parse_date(s))
        .transpose()?;

    if let Some(ref start) = start_date {
        info!("Start date: {}", start);
    }
    if let Some(ref end) = end_date {
        info!("End date: {}", end);
    }

    // Get symbols and primary timeframe
    let symbols = config.trading.symbols();
    let primary_tf = config.timeframe();
    debug!("Symbols: {:?}, Primary TF: {}", symbols, primary_tf);

    // Create strategy to query its requirements
    info!("Creating strategy: {}", config.strategy_name());
    let strategy = strategies::create_strategy(&config)?;
    let required_tfs = strategy.required_timeframes();

    // Build complete timeframe list
    let mut all_tfs: Vec<&str> = required_tfs;
    if !all_tfs.contains(&primary_tf.as_str()) {
        all_tfs.push(&primary_tf);
    }

    info!("Loading timeframes: {:?}", all_tfs);

    // Check and fetch missing data
    let tf_strings: Vec<String> = all_tfs.iter().map(|s| s.to_string()).collect();
    data::check_and_fetch_data(
        &config.backtest.data_dir,
        &symbols,
        &tf_strings,
        start_date,
        end_date,
    )?;

    // Load data - always use MTF format (unified interface)
    let mtf_data = if all_tfs.len() > 1 {
        // Multi-timeframe
        data::load_multi_timeframe(
            &config.backtest.data_dir,
            &symbols,
            &all_tfs,
            &primary_tf,
            start_date,
            end_date,
        )?
    } else {
        // Single-timeframe - wrap in MTF format
        let single_data = data::load_multi_symbol_with_range(
            &config.backtest.data_dir,
            &symbols,
            &primary_tf,
            start_date,
            end_date,
        )?;

        single_data
            .into_iter()
            .map(|(symbol, candles)| {
                let mut mtf = MultiTimeframeData::new(&primary_tf);
                mtf.add_timeframe(&primary_tf, candles);
                (symbol, mtf)
            })
            .collect()
    };

    info!("Loaded data for {} symbols", mtf_data.len());

    // Run backtest
    let mut backtester = Backtester::new(config.clone(), strategy);
    let result = backtester.run(&mtf_data);

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

    // Monthly P&L matrix
    let monthly = MonthlyPnLMatrix::from_trades(&result.trades);
    print!("{}", monthly.render_colored());

    info!("Backtest completed");
    Ok(())
}
