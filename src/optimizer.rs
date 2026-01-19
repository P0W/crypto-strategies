//! Generic Parameter Optimization Framework
//!
//! Provides abstractions for parallel grid search optimization across any strategy.
//! Fully decoupled from strategy implementation - works with both single-TF and MTF.

use indicatif::ProgressBar;
use rayon::prelude::*;
use std::collections::HashMap;

use crate::backtest::Backtester;
use crate::multi_timeframe::MultiTimeframeData;
use crate::Strategy;
use crate::{Candle, Config, MultiSymbolMultiTimeframeData, Symbol};

/// Optimization result for a single parameter combination
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub params: HashMap<String, f64>,
    pub sharpe_ratio: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub total_trades: usize,
    pub calmar_ratio: f64,
    pub profit_factor: f64,
    pub expectancy: f64,
}

/// Generic optimizer that works with any strategy
pub struct Optimizer;

impl Optimizer {
    pub fn new(_base_config: Config) -> Self {
        Optimizer
    }

    /// Run optimization with MTF data (unified interface)
    ///
    /// Takes a reference to data to avoid cloning for each parallel iteration.
    /// This significantly reduces memory usage with large datasets.
    pub fn optimize<F>(
        &self,
        data: &MultiSymbolMultiTimeframeData,
        configs: Vec<Config>,
        strategy_factory: F,
    ) -> Vec<OptimizationResult>
    where
        F: Fn(&Config) -> Box<dyn Strategy> + Send + Sync,
    {
        tracing::info!("Testing {} parameter combinations", configs.len());

        configs
            .par_iter()
            .map(|config| {
                let strategy = strategy_factory(config);
                let mut backtester = Backtester::new(config.clone(), strategy);
                let result = backtester.run(data);

                OptimizationResult {
                    params: crate::grid::extract_params(config),
                    sharpe_ratio: result.metrics.sharpe_ratio,
                    total_return: result.metrics.total_return,
                    max_drawdown: result.metrics.max_drawdown,
                    win_rate: result.metrics.win_rate,
                    total_trades: result.metrics.total_trades,
                    calmar_ratio: result.metrics.calmar_ratio,
                    profit_factor: result.metrics.profit_factor,
                    expectancy: result.metrics.expectancy,
                }
            })
            .collect()
    }

    /// Run optimization with progress tracking
    pub fn optimize_with_progress<F>(
        &self,
        data: &MultiSymbolMultiTimeframeData,
        configs: Vec<Config>,
        strategy_factory: F,
        progress_bar: ProgressBar,
    ) -> Vec<OptimizationResult>
    where
        F: Fn(&Config) -> Box<dyn Strategy> + Send + Sync,
    {
        tracing::info!(
            "Testing {} parameter combinations with progress",
            configs.len()
        );

        configs
            .par_iter()
            .map(|config| {
                let strategy = strategy_factory(config);
                let mut backtester = Backtester::new(config.clone(), strategy);
                let result = backtester.run(data);
                progress_bar.inc(1);

                OptimizationResult {
                    params: crate::grid::extract_params(config),
                    sharpe_ratio: result.metrics.sharpe_ratio,
                    total_return: result.metrics.total_return,
                    max_drawdown: result.metrics.max_drawdown,
                    win_rate: result.metrics.win_rate,
                    total_trades: result.metrics.total_trades,
                    calmar_ratio: result.metrics.calmar_ratio,
                    profit_factor: result.metrics.profit_factor,
                    expectancy: result.metrics.expectancy,
                }
            })
            .collect()
    }

    /// Run optimization sequentially (for debugging)
    pub fn optimize_sequential<F>(
        &self,
        data: &MultiSymbolMultiTimeframeData,
        configs: Vec<Config>,
        strategy_factory: &F,
    ) -> Vec<OptimizationResult>
    where
        F: Fn(&Config) -> Box<dyn Strategy>,
    {
        tracing::info!(
            "Testing {} parameter combinations sequentially",
            configs.len()
        );

        configs
            .iter()
            .map(|config| {
                let strategy = strategy_factory(config);
                let mut backtester = Backtester::new(config.clone(), strategy);
                let result = backtester.run(data);

                OptimizationResult {
                    params: crate::grid::extract_params(config),
                    sharpe_ratio: result.metrics.sharpe_ratio,
                    total_return: result.metrics.total_return,
                    max_drawdown: result.metrics.max_drawdown,
                    win_rate: result.metrics.win_rate,
                    total_trades: result.metrics.total_trades,
                    calmar_ratio: result.metrics.calmar_ratio,
                    profit_factor: result.metrics.profit_factor,
                    expectancy: result.metrics.expectancy,
                }
            })
            .collect()
    }

