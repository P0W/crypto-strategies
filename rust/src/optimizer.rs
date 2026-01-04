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
pub struct Optimizer {
    _base_config: Config,
}

impl Optimizer {
    pub fn new(base_config: Config) -> Self {
        Optimizer {
            _base_config: base_config,
        }
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
