# Crypto Strategies - Rust Implementation

High-performance Rust implementation of trading strategies for CoinDCX and Zerodha.

## Features

- **Performance**: 10-100x faster backtests enabling thorough optimization
- **Type Safety**: Compile-time guarantees eliminate runtime type errors
- **Multi-Timeframe**: Strategies can use multiple timeframes (1d ATR + 15m range + 5m patterns)
- **Parallel Optimization**: Rayon-based grid search across all CPU cores
- **Production Ready**: Circuit breakers, rate limiting, state persistence
- **Multiple Exchanges**: CoinDCX (crypto) and Zerodha Kite (equity) integrations

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.70+)
- API credentials (CoinDCX for crypto, Zerodha for equity)

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

# Add credentials
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
ZERODHA_API_KEY=your_kite_api_key
ZERODHA_ACCESS_TOKEN=your_access_token
```

## Commands

### Download Historical Data

```bash
# Download from Binance (default, no auth required)
cargo run -- download --symbols BTC,ETH,SOL --timeframes 5m,15m,1h,1d --days 180

# Download from CoinDCX
cargo run -- download --symbols BTC,ETH --timeframes 1h,1d --days 90 --source coindcx

# Download specific date range
cargo run -- download --symbols BTC --timeframes 1d --start 2023-01-01 --end 2024-01-01
```

### Backtesting

```bash
# Run backtest
cargo run -- backtest --config ../configs/btc_eth_sol_bnb_xrp_1d.json

# With date range filter
cargo run -- backtest --config ../configs/sample_config.json --start 2024-01-01 --end 2024-12-31

# Override capital
cargo run -- backtest --config ../configs/sample_config.json --capital 50000

# Verbose output
cargo run -- backtest -v
```

**Monthly P&L Matrix**: The backtest output now includes a professional month-on-month profit/loss matrix, displaying:
- Monthly P&L for each year in tabular format
- Color-coded profits (green) and losses (red) 
- Yearly totals and monthly win rate statistics
- Easy visualization of seasonal patterns and consistency

Example output:
```
========================================================================================================================
MONTHLY P&L MATRIX (₹)
========================================================================================================================
  Year │        Jan │        Feb │        Mar │        Apr │        May │        Jun │        Jul │        Aug │        Sep │        Oct │        Nov │        Dec │        Total
------------------------------------------------------------------------------------------------------------------------
  2023 │     910.62 │    -484.15 │    1562.64 │            │            │            │            │            │    -651.66 │    -536.11 │    2331.11 │            │      3132.45
  2024 │            │            │            │            │            │            │            │            │    1245.24 │            │     189.38 │            │      1434.63
========================================================================================================================
Total P&L: ₹4567.08
Monthly Win Rate: 66.7% (6 profitable / 3 losing / 9 total months)
========================================================================================================================
```

### Optimization

Grid parameters are defined in your config's `grid` section:

```json
{
  "grid": {
    "_optimization": [{
      "sharpe_ratio": 0.96,
      "total_return": 100.2,
      "max_drawdown": 13.6,
      "win_rate": 47.3,
      "total_trades": 55,
      "calmar_ratio": 1.39,
      "expectancy": 1853.29,
      "symbols": ["BTCINR", "ETHINR", "SOLINR"],
      "optimized_at": "2026-01-01 22:10:06"
    }],
    "ema_fast": [8, 13],
    "ema_slow": [21, 34],
    "stop_atr_multiple": [2.0, 2.5, 3.0]
  }
}
```

The `_optimization` field is auto-updated when better results are found, storing metrics and the exact symbols used.

```bash
# Run optimization (uses grid from config)
cargo run --release -- optimize --config ../configs/sample_config.json

# Test multiple coin combinations
cargo run --release -- optimize --coins BTC,ETH,SOL,BNB --min-combo 2

# Test specific symbol groups
cargo run --release -- optimize --symbols "BTC,ETH;SOL,BNB,XRP"

# Test multiple timeframes
cargo run --release -- optimize --timeframes 1h,4h,1d

# Override grid params via CLI
cargo run --release -- optimize -O "adx_threshold=20,25,30" -O "ema_fast=5,8,13"

