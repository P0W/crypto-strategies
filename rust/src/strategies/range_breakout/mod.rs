//! Range Breakout Strategy
//!
//! Simple, fast strategy for short timeframes.
//! Entry on breakout of N-bar high/low range.

use serde::{Deserialize, Serialize};

pub mod config;
pub mod grid_params;
pub mod strategy;
pub mod utils;

pub use config::RangeBreakoutConfig;
pub use grid_params::GridParams;
pub use strategy::RangeBreakoutStrategy;
pub use utils::{config_to_params, create_strategy_from_config, format_params};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakoutType {
    High,
    Low,
    None,
}