    /// Sort optimization results by specified metric
    pub fn sort_results(results: &mut [OptimizationResult], sort_by: &str) {
        results.sort_by(|a, b| {
            let (va, vb) = match sort_by {
                "calmar" => (a.calmar_ratio, b.calmar_ratio),
                "return" => (a.total_return, b.total_return),
                "win_rate" => (a.win_rate, b.win_rate),
                "profit_factor" => (a.profit_factor, b.profit_factor),
                "expectancy" => (a.expectancy, b.expectancy),
                _ => (a.sharpe_ratio, b.sharpe_ratio),
            };
            vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Helper to convert single-TF data to MTF format
pub fn single_tf_to_mtf(
    data: HashMap<Symbol, Vec<Candle>>,
    timeframe: &str,
) -> MultiSymbolMultiTimeframeData {
    data.into_iter()
        .map(|(symbol, candles)| {
            let mut mtf = MultiTimeframeData::new(timeframe);
            mtf.add_timeframe(timeframe, candles);
            (symbol, mtf)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn create_test_candle(datetime: chrono::DateTime<Utc>, close: f64) -> Candle {
        Candle::new(
            datetime,
            close - 5.0,
            close + 5.0,
            close - 10.0,
            close,
            1000.0,
        )
        .unwrap()
    }

    fn create_test_candles(base_time: chrono::DateTime<Utc>, count: usize) -> Vec<Candle> {
        (0..count)
            .map(|i| {
                let dt = base_time + Duration::days(i as i64);
                create_test_candle(dt, 100.0 + i as f64)
            })
            .collect()
    }

    fn create_test_config() -> Config {
        Config {
            exchange: crate::config::ExchangeConfig::default(),
            trading: crate::config::TradingConfig {
                symbols: vec!["BTCINR".to_string()],
                initial_capital: 100000.0,
                ..crate::config::TradingConfig::default()
            },
            strategy: serde_json::json!({
                "name": "volatility_regime",
                "timeframe": "1d",
                "atr_period": 14,
                "stop_atr": 2.0,
                "target_atr": 4.0
            }),
            tax: crate::config::TaxConfig::default(),
            backtest: crate::config::BacktestConfig::default(),
            grid: None,
        }
    }

    // ==================== OptimizationResult Tests ====================

    #[test]
    fn test_optimization_result_creation() {
        let mut params = HashMap::new();
        params.insert("atr_period".to_string(), 14.0);
        params.insert("stop_atr".to_string(), 2.0);

        let result = OptimizationResult {
            params,
            sharpe_ratio: 1.5,
            total_return: 50.0,
            max_drawdown: 10.0,
            win_rate: 55.0,
            total_trades: 100,
            calmar_ratio: 5.0,
            profit_factor: 2.0,
            expectancy: 150.0,
        };

        assert_eq!(result.sharpe_ratio, 1.5);
        assert_eq!(result.total_return, 50.0);
        assert_eq!(result.max_drawdown, 10.0);
        assert_eq!(result.win_rate, 55.0);
        assert_eq!(result.total_trades, 100);
        assert_eq!(result.calmar_ratio, 5.0);
        assert_eq!(result.profit_factor, 2.0);
        assert_eq!(result.expectancy, 150.0);
        assert_eq!(result.params.len(), 2);
    }

    #[test]
    fn test_optimization_result_clone() {
        let mut params = HashMap::new();
        params.insert("atr_period".to_string(), 14.0);

        let result = OptimizationResult {
            params,
            sharpe_ratio: 1.5,
            total_return: 50.0,
            max_drawdown: 10.0,
            win_rate: 55.0,
            total_trades: 100,
            calmar_ratio: 5.0,
            profit_factor: 2.0,
            expectancy: 150.0,
        };

        let cloned = result.clone();
        assert_eq!(cloned.sharpe_ratio, result.sharpe_ratio);
        assert_eq!(
            cloned.params.get("atr_period"),
            result.params.get("atr_period")
        );
    }

    // ==================== Optimizer Tests ====================

    #[test]
    fn test_optimizer_new() {
        let config = create_test_config();
        let _optimizer = Optimizer::new(config);
        // Optimizer created successfully
    }

    // ==================== sort_results Tests ====================

    fn create_test_results() -> Vec<OptimizationResult> {
        vec![
            OptimizationResult {
                params: HashMap::new(),
                sharpe_ratio: 1.0,
                total_return: 30.0,
                max_drawdown: 15.0,
                win_rate: 50.0,
                total_trades: 80,
                calmar_ratio: 2.0,
                profit_factor: 1.5,
                expectancy: 100.0,
            },
            OptimizationResult {
                params: HashMap::new(),
                sharpe_ratio: 2.0,
                total_return: 50.0,
                max_drawdown: 10.0,
                win_rate: 60.0,
                total_trades: 100,
                calmar_ratio: 5.0,
                profit_factor: 2.5,
                expectancy: 200.0,
            },
            OptimizationResult {
                params: HashMap::new(),
                sharpe_ratio: 1.5,
                total_return: 40.0,
                max_drawdown: 12.0,
                win_rate: 55.0,
                total_trades: 90,
                calmar_ratio: 3.3,
                profit_factor: 2.0,
                expectancy: 150.0,
            },
        ]
    }

    #[test]
    fn test_sort_results_by_sharpe() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "sharpe");

        // Should be sorted descending by sharpe_ratio
        assert_eq!(results[0].sharpe_ratio, 2.0);
        assert_eq!(results[1].sharpe_ratio, 1.5);
        assert_eq!(results[2].sharpe_ratio, 1.0);
    }

    #[test]
    fn test_sort_results_by_calmar() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "calmar");

        // Should be sorted descending by calmar_ratio
        assert_eq!(results[0].calmar_ratio, 5.0);
        assert_eq!(results[1].calmar_ratio, 3.3);
        assert_eq!(results[2].calmar_ratio, 2.0);
    }

    #[test]
    fn test_sort_results_by_return() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "return");

        // Should be sorted descending by total_return
        assert_eq!(results[0].total_return, 50.0);
        assert_eq!(results[1].total_return, 40.0);
        assert_eq!(results[2].total_return, 30.0);
    }

