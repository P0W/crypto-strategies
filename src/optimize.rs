//! Parameter optimization framework
//!
//! Grid search optimization with parallel execution using Rayon.

use rayon::prelude::*;
use std::collections::HashMap;

use crate::{Candle, Config, Symbol};
use crate::backtest::Backtester;
use crate::strategy::VolatilityRegimeStrategy;

#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub params: HashMap<String, f64>,
    pub sharpe_ratio: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub total_trades: usize,
}

pub struct Optimizer {
    base_config: Config,
}

impl Optimizer {
    pub fn new(base_config: Config) -> Self {
        Optimizer { base_config }
    }

    /// Run parameter optimization
    pub fn optimize(
        &self,
        data: HashMap<Symbol, Vec<Candle>>,
        param_grid: ParamGrid,
    ) -> Vec<OptimizationResult> {
        let configs = param_grid.generate_configs(&self.base_config);

        log::info!("Testing {} parameter combinations", configs.len());

        let results: Vec<OptimizationResult> = configs
            .par_iter()
            .map(|config| {
                let strategy = Box::new(VolatilityRegimeStrategy::new(config.strategy.clone()));
                let mut backtester = Backtester::new(config.clone(), strategy);
                let result = backtester.run(data.clone());

                let params = self.config_to_params(&config.strategy);

                OptimizationResult {
                    params,
                    sharpe_ratio: result.metrics.sharpe_ratio,
                    total_return: result.metrics.total_return,
                    max_drawdown: result.metrics.max_drawdown,
                    win_rate: result.metrics.win_rate,
                    total_trades: result.metrics.total_trades,
                }
            })
            .collect();

        results
    }

    fn config_to_params(&self, strategy: &crate::config::StrategyConfig) -> HashMap<String, f64> {
        let mut params = HashMap::new();
        params.insert("atr_period".to_string(), strategy.atr_period as f64);
        params.insert("ema_fast".to_string(), strategy.ema_fast as f64);
        params.insert("ema_slow".to_string(), strategy.ema_slow as f64);
        params.insert("adx_threshold".to_string(), strategy.adx_threshold);
        params.insert("stop_atr_multiple".to_string(), strategy.stop_atr_multiple);
        params.insert("target_atr_multiple".to_string(), strategy.target_atr_multiple);
        params
    }
}

/// Parameter grid for optimization
pub struct ParamGrid {
    pub atr_periods: Vec<usize>,
    pub ema_fast_periods: Vec<usize>,
    pub ema_slow_periods: Vec<usize>,
    pub adx_thresholds: Vec<f64>,
    pub stop_atr_multiples: Vec<f64>,
    pub target_atr_multiples: Vec<f64>,
}

impl ParamGrid {
    pub fn quick() -> Self {
        ParamGrid {
            atr_periods: vec![14],
            ema_fast_periods: vec![8, 13],
            ema_slow_periods: vec![21, 34],
            adx_thresholds: vec![25.0, 30.0],
            stop_atr_multiples: vec![2.0, 2.5],
            target_atr_multiples: vec![4.0, 5.0],
        }
    }

    pub fn full() -> Self {
        ParamGrid {
            atr_periods: vec![10, 14, 20],
            ema_fast_periods: vec![5, 8, 13],
            ema_slow_periods: vec![21, 34, 55],
            adx_thresholds: vec![20.0, 25.0, 30.0, 35.0],
            stop_atr_multiples: vec![1.5, 2.0, 2.5, 3.0],
            target_atr_multiples: vec![3.0, 4.0, 5.0, 6.0],
        }
    }

    fn generate_configs(&self, base: &Config) -> Vec<Config> {
        let mut configs = Vec::new();

        for &atr in &self.atr_periods {
            for &fast in &self.ema_fast_periods {
                for &slow in &self.ema_slow_periods {
                    if fast >= slow {
                        continue; // Skip invalid combinations
                    }
                    for &adx in &self.adx_thresholds {
                        for &stop in &self.stop_atr_multiples {
                            for &target in &self.target_atr_multiples {
                                if target <= stop {
                                    continue; // Skip invalid R:R
                                }

                                let mut config = base.clone();
                                config.strategy.atr_period = atr;
                                config.strategy.ema_fast = fast;
                                config.strategy.ema_slow = slow;
                                config.strategy.adx_threshold = adx;
                                config.strategy.stop_atr_multiple = stop;
                                config.strategy.target_atr_multiple = target;

                                configs.push(config);
                            }
                        }
                    }
                }
            }
        }

        configs
    }
}
