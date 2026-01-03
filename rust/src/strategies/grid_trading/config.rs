//! Grid Trading Configuration

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridTradingConfig {
    /// Number of grid levels (default: 10)
    pub num_grids: usize,

    /// Grid spacing as percentage of price (e.g., 0.02 = 2%)
    /// If set, overrides lower/upper range calculation
    #[serde(default)]
    pub grid_spacing_pct: Option<f64>,

    /// Use ATR-based dynamic grid sizing (default: true)
    #[serde(default = "default_true")]
    pub use_atr_grids: bool,

    /// ATR period for dynamic grid calculation (default: 14)
    #[serde(default = "default_atr_period")]
    pub atr_period: usize,

    /// ATR multiplier for grid range (default: 3.0 = 3x ATR range)
    #[serde(default = "default_atr_multiplier")]
    pub atr_multiplier: f64,

    /// Minimum bars needed to establish grid (default: 20)
    #[serde(default = "default_min_bars")]
    pub min_bars: usize,

    /// Recalculate grid every N bars (0 = never, default: 0)
    #[serde(default)]
    pub recalc_interval: usize,
}

fn default_true() -> bool {
    true
}

fn default_atr_period() -> usize {
    14
}

fn default_atr_multiplier() -> f64 {
    3.0
}

fn default_min_bars() -> usize {
    20
}

impl Default for GridTradingConfig {
    fn default() -> Self {
        Self {
            num_grids: 10,
            grid_spacing_pct: None,
            use_atr_grids: true,
            atr_period: 14,
            atr_multiplier: 3.0,
            min_bars: 20,
            recalc_interval: 0,
        }
    }
}
