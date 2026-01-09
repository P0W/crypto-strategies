# Creating a Trading Strategy

This guide walks you through creating a new trading strategy for the crypto-strategies system.

## Overview

Strategies in this system are implemented as Rust modules that:
1. Define a **config struct** for strategy parameters
2. Implement the **Strategy trait** for trading logic
3. Provide a **factory function** for dynamic creation

## Directory Structure

Each strategy lives in its own module under `src/strategies/`:

```
src/strategies/
├── mod.rs              # Strategy registry
└── my_strategy/        # Your new strategy
    ├── mod.rs          # Module exports + factory
    ├── config.rs       # Configuration struct
    └── strategy.rs     # Strategy implementation
```

## Step 1: Create the Config (`config.rs`)

Define your strategy parameters with sensible defaults:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyStrategyConfig {
    /// ATR period for volatility (default: 14)
    pub atr_period: usize,

    /// Fast EMA period (default: 8)
    #[serde(default = "default_ema_fast")]
    pub ema_fast: usize,

    /// Slow EMA period (default: 21)
    #[serde(default = "default_ema_slow")]
    pub ema_slow: usize,

    /// Stop loss as ATR multiple (default: 2.0)
    #[serde(default = "default_stop_atr")]
    pub stop_atr: f64,

    /// Take profit as ATR multiple (default: 4.0)
    #[serde(default = "default_target_atr")]
    pub target_atr: f64,

    /// Allow short positions (default: false)
    #[serde(default)]
    pub allow_shorts: bool,
}

// Default value functions
fn default_ema_fast() -> usize { 8 }
fn default_ema_slow() -> usize { 21 }
fn default_stop_atr() -> f64 { 2.0 }
fn default_target_atr() -> f64 { 4.0 }

impl Default for MyStrategyConfig {
    fn default() -> Self {
        Self {
            atr_period: 14,
            ema_fast: 8,
            ema_slow: 21,
            stop_atr: 2.0,
            target_atr: 4.0,
            allow_shorts: false,
        }
    }
}
```

**Tips:**
- Use `#[serde(default)]` for optional fields with defaults
- Document each field with `///` comments
- Implement `Default` for testing and fallback

## Step 2: Implement the Strategy (`strategy.rs`)

The `Strategy` trait requires these methods:

```rust
use crate::indicators::{atr, ema};
use crate::oms::{OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side};

use super::config::MyStrategyConfig;

pub struct MyStrategy {
    config: MyStrategyConfig,
}

impl MyStrategy {
    pub fn new(config: MyStrategyConfig) -> Self {
        Self { config }
    }

    /// Calculate current ATR value
    fn get_atr(&self, candles: &[Candle]) -> Option<f64> {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        atr(&high, &low, &close, self.config.atr_period)
            .last()
            .and_then(|&x| x)
    }

    /// Check if trend is bullish (fast EMA > slow EMA)
    fn is_bullish_trend(&self, candles: &[Candle]) -> bool {
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let ema_fast = ema(&close, self.config.ema_fast);
        let ema_slow = ema(&close, self.config.ema_slow);

        match (ema_fast.last(), ema_slow.last()) {
            (Some(Some(fast)), Some(Some(slow))) => fast > slow,
            _ => false,
        }
    }
}

impl Strategy for MyStrategy {
    fn name(&self) -> &'static str {
        "my_strategy"
    }

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(MyStrategy::new(self.config.clone()))
    }

    fn required_timeframes(&self) -> Vec<&'static str> {
        // Return additional timeframes if needed (e.g., vec!["4h", "1d"])
        vec![]
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        // Need enough data for indicators
        let min_bars = self.config.ema_slow + self.config.atr_period;
        if ctx.candles.len() < min_bars {
            return vec![];
        }

        // Already in position - no new entries
        if ctx.current_position.is_some() {
            return vec![];
        }

        // Entry logic: bullish trend
        if self.is_bullish_trend(ctx.candles) {
            return vec![OrderRequest::market_buy(ctx.symbol.clone(), 0.0)];
        }

        vec![]
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let atr_val = self.get_atr(candles).unwrap_or(entry_price * 0.02);
        match side {
            Side::Buy => entry_price - (atr_val * self.config.stop_atr),
            Side::Sell => entry_price + (atr_val * self.config.stop_atr),
        }
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let atr_val = self.get_atr(candles).unwrap_or(entry_price * 0.02);
        match side {
            Side::Buy => entry_price + (atr_val * self.config.target_atr),
            Side::Sell => entry_price - (atr_val * self.config.target_atr),
        }
    }

    fn update_trailing_stop(
        &self,
        _position: &Position,
        _current_price: f64,
        _candles: &[Candle],
    ) -> Option<f64> {
        None // No trailing stop (return Some(price) to enable)
    }
}
```

## Step 3: Create the Module Entry (`mod.rs`)

```rust
mod config;
mod strategy;

pub use config::MyStrategyConfig;
pub use strategy::MyStrategy;

use crate::{Config, Strategy};
use anyhow::Result;

/// Factory function - called by the strategy registry
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: MyStrategyConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse my_strategy config: {}", e))?;
    Ok(Box::new(MyStrategy::new(strategy_config)))
}
```

## Step 4: Register the Strategy

In `src/strategies/mod.rs`, add your strategy:

