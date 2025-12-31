//! Optimize command - JSON-driven grid search optimization

use anyhow::Result;
use crypto_strategies::{data, grid, optimizer::OptimizationResult, strategies, Config, Symbol};
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::info;

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
    sort_by: String,
    top: usize,
    coins: Option<String>,
    symbols: Option<String>,
    min_combo: usize,
    max_combo: Option<usize>,
    timeframes: Option<String>,
    overrides: Vec<String>,
    sequential: bool,
) -> Result<()> {
    info!("Starting optimization");

    // Load configuration
    let mut config = Config::from_file(&config_path)?;
    info!("Loaded configuration from: {}", config_path);

    // Apply CLI overrides to grid
    if !overrides.is_empty() {
        grid::apply_overrides(&mut config, &overrides);
        info!("Applied {} CLI overrides to grid", overrides.len());
    }

    // Verify grid exists
    if config.grid.is_none() || config.grid.as_ref().map(|g| g.is_empty()).unwrap_or(true) {
        anyhow::bail!(
            "No grid parameters found in config. Add a 'grid' section to your config file.\n\
             Example:\n\
             \"grid\": {{\n\
               \"atr_period\": [10, 14, 20],\n\
               \"ema_fast\": [5, 8, 13],\n\
               \"ema_slow\": [21, 34, 55]\n\
             }}"
        );
    }

    // Parse coin list
    let coins_parsed: Option<Vec<String>> = coins
        .as_ref()
        .map(|s| s.split(',').map(|c| c.trim().to_uppercase()).collect());

    // Parse symbol groups
    let symbols_parsed: Option<Vec<Vec<String>>> = symbols.as_ref().map(|s| parse_symbol_groups(s));

    // Parse timeframes
    let timeframes_parsed: Option<Vec<String>> = timeframes
        .as_ref()
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

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
    let timeframes_to_test: Vec<String> =
        timeframes_parsed.unwrap_or_else(|| vec![config.timeframe()]);

    info!("Timeframes to test: {:?}", timeframes_to_test);
    info!("Strategy: {}", config.strategy_name);

    // Collect all unique symbols across all groups
    let all_symbols: Vec<Symbol> = symbol_groups
        .iter()
        .flatten()
        .map(|s| Symbol(s.clone()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Check for missing data and fetch if needed
    info!("Checking for missing data files...");
    let missing =
        data::find_missing_data(&config.backtest.data_dir, &all_symbols, &timeframes_to_test);

    if !missing.is_empty() {
        println!("\n{}", "=".repeat(60));
        println!("FETCHING MISSING DATA");
        println!("{}", "=".repeat(60));
        println!("  Missing files: {}", missing.len());
        for (sym, tf) in &missing {
            println!("    - {}_{}.csv", sym.as_str(), tf);
        }
        println!("{}\n", "=".repeat(60));

        match data::ensure_data_available_sync(
            &config.backtest.data_dir,
            &all_symbols,
            &timeframes_to_test,
            365,
        ) {
            Ok(failed) => {
                if !failed.is_empty() {
                    println!("  Warning: Could not fetch {} files:", failed.len());
                    for (sym, tf) in &failed {
                        println!("    - {}_{}.csv", sym.as_str(), tf);
                    }
                } else {
                    println!("  All missing data fetched successfully\n");
                }
            }
            Err(e) => {
                println!("  Warning: Error fetching data: {}\n", e);
            }
        }
    }

    // Calculate total parameter combinations from grid
    let total_param_combinations = grid::total_combinations(&config);
    info!("Grid parameter combinations: {}", total_param_combinations);

    // Build task list (symbol group × timeframe combinations)
    let mut tasks: Vec<OptTask> = Vec::new();
    let mut symbol_groups_flat: Vec<String> = Vec::new();

    for (group_idx, symbols_vec) in symbol_groups.iter().enumerate() {
        let group_name = symbols_vec
            .iter()
            .map(|s| s.replace("INR", ""))
            .collect::<Vec<_>>()
            .join("+");
        symbol_groups_flat.push(group_name);

        let mut task_config = config.clone();
        task_config.trading.pairs = symbols_vec.clone();

        for timeframe in &timeframes_to_test {
            task_config.set_timeframe(timeframe);

            let symbol_list: Vec<Symbol> = symbols_vec.iter().map(|s| Symbol(s.clone())).collect();
            if let Ok(data) =
                data::load_multi_symbol(&task_config.backtest.data_dir, &symbol_list, timeframe)
            {
                if !data.is_empty() {
                    tasks.push(OptTask {
                        group_idx,
                        symbols_vec: symbols_vec.clone(),
                        timeframe: timeframe.clone(),
                        config: task_config.clone(),
                    });
                }
            }
        }
    }

    // Generate all (task, param_config) combinations using generic grid generator
    let mut all_runs: Vec<(OptTask, Config)> = Vec::new();
    for task in &tasks {
        let configs = grid::generate_grid_configs(&task.config);
        for cfg in configs {
            all_runs.push((task.clone(), cfg));
        }
    }

    let total_runs = all_runs.len();
    info!(
        "Total runs: {} groups x {} timeframes x {} params = {} actual runs",
        symbol_groups.len(),
        timeframes_to_test.len(),
        total_param_combinations,
        total_runs
    );

    if total_runs == 0 {
        info!("No valid runs found. Check data availability and grid config.");
        return Ok(());
    }

    // Print summary
    println!();
    println!("  Strategy      {}", config.strategy_name);
    println!("  Symbols       {} group(s)", symbol_groups.len());
    println!("  Timeframes    {}", timeframes_to_test.join(", "));
    println!("  Grid          {} combinations", total_param_combinations);
    println!("  Total runs    {}", total_runs);
    println!(
        "  Execution     {}",
        if sequential { "sequential" } else { "parallel" }
    );
    println!();

    // Create progress bar with solid blocks
    let pb = ProgressBar::new(total_runs as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) ETA: {eta} {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb.set_message("");

    let valid_count = Arc::new(AtomicUsize::new(0));
    let valid_count_clone = valid_count.clone();

    // Run all backtests
    let all_results: Vec<OptimizationResult> = if sequential {
        all_runs
            .iter()
            .filter_map(|(task, param_config)| {
                let result = run_single_backtest(task, param_config);
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
                let result = run_single_backtest(task, param_config);
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
    info!(
        "Total results: {}, sorted by: {}",
        all_results.len(),
        sort_by
    );

    // Get grid param keys for filtering display
    let grid_keys: Vec<String> = config
        .grid
        .as_ref()
        .map(|g| g.keys().cloned().collect())
        .unwrap_or_default();

    // Display top results
    let display_count = top.min(all_results.len());
    println!();
    println!("  Top {} results (sorted by {})", display_count, sort_by);
    println!();
    println!(
        "  {:<3} {:>7} {:>8} {:>7} {:>6} {:>5}  {:<12} {:>3}  Grid Parameters",
        "#", "Sharpe", "Return", "MaxDD", "WinR", "Trd", "Symbols", "TF"
    );
    println!("  {}", "-".repeat(90));

    for (i, result) in all_results.iter().take(top).enumerate() {
        let group_idx = *result.params.get("_group_idx").unwrap_or(&0.0) as usize;
        let symbols_str = if group_idx < symbol_groups_flat.len() {
            symbol_groups_flat[group_idx]
                .chars()
                .take(12)
                .collect::<String>()
        } else {
            "N/A".to_string()
        };

        let tf_val = *result.params.get("_timeframe").unwrap_or(&0.0);
        let tf = match tf_val {
            v if (v - 0.083).abs() < 0.01 => "5m",
            v if (v - 0.25).abs() < 0.01 => "15m",
            v if (v - 1.0).abs() < 0.01 => "1h",
            v if (v - 4.0).abs() < 0.01 => "4h",
            v if (v - 24.0).abs() < 0.01 => "1d",
            _ => "?",
        };

        // Format only grid params (the ones that vary)
        let grid_params: String = grid_keys
            .iter()
            .filter_map(|k| {
                result.params.get(k).map(|v| {
                    let short_key = k.replace("_multiple", "").replace("_threshold", "");
                    if v.fract() == 0.0 {
                        format!("{}={}", short_key, *v as i64)
                    } else {
                        format!("{}={:.1}", short_key, v)
                    }
                })
            })
            .collect::<Vec<_>>()
            .join(" ");

        println!(
            "  {:<3} {:>7.2} {:>7.1}% {:>6.1}% {:>5.0}% {:>5}  {:<12} {:>3}  {}",
            i + 1,
            result.sharpe_ratio,
            result.total_return,
            result.max_drawdown,
            result.win_rate,
            result.total_trades,
            symbols_str,
            tf,
            grid_params
        );
    }
    println!();

    info!("Optimization completed successfully");

    Ok(())
}

#[derive(Clone)]
struct OptTask {
    group_idx: usize,
    symbols_vec: Vec<String>,
    timeframe: String,
    config: Config,
}

fn run_single_backtest(task: &OptTask, param_config: &Config) -> Option<OptimizationResult> {
    use crypto_strategies::backtest::Backtester;

    let symbol_list: Vec<Symbol> = task.symbols_vec.iter().map(|s| Symbol(s.clone())).collect();
    let data = match data::load_multi_symbol(
        &task.config.backtest.data_dir,
        &symbol_list,
        &task.timeframe,
    ) {
        Ok(d) if !d.is_empty() => d,
        _ => return None,
    };

    let strategy = match strategies::create_strategy(param_config) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let mut backtester = Backtester::new(param_config.clone(), strategy);
    let result = backtester.run(data);

    // Build params with metadata
    let mut params: HashMap<String, f64> = HashMap::new();
    params.insert("_group_idx".to_string(), task.group_idx as f64);
    params.insert(
        "_timeframe".to_string(),
        match task.timeframe.as_str() {
            "5m" => 0.083,
            "15m" => 0.25,
            "1h" => 1.0,
            "4h" => 4.0,
            "1d" => 24.0,
            _ => 0.0,
        },
    );

    // Extract params from config
    for (k, v) in grid::extract_params(param_config) {
        params.insert(k, v);
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
        val_b
            .partial_cmp(&val_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
