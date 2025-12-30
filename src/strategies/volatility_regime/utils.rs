//! Utility functions for Volatility Regime Strategy
//!
//! Helper functions for strategy instantiation and parameter generation.

use anyhow::Result;
use crate::Config;
use super::config::VolatilityRegimeConfig;
use super::grid_params::GridParams;
use super::strategy::VolatilityRegimeStrategy;

/// Create a Volatility Regime Strategy from global config
pub fn create_strategy_from_config(config: &Config) -> Result<VolatilityRegimeStrategy> {
    let vr_config: VolatilityRegimeConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse strategy config: {}", e))?;
    
    Ok(VolatilityRegimeStrategy::new(vr_config))
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
                            
                            // Update strategy parameters in the JSON value
                            if let Some(obj) = config.strategy.as_object_mut() {
                                obj.insert("atr_period".to_string(), serde_json::json!(atr));
                                obj.insert("ema_fast".to_string(), serde_json::json!(fast));
                                obj.insert("ema_slow".to_string(), serde_json::json!(slow));
                                obj.insert("adx_threshold".to_string(), serde_json::json!(adx));
                                obj.insert("stop_atr_multiple".to_string(), serde_json::json!(stop));
                                obj.insert("target_atr_multiple".to_string(), serde_json::json!(target));
                            }

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
