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
    pub strategy: serde_json::Value,
    pub tax: TaxConfig,
    pub backtest: BacktestConfig,
    /// Grid search parameters for optimization (optional)
    /// Each key is a strategy param name, value is array of values to test
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<HashMap<String, Vec<serde_json::Value>>>,
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

    /// Get strategy name from strategy config
    /// Panics if name is not set in the strategy section
    pub fn strategy_name(&self) -> String {
        self.strategy
            .get("name")
            .and_then(|v| v.as_str())
            .expect("FATAL: 'name' is required in the 'strategy' section of config. Example: \"strategy\": { \"name\": \"volatility_regime\", ... }")
            .to_string()
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
    #[serde(default)]
    pub cost_model: TransactionCostConfig,
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
            cost_model: TransactionCostConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransactionCostConfig {
    #[default]
    Percentage,
    Components {
        #[serde(default)]
        brokerage_rate: f64,
        #[serde(default)]
        brokerage_cap_per_order: f64,
        #[serde(default)]
        buy_turnover_rate: f64,
        #[serde(default)]
        sell_turnover_rate: f64,
        #[serde(default)]
        exchange_rate: f64,
        #[serde(default)]
        regulatory_rate: f64,
        #[serde(default)]
        buy_stamp_rate: f64,
        #[serde(default)]
        indirect_tax_rate: f64,
        #[serde(default)]
        sell_fixed_charge: f64,
        #[serde(default)]
        sell_fixed_charge_frequency: FixedChargeFrequency,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FixedChargeFrequency {
    #[default]
    PerFill,
    PerSymbolPerDay,
}

/// Trading configuration
///
/// # Currency Handling
///
/// This system is **currency-agnostic** - all calculations work with dimensionless numbers.
/// The code does NOT perform any currency conversion. It only requires that `initial_capital`
/// and price data are denominated in the **same currency**.
///
/// For example:
/// - If your CSV price data is in USD, set `initial_capital` in USD (e.g., 100000 = $100,000)
/// - If your CSV price data is in INR, set `initial_capital` in INR (e.g., 100000 = ₹1,00,000)
///
/// Performance metrics (returns, Sharpe ratio, drawdown) are calculated as **percentages**,
/// making them currency-independent. The absolute currency unit does not affect results
/// as long as capital and prices are consistent.
///
/// Note: File names like "BTCINR.csv" are just labels - they don't enforce currency.
/// Always verify your data source's actual currency denomination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub symbols: Vec<String>,
    /// Initial trading capital in the same currency as your price data.
    /// No currency conversion is performed - ensure this matches your CSV data currency.
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
            symbols: vec![
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
        self.symbols
            .iter()
            .map(|s| Symbol::new(s.clone()))
            .collect()
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
    /// Use T+1 execution model (signal on day N, execute at day N+1 open)
    /// Default is false (intra-candle execution for realistic algo trading)
    #[serde(default)]
    pub use_t1_execution: bool,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        BacktestConfig {
            data_dir: "data".to_string(),
            results_dir: "results".to_string(),
            commission: 0.001,
            use_t1_execution: false, // Default to realistic intra-candle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ==================== Config Tests ====================

    #[test]
    fn test_config_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.json");

        let config_json = r#"{
            "exchange": {
                "maker_fee": 0.001,
                "taker_fee": 0.001,
                "assumed_slippage": 0.001,
                "rate_limit": 10
            },
            "trading": {
                "symbols": ["BTCINR"],
                "initial_capital": 50000.0,
                "risk_per_trade": 0.1,
                "max_positions": 3,
                "max_portfolio_heat": 0.25,
                "max_position_pct": 0.15,
                "max_drawdown": 0.2,
                "drawdown_warning": 0.1,
                "drawdown_critical": 0.15,
                "drawdown_warning_multiplier": 0.5,
                "drawdown_critical_multiplier": 0.25,
                "consecutive_loss_limit": 3,
                "consecutive_loss_multiplier": 0.75
            },
            "strategy": {
                "name": "test_strategy",
                "timeframe": "1h"
            },
            "tax": {
                "tax_rate": 0.3,
                "tds_rate": 0.01,
                "loss_offset_allowed": false
            },
            "backtest": {
                "data_dir": "./data",
                "results_dir": "./results",
                "commission": 0.001
            }
        }"#;

        std::fs::write(&config_path, config_json).unwrap();

        let config = Config::from_file(&config_path).unwrap();

        assert_eq!(config.strategy_name(), "test_strategy");
        assert_eq!(config.timeframe(), "1h");
        assert_eq!(config.trading.initial_capital, 50000.0);
        assert_eq!(config.trading.symbols, vec!["BTCINR"]);
        assert_eq!(config.exchange.maker_fee, 0.001);
        assert_eq!(config.tax.tax_rate, 0.3);
    }

    #[test]
    fn test_config_with_grid() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("grid_config.json");

        let config_json = r#"{
            "exchange": {
                "maker_fee": 0.001,
                "taker_fee": 0.001,
                "assumed_slippage": 0.001,
                "rate_limit": 10
            },
            "trading": {
                "symbols": ["BTCINR"],
                "initial_capital": 100000.0,
                "risk_per_trade": 0.15,
                "max_positions": 5,
                "max_portfolio_heat": 0.3,
                "max_position_pct": 0.2,
                "max_drawdown": 0.2,
                "drawdown_warning": 0.1,
                "drawdown_critical": 0.15,
                "drawdown_warning_multiplier": 0.5,
                "drawdown_critical_multiplier": 0.25,
                "consecutive_loss_limit": 3,
                "consecutive_loss_multiplier": 0.75
            },
            "strategy": {
                "name": "volatility_regime",
                "timeframe": "1d"
            },
            "tax": {
                "tax_rate": 0.3,
                "tds_rate": 0.01,
                "loss_offset_allowed": false
            },
            "backtest": {
                "data_dir": "./data",
                "results_dir": "./results",
                "commission": 0.001
            },
            "grid": {
                "atr_period": [10, 14, 20],
                "stop_atr": [1.5, 2.0]
            }
        }"#;

        std::fs::write(&config_path, config_json).unwrap();

        let config = Config::from_file(&config_path).unwrap();

        assert!(config.grid.is_some());
        let grid = config.grid.as_ref().unwrap();
        assert!(grid.contains_key("atr_period"));
        assert_eq!(grid["atr_period"].len(), 3);
        assert!(grid.contains_key("stop_atr"));
        assert_eq!(grid["stop_atr"].len(), 2);
    }

    #[test]
    fn test_config_missing_file() {
        let result = Config::from_file("nonexistent_file.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("invalid.json");

        std::fs::write(&config_path, "{ invalid json }").unwrap();

        let result = Config::from_file(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_set_timeframe() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let config_json = r#"{
            "exchange": {"maker_fee": 0.001, "taker_fee": 0.001, "assumed_slippage": 0.001, "rate_limit": 10},
            "trading": {"symbols": ["BTCINR"], "initial_capital": 100000.0, "risk_per_trade": 0.15, "max_positions": 5, "max_portfolio_heat": 0.3, "max_position_pct": 0.2, "max_drawdown": 0.2, "drawdown_warning": 0.1, "drawdown_critical": 0.15, "drawdown_warning_multiplier": 0.5, "drawdown_critical_multiplier": 0.25, "consecutive_loss_limit": 3, "consecutive_loss_multiplier": 0.75},
            "strategy": {"name": "test", "timeframe": "1d"},
            "tax": {"tax_rate": 0.3, "tds_rate": 0.01, "loss_offset_allowed": false},
            "backtest": {"data_dir": "./data", "results_dir": "./results", "commission": 0.001}
        }"#;

        std::fs::write(&config_path, config_json).unwrap();

        let mut config = Config::from_file(&config_path).unwrap();
        assert_eq!(config.timeframe(), "1d");

        config.set_timeframe("4h");
        assert_eq!(config.timeframe(), "4h");

        config.set_timeframe("15m");
        assert_eq!(config.timeframe(), "15m");
    }

    // ==================== ExchangeConfig Tests ====================

    #[test]
    fn test_exchange_config_default() {
        let config = ExchangeConfig::default();

        assert!(config.api_key.is_none());
        assert!(config.api_secret.is_none());
        assert_eq!(config.maker_fee, 0.001);
        assert_eq!(config.taker_fee, 0.001);
        assert_eq!(config.assumed_slippage, 0.001);
        assert_eq!(config.rate_limit, 10);
    }

    // ==================== TradingConfig Tests ====================

    #[test]
    fn test_trading_config_default() {
        let config = TradingConfig::default();

        assert_eq!(config.symbols.len(), 5);
        assert!(config.symbols.contains(&"BTCINR".to_string()));
        assert_eq!(config.initial_capital, 100_000.0);
        assert_eq!(config.risk_per_trade, 0.15);
        assert_eq!(config.max_positions, 5);
        assert_eq!(config.max_portfolio_heat, 0.30);
        assert_eq!(config.max_drawdown, 0.20);
        assert_eq!(config.consecutive_loss_limit, 3);
    }

    #[test]
    fn test_trading_config_symbols() {
        let config = TradingConfig {
            symbols: vec!["BTCINR".to_string(), "ETHINR".to_string()],
            ..TradingConfig::default()
        };

        let symbols = config.symbols();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].as_str(), "BTCINR");
        assert_eq!(symbols[1].as_str(), "ETHINR");
    }

    // ==================== TaxConfig Tests ====================

    #[test]
    fn test_tax_config_default() {
        let config = TaxConfig::default();

        assert_eq!(config.tax_rate, 0.30);
        assert_eq!(config.tds_rate, 0.01);
        assert!(!config.loss_offset_allowed);
    }

    // ==================== BacktestConfig Tests ====================

    #[test]
    fn test_backtest_config_default() {
        let config = BacktestConfig::default();

        assert_eq!(config.data_dir, "data");
        assert_eq!(config.results_dir, "results");
        assert_eq!(config.commission, 0.001);
        assert!(!config.use_t1_execution);
    }

    #[test]
    fn test_backtest_config_t1_execution() {
        // Default should be false
        let config = BacktestConfig::default();
        assert!(!config.use_t1_execution);

        // Explicit true
        let config = BacktestConfig {
            use_t1_execution: true,
            ..BacktestConfig::default()
        };
        assert!(config.use_t1_execution);
    }
}