    #[test]
    fn test_sort_results_by_win_rate() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "win_rate");

        // Should be sorted descending by win_rate
        assert_eq!(results[0].win_rate, 60.0);
        assert_eq!(results[1].win_rate, 55.0);
        assert_eq!(results[2].win_rate, 50.0);
    }

    #[test]
    fn test_sort_results_by_profit_factor() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "profit_factor");

        // Should be sorted descending by profit_factor
        assert_eq!(results[0].profit_factor, 2.5);
        assert_eq!(results[1].profit_factor, 2.0);
        assert_eq!(results[2].profit_factor, 1.5);
    }

    #[test]
    fn test_sort_results_by_expectancy() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "expectancy");

        // Should be sorted descending by expectancy
        assert_eq!(results[0].expectancy, 200.0);
        assert_eq!(results[1].expectancy, 150.0);
        assert_eq!(results[2].expectancy, 100.0);
    }

    #[test]
    fn test_sort_results_unknown_metric_defaults_to_sharpe() {
        let mut results = create_test_results();
        Optimizer::sort_results(&mut results, "unknown_metric");

        // Should default to sharpe_ratio
        assert_eq!(results[0].sharpe_ratio, 2.0);
        assert_eq!(results[1].sharpe_ratio, 1.5);
        assert_eq!(results[2].sharpe_ratio, 1.0);
    }

    #[test]
    fn test_sort_results_empty() {
        let mut results: Vec<OptimizationResult> = vec![];
        Optimizer::sort_results(&mut results, "sharpe");
        assert!(results.is_empty());
    }

    #[test]
    fn test_sort_results_single_element() {
        let mut results = vec![OptimizationResult {
            params: HashMap::new(),
            sharpe_ratio: 1.5,
            total_return: 40.0,
            max_drawdown: 12.0,
            win_rate: 55.0,
            total_trades: 90,
            calmar_ratio: 3.3,
            profit_factor: 2.0,
            expectancy: 150.0,
        }];

        Optimizer::sort_results(&mut results, "sharpe");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].sharpe_ratio, 1.5);
    }

    // ==================== single_tf_to_mtf Tests ====================

    #[test]
    fn test_single_tf_to_mtf_empty() {
        let data: HashMap<Symbol, Vec<Candle>> = HashMap::new();
        let mtf_data = single_tf_to_mtf(data, "1d");

        assert!(mtf_data.is_empty());
    }

    #[test]
    fn test_single_tf_to_mtf_single_symbol() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles = create_test_candles(base_time, 10);

        let mut data: HashMap<Symbol, Vec<Candle>> = HashMap::new();
        data.insert(Symbol::new("BTCINR"), candles);

        let mtf_data = single_tf_to_mtf(data, "1d");

        assert_eq!(mtf_data.len(), 1);
        assert!(mtf_data.contains_key(&Symbol::new("BTCINR")));

        let btc_mtf = mtf_data.get(&Symbol::new("BTCINR")).unwrap();
        assert_eq!(btc_mtf.primary_timeframe(), "1d");
        assert!(btc_mtf.has_timeframe("1d"));
        assert_eq!(btc_mtf.primary().len(), 10);
    }

    #[test]
    fn test_single_tf_to_mtf_multiple_symbols() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let mut data: HashMap<Symbol, Vec<Candle>> = HashMap::new();
        data.insert(Symbol::new("BTCINR"), create_test_candles(base_time, 10));
        data.insert(Symbol::new("ETHINR"), create_test_candles(base_time, 15));
        data.insert(Symbol::new("SOLINR"), create_test_candles(base_time, 20));

        let mtf_data = single_tf_to_mtf(data, "4h");

        assert_eq!(mtf_data.len(), 3);
        assert!(mtf_data.contains_key(&Symbol::new("BTCINR")));
        assert!(mtf_data.contains_key(&Symbol::new("ETHINR")));
        assert!(mtf_data.contains_key(&Symbol::new("SOLINR")));

        // Check each has correct timeframe
        for mtf in mtf_data.values() {
            assert_eq!(mtf.primary_timeframe(), "4h");
            assert!(mtf.has_timeframe("4h"));
        }

        // Check lengths
        assert_eq!(
            mtf_data
                .get(&Symbol::new("BTCINR"))
                .unwrap()
                .primary()
                .len(),
            10
        );
        assert_eq!(
            mtf_data
                .get(&Symbol::new("ETHINR"))
                .unwrap()
                .primary()
                .len(),
            15
        );
        assert_eq!(
            mtf_data
                .get(&Symbol::new("SOLINR"))
                .unwrap()
                .primary()
                .len(),
            20
        );
    }

    #[test]
    fn test_single_tf_to_mtf_preserves_candle_data() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let candles = create_test_candles(base_time, 5);
        let first_close = candles[0].close;
        let last_close = candles[4].close;

        let mut data: HashMap<Symbol, Vec<Candle>> = HashMap::new();
        data.insert(Symbol::new("BTCINR"), candles);

        let mtf_data = single_tf_to_mtf(data, "1d");

        let primary = mtf_data.get(&Symbol::new("BTCINR")).unwrap().primary();
        assert_eq!(primary[0].close, first_close);
        assert_eq!(primary[4].close, last_close);
    }

    #[test]
    fn test_single_tf_to_mtf_different_timeframes() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

        let mut data: HashMap<Symbol, Vec<Candle>> = HashMap::new();
        data.insert(Symbol::new("BTCINR"), create_test_candles(base_time, 5));

        // Test with different timeframe strings
        for tf in ["1m", "5m", "15m", "1h", "4h", "1d", "1w"] {
            let data_clone = data.clone();
            let mtf_data = single_tf_to_mtf(data_clone, tf);

            let btc_mtf = mtf_data.get(&Symbol::new("BTCINR")).unwrap();
            assert_eq!(btc_mtf.primary_timeframe(), tf);
            assert!(btc_mtf.has_timeframe(tf));
        }
    }

    // ==================== Integration-style Tests ====================

    #[test]
    fn test_sort_results_stability_with_equal_values() {
        // Test that sorting is stable/deterministic with equal values
        let mut results = vec![
            OptimizationResult {
                params: {
                    let mut p = HashMap::new();
                    p.insert("id".to_string(), 1.0);
                    p
                },
                sharpe_ratio: 1.5,
                total_return: 40.0,
                max_drawdown: 12.0,
                win_rate: 55.0,
                total_trades: 90,
                calmar_ratio: 3.0,
                profit_factor: 2.0,
                expectancy: 150.0,
            },
            OptimizationResult {
                params: {
                    let mut p = HashMap::new();
                    p.insert("id".to_string(), 2.0);
                    p
                },
                sharpe_ratio: 1.5, // Same sharpe
                total_return: 40.0,
                max_drawdown: 12.0,
                win_rate: 55.0,
                total_trades: 90,
                calmar_ratio: 3.0,
                profit_factor: 2.0,
                expectancy: 150.0,
            },
        ];

        Optimizer::sort_results(&mut results, "sharpe");

        // Both have same sharpe, so order should be preserved (stable sort behavior)
        assert_eq!(results.len(), 2);
        // Just verify both are still present
        let ids: Vec<f64> = results
            .iter()
            .filter_map(|r| r.params.get("id").copied())
            .collect();
        assert!(ids.contains(&1.0));
        assert!(ids.contains(&2.0));
    }
}
