//! Backtest binary
//!
//! Run strategy backtests from the command line.

use anyhow::Result;
use clap::Parser;
use crypto_strategies::{Config, data, backtest::Backtester};
use crypto_strategies::strategies::volatility_regime;

#[derive(Parser, Debug)]
#[command(name = "backtest")]
#[command(about = "Run strategy backtest", long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "configs/btc_eth_sol_bnb_xrp_1d.json")]
    config: String,

    /// Initial capital
    #[arg(long)]
    capital: Option<f64>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    start: Option<String>,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    end: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logger
    let log_level = if args.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .init();

    // Load configuration
    let mut config = Config::from_file(&args.config)?;

    if let Some(capital) = args.capital {
        config.trading.initial_capital = capital;
    }

    if let Some(start) = args.start {
        config.backtest.start_date = Some(start);
    }

    if let Some(end) = args.end {
        config.backtest.end_date = Some(end);
    }

    log::info!("Loading data from {}", config.backtest.data_dir);
    let symbols = config.trading.symbols();
    let data = data::load_multi_symbol(
        &config.backtest.data_dir,
        &symbols,
        &config.trading.timeframe,
    )?;

    log::info!("Loaded data for {} symbols", data.len());

    // Create strategy using the strategy module's utility
    let strategy = Box::new(volatility_regime::create_strategy_from_config(&config)?);
    let mut backtester = Backtester::new(config.clone(), strategy);

    log::info!("Running backtest...");
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

    if args.verbose {
        println!("\nTRADES:");
        for (i, trade) in result.trades.iter().enumerate() {
            println!(
                "#{} {} | Entry: ₹{:.2} @ {} | Exit: ₹{:.2} @ {} | P&L: ₹{:.2} ({:.2}%)",
                i + 1,
                trade.symbol,
                trade.entry_price,
                trade.entry_time.format("%Y-%m-%d"),
                trade.exit_price,
                trade.exit_time.format("%Y-%m-%d"),
                trade.net_pnl,
                trade.return_pct()
            );
        }
    }

    Ok(())
}
