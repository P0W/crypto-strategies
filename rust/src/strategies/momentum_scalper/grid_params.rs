//! Grid search parameters for Momentum Scalper Strategy
//!
//! Optimized for short timeframe trading (5m, 15m, 1h).

use crate::Config;
use serde::{Deserialize, Serialize};

/// Grid search parameters for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridParams {
    /// Fast EMA periods
    pub ema_fast: Vec<usize>,
    /// Slow EMA periods
    pub ema_slow: Vec<usize>,
    /// ADX thresholds
    pub adx_thresholds: Vec<f64>,
    /// Stop ATR multiples
    pub stop_atr_multiples: Vec<f64>,
    /// Target ATR multiples
    pub target_atr_multiples: Vec<f64>,
    /// Use MACD filter
    pub use_macd: Vec<bool>,
    /// Trade with trend filter
    pub trade_with_trend: Vec<bool>,
}

impl GridParams {
    /// Quick grid for faster optimization
    pub fn quick() -> Self {
        GridParams {
            ema_fast: vec![5, 9],
            ema_slow: vec![13, 21],
            adx_thresholds: vec![15.0, 20.0, 25.0],
            stop_atr_multiples: vec![0.75, 1.0, 1.5],
            target_atr_multiples: vec![1.0, 1.5, 2.0],
            use_macd: vec![true, false],
            trade_with_trend: vec![true, false],
        }
    }

    /// Full grid for comprehensive optimization
    pub fn full() -> Self {
        GridParams {
            ema_fast: vec![3, 5, 8, 9, 13],
            ema_slow: vec![13, 21, 34, 55],
            adx_thresholds: vec![10.0, 15.0, 20.0, 25.0, 30.0],
            stop_atr_multiples: vec![0.5, 0.75, 1.0, 1.25, 1.5],
            target_atr_multiples: vec![1.0, 1.5, 2.0, 2.5, 3.0],
            use_macd: vec![true, false],
            trade_with_trend: vec![true, false],
        }
    }

    /// Generate all parameter combinations
    pub fn generate_configs(&self, base_config: &Config) -> Vec<Config> {
        use itertools::iproduct;

        iproduct!(
            &self.ema_fast,
            &self.ema_slow,
            &self.adx_thresholds,
            &self.stop_atr_multiples,
            &self.target_atr_multiples,
            &self.use_macd,
            &self.trade_with_trend
        )
        .filter_map(
            |(fast, slow, adx, stop, target, use_macd, with_trend)| {
                // Skip invalid: fast must be less than slow
                if fast >= slow {
                    return None;
                }
                // Target should be >= stop for positive R:R
                if target < stop {
                    return None;
                }

                let mut config = base_config.clone();

                if let Some(obj) = config.strategy.as_object_mut() {
                    obj.insert("ema_fast".to_string(), serde_json::json!(fast));
                    obj.insert("ema_slow".to_string(), serde_json::json!(slow));
                    obj.insert("adx_threshold".to_string(), serde_json::json!(adx));
                    obj.insert("stop_atr_multiple".to_string(), serde_json::json!(stop));
                    obj.insert("target_atr_multiple".to_string(), serde_json::json!(target));
                    obj.insert("use_macd".to_string(), serde_json::json!(use_macd));
                    obj.insert("trade_with_trend".to_string(), serde_json::json!(with_trend));
                }

                Some(config)
            },
        )
        .collect()
    }

    /// Get total number of valid combinations
    pub fn total_combinations(&self) -> usize {
        use itertools::iproduct;

        iproduct!(
            &self.ema_fast,
            &self.ema_slow,
            &self.adx_thresholds,
            &self.stop_atr_multiples,
            &self.target_atr_multiples,
            &self.use_macd,
            &self.trade_with_trend
        )
        .filter(|(fast, slow, _, stop, target, _, _)| fast < slow && target >= stop)
        .count()
    }
}
