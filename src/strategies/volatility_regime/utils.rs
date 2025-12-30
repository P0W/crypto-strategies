//! Utility functions for Volatility Regime Strategy
//!
//! Helper functions for strategy instantiation and parameter generation.

use crate::Config;
use super::config::VolatilityRegimeConfig;
use super::grid_params::GridParams;
use super::strategy::VolatilityRegimeStrategy;

/// Create a Volatility Regime Strategy from global config
pub fn create_strategy_from_config(config: &Config) -> VolatilityRegimeStrategy {
    let vr_config = VolatilityRegimeConfig {
        atr_period: config.strategy.atr_period,
        volatility_lookback: config.strategy.volatility_lookback,
        compression_threshold: config.strategy.compression_threshold,
        expansion_threshold: config.strategy.expansion_threshold,
        extreme_threshold: config.strategy.extreme_threshold,
        ema_fast: config.strategy.ema_fast,
        ema_slow: config.strategy.ema_slow,
        adx_period: config.strategy.adx_period,
        adx_threshold: config.strategy.adx_threshold,
        breakout_atr_multiple: config.strategy.breakout_atr_multiple,
        stop_atr_multiple: config.strategy.stop_atr_multiple,
        target_atr_multiple: config.strategy.target_atr_multiple,
        trailing_activation: config.strategy.trailing_activation,
        trailing_atr_multiple: config.strategy.trailing_atr_multiple,
    };
    
    VolatilityRegimeStrategy::new(vr_config)
}

/// Generate all parameter combinations from grid params
pub fn generate_configs(base_config: &Config, grid: &GridParams) -> Vec<Config> {
    let mut configs = Vec::new();

    for &atr in &grid.atr_periods {
        for &fast in &grid.ema_fast_periods {
            for &slow in &grid.ema_slow_periods {
                if fast >= slow {
                    continue; // Skip invalid combinations
                }
                for &adx in &grid.adx_thresholds {
                    for &stop in &grid.stop_atr_multiples {
                        for &target in &grid.target_atr_multiples {
                            if target <= stop {
                                continue; // Skip invalid R:R
                            }

                            let mut config = base_config.clone();
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

/// Convert VolatilityRegimeConfig to a parameter map for reporting
pub fn config_to_params(config: &VolatilityRegimeConfig) -> std::collections::HashMap<String, f64> {
    let mut params = std::collections::HashMap::new();
    params.insert("atr_period".to_string(), config.atr_period as f64);
    params.insert("ema_fast".to_string(), config.ema_fast as f64);
    params.insert("ema_slow".to_string(), config.ema_slow as f64);
    params.insert("adx_threshold".to_string(), config.adx_threshold);
    params.insert("stop_atr_multiple".to_string(), config.stop_atr_multiple);
    params.insert("target_atr_multiple".to_string(), config.target_atr_multiple);
    params
}
