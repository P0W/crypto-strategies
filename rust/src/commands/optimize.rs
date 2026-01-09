//! Optimize command - JSON-driven grid search optimization

use anyhow::Result;
use chrono::{DateTime, Utc};
use crypto_strategies::{data, grid, optimizer::OptimizationResult, strategies, Config, Symbol};
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

/// Format duration in human readable format
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.0}s", secs)
    } else if secs < 3600.0 {
        let mins = (secs / 60.0).floor();
        let remaining = secs % 60.0;
        format!("{}m {}s", mins as u32, remaining as u32)
    } else {
        let hours = (secs / 3600.0).floor();
        let remaining_mins = ((secs % 3600.0) / 60.0).floor();
        format!("{}h {}m", hours as u32, remaining_mins as u32)
    }
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
    sort_by: String,
    top: usize,
    coins: Option<String>,
    symbols: Option<String>,
    min_combo: usize,
    max_combo: Option<usize>,
    timeframes: Option<String>,
    start: Option<String>,
    end: Option<String>,
    overrides: Vec<String>,
    sequential: bool,
    no_update: bool,
) -> Result<()> {
    info!("Starting optimization");

    // Load configuration
    let mut config = Config::from_file(&config_path)?;
    info!("Loaded configuration from: {}", config_path);

    // Parse date range filters
    let start_date: Option<DateTime<Utc>> =
        start.as_ref().map(|s| data::parse_date(s)).transpose()?;
    let end_date: Option<DateTime<Utc>> = end.as_ref().map(|s| data::parse_date(s)).transpose()?;

    if let Some(ref start) = start_date {
        info!("Filtering data from: {}", start);
    }
    if let Some(ref end) = end_date {
        info!("Filtering data until: {}", end);
    }

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
    info!("Strategy: {}", config.strategy_name());

    // Collect all unique symbols across all groups
    let all_symbols: Vec<Symbol> = symbol_groups
        .iter()
        .flatten()
        .map(|s| Symbol(s.clone()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Check for missing data and fetch if needed (including date range coverage)
    info!("Checking for missing data files...");
    data::check_and_fetch_data(
        &config.backtest.data_dir,
        &all_symbols,
        &timeframes_to_test,
        start_date,
        end_date,
    )?;

    // Check actual data coverage and warn if not fully covered
    if start_date.is_some() {
        let mut coverage_warnings: Vec<String> = Vec::new();
        for symbol in &all_symbols {
            for tf in &timeframes_to_test {
                let filename = format!("{}_{}.csv", symbol.as_str(), tf);
                let path = std::path::Path::new(&config.backtest.data_dir).join(&filename);
                if let Ok(Some((data_start, _))) = data::get_data_date_range(&path) {
                    if let Some(req_start) = start_date {
                        if data_start > req_start {
                            coverage_warnings.push(format!(
                                "{}_{}: data starts {} (requested {})",
                                symbol.as_str(),
                                tf,
                                data_start.format("%Y-%m-%d"),
                                req_start.format("%Y-%m-%d")
                            ));
                        }
                    }
                }
            }
        }
        if !coverage_warnings.is_empty() {
            println!("  ⚠ Note: Some data doesn't cover full date range:");
            for w in coverage_warnings.iter().take(5) {
                println!("    {}", w);
            }
            if coverage_warnings.len() > 5 {
                println!("    ... and {} more", coverage_warnings.len() - 5);
            }
            println!("    (Binance may not have earlier historical data)\n");
        }
    }

    // Calculate total parameter combinations from grid
    let total_param_combinations = grid::total_combinations(&config);
    info!("Grid parameter combinations: {}", total_param_combinations);

    // Build task list (symbol group × timeframe combinations)
    let mut tasks: Vec<OptTask> = Vec::new();
    let mut symbol_groups_flat: Vec<String> = Vec::new();
    let mut skipped_reasons: Vec<String> = Vec::new();

    for (group_idx, symbols_vec) in symbol_groups.iter().enumerate() {
        let group_name = symbols_vec
            .iter()
            .map(|s| s.replace("INR", ""))
            .collect::<Vec<_>>()
            .join("+");
        symbol_groups_flat.push(group_name.clone());

        let mut task_config = config.clone();
        task_config.trading.symbols = symbols_vec.clone();

        for timeframe in &timeframes_to_test {
            task_config.set_timeframe(timeframe);

            let symbol_list: Vec<Symbol> = symbols_vec.iter().map(|s| Symbol(s.clone())).collect();
            match data::load_multi_symbol_with_range(
                &task_config.backtest.data_dir,
                &symbol_list,
                timeframe,
                start_date,
                end_date,
            ) {
                Ok(data) if !data.is_empty() => {
                    tasks.push(OptTask {
                        group_idx,
                        symbols_vec: symbols_vec.clone(),
                        timeframe: timeframe.clone(),
                        config: task_config.clone(),
                        start_date,
                        end_date,
                    });
                }
                Ok(_) => {
                    skipped_reasons.push(format!(
                        "{}_{}: No data in date range{}{}",
                        group_name,
                        timeframe,
                        start_date
                            .map(|d| format!(" from {}", d.format("%Y-%m-%d")))
                            .unwrap_or_default(),
                        end_date
                            .map(|d| format!(" to {}", d.format("%Y-%m-%d")))
                            .unwrap_or_default()
                    ));
                }
                Err(e) => {
                    skipped_reasons.push(format!("{}_{}: {}", group_name, timeframe, e));
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
        println!();
        println!("  ❌ No valid runs found!");
        println!();
        if !skipped_reasons.is_empty() {
            println!("  Reasons:");
            for reason in skipped_reasons.iter().take(10) {
                println!("    - {}", reason);
            }
            if skipped_reasons.len() > 10 {
                println!("    ... and {} more", skipped_reasons.len() - 10);
            }
            println!();
        }
        println!("  Check:");
        println!("    1. Data files exist in: {}", config.backtest.data_dir);
        println!("    2. Date range has data (--start/--end)");
        println!("    3. Grid config is valid");
        println!();
        info!("No valid runs found. Check data availability and grid config.");
        return Ok(());
    }

    // Print professional configuration summary
    let cpu_threads = rayon::current_num_threads();
    let est_time_per_run = 0.05; // rough estimate in seconds
    let est_total_secs = if sequential {
        total_runs as f64 * est_time_per_run
    } else {
        (total_runs as f64 * est_time_per_run) / cpu_threads as f64
    };

    let border = "═".repeat(78);
    println!();
    println!("  ╔{}╗", border);
    println!("  ║                      📊 OPTIMIZATION CONFIGURATION                         ║");
    println!("  ╠{}╣", border);
    println!("  ║  Strategy     │ {:<58} ║", config.strategy_name());
    println!(
        "  ║  Symbols      │ {} group(s){:<47} ║",
        symbol_groups.len(),
        ""
    );
    println!(
        "  ║  Timeframes   │ {:<58} ║",
        timeframes_to_test.join(", ")
    );
    if let Some(ref start) = start_date {
        println!(
            "  ║  Start Date   │ {:<58} ║",
            start.format("%Y-%m-%d %H:%M UTC")
        );
    }
    if let Some(ref end) = end_date {
        println!(
            "  ║  End Date     │ {:<58} ║",
            end.format("%Y-%m-%d %H:%M UTC")
        );
    }
    println!("  ╠{}╣", border);
    println!(
        "  ║  Grid Params  │ {:>8} combinations{:<37} ║",
        total_param_combinations, ""
    );
    println!(
        "  ║  Total Runs   │ {:>8} backtests{:<39} ║",
        total_runs, ""
    );
    println!(
        "  ║  Execution    │ {:<58} ║",
        if sequential {
            "Sequential (single-threaded)".to_string()
        } else {
            format!("Parallel ({} CPU threads)", cpu_threads)
        }
    );
    println!(
        "  ║  Est. Time    │ ~{:<57} ║",
        format_duration(est_total_secs)
    );
    println!("  ╚{}╝", border);
    println!();

    // Create professional progress display
    println!("  ╔{}╗", border);
    println!("  ║                         🚀 OPTIMIZATION ENGINE                              ║");
    println!("  ╠{}╣", border);
    println!("  ║  Phase 2/3: Running backtests...                                            ║");
    println!("  ╚{}╝", border);
    println!();

    // Start timer for actual elapsed time
    let start_time = Instant::now();

    let pb = ProgressBar::new(total_runs as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} │{percent:>3}%│ [{bar:35.green/dim}] {pos:>6}/{len:6} │ ⚡ {per_sec:>8} │ ⏱  {elapsed_precise} │ ETA {eta_precise} │ ✓ {msg}")
            .expect("valid progress bar template")
            .progress_chars("━━╸")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb.set_message("0 valid");

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

    let elapsed = start_time.elapsed();
    let elapsed_secs = elapsed.as_secs_f64();
    let final_valid = valid_count.load(Ordering::Relaxed);
    let valid_pct = if total_runs > 0 {
        (final_valid as f64 / total_runs as f64) * 100.0
    } else {
        0.0
    };
    let throughput = if elapsed_secs > 0.0 {
        total_runs as f64 / elapsed_secs
    } else {
        0.0
    };
    pb.finish_and_clear();

    // Print completion summary
    println!();
    println!("  ╔{}╗", border);
    println!("  ║                         ✅ OPTIMIZATION COMPLETE                             ║");
    println!("  ╠{}╣", border);
    println!("  ║  📊 Results Summary                                                          ║");
    println!(
        "  ║  ├─ Total runs:      {:>8}                                               ║",
        total_runs
    );
    println!(
        "  ║  ├─ Valid results:   {:>8} ({:>5.1}%)                                      ║",
        final_valid, valid_pct
    );
    println!(
        "  ║  └─ Invalid/Empty:   {:>8}                                               ║",
        total_runs - final_valid
    );
    println!("  ╠{}╣", border);
    println!("  ║  ⏱  Performance                                                              ║");
    println!(
        "  ║  ├─ Elapsed time:    {:<58} ║",
        format_duration(elapsed_secs)
    );
    println!(
        "  ║  └─ Throughput:      {:.1} runs/sec{:<40} ║",
        throughput, ""
    );
    println!("  ╚{}╝", border);
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

    // Display top results with professional formatting
    let display_count = top.min(all_results.len());

    // Calculate statistics for the top results
    let top_results: Vec<_> = all_results.iter().take(display_count).collect();
    let avg_sharpe: f64 =
        top_results.iter().map(|r| r.sharpe_ratio).sum::<f64>() / display_count as f64;
    let avg_return: f64 =
        top_results.iter().map(|r| r.total_return).sum::<f64>() / display_count as f64;
    let best_sharpe = top_results
        .iter()
        .map(|r| r.sharpe_ratio)
        .fold(f64::NEG_INFINITY, f64::max);
    let best_return = top_results
        .iter()
        .map(|r| r.total_return)
        .fold(f64::NEG_INFINITY, f64::max);

    println!("  ╔{}╗", border);
    println!(
        "  ║                         🏆 TOP {} RESULTS (by {})                           ║",
        display_count, sort_by
    );
    println!("  ╠{}╣", border);
    println!("  ║  Quick Stats: Best Sharpe: {:.2} │ Best Return: {:.1}% │ Avg Sharpe: {:.2} │ Avg Return: {:.1}% ║", best_sharpe, best_return, avg_sharpe, avg_return);
    println!("  ╚{}╝", border);
    println!();
    println!(
        "  {:<3} │ {:>7} │ {:>8} │ {:>7} │ {:>6} │ {:>8} │ {:>5} │ {:<12} │ {:>3} │ Grid Parameters",
        "#", "Sharpe", "Return", "MaxDD", "WinR", "Expect", "Trd", "Symbols", "TF"
    );
    println!("  ───┼─────────┼──────────┼─────────┼────────┼──────────┼───────┼──────────────┼─────┼─────────────────");

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

        // Add rank indicator for top 3
        let rank_indicator = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "  ",
        };

        println!(
            "{} {:<2} │ {:>7.2} │ {:>7.1}% │ {:>6.1}% │ {:>5.0}% │ {:>8.2} │ {:>5} │ {:<12} │ {:>3} │ {}",
            rank_indicator,
            i + 1,
            result.sharpe_ratio,
            result.total_return,
            result.max_drawdown,
            result.win_rate,
            result.expectancy,
            result.total_trades,
            symbols_str,
            tf,
            grid_params
        );
    }
    println!();

    // Update config file with best parameters (unless --no-update)
    if !no_update && !all_results.is_empty() {
        let best = &all_results[0];
        let best_metric = get_metric_value(best, &sort_by);

        // Get best result's timeframe
        let tf_val = *best.params.get("_timeframe").unwrap_or(&0.0);
        let best_tf = match tf_val {
            v if (v - 0.083).abs() < 0.01 => "5m",
            v if (v - 0.25).abs() < 0.01 => "15m",
            v if (v - 1.0).abs() < 0.01 => "1h",
            v if (v - 4.0).abs() < 0.01 => "4h",
            v if (v - 24.0).abs() < 0.01 => "1d",
            _ => "unknown",
        };

        // Check if we should update
        if best_metric < 0.0 {
            // Don't update with a losing strategy
            println!(
                "  Skipping config update: best result has negative {} ({:.2})",
                sort_by, best_metric
            );
            println!("  Won't save a losing strategy configuration");
            println!();
        } else {
            // Compare against saved optimization metrics, or run backtest as fallback
            let saved_metric = get_saved_optimization_metric(&config, &sort_by);
            let baseline_metric = match saved_metric {
                Some(m) => {
                    println!("  Using saved optimization metrics for comparison");
                    Some(m)
                }
                None => {
                    println!("  No saved metrics, running baseline backtest...");
                    run_baseline_backtest(&config, &sort_by, start_date, end_date)
                }
            };

            let should_update = match baseline_metric {
                Some(baseline) => {
                    let improvement = best_metric - baseline;
                    // Use small epsilon to avoid updates due to floating-point precision
                    if improvement > 0.001 {
                        println!(
                            "  Best result ({:.2}) is better than current ({:.2}) by {:.2}",
                            best_metric, baseline, improvement
                        );
                        true
                    } else {
                        println!(
                            "  Current config ({:.2}) is already optimal (best found: {:.2})",
                            baseline, best_metric
                        );
                        false
                    }
                }
                None => {
                    println!("  No baseline available, updating with best result");
                    true
                }
            };

            if should_update {
                let group_idx = *best.params.get("_group_idx").unwrap_or(&0.0) as usize;
                let symbols: Vec<String> = symbol_groups
                    .get(group_idx)
                    .cloned()
                    .unwrap_or_else(|| config.trading.symbols.clone());
                match update_config_with_best(
                    &config_path,
                    best,
                    &grid_keys,
                    &symbols,
                    best_tf,
                    start_date,
                    end_date,
                ) {
                    Ok(()) => println!("  Config updated: {}", config_path),
                    Err(e) => println!("  Warning: Failed to update config: {}", e),
                }
            }
            println!();
        }
    }

    info!("Optimization completed successfully");

    Ok(())
}

/// Get metric value from result based on sort key
fn get_metric_value(result: &OptimizationResult, sort_by: &str) -> f64 {
    match sort_by {
        "sharpe" => result.sharpe_ratio,
        "return" => result.total_return,
        "calmar" => result.calmar_ratio,
        "win_rate" => result.win_rate,
        "profit_factor" => result.profit_factor,
        "expectancy" => result.expectancy,
        _ => result.sharpe_ratio,
    }
}

/// Get saved optimization metric from config's grid._optimization field
fn get_saved_optimization_metric(config: &Config, sort_by: &str) -> Option<f64> {
    // Read from grid._optimization (where optimization metadata is stored)
    let grid = config.grid.as_ref()?;
    let opt_value = grid.get("_optimization")?;
    // The value is stored as Vec<serde_json::Value>, take first element
    let opt = opt_value.first()?.as_object()?;

    let metric_name = match sort_by {
        "sharpe" => "sharpe_ratio",
        "return" => "total_return",
        "calmar" => "calmar_ratio",
        "win_rate" => "win_rate",
        "profit_factor" => "profit_factor",
        "expectancy" => "expectancy",
        _ => "sharpe_ratio",
    };
    opt.get(metric_name)?.as_f64()
}

/// Run baseline backtest with current config to get comparison metric (fallback when no saved metrics)
fn run_baseline_backtest(
    config: &Config,
    sort_by: &str,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
) -> Option<f64> {
    use crypto_strategies::backtest::Backtester;
    use crypto_strategies::multi_timeframe::MultiTimeframeData;

    let symbols: Vec<Symbol> = config.trading.symbols();
    let timeframe = config.timeframe();

    // Create strategy to get requirements
    let strategy = strategies::create_strategy(config).ok()?;
    let required_tfs = strategy.required_timeframes();

    // Load data in MTF format
    let mtf_data = if !required_tfs.is_empty() {
        let mut all_tfs: Vec<&str> = required_tfs;
        if !all_tfs.contains(&timeframe.as_str()) {
            all_tfs.push(&timeframe);
        }
        data::load_multi_timeframe(
            &config.backtest.data_dir,
            &symbols,
            &all_tfs,
            &timeframe,
            start_date,
            end_date,
        )
        .ok()?
    } else {
        let single_data = data::load_multi_symbol_with_range(
            &config.backtest.data_dir,
            &symbols,
            &timeframe,
            start_date,
            end_date,
        )
        .ok()?;

        single_data
            .into_iter()
            .map(|(symbol, candles)| {
                let mut mtf = MultiTimeframeData::new(&timeframe);
                mtf.add_timeframe(&timeframe, candles);
                (symbol, mtf)
            })
            .collect()
    };

    if mtf_data.is_empty() {
        return None;
    }

    let strategy = strategies::create_strategy(config).ok()?;
    let mut backtester = Backtester::new(config.clone(), strategy);
    let result = backtester.run(&mtf_data);

    Some(match sort_by {
        "sharpe" => result.metrics.sharpe_ratio,
        "return" => result.metrics.total_return,
        "calmar" => result.metrics.calmar_ratio,
        "win_rate" => result.metrics.win_rate,
        "profit_factor" => result.metrics.profit_factor,
        "expectancy" => result.metrics.expectancy,
        _ => result.metrics.sharpe_ratio,
    })
}

/// Update config file with best parameters
fn update_config_with_best(
    config_path: &str,
    best: &OptimizationResult,
    grid_keys: &[String],
    symbols: &[String],
    timeframe: &str,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
) -> Result<()> {
    use std::fs;

    let mut config_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(config_path)?)?;

    // Detect which grid params are booleans from the original config
    let boolean_params: std::collections::HashSet<String> = config_json
        .get("grid")
        .and_then(|g| g.as_object())
        .map(|grid| {
            grid.iter()
                .filter(|(_, v)| {
                    // Check if the array contains boolean values
                    v.as_array()
                        .is_some_and(|arr| arr.iter().any(|val| val.is_boolean()))
                })
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();

    // Update strategy params including timeframe
    if let Some(obj) = config_json
        .get_mut("strategy")
        .and_then(|s| s.as_object_mut())
    {
        // Update timeframe
        obj.insert("timeframe".to_string(), serde_json::json!(timeframe));

        // Update other strategy params
        for key in grid_keys {
            if let Some(&value) = best.params.get(key) {
                // Handle booleans dynamically based on original grid config
                let json_val = if boolean_params.contains(key) {
                    serde_json::json!(value != 0.0)
                } else if value.fract() == 0.0 && value >= 0.0 && value < i64::MAX as f64 {
                    serde_json::json!(value as i64)
                } else {
                    serde_json::json!(value)
                };
                obj.insert(key.clone(), json_val);
            }
        }
    }

    // Update trading.symbols with best symbols (also remove old "pairs" key if present)
    if let Some(obj) = config_json
        .get_mut("trading")
        .and_then(|t| t.as_object_mut())
    {
        obj.remove("pairs"); // Remove old key if present
        obj.insert("symbols".to_string(), serde_json::json!(symbols));
    }

    // Save optimization metadata in grid section
    if let Some(obj) = config_json.get_mut("grid").and_then(|g| g.as_object_mut()) {
        obj.insert(
            "_optimization".to_string(),
            serde_json::json!([{
                "sharpe_ratio": (best.sharpe_ratio * 100.0).round() / 100.0,
                "total_return": (best.total_return * 10.0).round() / 10.0,
                "max_drawdown": (best.max_drawdown * 10.0).round() / 10.0,
                "win_rate": (best.win_rate * 10.0).round() / 10.0,
                "total_trades": best.total_trades,
                "calmar_ratio": (best.calmar_ratio * 100.0).round() / 100.0,
                "expectancy": (best.expectancy * 100.0).round() / 100.0,
                "symbols": symbols,
                "optimized_at": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            }]),
        );
    }

    // Update backtest date range
    if let Some(obj) = config_json
        .get_mut("backtest")
        .and_then(|b| b.as_object_mut())
    {
        match start_date {
            Some(d) => obj.insert(
                "start_date".to_string(),
                serde_json::json!(d.format("%Y-%m-%d").to_string()),
            ),
            None => obj.remove("start_date"),
        };
        match end_date {
            Some(d) => obj.insert(
                "end_date".to_string(),
                serde_json::json!(d.format("%Y-%m-%d").to_string()),
            ),
            None => obj.remove("end_date"),
        };
    }

    fs::write(config_path, serde_json::to_string_pretty(&config_json)?)?;
    Ok(())
}

#[derive(Clone)]
struct OptTask {
    group_idx: usize,
    symbols_vec: Vec<String>,
    timeframe: String,
    config: Config,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
}

fn run_single_backtest(task: &OptTask, param_config: &Config) -> Option<OptimizationResult> {
    use crypto_strategies::backtest::Backtester;
    use crypto_strategies::multi_timeframe::MultiTimeframeData;

    let symbol_list: Vec<Symbol> = task.symbols_vec.iter().map(|s| Symbol(s.clone())).collect();

    // Create strategy to get its requirements
    let strategy = match strategies::create_strategy(param_config) {
        Ok(s) => s,
        Err(_) => return None,
    };

    let required_tfs = strategy.required_timeframes();

    // Load data based on strategy requirements
    let mtf_data = if !required_tfs.is_empty() {
        // MTF strategy - load all required timeframes
        let mut all_tfs: Vec<&str> = required_tfs;
        if !all_tfs.contains(&task.timeframe.as_str()) {
            all_tfs.push(&task.timeframe);
        }

        match data::load_multi_timeframe(
            &task.config.backtest.data_dir,
            &symbol_list,
            &all_tfs,
            &task.timeframe,
            task.start_date,
            task.end_date,
        ) {
            Ok(d) if !d.is_empty() => d,
            _ => return None,
        }
    } else {
        // Single-TF strategy - wrap in MTF format
        let single_data = match data::load_multi_symbol_with_range(
            &task.config.backtest.data_dir,
            &symbol_list,
            &task.timeframe,
            task.start_date,
            task.end_date,
        ) {
            Ok(d) if !d.is_empty() => d,
            _ => return None,
        };

        single_data
            .into_iter()
            .map(|(symbol, candles)| {
                let mut mtf = MultiTimeframeData::new(&task.timeframe);
                mtf.add_timeframe(&task.timeframe, candles);
                (symbol, mtf)
            })
            .collect()
    };

    let mut backtester = Backtester::new(param_config.clone(), strategy);
    let result = backtester.run(&mtf_data);

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
        expectancy: result.metrics.expectancy,
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
            "expectancy" => a.expectancy,
            _ => a.sharpe_ratio,
        };
        let val_b = match sort_by {
            "sharpe" => b.sharpe_ratio,
            "return" => b.total_return,
            "calmar" => b.calmar_ratio,
            "win_rate" => b.win_rate,
            "profit_factor" => b.profit_factor,
            "expectancy" => b.expectancy,
            _ => b.sharpe_ratio,
        };
        val_b
            .partial_cmp(&val_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
