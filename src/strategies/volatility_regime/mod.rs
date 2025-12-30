//! Volatility Regime Strategy Module
//!
//! Contains all components for the Volatility Regime Adaptive Strategy.

pub mod config;
pub mod grid_params;
pub mod strategy;
pub mod utils;

pub use config::VolatilityRegimeConfig;
pub use grid_params::GridParams;
pub use strategy::VolatilityRegimeStrategy;
pub use utils::{create_strategy_from_config, generate_configs, config_to_params};