```rust
// Add module declaration
pub mod my_strategy;

// In get_registry() function, add:
map.insert("my_strategy", my_strategy::create as StrategyFactory);
```

## Step 5: Create a Config File

Create `configs/my_strategy_config.json`:

```json
{
    "exchange": {
        "maker_fee": 0.001,
        "taker_fee": 0.001,
        "assumed_slippage": 0.001,
        "rate_limit": 10
    },
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_portfolio_heat": 0.30,
        "max_position_pct": 0.20,
        "max_drawdown": 0.20
    },
    "strategy": {
        "name": "my_strategy",
        "timeframe": "1d",
        "atr_period": 14,
        "ema_fast": 8,
        "ema_slow": 21,
        "stop_atr": 2.0,
        "target_atr": 4.0,
        "allow_shorts": false
    },
    "tax": {
        "tax_rate": 0.30,
        "tds_rate": 0.01,
        "loss_offset_allowed": false
    },
    "backtest": {
        "data_dir": "./data",
        "results_dir": "./results"
    }
}
```

## Step 6: Test Your Strategy

```bash
# Run backtest
cargo run -- backtest --config configs/my_strategy_config.json

# Run with verbose logging
cargo run -- backtest --config configs/my_strategy_config.json -v

# Test specific date range
cargo run -- backtest --config configs/my_strategy_config.json --start 2024-01-01 --end 2024-12-31
```

## Available Indicators

Import from `crate::indicators`:

| Indicator | Function | Description |
|-----------|----------|-------------|
| ATR | `atr(&high, &low, &close, period)` | Average True Range |
| EMA | `ema(&close, period)` | Exponential Moving Average |
| SMA | `sma(&close, period)` | Simple Moving Average |
| RSI | `rsi(&close, period)` | Relative Strength Index |
| ADX | `adx(&high, &low, &close, period)` | Average Directional Index |
| MACD | `macd(&close, fast, slow, signal)` | MACD histogram |
| Bollinger | `bollinger(&close, period, std_dev)` | Bollinger Bands |
| Stochastic | `stochastic(&high, &low, &close, k, d)` | Stochastic oscillator |

## Strategy Trait Methods

### Required Methods

| Method | Purpose |
|--------|---------|
| `name()` | Return strategy identifier (must match config) |
| `clone_boxed()` | Clone for per-symbol isolation |
| `generate_orders()` | Main entry logic - return `Vec<OrderRequest>` |
| `calculate_stop_loss()` | Calculate stop price for new positions |
| `calculate_take_profit()` | Calculate target price for new positions |
| `update_trailing_stop()` | Return `Some(price)` to update trailing stop |

### Optional Methods (with defaults)

| Method | Default | Purpose |
|--------|---------|---------|
| `required_timeframes()` | `vec![]` | Declare additional timeframes needed |
| `get_regime_score()` | `1.0` | Regime score for position sizing |
| `on_order_filled()` | no-op | Callback when order fills |
| `on_order_cancelled()` | no-op | Callback when order cancelled |
| `on_trade_closed()` | no-op | Callback when position closes |
| `on_bar()` | no-op | Called each candle |
| `init()` | no-op | One-time initialization |

## Order Types

```rust
// Market orders (executed at current price)
OrderRequest::market_buy(symbol, quantity)
OrderRequest::market_sell(symbol, quantity)

// Limit orders (executed at specified price)
OrderRequest::limit_buy(symbol, quantity, price)
OrderRequest::limit_sell(symbol, quantity, price)
```

**Note:** Quantity `0.0` means the risk manager will calculate position size based on config.

## Multi-Timeframe Strategies

Access multiple timeframes via `ctx.mtf_candles`:

```rust
fn required_timeframes(&self) -> Vec<&'static str> {
    vec!["4h", "1d"]  // Request additional timeframes
}

fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
    // Primary timeframe (from config)
    let primary = ctx.candles;

    // Additional timeframes
    if let Some(mtf) = &ctx.mtf_candles {
        let daily = mtf.get("1d").unwrap_or(primary);
        let h4 = mtf.get("4h").unwrap_or(primary);

        // Use daily for trend, 4h for entries
        let daily_bullish = self.is_bullish(daily);
        let h4_entry = self.has_entry_signal(h4);

        if daily_bullish && h4_entry {
            return vec![OrderRequest::market_buy(ctx.symbol.clone(), 0.0)];
        }
    }
    vec![]
}
```

## Optimization

Add a `grid` section to your config for parameter optimization:

```json
{
    "grid": {
        "ema_fast": [5, 8, 13],
        "ema_slow": [21, 34, 55],
        "stop_atr": [1.5, 2.0, 2.5],
        "target_atr": [3.0, 4.0, 5.0]
    }
}
```

Run optimization:

```bash
cargo run --release -- optimize --config configs/my_strategy_config.json
```

## Tips

1. **Start simple** - Get basic logic working before adding complexity
2. **Use ATR** - ATR-based stops/targets adapt to volatility automatically
3. **Test thoroughly** - Run backtests across different date ranges
4. **Check logs** - Use `-v` flag for detailed execution logs
5. **Validate indicators** - Print indicator values during development
6. **Position sizing** - Return `0.0` quantity to let risk manager size positions

## Example: Complete Simple Strategy

See `src/strategies/quick_flip/` for a clean, well-documented example with:
- Range breakout entry logic
- ATR-based stops and targets
- Per-symbol cooldown tracking
- Strong candle filters
