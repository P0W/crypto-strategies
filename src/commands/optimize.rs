//! Optimize command implementation with progress tracking and custom parameter support

use anyhow::Result;
use crypto_strategies::strategies::volatility_regime::{self, GridParams};
use crypto_strategies::{data, optimizer::OptimizationResult, Config, Strategy, Symbol};
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::info;

/// Parse comma-separated floats
fn parse_float_list(s: &str) -> Vec<f64> {
    s.split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect()
}

/// Parse comma-separated integers
fn parse_int_list(s: &str) -> Vec<usize> {
    s.split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect()
}

/// Parse symbol groups (semicolon-separated groups, comma-separated within)
fn parse_symbol_groups(s: &str) -> Vec<Vec<String>> {
    s.split(';')
        .map(|group| {
            group
                .split(',')
                .map(|sym| sym.trim().to_uppercase())
                .filter(|sym| !sym.is_empty())
                .collect()
        })
        .filter(|group: &Vec<String>| !group.is_empty())
        .collect()
}

/// Generate all coin combinations from min_size to max_size
fn generate_coin_combinations(
    coins: &[String],
    min_size: usize,
    max_size: usize,
) -> Vec<Vec<String>> {
    let mut result = Vec::new();

    for size in min_size..=max_size {
        for combo in coins.iter().combinations(size) {
            result.push(combo.into_iter().map(|c| format!("{}INR", c)).collect());
        }
    }

    result
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    config_path: String,
    mode: String,
    sort_by: String,
    top: usize,
    coins: Option<String>,
    symbols: Option<String>,
    min_combo: usize,
    max_combo: Option<usize>,
    timeframes: Option<String>,
    adx: Option<String>,
    stop_atr: Option<String>,
    target_atr: Option<String>,
    compression: Option<String>,
    ema_fast: Option<String>,
    ema_slow: Option<String>,
    atr_period: Option<String>,
    sequential: bool,
) -> Result<()> {
    info!("Starting optimization");

    // Load base configuration
    let config = Config::from_file(&config_path)?;
    info!("Loaded configuration from: {}", config_path);

    // Parse coin list
    let coins_parsed: Option<Vec<String>> = coins.as_ref().map(|s| {
        s.split(',')
            .map(|c| c.trim().to_uppercase())
            .collect()
    });

    // Parse symbol groups
    let symbols_parsed: Option<Vec<Vec<String>>> = symbols.as_ref().map(|s| parse_symbol_groups(s));

    // Parse custom parameter grids
    let adx_parsed = adx.as_ref().map(|s| parse_float_list(s));
    let stop_atr_parsed = stop_atr.as_ref().map(|s| parse_float_list(s));
    let target_atr_parsed = target_atr.as_ref().map(|s| parse_float_list(s));
    let _compression_parsed = compression.as_ref().map(|s| parse_float_list(s));
    let ema_fast_parsed = ema_fast.as_ref().map(|s| parse_int_list(s));
    let ema_slow_parsed = ema_slow.as_ref().map(|s| parse_int_list(s));
    let atr_period_parsed = atr_period.as_ref().map(|s| parse_int_list(s));

    // Parse timeframes
    let timeframes_parsed: Option<Vec<String>> = timeframes.as_ref().map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .collect()
    });

    // Determine symbol combinations to test
    let symbol_groups: Vec<Vec<String>> = if let Some(ref coins) = coins_parsed {
        let max = max_combo.unwrap_or(coins.len());
        let combos = generate_coin_combinations(coins, min_combo, max);
        info!(
            "Generated {} coin combinations from {:?} (sizes {}-{})",
            combos.len(),
            coins,
            min_combo,
            max
        );
        combos
    } else if let Some(ref symbols) = symbols_parsed {
        info!("Using {} symbol groups from --symbols", symbols.len());
        symbols.clone()
    } else {
        let default_symbols: Vec<String> = config
            .trading
            .symbols()
            .iter()
            .map(|s| s.0.clone())
            .collect();
        info!("Using config symbols: {:?}", default_symbols);
        vec![default_symbols]
    };

    // Determine timeframes to test
    let timeframes_to_test: Vec<String> = timeframes_parsed
        .unwrap_or_else(|| vec![config.trading.timeframe.clone()]);

    info!("Timeframes to test: {:?}", timeframes_to_test);
    info!("Strategy: {}", config.strategy_name);

    // Check if any custom params were provided
    let has_custom = adx_parsed.is_some()
        || stop_atr_parsed.is_some()
        || target_atr_parsed.is_some()
        || ema_fast_parsed.is_some()
        || ema_slow_parsed.is_some()
        || atr_period_parsed.is_some();

    // Build grid params based on mode and CLI overrides
    let grid_params = if mode == "custom" || has_custom {
        GridParams::custom(
            atr_period_parsed.unwrap_or_else(|| vec![14]),
            ema_fast_parsed.unwrap_or_else(|| vec![8, 13]),
            ema_slow_parsed.unwrap_or_else(|| vec![21, 34]),
            adx_parsed.unwrap_or_else(|| vec![25.0, 30.0]),
            stop_atr_parsed.unwrap_or_else(|| vec![2.0, 2.5]),
            target_atr_parsed.unwrap_or_else(|| vec![4.0, 5.0]),
        )
    } else if mode == "full" {
        GridParams::full()
    } else {
        GridParams::quick()
    };

    let total_param_combinations = grid_params.total_combinations();
    info!("Optimization mode: {}", mode);
    info!("Parameter combinations: {}", total_param_combinations);

    // Pre-calculate total runs and build task list
    let mut tasks: Vec<OptTask> = Vec::new();
    let mut symbol_groups_flat: Vec<String> = Vec::new();

    for (group_idx, symbols_vec) in symbol_groups.iter().enumerate() {
        let group_name = symbols_vec
            .iter()
            .map(|s| s.replace("INR", ""))
            .collect::<Vec<_>>()
            .join("+");
        symbol_groups_flat.push(group_name.clone());

        let mut task_config = config.clone();
        task_config.trading.pairs = symbols_vec.clone();

        for timeframe in &timeframes_to_test {
            task_config.trading.timeframe = timeframe.clone();
            task_config.backtest.timeframe = timeframe.clone();

            let symbol_list: Vec<Symbol> = symbols_vec.iter().map(|s| Symbol(s.clone())).collect();
            if let Ok(data) = data::load_multi_symbol(&task_config.backtest.data_dir, &symbol_list, timeframe) {
                if !data.is_empty() {
                    tasks.push(OptTask {
                        group_idx,
                        group_name: group_name.clone(),
                        symbols_vec: symbols_vec.clone(),
                        timeframe: timeframe.clone(),
                        config: task_config.clone(),
                    });
                }
            }
        }
    }

    // Generate all (task, param_config) combinations
    let mut all_runs: Vec<(OptTask, Config)> = Vec::new();
    for task in &tasks {
        let configs = grid_params.generate_configs(&task.config);
        for cfg in configs {
            all_runs.push((task.clone(), cfg));
        }
    }

    let total_runs = all_runs.len();
    info!(
        "Total runs: {} groups × {} timeframes × {} params = {} actual runs",
        symbol_groups.len(),
        timeframes_to_test.len(),
        total_param_combinations,
        total_runs
    );

    if total_runs == 0 {
        info!("No valid runs found. Check data availability.");
        return Ok(());
    }

    // Print summary
    println!("\n{}", "=".repeat(70));
    println!("OPTIMIZATION SUMMARY");
    println!("{}", "=".repeat(70));
    println!("  Symbol groups: {}", symbol_groups.len());
    println!("  Timeframes:    {:?}", timeframes_to_test);
    println!("  Parameters:    {} combinations", total_param_combinations);
    println!("  Total tests:   {}", total_runs);
    println!("  Mode:          {}", if sequential { "sequential" } else { "parallel" });
    println!("{}\n", "=".repeat(70));

    // Create single progress bar (tqdm style)
    let pb = ProgressBar::new(total_runs as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("⚡ {percent:>3}%|{bar:40}| {pos}/{len} [{elapsed}<{eta}, {per_sec:.2}] ✓ {msg}")
            .unwrap()
            .progress_chars("█░ "),
    );
    
    let valid_count = Arc::new(AtomicUsize::new(0));
    let valid_count_clone = valid_count.clone();

    // Run all backtests
    let all_results: Vec<OptimizationResult> = if sequential {
        all_runs
            .iter()
            .filter_map(|(task, param_config)| {
                let result = run_single_backtest(task, param_config, &symbol_groups_flat);
                pb.inc(1);
                if let Some(ref r) = result {
                    if r.total_trades > 0 {
                        let count = valid_count.fetch_add(1, Ordering::Relaxed) + 1;
                        pb.set_message(format!("{} valid", count));
                    }
                }
                result
            })
            .collect()
    } else {
        all_runs
            .par_iter()
            .filter_map(|(task, param_config)| {
                let result = run_single_backtest(task, param_config, &symbol_groups_flat);
                pb.inc(1);
                if let Some(ref r) = result {
                    if r.total_trades > 0 {
                        let count = valid_count_clone.fetch_add(1, Ordering::Relaxed) + 1;
                        pb.set_message(format!("{} valid", count));
                    }
                }
                result
            })
            .collect()
    };

    pb.finish_with_message(format!("{} valid", valid_count.load(Ordering::Relaxed)));
    println!();

    if all_results.is_empty() {
        info!("No valid results found.");
        return Ok(());
    }

    // Sort results
    let mut all_results = all_results;
    sort_results(&mut all_results, &sort_by);
    info!("Total results: {}, sorted by: {}", all_results.len(), sort_by);

    // Display top results
    let display_count = top.min(all_results.len());
    println!("\n{}", "=".repeat(120));
    println!("TOP {} OPTIMIZATION RESULTS (sorted by {})", display_count, sort_by);
    println!("{}", "=".repeat(120));
    println!(
        "{:<4} {:>7} {:>9} {:>8} {:>8} {:>6} | {:<15} {:>3} | Parameters",
        "Rank", "Sharpe", "Return%", "MaxDD%", "WinR%", "Trades", "Symbols", "TF"
    );
    println!("{}", "-".repeat(120));

    for (i, result) in all_results.iter().take(top).enumerate() {
        let group_idx = *result.params.get("_group_idx").unwrap_or(&0.0) as usize;
        let symbols_str = if group_idx < symbol_groups_flat.len() {
            &symbol_groups_flat[group_idx]
        } else {
            "N/A"
        };

        let tf = match *result.params.get("_timeframe").unwrap_or(&0.0) as i32 {
            1 => "1h",
            4 => "4h",
            24 => "1d",
            _ => "?",
        };

        let params_str = format!(
            "ATR:{} EMA:{}/{} ADX:{} Stop:{:.1} Tgt:{:.1}",
            *result.params.get("atr_period").unwrap_or(&0.0) as usize,
            *result.params.get("ema_fast").unwrap_or(&0.0) as usize,
            *result.params.get("ema_slow").unwrap_or(&0.0) as usize,
            result.params.get("adx_threshold").unwrap_or(&0.0),
            result.params.get("stop_atr_multiple").unwrap_or(&0.0),
            result.params.get("target_atr_multiple").unwrap_or(&0.0),
        );

        println!(
            "{:<4} {:>7.2} {:>9.2} {:>8.2} {:>8.2} {:>6} | {:<15} {:>3} | {}",
            i + 1,
            result.sharpe_ratio,
            result.total_return,
            result.max_drawdown,
            result.win_rate,
            result.total_trades,
            symbols_str,
            tf,
            params_str
        );
    }
    println!("{}", "=".repeat(120));

    info!("Optimization completed successfully");

    Ok(())
}

