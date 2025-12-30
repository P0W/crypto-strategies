//! Optimize command implementation with progress tracking

use anyhow::Result;
use tracing::{info, debug, error};
use indicatif::{ProgressBar, ProgressStyle};
use crypto_strategies::{Config, data, optimizer::Optimizer, Strategy};
use crypto_strategies::strategies::volatility_regime::{self, GridParams};

pub fn run(
    config_path: String,
    mode: String,
    sort_by: String,
    top: usize,
) -> Result<()> {
    info!("Starting optimization");
    
    // Load base configuration
    let config = Config::from_file(&config_path)?;
    info!("Loaded configuration from: {}", config_path);

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

    // Determine strategy and create appropriate parameter grid
    info!("Strategy: {}", config.strategy_name);
    let (configs, strategy_factory): (Vec<Config>, Box<dyn Fn(&Config) -> Box<dyn Strategy> + Send + Sync>) = 
        match config.strategy_name.as_str() {
            "volatility_regime" => {
                // Create parameter grid
                let grid_params = match mode.as_str() {
                    "full" => GridParams::full(),
                    "quick" | _ => GridParams::quick(),
                };

                let total_combinations = grid_params.total_combinations();
                info!("Optimization mode: {}", mode);
                info!("Grid will test {} combinations", total_combinations);

                // Generate all configs from grid
                let configs = grid_params.generate_configs(&config);
                
                let factory: Box<dyn Fn(&Config) -> Box<dyn Strategy> + Send + Sync> = 
                    Box::new(|cfg: &Config| -> Box<dyn Strategy> {
                        match volatility_regime::create_strategy_from_config(cfg) {
                            Ok(strategy) => Box::new(strategy),
                            Err(e) => {
                                error!("Failed to create strategy: {}", e);
                                panic!("Strategy creation failed");
                            }
                        }
                    });
                
                (configs, factory)
            }
            other => {
                anyhow::bail!("Unknown strategy: {}. Available strategies: volatility_regime", other);
            }
        };

    // Create progress bar
    let pb = ProgressBar::new(configs.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb.set_message("Optimizing...");

    // Run optimization using the generic optimizer
    let optimizer = Optimizer::new(config.clone());
    info!("Starting parallel optimization...");
    
    let mut results = optimizer.optimize_with_progress(data, configs.clone(), strategy_factory, pb.clone());

    pb.finish_with_message("Optimization complete");

    // Add parameter information to results based on strategy
    match config.strategy_name.as_str() {
        "volatility_regime" => {
            for (i, result) in results.iter_mut().enumerate() {
                if let Ok(vr_config) = serde_json::from_value::<volatility_regime::VolatilityRegimeConfig>(configs[i].strategy.clone()) {
                    result.params = volatility_regime::config_to_params(&vr_config);
                }
            }
        }
        _ => {}
    }

    // Sort results
    Optimizer::sort_results(&mut results, &sort_by);
    info!("Results sorted by: {}", sort_by);

    // Display top results
    let display_count = top.min(results.len());
    println!("\n{}", "=".repeat(100));
    println!("TOP {} OPTIMIZATION RESULTS (sorted by {})", display_count, sort_by);
    println!("{}", "=".repeat(100));
    println!(
        "{:<6} {:>8} {:>10} {:>10} {:>10} {:>8} | {}",
        "Rank", "Sharpe", "Return%", "MaxDD%", "WinRate%", "Trades", "Parameters"
    );
    println!("{}", "-".repeat(100));

    for (i, result) in results.iter().take(top).enumerate() {
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

    info!("Optimization completed successfully");

    Ok(())
}
