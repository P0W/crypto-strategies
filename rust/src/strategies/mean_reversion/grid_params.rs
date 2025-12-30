//! Grid search parameters for Mean Reversion Scalper Strategy
//!
//! Defines parameter ranges for optimization and grid generation using itertools.
//! Optimized for short timeframe trading (5m, 15m, 1h).

use crate::Config;
use serde::{Deserialize, Serialize};

/// Grid search parameters for optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridParams {
    /// Bollinger Band periods to test
    pub bb_periods: Vec<usize>,
    /// Bollinger Band standard deviations to test
    pub bb_stds: Vec<f64>,
    /// RSI periods to test
    pub rsi_periods: Vec<usize>,
    /// RSI oversold thresholds to test
    pub rsi_oversolds: Vec<f64>,
    /// RSI overbought thresholds to test
    pub rsi_overboughts: Vec<f64>,
    /// Volume spike thresholds to test
    pub volume_spikes: Vec<f64>,
    /// Stop ATR multiples to test
    pub stop_atr_multiples: Vec<f64>,
    /// Target ATR multiples to test (for ATR mode)
    pub target_atr_multiples: Vec<f64>,
}

impl GridParams {
    /// Quick grid for faster optimization
    /// Tests key parameters that have the most impact
    pub fn quick() -> Self {
        GridParams {
            bb_periods: vec![20],
            bb_stds: vec![2.0, 2.5],
            rsi_periods: vec![14],
            rsi_oversolds: vec![25.0, 30.0],
            rsi_overboughts: vec![70.0, 75.0],
            volume_spikes: vec![1.5, 2.0],
            stop_atr_multiples: vec![1.0, 1.5, 2.0],
            target_atr_multiples: vec![2.0, 3.0],
        }
    }

    /// Full grid for comprehensive optimization
    pub fn full() -> Self {
        GridParams {
            bb_periods: vec![15, 20, 25],
            bb_stds: vec![1.5, 2.0, 2.5, 3.0],
            rsi_periods: vec![7, 14, 21],
            rsi_oversolds: vec![20.0, 25.0, 30.0, 35.0],
            rsi_overboughts: vec![65.0, 70.0, 75.0, 80.0],
            volume_spikes: vec![1.2, 1.5, 2.0, 2.5],
            stop_atr_multiples: vec![1.0, 1.5, 2.0, 2.5],
            target_atr_multiples: vec![1.5, 2.0, 2.5, 3.0],
        }
    }

    /// Custom grid from vectors
    #[allow(clippy::too_many_arguments)]
    pub fn custom(
        bb_periods: Vec<usize>,
        bb_stds: Vec<f64>,
        rsi_periods: Vec<usize>,
        rsi_oversolds: Vec<f64>,
        rsi_overboughts: Vec<f64>,
        volume_spikes: Vec<f64>,
        stop_atr_multiples: Vec<f64>,
        target_atr_multiples: Vec<f64>,
    ) -> Self {
        GridParams {
            bb_periods,
            bb_stds,
            rsi_periods,
            rsi_oversolds,
            rsi_overboughts,
            volume_spikes,
            stop_atr_multiples,
            target_atr_multiples,
        }
    }

    /// Generate all parameter combinations using itertools
    pub fn generate_configs(&self, base_config: &Config) -> Vec<Config> {
        use itertools::iproduct;

        iproduct!(
            &self.bb_periods,
            &self.bb_stds,
            &self.rsi_periods,
            &self.rsi_oversolds,
            &self.rsi_overboughts,
            &self.volume_spikes,
            &self.stop_atr_multiples,
            &self.target_atr_multiples
        )
        .filter_map(
            |(bb_period, bb_std, rsi_period, rsi_os, rsi_ob, vol_spike, stop_atr, target_atr)| {
                // Skip invalid combinations
                // RSI oversold must be less than overbought
                if rsi_os >= rsi_ob {
                    return None;
                }
                // Target should be greater than stop for positive R:R
                if target_atr <= stop_atr {
                    return None;
                }

                let mut config = base_config.clone();

                // Update strategy parameters in the JSON value
                if let Some(obj) = config.strategy.as_object_mut() {
                    obj.insert("bb_period".to_string(), serde_json::json!(bb_period));
                    obj.insert("bb_std".to_string(), serde_json::json!(bb_std));
                    obj.insert("rsi_period".to_string(), serde_json::json!(rsi_period));
                    obj.insert("rsi_oversold".to_string(), serde_json::json!(rsi_os));
                    obj.insert("rsi_overbought".to_string(), serde_json::json!(rsi_ob));
                    obj.insert(
                        "volume_spike_threshold".to_string(),
                        serde_json::json!(vol_spike),
                    );
                    obj.insert(
                        "stop_atr_multiple".to_string(),
                        serde_json::json!(stop_atr),
                    );
                    obj.insert(
                        "target_atr_multiple".to_string(),
                        serde_json::json!(target_atr),
                    );
                }

                Some(config)
            },
        )
        .collect()
    }

    /// Get total number of valid parameter combinations
    pub fn total_combinations(&self) -> usize {
        use itertools::iproduct;

        iproduct!(
            &self.bb_periods,
            &self.bb_stds,
            &self.rsi_periods,
            &self.rsi_oversolds,
            &self.rsi_overboughts,
            &self.volume_spikes,
            &self.stop_atr_multiples,
            &self.target_atr_multiples
        )
        .filter(|(_, _, _, rsi_os, rsi_ob, _, stop_atr, target_atr)| {
            rsi_os < rsi_ob && target_atr > stop_atr
        })
        .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quick_grid_combinations() {
        let grid = GridParams::quick();
        let count = grid.total_combinations();
        // Should generate reasonable number of combinations
        assert!(count > 0, "Quick grid should have at least some combinations");
        assert!(count < 500, "Quick grid should not have too many combinations");
        println!("Quick grid combinations: {}", count);
    }

    #[test]
    fn test_full_grid_combinations() {
        let grid = GridParams::full();
        let count = grid.total_combinations();
        // Full grid should have more combinations
        assert!(count > 100, "Full grid should have significant combinations");
        println!("Full grid combinations: {}", count);
    }

    #[test]
    fn test_invalid_combinations_filtered() {
        // Create a grid where some RSI combinations are invalid
        let grid = GridParams {
            bb_periods: vec![20],
            bb_stds: vec![2.0],
            rsi_periods: vec![14],
            rsi_oversolds: vec![30.0, 70.0], // 70.0 should be filtered when paired with 70.0 overbought
            rsi_overboughts: vec![70.0],
            volume_spikes: vec![1.5],
            stop_atr_multiples: vec![1.5],
            target_atr_multiples: vec![3.0],
        };

        let count = grid.total_combinations();
        // Only the 30.0/70.0 combination should be valid
        assert_eq!(count, 1, "Should filter out invalid RSI combinations");
    }
}
