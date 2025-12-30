//! Grid search parameters for Volatility Regime Strategy
//!
//! Defines parameter ranges for optimization and grid generation using itertools.

use crate::Config;
use serde::{Deserialize, Serialize};

/// Grid search parameters for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridParams {
    pub atr_periods: Vec<usize>,
    pub ema_fast_periods: Vec<usize>,
    pub ema_slow_periods: Vec<usize>,
    pub adx_thresholds: Vec<f64>,
    pub stop_atr_multiples: Vec<f64>,
    pub target_atr_multiples: Vec<f64>,
}

impl GridParams {
    /// Quick grid for faster optimization
    pub fn quick() -> Self {
        GridParams {
            atr_periods: vec![14],
            ema_fast_periods: vec![8, 13],
            ema_slow_periods: vec![21, 34],
            adx_thresholds: vec![25.0, 30.0],
            stop_atr_multiples: vec![2.0, 2.5],
            target_atr_multiples: vec![4.0, 5.0],
        }
    }

    /// Full grid for comprehensive optimization
    pub fn full() -> Self {
        GridParams {
            atr_periods: vec![10, 14, 20],
            ema_fast_periods: vec![5, 8, 13],
            ema_slow_periods: vec![21, 34, 55],
            adx_thresholds: vec![20.0, 25.0, 30.0, 35.0],
            stop_atr_multiples: vec![1.5, 2.0, 2.5, 3.0],
            target_atr_multiples: vec![3.0, 4.0, 5.0, 6.0],
        }
    }

    /// Custom grid from vectors
    pub fn custom(
        atr_periods: Vec<usize>,
        ema_fast_periods: Vec<usize>,
        ema_slow_periods: Vec<usize>,
        adx_thresholds: Vec<f64>,
        stop_atr_multiples: Vec<f64>,
        target_atr_multiples: Vec<f64>,
    ) -> Self {
        GridParams {
            atr_periods,
            ema_fast_periods,
            ema_slow_periods,
            adx_thresholds,
            stop_atr_multiples,
            target_atr_multiples,
        }
    }

    /// Generate all parameter combinations using itertools
    pub fn generate_configs(&self, base_config: &Config) -> Vec<Config> {
        // Use itertools iproduct! macro for clean cartesian product
        use itertools::iproduct;

        iproduct!(
            &self.atr_periods,
            &self.ema_fast_periods,
            &self.ema_slow_periods,
            &self.adx_thresholds,
            &self.stop_atr_multiples,
            &self.target_atr_multiples
        )
        .filter_map(|(atr, fast, slow, adx, stop, target)| {
            // Skip invalid combinations
            if fast >= slow || target <= stop {
                return None;
            }

            let mut config = base_config.clone();

            // Update strategy parameters in the JSON value
            if let Some(obj) = config.strategy.as_object_mut() {
                obj.insert("atr_period".to_string(), serde_json::json!(atr));
                obj.insert("ema_fast".to_string(), serde_json::json!(fast));
                obj.insert("ema_slow".to_string(), serde_json::json!(slow));
                obj.insert("adx_threshold".to_string(), serde_json::json!(adx));
                obj.insert("stop_atr_multiple".to_string(), serde_json::json!(stop));
                obj.insert("target_atr_multiple".to_string(), serde_json::json!(target));
            }

            Some(config)
        })
        .collect()
    }

    /// Get total number of parameter combinations
    pub fn total_combinations(&self) -> usize {
        use itertools::iproduct;

        iproduct!(
            &self.atr_periods,
            &self.ema_fast_periods,
            &self.ema_slow_periods,
            &self.adx_thresholds,
            &self.stop_atr_multiples,
            &self.target_atr_multiples
        )
        .filter(|(_, fast, slow, _, stop, target)| fast < slow && target > stop)
        .count()
    }
}
