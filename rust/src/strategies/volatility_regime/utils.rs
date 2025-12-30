//! Utility functions for Volatility Regime Strategy
//!
//! Helper functions for strategy instantiation and reporting.

use super::config::VolatilityRegimeConfig;
use super::strategy::VolatilityRegimeStrategy;
use crate::Config;
use anyhow::Result;
use std::collections::HashMap;

/// Create a Volatility Regime Strategy from global config
pub fn create_strategy_from_config(config: &Config) -> Result<VolatilityRegimeStrategy> {
    let vr_config: VolatilityRegimeConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse strategy config: {}", e))?;

    Ok(VolatilityRegimeStrategy::new(vr_config))
}

/// Convert VolatilityRegimeConfig to a parameter map for reporting
pub fn config_to_params(config: &VolatilityRegimeConfig) -> HashMap<String, f64> {
    let mut params = HashMap::new();
    params.insert("atr_period".to_string(), config.atr_period as f64);
    params.insert("ema_fast".to_string(), config.ema_fast as f64);
    params.insert("ema_slow".to_string(), config.ema_slow as f64);
    params.insert("adx_threshold".to_string(), config.adx_threshold);
    params.insert("stop_atr_multiple".to_string(), config.stop_atr_multiple);
    params.insert(
        "target_atr_multiple".to_string(),
        config.target_atr_multiple,
    );
    params
}

/// Format parameters for display
pub fn format_params(params: &HashMap<String, f64>) -> String {
    format!(
        "ATR:{} EMA:{}/{} ADX:{} Stop:{:.1} Tgt:{:.1}",
        *params.get("atr_period").unwrap_or(&0.0) as usize,
        *params.get("ema_fast").unwrap_or(&0.0) as usize,
        *params.get("ema_slow").unwrap_or(&0.0) as usize,
        params.get("adx_threshold").unwrap_or(&0.0),
        params.get("stop_atr_multiple").unwrap_or(&0.0),
        params.get("target_atr_multiple").unwrap_or(&0.0),
    )
}
