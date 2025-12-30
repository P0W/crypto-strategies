//! Grid search parameters for Volatility Regime Strategy
//!
//! Defines parameter ranges for optimization.

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

    /// Get total number of parameter combinations
    pub fn total_combinations(&self) -> usize {
        let mut count = 0;
        for _ in &self.atr_periods {
            for &fast in &self.ema_fast_periods {
                for &slow in &self.ema_slow_periods {
                    if fast >= slow {
                        continue; // Skip invalid combinations
                    }
                    for _ in &self.adx_thresholds {
                        for &stop in &self.stop_atr_multiples {
                            for &target in &self.target_atr_multiples {
                                if target <= stop {
                                    continue; // Skip invalid R:R
                                }
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
        count
    }
}
