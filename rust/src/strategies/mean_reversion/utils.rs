//! Utility functions for Mean Reversion Scalper Strategy
//!
//! Helper functions for strategy instantiation and parameter reporting.

use super::config::MeanReversionConfig;
use super::strategy::MeanReversionStrategy;
use crate::Config;
use anyhow::Result;
use std::collections::HashMap;

/// Create a Mean Reversion Strategy from global config
pub fn create_strategy_from_config(config: &Config) -> Result<MeanReversionStrategy> {
    let mr_config: MeanReversionConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse mean_reversion strategy config: {}", e))?;

    Ok(MeanReversionStrategy::new(mr_config))
}

/// Convert MeanReversionConfig to a parameter map for reporting
pub fn config_to_params(config: &MeanReversionConfig) -> HashMap<String, f64> {
    let mut params = HashMap::new();

    // Bollinger Band params
    params.insert("bb_period".to_string(), config.bb_period as f64);
    params.insert("bb_std".to_string(), config.bb_std);

    // RSI params
    params.insert("rsi_period".to_string(), config.rsi_period as f64);
    params.insert("rsi_oversold".to_string(), config.rsi_oversold);
    params.insert("rsi_overbought".to_string(), config.rsi_overbought);

    // Volume params
    params.insert("volume_period".to_string(), config.volume_period as f64);
    params.insert(
        "volume_spike_threshold".to_string(),
        config.volume_spike_threshold,
    );

    // Trend filter
    params.insert(
        "trend_ema_period".to_string(),
        config.trend_ema_period as f64,
    );

    // Risk management
    params.insert("atr_period".to_string(), config.atr_period as f64);
    params.insert("stop_atr_multiple".to_string(), config.stop_atr_multiple);
    params.insert(
        "target_atr_multiple".to_string(),
        config.target_atr_multiple,
    );
    params.insert("trailing_activation".to_string(), config.trailing_activation);
    params.insert(
        "trailing_atr_multiple".to_string(),
        config.trailing_atr_multiple,
    );

    params
}

/// Format parameters for display
pub fn format_params(params: &HashMap<String, f64>) -> String {
    format!(
        "BB:{}/{:.1} RSI:{}/{:.0}/{:.0} Vol:{:.1} Stop:{:.1}",
        *params.get("bb_period").unwrap_or(&20.0) as usize,
        params.get("bb_std").unwrap_or(&2.0),
        *params.get("rsi_period").unwrap_or(&14.0) as usize,
        params.get("rsi_oversold").unwrap_or(&30.0),
        params.get("rsi_overbought").unwrap_or(&70.0),
        params.get("volume_spike_threshold").unwrap_or(&1.5),
        params.get("stop_atr_multiple").unwrap_or(&0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_to_params() {
        let config = MeanReversionConfig::default();
        let params = config_to_params(&config);

        assert_eq!(params.get("bb_period"), Some(&20.0));
        assert_eq!(params.get("bb_std"), Some(&2.0));
        assert_eq!(params.get("rsi_period"), Some(&14.0));
        assert_eq!(params.get("rsi_oversold"), Some(&30.0));
        assert_eq!(params.get("rsi_overbought"), Some(&70.0));
    }
}
