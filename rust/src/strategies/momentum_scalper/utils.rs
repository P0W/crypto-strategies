//! Utility functions for Momentum Scalper Strategy

use super::config::MomentumScalperConfig;
use super::strategy::MomentumScalperStrategy;
use crate::Config;
use anyhow::Result;
use std::collections::HashMap;

/// Create a Momentum Scalper Strategy from global config
pub fn create_strategy_from_config(config: &Config) -> Result<MomentumScalperStrategy> {
    let ms_config: MomentumScalperConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse momentum_scalper config: {}", e))?;

    Ok(MomentumScalperStrategy::new(ms_config))
}

/// Convert config to parameter map for reporting
pub fn config_to_params(config: &MomentumScalperConfig) -> HashMap<String, f64> {
    let mut params = HashMap::new();

    params.insert("ema_fast".to_string(), config.ema_fast as f64);
    params.insert("ema_slow".to_string(), config.ema_slow as f64);
    params.insert("ema_trend".to_string(), config.ema_trend as f64);
    params.insert("adx_threshold".to_string(), config.adx_threshold);
    params.insert("stop_atr_multiple".to_string(), config.stop_atr_multiple);
    params.insert("target_atr_multiple".to_string(), config.target_atr_multiple);
    params.insert("use_macd".to_string(), if config.use_macd { 1.0 } else { 0.0 });
    params.insert("trade_with_trend".to_string(), if config.trade_with_trend { 1.0 } else { 0.0 });

    params
}

/// Format parameters for display
pub fn format_params(params: &HashMap<String, f64>) -> String {
    let use_macd = *params.get("use_macd").unwrap_or(&1.0) > 0.5;
    let with_trend = *params.get("trade_with_trend").unwrap_or(&1.0) > 0.5;

    format!(
        "EMA:{}/{} ADX:{:.0} Stop:{:.2} Tgt:{:.1} MACD:{} Trend:{}",
        *params.get("ema_fast").unwrap_or(&9.0) as usize,
        *params.get("ema_slow").unwrap_or(&21.0) as usize,
        params.get("adx_threshold").unwrap_or(&20.0),
        params.get("stop_atr_multiple").unwrap_or(&1.0),
        params.get("target_atr_multiple").unwrap_or(&1.5),
        if use_macd { "Y" } else { "N" },
        if with_trend { "Y" } else { "N" },
    )
}
