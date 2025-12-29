//! Optimization binary
//!
//! Run parameter optimization from the command line.

use anyhow::Result;
use clap::Parser;
use crypto_strategies::{Config, data, optimize::{Optimizer, ParamGrid}};

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

    /// Sort results by metric (sharpe, calmar, return, win_rate)
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
    let param_grid = match args.mode.as_str() {
        "full" => ParamGrid::full(),
        "quick" | _ => ParamGrid::quick(),
    };

    // Run optimization
    let optimizer = Optimizer::new(config.clone());
    log::info!("Starting optimization in {} mode...", args.mode);
    
    let mut results = optimizer.optimize(data, param_grid);

    // Sort results
    match args.sort_by.as_str() {
        "calmar" => results.sort_by(|a, b| b.sharpe_ratio.partial_cmp(&a.sharpe_ratio).unwrap()),
        "return" => results.sort_by(|a, b| b.total_return.partial_cmp(&a.total_return).unwrap()),
        "win_rate" => results.sort_by(|a, b| b.win_rate.partial_cmp(&a.win_rate).unwrap()),
        "sharpe" | _ => results.sort_by(|a, b| b.sharpe_ratio.partial_cmp(&a.sharpe_ratio).unwrap()),
    }

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
