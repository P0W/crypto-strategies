//! Grid Search Parameters for Simple Trend Strategy

use crate::Config;
use super::config::SimpleTrendConfig;

/// Grid search parameter combinations
pub struct GridParams {
    pub ema_periods: Vec<usize>,
    pub stop_atrs: Vec<f64>,
    pub target_atrs: Vec<f64>,
    pub require_expansion: Vec<bool>,
}

impl GridParams {
    /// Quick mode - fewer combinations for rapid testing
    pub fn quick() -> Self {
        Self {
            ema_periods: vec![10, 20, 50],
            stop_atrs: vec![1.5, 2.0, 2.5],
            target_atrs: vec![3.0, 4.0, 5.0],
            require_expansion: vec![true, false],
        }
    }

    /// Full mode - comprehensive search
    pub fn full() -> Self {
        Self {
            ema_periods: vec![8, 13, 20, 34, 50],
            stop_atrs: vec![1.0, 1.5, 2.0, 2.5, 3.0],
            target_atrs: vec![2.0, 3.0, 4.0, 5.0, 6.0],
            require_expansion: vec![true, false],
        }
    }

    /// Total number of combinations
    pub fn total_combinations(&self) -> usize {
        self.ema_periods.len()
            * self.stop_atrs.len()
            * self.target_atrs.len()
            * self.require_expansion.len()
    }

    /// Generate all config combinations
    pub fn generate_configs(&self, base_config: &Config) -> Vec<Config> {
        let mut configs = Vec::with_capacity(self.total_combinations());

        for &ema in &self.ema_periods {
            for &stop in &self.stop_atrs {
                for &target in &self.target_atrs {
                    for &require_exp in &self.require_expansion {
                        let mut config = base_config.clone();
                        
                        let strategy_config = SimpleTrendConfig {
                            ema_period: ema,
                            atr_period: 14,
                            atr_lookback: 5,
                            stop_atr_multiple: stop,
                            target_atr_multiple: target,
                            trailing_activation: 0.5,
                            trailing_atr_multiple: 1.5,
                            require_expansion: require_exp,
                            expansion_threshold: 1.0,
                        };
                        
                        config.strategy = serde_json::to_value(&strategy_config).unwrap();
                        configs.push(config);
                    }
                }
            }
        }

        configs
    }
}
