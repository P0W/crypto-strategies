//! Configuration management
//!
//! Handles loading and parsing of JSON configuration files with environment
//! variable support for API credentials.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::Symbol;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub exchange: ExchangeConfig,
    pub trading: TradingConfig,
    #[serde(default = "default_strategy_name")]
    pub strategy_name: String,
    pub strategy: serde_json::Value,
    pub tax: TaxConfig,
    pub backtest: BacktestConfig,
    /// Grid search parameters for optimization (optional)
    /// Each key is a strategy param name, value is array of values to test
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<HashMap<String, Vec<serde_json::Value>>>,
}

fn default_strategy_name() -> String {
    "volatility_regime".to_string()
}

impl Config {
    /// Load configuration from JSON file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read_to_string(path.as_ref()).context("Failed to read config file")?;
        let mut config: Config =
            serde_json::from_str(&contents).context("Failed to parse config JSON")?;

        // Load API credentials from environment if not set
        if let Ok(api_key) = std::env::var("COINDCX_API_KEY") {
            config.exchange.api_key = Some(api_key);
        }
        if let Ok(api_secret) = std::env::var("COINDCX_API_SECRET") {
            config.exchange.api_secret = Some(api_secret);
        }

        Ok(config)
    }

    /// Get timeframe from strategy config
    /// Panics if timeframe is not set in the strategy section
    pub fn timeframe(&self) -> String {
        self.strategy
            .get("timeframe")
            .and_then(|v| v.as_str())
            .expect("FATAL: 'timeframe' is required in the 'strategy' section of config. Example: \"strategy\": { \"timeframe\": \"1d\", ... }")
            .to_string()
    }

    /// Set timeframe in strategy config
    pub fn set_timeframe(&mut self, timeframe: &str) {
        if let Some(obj) = self.strategy.as_object_mut() {
            obj.insert("timeframe".to_string(), serde_json::json!(timeframe));
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            exchange: ExchangeConfig::default(),
            trading: TradingConfig::default(),
            strategy_name: "volatility_regime".to_string(),
            strategy: serde_json::json!({}),
            tax: TaxConfig::default(),
            backtest: BacktestConfig::default(),
            grid: None,
        }
    }
}

/// Exchange configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_secret: Option<String>,
    pub maker_fee: f64,
    pub taker_fee: f64,
    pub assumed_slippage: f64,
    pub rate_limit: u32,
}

impl Default for ExchangeConfig {
    fn default() -> Self {
        ExchangeConfig {
            api_key: None,
            api_secret: None,
            maker_fee: 0.001, // 0.1%
            taker_fee: 0.001, // 0.1%
            assumed_slippage: 0.001,
            rate_limit: 10,
        }
    }
}

/// Trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub pairs: Vec<String>,
    pub initial_capital: f64,
    pub risk_per_trade: f64,
    pub max_positions: usize,
    pub max_portfolio_heat: f64,
    pub max_position_pct: f64,
    pub max_drawdown: f64,
    pub drawdown_warning: f64,
    pub drawdown_critical: f64,
    pub drawdown_warning_multiplier: f64,
    pub drawdown_critical_multiplier: f64,
    pub consecutive_loss_limit: usize,
    pub consecutive_loss_multiplier: f64,
}

impl Default for TradingConfig {
    fn default() -> Self {
        TradingConfig {
            pairs: vec![
                "BTCINR".to_string(),
                "ETHINR".to_string(),
                "SOLINR".to_string(),
                "BNBINR".to_string(),
                "XRPINR".to_string(),
            ],
            initial_capital: 100_000.0,
            risk_per_trade: 0.15,
            max_positions: 5,
            max_portfolio_heat: 0.30,
            max_position_pct: 0.20,
            max_drawdown: 0.20,
            drawdown_warning: 0.10,
            drawdown_critical: 0.15,
            drawdown_warning_multiplier: 0.50,
            drawdown_critical_multiplier: 0.25,
            consecutive_loss_limit: 3,
            consecutive_loss_multiplier: 0.75,
        }
    }
}

impl TradingConfig {
    pub fn symbols(&self) -> Vec<Symbol> {
        self.pairs.iter().map(|s| Symbol::new(s.clone())).collect()
    }
}

/// Tax configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxConfig {
    pub tax_rate: f64,
    pub tds_rate: f64,
    pub loss_offset_allowed: bool,
}

impl Default for TaxConfig {
    fn default() -> Self {
        TaxConfig {
            tax_rate: 0.30, // 30% flat tax in India
            tds_rate: 0.01, // 1% TDS
            loss_offset_allowed: false,
        }
    }
}

/// Backtest configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub data_dir: String,
    pub results_dir: String,
    pub commission: f64,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        BacktestConfig {
            data_dir: "data".to_string(),
            results_dir: "results".to_string(),
            commission: 0.001,
        }
    }
}
