# Crypto Strategies - Rust Implementation

High-performance Rust implementation of trading strategies for CoinDCX.

## Features

- **Performance**: 10-100x faster backtests enabling thorough optimization
- **Type Safety**: Compile-time guarantees eliminate runtime type errors
- **Production Ready**: Memory safety, graceful error handling
- **Parallel Optimization**: Safe parallelization with Rayon
- **Generic Strategy Framework**: Add new strategies with minimal boilerplate

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.70+)
- CoinDCX API credentials (for live trading)

## Quick Start

```bash
cd rust

# Build (debug for development)
cargo build

# Build (release for production/optimization)
cargo build --release

# Run tests
cargo test
```

### Environment Configuration

```bash
# Create .env from template (in repo root)
copy ..\.env.example ..\.env  # Windows

# Add CoinDCX credentials
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
```

## Usage

### Backtesting

```bash
# Run with default config
cargo run --release -- backtest --config ../configs/sample_config.json

# Override parameters
cargo run --release -- backtest --capital 100000

# Verbose output
cargo run --release -- backtest -v
```

### Optimization

Grid search parameters are defined in your JSON config file under the `grid` section:

```json
{
  "strategy": { ... },
  "grid": {
    "atr_period": [14],
    "ema_fast": [8, 13],
    "ema_slow": [21, 34],
    "adx_threshold": [20.0, 25.0, 30.0],
    "stop_atr_multiple": [2.0, 2.5, 3.0],
    "target_atr_multiple": [4.0, 5.0, 6.0]
  }
}
```

```bash
# Run optimization (uses grid from config)
cargo run --release -- optimize --config ../configs/sample_config.json

# Override grid params via CLI
cargo run --release -- optimize -O "adx_threshold=20,25,30,35" -O "ema_fast=5,8,13"

# Sort by different metrics
cargo run --release -- optimize --sort-by calmar
cargo run --release -- optimize --sort-by return

# Show top N results
cargo run --release -- optimize --top 20

# Test multiple coin combinations
cargo run --release -- optimize --coins BTC,ETH,SOL
```

**Sorting Options:**

| Option | Description |
|--------|-------------|
| `sharpe` | Risk-adjusted return (default) |
| `calmar` | Return / Max Drawdown |
| `return` | Raw total return |
| `profit_factor` | Gross profits / Gross losses |
| `win_rate` | Winning trades % |

### Live Trading

```bash
# Paper trading (safe, simulated)
cargo run --release -- live --paper

# Custom cycle interval (seconds)
cargo run --release -- live --paper --interval 300

# Live trading with real money (CAUTION!)
cargo run --release -- live --live
```

## Architecture

```
src/
├── main.rs              # CLI dispatch
├── lib.rs               # Library exports
├── commands/            # Command implementations
│   ├── backtest.rs
│   ├── optimize.rs
│   ├── live.rs
│   └── download.rs
├── strategies/          # Strategy implementations
│   ├── mod.rs           # Strategy trait + registry
│   ├── volatility_regime/
│   ├── mean_reversion/
│   ├── momentum_scalper/
│   ├── range_breakout/
│   └── vwap_scalper/
├── backtest.rs          # Simulation engine
├── grid.rs              # Generic grid search
├── risk.rs              # Position sizing
├── indicators.rs        # Technical indicators
├── config.rs            # Configuration parsing
├── types.rs             # Domain model
└── data.rs              # Data loading
```

## Creating a New Strategy

The strategy framework uses a trait-based plugin architecture. To add a new strategy:

### Step 1: Create Strategy Directory

```
src/strategies/my_strategy/
├── mod.rs
├── config.rs
└── strategy.rs
```

### Step 2: Define Config (`config.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyStrategyConfig {
    pub param1: usize,
    pub param2: f64,
    // ... your parameters
}

impl Default for MyStrategyConfig {
    fn default() -> Self {
        Self {
            param1: 14,
            param2: 2.5,
        }
    }
}
```

### Step 3: Implement Strategy (`strategy.rs`)

```rust
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};
use super::config::MyStrategyConfig;

pub struct MyStrategy {
    config: MyStrategyConfig,
}

impl MyStrategy {
    pub fn new(config: MyStrategyConfig) -> Self {
        Self { config }
    }
}