# Sort by different metrics
cargo run --release -- optimize --sort-by calmar
cargo run --release -- optimize --sort-by return

# Show top N results
cargo run --release -- optimize --top 20

# Skip auto-update of config
cargo run --release -- optimize --no-update
```

**Sorting Options:**

| Option | Description |
|--------|-------------|
| `sharpe` | Risk-adjusted return (default) |
| `calmar` | Return / Max Drawdown |
| `return` | Raw total return |
| `profit_factor` | Gross profits / Gross losses |
| `win_rate` | Winning trades % |
| `expectancy` | Average trade expectancy |

### Live Trading

```bash
# Paper trading (safe, simulated)
cargo run -- live --config ../configs/sample_config.json --paper

# Custom cycle interval (seconds)
cargo run -- live --paper --interval 300

# Live trading with real money (CAUTION!)
cargo run -- live --live
```

## Architecture

```
src/
├── main.rs                  # CLI entry point
├── lib.rs                   # Library exports
│
├── commands/                # Command implementations
│   ├── backtest.rs          # Historical simulation
│   ├── optimize.rs          # Grid search optimization
│   ├── live.rs              # Real-time trading
│   └── download.rs          # Data fetching
│
├── strategies/              # Strategy implementations
│   ├── mod.rs               # Strategy trait + registry
│   ├── volatility_regime/   # ATR regime classification
│   ├── quick_flip/          # Multi-TF range breakout
│   ├── vwap_scalper/        # VWAP crossover scalping
│   ├── mean_reversion/      # Bollinger + RSI reversion
│   ├── momentum_scalper/    # EMA crossover momentum
│   └── range_breakout/      # N-bar high/low breakout
│
├── binance/                 # Binance API (data only)
│   ├── client.rs
│   └── types.rs
│
├── coindcx/                 # CoinDCX API (trading)
│   ├── client.rs            # REST client
│   ├── auth.rs              # HMAC-SHA256 signing
│   ├── circuit_breaker.rs   # Fault tolerance
│   └── rate_limiter.rs      # Token bucket
│
├── zerodha/                 # Zerodha Kite API (equity)
│   ├── client.rs            # HFT-grade client
│   ├── auth.rs              # OAuth handling
│   └── types.rs
│
├── common/                  # Shared utilities
│   ├── circuit_breaker.rs
│   └── rate_limiter.rs
│
├── backtest.rs              # Simulation engine
├── grid.rs                  # Grid generation
├── optimizer.rs             # Parallel optimization
├── risk.rs                  # Position sizing
├── indicators.rs            # 25+ technical indicators
├── config.rs                # Configuration parsing
├── types.rs                 # Domain model
├── data.rs                  # Data loading
├── state_manager.rs         # SQLite persistence
└── multi_timeframe.rs       # Multi-TF data management
```

## Available Strategies

| Strategy | Description | Best Timeframe | Key Feature |
|----------|-------------|----------------|-------------|
| `volatility_regime` | ATR-based regime classification | 1d | Volatility clustering |
| `quick_flip` | Multi-TF range breakout | 5m (uses 1d, 15m) | Pattern recognition |
| `vwap_scalper` | VWAP crossover scalping | 5m, 15m | Mean reversion |
| `mean_reversion` | Bollinger Band + RSI | 1h, 4h | Oversold bounces |
| `momentum_scalper` | EMA crossover + MACD | 5m, 15m | Trend following |
| `range_breakout` | N-bar high/low breakout | 1h, 4h | Breakout trading |

### Quick Flip Strategy

Multi-timeframe pattern scalper:
- **1d**: ATR for volatility context
- **15m**: Range box detection (opening bars high/low)
- **5m**: Entry patterns (Hammer, Engulfing)
- **Filters**: Trend EMA, volume confirmation
- **Exit**: Signal candle extreme (stop), range boundary (target)

### VWAP Scalper Strategy

Price-to-VWAP mean reversion:
- **Entry**: Price crosses above VWAP
- **Filter**: Max distance from VWAP, volume spike optional
- **Exit**: ATR-based stops, max hold bars
- **Features**: Cooldown between trades, trailing stop

## Creating a New Strategy

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
}

impl Default for MyStrategyConfig {
    fn default() -> Self {
        Self { param1: 14, param2: 2.5 }
    }
}
```

