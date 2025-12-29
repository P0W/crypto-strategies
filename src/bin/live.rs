//! Live trading binary
//!
//! Run strategy in live trading mode (paper or real).

use anyhow::Result;
use clap::Parser;
use crypto_strategies::Config;

#[derive(Parser, Debug)]
#[command(name = "live")]
#[command(about = "Run live trading", long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "configs/btc_eth_sol_bnb_xrp_1d.json")]
    config: String,

    /// Paper trading mode (safe, no real money)
    #[arg(long)]
    paper: bool,

    /// Live trading mode (CAUTION - REAL MONEY!)
    #[arg(long)]
    live: bool,

    /// Cycle interval in seconds
    #[arg(long, default_value = "300")]
    interval: u64,

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
    let _config = Config::from_file(&args.config)?;

    if !args.paper && !args.live {
        log::error!("Must specify either --paper or --live mode");
        std::process::exit(1);
    }

    if args.live {
        log::warn!("LIVE TRADING MODE - REAL MONEY AT RISK!");
        log::warn!("Press Ctrl+C within 5 seconds to abort...");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    log::info!(
        "Starting live trading in {} mode (interval: {}s)",
        if args.paper { "PAPER" } else { "LIVE" },
        args.interval
    );

    // TODO: Implement live trading loop
    log::warn!("Live trading implementation pending - this is a stub");

    Ok(())
}
