//! Utils for Range Breakout

use super::config::RangeBreakoutConfig;
use super::strategy::RangeBreakoutStrategy;
use crate::Config;
use anyhow::Result;
use std::collections::HashMap;

pub fn create_strategy_from_config(config: &Config) -> Result<RangeBreakoutStrategy> {
    let rb_config: RangeBreakoutConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse range_breakout config: {}", e))?;
    Ok(RangeBreakoutStrategy::new(rb_config))
}

pub fn config_to_params(config: &RangeBreakoutConfig) -> HashMap<String, f64> {
    let mut params = HashMap::new();
    params.insert("lookback".to_string(), config.lookback as f64);
    params.insert("stop_atr".to_string(), config.stop_atr);
    params.insert("target_atr".to_string(), config.target_atr);
    params
}

pub fn format_params(params: &HashMap<String, f64>) -> String {
    format!(
        "LB:{} Stop:{:.1} Tgt:{:.1}",
        *params.get("lookback").unwrap_or(&20.0) as usize,
        params.get("stop_atr").unwrap_or(&1.0),
        params.get("target_atr").unwrap_or(&2.0),
    )
}