impl Strategy for MyStrategy {
    // REQUIRED: Strategy identifier
    fn name(&self) -> &'static str {
        "my_strategy"
    }

    // REQUIRED: Generate trading signal
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Your signal logic here
        Signal::Flat
    }

    // REQUIRED: Calculate stop loss
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        entry_price * 0.95  // Example: 5% stop
    }

    // REQUIRED: Calculate take profit
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        entry_price * 1.10  // Example: 10% target
    }

    // REQUIRED: Update trailing stop
    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        None  // No trailing stop
    }

    // OPTIONAL: Regime-based position sizing (default: 1.0)
    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        1.0
    }
}
```

### Step 4: Create Module (`mod.rs`)

```rust
mod config;
mod strategy;

pub use config::MyStrategyConfig;
pub use strategy::MyStrategy;

use crate::{Config, Strategy};
use anyhow::Result;

/// Factory function called by registry
pub fn create(config: &Config) -> Result<Box<dyn Strategy>> {
    let strategy_config: MyStrategyConfig = serde_json::from_value(config.strategy.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse my_strategy config: {}", e))?;
    Ok(Box::new(MyStrategy::new(strategy_config)))
}
```

### Step 5: Register Strategy

In `src/strategies/mod.rs`, add your strategy:

```rust
// Add module declaration
pub mod my_strategy;

// Add to registry (in get_registry function)
fn get_registry() -> &'static RwLock<HashMap<&'static str, StrategyFactory>> {
    REGISTRY.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert("volatility_regime", volatility_regime::create as StrategyFactory);
        map.insert("mean_reversion", mean_reversion::create as StrategyFactory);
        map.insert("momentum_scalper", momentum_scalper::create as StrategyFactory);
        map.insert("range_breakout", range_breakout::create as StrategyFactory);
        map.insert("my_strategy", my_strategy::create as StrategyFactory);  // <-- Add this
        RwLock::new(map)
    })
}
```

### Step 6: Create Config File

```json
{
  "strategy_name": "my_strategy",
  "strategy": {
    "timeframe": "1h",
    "param1": 14,
    "param2": 2.5
  },
  "grid": {
    "param1": [10, 14, 20],
    "param2": [2.0, 2.5, 3.0]
  },
  ...
}
```

### Step 7: Test

```bash
# Backtest
cargo run -- backtest --config ../configs/my_strategy.json

# Optimize
cargo run -- optimize --config ../configs/my_strategy.json
```

## Strategy Trait Reference

```rust
pub trait Strategy: Send + Sync {
    /// Strategy identifier (must match config's strategy_name)
    fn name(&self) -> &'static str;

    /// Generate trading signal
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal;

    /// Calculate stop loss price for entry
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Calculate take profit price for entry
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Update trailing stop (return None if not using trailing)
    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64>;

    /// Position sizing multiplier based on market regime (default: 1.0)
    fn get_regime_score(&self, candles: &[Candle]) -> f64 { 1.0 }

    /// Called when order status changes (default: logging)
    fn notify_order(&mut self, order: &Order) { ... }

    /// Called when trade closes (default: logging)
    fn notify_trade(&mut self, trade: &Trade) { ... }

    /// Called once before trading starts
    fn init(&mut self) { }
}
```

## Available Strategies

| Strategy | Description | Best Timeframe |
|----------|-------------|----------------|
| `volatility_regime` | Volatility clustering breakouts | 1d |
| `mean_reversion` | Bollinger Band + RSI reversion | 5m, 15m, 1h |
| `momentum_scalper` | EMA crossover momentum | 5m, 15m |
| `range_breakout` | N-bar high/low breakouts | 1h, 4h |
| `vwap_scalper` | VWAP crossover price action | 5m, 15m |

## Configuration Structure

```json
{
  "exchange": {
    "maker_fee": 0.001,
    "taker_fee": 0.001,
    "assumed_slippage": 0.001
  },
  "trading": {
    "pairs": ["BTCINR", "ETHINR"],
    "initial_capital": 100000,
    "risk_per_trade": 0.15,
    "max_positions": 5,
    "max_drawdown": 0.20
  },
  "strategy_name": "volatility_regime",
  "strategy": {
    "timeframe": "1d",
    "atr_period": 14,
    ...
  },
  "tax": {
    "tax_rate": 0.30,
    "tds_rate": 0.01
  },
  "backtest": {
    "data_dir": "../data",
    "results_dir": "../results",
    "commission": 0.001
  },
  "grid": {
    "param1": [10, 14, 20],
    "param2": [2.0, 2.5, 3.0]
  }
}
```

## Testing

```bash
cargo test                    # Run all tests
cargo test --release          # With optimizations
cargo test -- --nocapture     # Show output
cargo test strategies::       # Test strategies module
```

## License

MIT License
