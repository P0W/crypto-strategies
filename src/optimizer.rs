//! Generic Parameter Optimization Framework
//!
//! Provides abstractions for parallel grid search optimization across any strategy.

use rayon::prelude::*;
use std::collections::HashMap;

use crate::{Candle, Config, Symbol};
use crate::backtest::Backtester;
use crate::Strategy;

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
}

/// Generic optimizer that works with any strategy
pub struct Optimizer {
    #[allow(dead_code)]
    base_config: Config,
}

impl Optimizer {
    pub fn new(base_config: Config) -> Self {
        Optimizer { base_config }
    }

    /// Run optimization with a custom strategy factory function
    ///
    /// The `strategy_factory` function takes a Config and returns a boxed Strategy.
    /// This allows for flexible strategy instantiation during optimization.
    pub fn optimize<F>(
        &self,
        data: HashMap<Symbol, Vec<Candle>>,
        configs: Vec<Config>,
        strategy_factory: F,
    ) -> Vec<OptimizationResult>
    where
        F: Fn(&Config) -> Box<dyn Strategy> + Send + Sync,
    {
        log::info!("Testing {} parameter combinations", configs.len());

        let results: Vec<OptimizationResult> = configs
            .par_iter()
            .map(|config| {
                let strategy = strategy_factory(config);
                let mut backtester = Backtester::new(config.clone(), strategy);
                let result = backtester.run(data.clone());

                OptimizationResult {
                    params: HashMap::new(), // To be filled by caller
                    sharpe_ratio: result.metrics.sharpe_ratio,
                    total_return: result.metrics.total_return,
                    max_drawdown: result.metrics.max_drawdown,
                    win_rate: result.metrics.win_rate,
                    total_trades: result.metrics.total_trades,
                    calmar_ratio: result.metrics.calmar_ratio,
                    profit_factor: result.metrics.profit_factor,
                }
            })
            .collect();

        results
    }

    /// Sort optimization results by specified metric
    pub fn sort_results(results: &mut Vec<OptimizationResult>, sort_by: &str) {
        match sort_by {
            "calmar" => results.sort_by(|a, b| b.calmar_ratio.partial_cmp(&a.calmar_ratio).unwrap_or(std::cmp::Ordering::Equal)),
            "return" => results.sort_by(|a, b| b.total_return.partial_cmp(&a.total_return).unwrap_or(std::cmp::Ordering::Equal)),
            "win_rate" => results.sort_by(|a, b| b.win_rate.partial_cmp(&a.win_rate).unwrap_or(std::cmp::Ordering::Equal)),
            "profit_factor" => results.sort_by(|a, b| b.profit_factor.partial_cmp(&a.profit_factor).unwrap_or(std::cmp::Ordering::Equal)),
            "sharpe" | _ => results.sort_by(|a, b| b.sharpe_ratio.partial_cmp(&a.sharpe_ratio).unwrap_or(std::cmp::Ordering::Equal)),
        }
    }
}
