//! Backtest command implementation

use anyhow::Result;
use chrono::{DateTime, Utc};
use crypto_strategies::analysis::{DayOfWeekAnalysis, MonthlyPnLMatrix, StreakAnalysis};
use crypto_strategies::multi_timeframe::MultiTimeframeData;
use crypto_strategies::strategies;
use crypto_strategies::{backtest::Backtester, data, Config};
use tracing::{debug, info};

/// Options for running a backtest
#[derive(Default)]
pub struct BacktestOptions {
    pub config_path: String,
    pub strategy_override: Option<String>,
    pub capital_override: Option<f64>,
    pub symbols_override: Option<String>,
    pub start_override: Option<String>,
    pub end_override: Option<String>,
    pub no_risk_limits: bool,
    pub use_t1_execution: bool,
}

pub fn run(opts: BacktestOptions) -> Result<()> {
    let BacktestOptions {
        config_path,
        strategy_override,
        capital_override,
        symbols_override,
        start_override,
        end_override,
        no_risk_limits,
        use_t1_execution,
    } = opts;
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

    if let Some(ref symbols_str) = symbols_override {
        let symbols: Vec<String> = symbols_str
            .split(',')
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .collect();
        info!("Overriding symbols to: {:?}", symbols);
        config.trading.symbols = symbols;
    }

    if no_risk_limits {
        info!("Risk limits DISABLED");
        config.trading.max_drawdown = 1.0;
        config.trading.max_positions = 100;
        config.trading.max_portfolio_heat = 1.0;
    }

    if use_t1_execution {
        info!("Using T+1 execution model (signal on day N, execute at day N+1 open)");
        config.backtest.use_t1_execution = true;
    } else {
        info!("Using intra-candle execution model (realistic algo trading)");
        config.backtest.use_t1_execution = false;
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
    let load_start_date = start_date.map(|start| data::warmup_start(start, &tf_strings, 300));
    data::check_and_fetch_data(
        &config.backtest.data_dir,
        &symbols,
        &tf_strings,
        load_start_date,
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
            load_start_date,
            end_date,
        )?
    } else {
        // Single-timeframe - wrap in MTF format
        let single_data = data::load_multi_symbol_with_range(
            &config.backtest.data_dir,
            &symbols,
            &primary_tf,
            load_start_date,
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

    // Extract actual date range from loaded data
    let (mut data_start, data_end) = mtf_data
        .values()
        .next()
        .and_then(|mtf| {
            let candles = mtf.primary();
            if candles.is_empty() {
                None
            } else {
                Some((
                    candles.first().unwrap().datetime,
                    candles.last().unwrap().datetime,
                ))
            }
        })
        .unzip();
    if start_date.is_some() {
        data_start = start_date;
    }

    // Run backtest
    let mut backtester =
        Backtester::new(config.clone(), strategy).with_evaluation_start(start_date);
    let result = backtester.run(&mtf_data);

    // Print results
    println!("\n{}", "=".repeat(60));
    println!("BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    if let Some(start) = data_start {
        println!("Start Date:         {}", start.format("%Y-%m-%d"));
    }
    if let Some(end) = data_end {
        println!("End Date:           {}", end.format("%Y-%m-%d"));
    }
    println!("Initial Capital:    ₹{:.2}", config.trading.initial_capital);
    println!("Total Return:       {:.2}%", result.metrics.total_return);
    println!("Post-Tax Return:    {:.2}%", result.metrics.post_tax_return);
    println!("Sharpe Ratio:       {:.2}", result.metrics.sharpe_ratio);
    println!("Calmar Ratio:       {:.2}", result.metrics.calmar_ratio);
    println!("{}", "-".repeat(60));
    println!("DRAWDOWN METRICS");
    println!("{}", "-".repeat(60));
    println!("Max Drawdown:       {:.2}%", result.metrics.max_drawdown);
    println!("Avg Drawdown:       {:.2}%", result.metrics.avg_drawdown);
    println!(
        "Underwater Time:    {:.1}%",
        result.metrics.underwater_time_pct
    );
    println!(
        "Max Underwater:     {} bars",
        result.metrics.max_underwater_bars
    );
    println!("Recovery Factor:    {:.2}", result.metrics.recovery_factor);
    println!("{}", "-".repeat(60));
    println!("TRADE STATISTICS");
    println!("{}", "-".repeat(60));
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

    // Performance breakdowns
    let streaks = StreakAnalysis::from_trades(&result.trades);
    print!("{}", streaks.render());

    let dow = DayOfWeekAnalysis::from_trades(&result.trades);
    print!("{}", dow.render());

    let monthly = MonthlyPnLMatrix::from_trades(&result.trades);
    print!("{}", monthly.render_colored());
    print!(
        "{}",
        monthly.render_yearly_summary(config.trading.initial_capital)
    );

    info!("Backtest completed");
    Ok(())
}
