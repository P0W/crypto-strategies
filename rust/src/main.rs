//! Crypto trading strategies - main entry point
//!
//! This binary provides four subcommands:
//! - backtest: Run strategy backtests
//! - optimize: Run parameter optimization
//! - live: Run live trading (paper or real)
//! - download: Download historical data from Binance (default) or CoinDCX

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod commands;

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
#[allow(clippy::large_enum_variant)]
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

    /// Optimize strategy parameters (grid search from JSON config)
    Optimize {
        /// Path to configuration file with grid section
        #[arg(short, long, default_value = "configs/btc_eth_sol_bnb_xrp_1d.json")]
        config: String,

        /// Sort results by metric (sharpe, calmar, return, win_rate, profit_factor)
        #[arg(long, default_value = "sharpe")]
        sort_by: String,

        /// Number of top results to show
        #[arg(short, long, default_value = "10")]
        top: usize,

        /// Coins to test (comma-separated). E.g., "BTC,ETH,SOL"
        #[arg(long)]
        coins: Option<String>,

        /// Symbols to test directly (semicolon-separated groups, comma-separated within)
        #[arg(long)]
        symbols: Option<String>,

        /// Minimum coin combination size
        #[arg(long, default_value = "1")]
        min_combo: usize,

        /// Maximum coin combination size
        #[arg(long)]
        max_combo: Option<usize>,

        /// Timeframes to test (comma-separated). E.g., "1h,4h,1d"
        #[arg(long)]
        timeframes: Option<String>,

        /// Override grid params. Format: "param=val1,val2,val3". Can be used multiple times.
        /// Example: --override "atr_period=10,14,20" --override "ema_fast=5,8,13"
        #[arg(short = 'O', long = "override")]
        overrides: Vec<String>,

        /// Run sequentially instead of parallel
        #[arg(long)]
        sequential: bool,
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

    /// Download historical data from Binance (default) or CoinDCX
    Download {
        /// Symbols to download (comma-separated). E.g., "BTC,ETH,SOL,BNB,XRP"
        #[arg(short, long, default_value = "BTC,ETH,SOL,BNB,XRP")]
        symbols: String,

        /// Timeframe intervals (comma-separated). E.g., "1h,4h,1d"
        #[arg(short, long, default_value = "5m,15m,1h,4h,1d")]
        timeframes: String,

        /// Number of days of history to fetch
        #[arg(short, long, default_value = "180")]
        days: u32,

        /// Output directory
        #[arg(short, long, default_value = "data")]
        output: String,

        /// Data source: "binance" (default) or "coindcx"
        #[arg(long, default_value = "binance")]
        source: String,
    },
}

fn setup_logging(verbose: bool, command_name: &str, file_only: bool) -> Result<()> {
    // Create logs directory
    std::fs::create_dir_all("logs")?;

    // Create log file with naming pattern: {command}_{date}.log
    let log_filename = format!(
        "{}_{}.log",
        command_name,
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let log_path = PathBuf::from("logs").join(&log_filename);

    // Set log level - filter out noisy external crates
    let level = if verbose { "debug" } else { "info" };
    let filter_str = format!(
        "{},hyper=warn,hyper_util=warn,reqwest=warn,rustls=warn,h2=warn",
        level
    );
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&filter_str));

    // File appender
    let file_appender = tracing_appender::rolling::never("logs", &log_filename);

    if file_only {
        // For optimizer: only log to file, keep console clean for progress bar
        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(file_appender)
            .with_target(true)
            .with_line_number(true)
            .with_file(true)
            .with_ansi(false);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(file_layer)
            .init();
    } else {
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

        // Initialize subscriber with both console and file
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer)
            .init();

        info!("Logging initialized");
        info!("Log file: {}", log_path.display());
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine command name and whether to use file-only logging
    let (command_name, file_only) = match &cli.command {
        Commands::Backtest { .. } => ("backtest", false),
        Commands::Optimize { .. } => ("optimize", true), // File-only for clean progress bar
        Commands::Live { .. } => ("live", false),
        Commands::Download { .. } => ("download", false),
    };

    // Setup logging
    setup_logging(cli.verbose, command_name, file_only)?;

    // Execute command
    match cli.command {
        Commands::Backtest {
            config,
            strategy,
            capital,
            start,
            end,
        } => commands::backtest::run(config, strategy, capital, start, end),

        Commands::Optimize {
            config,
            sort_by,
            top,
            coins,
            symbols,
            min_combo,
            max_combo,
            timeframes,
            overrides,
            sequential,
        } => commands::optimize::run(
            config,
            sort_by,
            top,
            coins,
            symbols,
            min_combo,
            max_combo,
            timeframes,
            overrides,
            sequential,
        ),

        Commands::Live {
            config,
            paper,
            live,
            interval,
            state_db,
        } => commands::live::run(config, paper, live, interval, state_db),

        Commands::Download {
            symbols,
            timeframes,
            days,
            output,
            source,
        } => {
            let data_source = source.parse().unwrap_or_else(|e| {
                eprintln!("Warning: {}, using binance", e);
                crypto_strategies::data::DataSource::Binance
            });
            commands::download::run(symbols, timeframes, days, output, data_source)
        }
    }
}