#[derive(Clone)]
struct OptTask {
    group_idx: usize,
    #[allow(dead_code)]
    group_name: String,
    symbols_vec: Vec<String>,
    timeframe: String,
    config: Config,
}

fn run_single_backtest(
    task: &OptTask,
    param_config: &Config,
    _symbol_groups_flat: &[String],
) -> Option<OptimizationResult> {
    use crypto_strategies::backtest::Backtester;

    let symbol_list: Vec<Symbol> = task.symbols_vec.iter().map(|s| Symbol(s.clone())).collect();
    let data = match data::load_multi_symbol(&task.config.backtest.data_dir, &symbol_list, &task.timeframe) {
        Ok(d) if !d.is_empty() => d,
        _ => return None,
    };

    let strategy: Box<dyn Strategy> = match param_config.strategy_name.as_str() {
        "volatility_regime" => {
            match volatility_regime::create_strategy_from_config(param_config) {
                Ok(s) => Box::new(s),
                Err(_) => return None,
            }
        }
        _ => return None,
    };

    let mut backtester = Backtester::new(param_config.clone(), strategy);
    let result = backtester.run(data);

    let mut params: HashMap<String, f64> = HashMap::new();
    params.insert("_group_idx".to_string(), task.group_idx as f64);
    params.insert(
        "_timeframe".to_string(),
        match task.timeframe.as_str() {
            "1h" => 1.0,
            "4h" => 4.0,
            "1d" => 24.0,
            _ => 0.0,
        },
    );

    if let Ok(vr_config) = serde_json::from_value::<volatility_regime::VolatilityRegimeConfig>(
        param_config.strategy.clone(),
    ) {
        for (k, v) in volatility_regime::config_to_params(&vr_config) {
            params.insert(k, v);
        }
    }

    Some(OptimizationResult {
        params,
        sharpe_ratio: result.metrics.sharpe_ratio,
        total_return: result.metrics.total_return,
        max_drawdown: result.metrics.max_drawdown,
        win_rate: result.metrics.win_rate,
        total_trades: result.metrics.total_trades,
        calmar_ratio: result.metrics.calmar_ratio,
        profit_factor: result.metrics.profit_factor,
    })
}

fn sort_results(results: &mut [OptimizationResult], sort_by: &str) {
    results.sort_by(|a, b| {
        let val_a = match sort_by {
            "sharpe" => a.sharpe_ratio,
            "return" => a.total_return,
            "calmar" => a.calmar_ratio,
            "win_rate" => a.win_rate,
            "profit_factor" => a.profit_factor,
            _ => a.sharpe_ratio,
        };
        let val_b = match sort_by {
            "sharpe" => b.sharpe_ratio,
            "return" => b.total_return,
            "calmar" => b.calmar_ratio,
            "win_rate" => b.win_rate,
            "profit_factor" => b.profit_factor,
            _ => b.sharpe_ratio,
        };
        val_b.partial_cmp(&val_a).unwrap_or(std::cmp::Ordering::Equal)
    });
}