### Step 3: Implement Strategy (`strategy.rs`)

```rust
use crate::strategies::Strategy;
use crate::{Candle, Position, Signal, Symbol};

pub struct MyStrategy {
    config: MyStrategyConfig,
}

impl Strategy for MyStrategy {
    fn name(&self) -> &'static str { "my_strategy" }

    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal {
        // Your logic here
        Signal::Flat
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        entry_price * 0.95
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        entry_price * 1.10
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        None
    }
}
```

### Step 4: Register in `src/strategies/mod.rs`

```rust
pub mod my_strategy;

// In get_registry():
map.insert("my_strategy", my_strategy::create as StrategyFactory);
```

## Multi-Timeframe Support

Strategies can declare required timeframes:

```rust
impl Strategy for QuickFlipStrategy {
    fn required_timeframes(&self) -> Vec<String> {
        vec!["1d".to_string(), "15m".to_string(), "5m".to_string()]
    }

    fn generate_signal_mtf(
        &self,
        symbol: &Symbol,
        mtf_candles: &MultiTimeframeCandles,
        position: Option<&Position>,
    ) -> Signal {
        let daily = mtf_candles.get("1d").unwrap();
        let m15 = mtf_candles.get("15m").unwrap();
        let m5 = mtf_candles.get("5m").unwrap();
        // Use all timeframes for decision
    }
}
```

## Configuration Structure

```json
{
  "exchange": {
    "maker_fee": 0.001,
    "taker_fee": 0.001,
    "assumed_slippage": 0.001,
    "rate_limit": 10
  },
  "trading": {
    "pairs": ["BTCINR", "ETHINR"],
    "initial_capital": 100000,
    "risk_per_trade": 0.15,
    "max_positions": 5,
    "max_portfolio_heat": 0.30,
    "max_position_pct": 0.20,
    "max_drawdown": 0.20,
    "drawdown_warning": 0.10,
    "drawdown_critical": 0.15,
    "consecutive_loss_limit": 3,
    "consecutive_loss_multiplier": 0.75
  },
  "strategy": {
    "name": "volatility_regime",
    "timeframe": "1d",
    "atr_period": 14,
    "ema_fast": 8,
    "ema_slow": 21
  },
  "tax": {
    "tax_rate": 0.30,
    "tds_rate": 0.01,
    "loss_offset_allowed": false
  },
  "backtest": {
    "data_dir": "../data",
    "results_dir": "../results",
    "start_date": "2022-01-01",
    "end_date": "2025-12-31"
  },
  "grid": {
    "_optimization": [{ "sharpe_ratio": 0.96, "symbols": [...], ... }],
    "ema_fast": [8, 13],
    "ema_slow": [21, 34]
  }
}
```

## Risk Management

The risk manager enforces:

| Rule | Default | Description |
|------|---------|-------------|
| Max Drawdown | 20% | Hard halt on trading |
| Drawdown Warning | 10% | 50% position size reduction |
| Drawdown Critical | 15% | 25% position size reduction |
| Consecutive Losses | 3 | 75% position size reduction |
| Max Positions | 5 | Concurrent open positions |
| Max Position % | 20% | Single position capital limit |
| Portfolio Heat | 30% | Total risk exposure limit |

## Exchange Integrations

### CoinDCX (Crypto)
- HMAC-SHA256 authentication
- Circuit breaker for fault tolerance
- Rate limiting (token bucket)
- Exponential backoff retries

### Zerodha Kite (Equity)
- OAuth authentication
- HFT-grade optimizations
- NSE/BSE support
- Production-ready, fully decoupled

### Binance (Data Only)
- Public API, no auth required
- Historical kline fetching
- Auto-pagination for large ranges

## Testing

```bash
cargo test                    # Run all tests
cargo test --release          # With optimizations
cargo test -- --nocapture     # Show output
cargo test strategies::       # Test strategies module
```

## Performance

- **Backtest Speed**: 10-100x faster than Python
- **Parallelization**: Automatic across all CPU cores
- **Memory**: Windowed history (300-bar lookback)
- **Release Build**: LTO enabled, single codegen unit

## License

MIT License
