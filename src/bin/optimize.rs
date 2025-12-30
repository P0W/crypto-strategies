//! Optimization binary
//!
//! Run parameter optimization from the command line.

use anyhow::Result;
use clap::Parser;
use crypto_strategies::{Config, data, optimizer::Optimizer};
use crypto_strategies::strategies::volatility_regime::{self, GridParams};

#[derive(Parser, Debug)]
#[command(name = "optimize")]
#[command(about = "Optimize strategy parameters", long_about = None)]
struct Args {
    /// Path to base configuration file
    #[arg(short, long, default_value = "configs/btc_eth_sol_bnb_xrp_1d.json")]
    config: String,

    /// Optimization mode (quick or full)
    #[arg(short, long, default_value = "quick")]
    mode: String,

    /// Sort results by metric (sharpe, calmar, return, win_rate, profit_factor)
    #[arg(long, default_value = "sharpe")]
    sort_by: String,

    /// Number of top results to show
    #[arg(short, long, default_value = "10")]
    top: usize,
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    let args = Args::parse();

    // Load base configuration
    let config = Config::from_file(&args.config)?;

    log::info!("Loading data from {}", config.backtest.data_dir);
    let symbols = config.trading.symbols();
    let data = data::load_multi_symbol(
        &config.backtest.data_dir,
        &symbols,
        &config.trading.timeframe,
    )?;

    log::info!("Loaded data for {} symbols", data.len());

    // Create parameter grid
    let grid_params = match args.mode.as_str() {
        "full" => GridParams::full(),
        "quick" | _ => GridParams::quick(),
    };

    log::info!("Grid will test {} combinations", grid_params.total_combinations());

    // Generate all configs from grid
    let configs = volatility_regime::generate_configs(&config, &grid_params);

    // Run optimization using the generic optimizer
    let optimizer = Optimizer::new(config.clone());
    log::info!("Starting optimization in {} mode...", args.mode);
    
    let strategy_factory = |cfg: &Config| {
        Box::new(volatility_regime::create_strategy_from_config(cfg)) as Box<dyn crypto_strategies::Strategy>
    };
    
    let mut results = optimizer.optimize(data, configs.clone(), strategy_factory);

    // Add parameter information to results
    for (i, result) in results.iter_mut().enumerate() {
        let vr_config = volatility_regime::VolatilityRegimeConfig {
            atr_period: configs[i].strategy.atr_period,
            volatility_lookback: configs[i].strategy.volatility_lookback,
            compression_threshold: configs[i].strategy.compression_threshold,
            expansion_threshold: configs[i].strategy.expansion_threshold,
            extreme_threshold: configs[i].strategy.extreme_threshold,
            ema_fast: configs[i].strategy.ema_fast,
            ema_slow: configs[i].strategy.ema_slow,
            adx_period: configs[i].strategy.adx_period,
            adx_threshold: configs[i].strategy.adx_threshold,
            breakout_atr_multiple: configs[i].strategy.breakout_atr_multiple,
            stop_atr_multiple: configs[i].strategy.stop_atr_multiple,
            target_atr_multiple: configs[i].strategy.target_atr_multiple,
            trailing_activation: configs[i].strategy.trailing_activation,
            trailing_atr_multiple: configs[i].strategy.trailing_atr_multiple,
        };
        result.params = volatility_regime::config_to_params(&vr_config);
    }

    // Sort results
    Optimizer::sort_results(&mut results, &args.sort_by);

    // Display top results
    println!("\n{}", "=".repeat(100));
    println!("TOP {} OPTIMIZATION RESULTS (sorted by {})", args.top.min(results.len()), args.sort_by);
    println!("{}", "=".repeat(100));
    println!(
        "{:<6} {:>8} {:>10} {:>10} {:>10} {:>8} | {}",
        "Rank", "Sharpe", "Return%", "MaxDD%", "WinRate%", "Trades", "Parameters"
    );
    println!("{}", "-".repeat(100));

    for (i, result) in results.iter().take(args.top).enumerate() {
        let params_str = format!(
            "ATR:{} EMA:{}/{} ADX:{} Stop:{:.1} Target:{:.1}",
            *result.params.get("atr_period").unwrap_or(&0.0) as usize,
            *result.params.get("ema_fast").unwrap_or(&0.0) as usize,
            *result.params.get("ema_slow").unwrap_or(&0.0) as usize,
            result.params.get("adx_threshold").unwrap_or(&0.0),
            result.params.get("stop_atr_multiple").unwrap_or(&0.0),
            result.params.get("target_atr_multiple").unwrap_or(&0.0),
        );

        println!(
            "{:<6} {:>8.2} {:>10.2} {:>10.2} {:>10.2} {:>8} | {}",
            i + 1,
            result.sharpe_ratio,
            result.total_return,
            result.max_drawdown,
            result.win_rate,
            result.total_trades,
            params_str
        );
    }
    println!("{}", "=".repeat(100));

    Ok(())
}
