//! Crypto trading strategies - main entry point
//!
//! This binary provides three subcommands:
//! - backtest: Run strategy backtests
//! - optimize: Run parameter optimization
//! - live: Run live trading (paper or real)

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod main_backtest_cmd;
mod main_live_cmd;
mod main_optimize_cmd;

#[derive(Parser, Debug)]
#[command(name = "crypto-strategies")]
#[command(about = "Crypto trading strategies with backtesting, optimization, and live trading", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run strategy backtest
    Backtest {
        /// Path to configuration file
        #[arg(short, long, default_value = "configs/btc_eth_sol_bnb_xrp_1d.json")]
        config: String,

        /// Strategy name (overrides config file)
        #[arg(short, long)]
        strategy: Option<String>,

        /// Initial capital
        #[arg(long)]
        capital: Option<f64>,

        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: Option<String>,

        /// End date (YYYY-MM-DD)
        #[arg(long)]
        end: Option<String>,
    },

    /// Optimize strategy parameters
    Optimize {
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
    },

    /// Run live trading
    Live {
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

        /// State database path
        #[arg(long, default_value = "state.db")]
        state_db: String,
    },
}

fn setup_logging(verbose: bool, command_name: &str) -> Result<()> {
    // Create logs directory
    std::fs::create_dir_all("logs")?;

    // Create log file with naming pattern: {command}_{date}.log
    let log_filename = format!(
        "{}_{}.log",
        command_name,
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let log_path = PathBuf::from("logs").join(&log_filename);

    // File appender
    let file_appender = tracing_appender::rolling::never("logs", log_filename);

    // Console layer with custom format matching Python:
    // %(asctime)s %(levelname)-8s [%(funcName)s:%(lineno)d] %(message)s
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_line_number(true)
        .with_file(true)
        .with_ansi(true);

    // File layer - same format but without ANSI colors
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_target(true)
        .with_line_number(true)
        .with_file(true)
        .with_ansi(false);

    // Set log level
    let level = if verbose { "debug" } else { "info" };
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    // Initialize subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    info!("Logging initialized");
    info!("Log file: {}", log_path.display());

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine command name for logging
    let command_name = match &cli.command {
        Commands::Backtest { .. } => "backtest",
        Commands::Optimize { .. } => "optimize",
        Commands::Live { .. } => "live",
    };

    // Setup logging
    setup_logging(cli.verbose, command_name)?;

    // Execute command
    match cli.command {
        Commands::Backtest {
            config,
            strategy,
            capital,
            start,
            end,
        } => main_backtest_cmd::run(config, strategy, capital, start, end),

        Commands::Optimize {
            config,
            mode,
            sort_by,
            top,
        } => main_optimize_cmd::run(config, mode, sort_by, top),

        Commands::Live {
            config,
            paper,
            live,
            interval,
            state_db,
        } => main_live_cmd::run(config, paper, live, interval, state_db),
    }
}
