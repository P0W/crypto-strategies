# Crypto Strategies

[![codecov](https://codecov.io/gh/P0W/crypto-strategies/graph/badge.svg)](https://codecov.io/gh/P0W/crypto-strategies)
[![Strategy Regression Tests](https://github.com/P0W/crypto-strategies/actions/workflows/regression-tests.yml/badge.svg)](https://github.com/P0W/crypto-strategies/actions/workflows/regression-tests.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

High-performance Rust trading system for backtesting and live trading on CoinDCX (crypto) and Zerodha (equity).

> **Note**: A legacy Python implementation exists in the [`python`](https://github.com/P0W/crypto-strategies/tree/python) branch but is deprecated and unmaintained.

## Quick Start

```bash
# Build
cargo build

# Run backtest
cargo run -- backtest --config configs/sample_config.json

# Run optimization
cargo run --release -- optimize --config configs/sample_config.json

# Run tests
cargo test
```

### Environment Setup

```bash
# Create .env from template
copy .env.example .env  # Windows
cp .env.example .env    # Linux/Mac

# Add exchange credentials to .env
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
```

### Development Setup

```bash
# Enable pre-commit hooks (runs fmt + clippy before each commit)
git config core.hooksPath .githooks
```

## Commands

### Backtest

```bash
cargo run -- backtest --config configs/sample_config.json

# Options:
#   -c, --config <FILE>     Config file path
#   -s, --strategy <NAME>   Override strategy name
#   --capital <AMOUNT>      Override initial capital
#   --start <YYYY-MM-DD>    Start date filter
#   --end <YYYY-MM-DD>      End date filter
#   -v, --verbose           Verbose logging
```

### Optimize

```bash
cargo run --release -- optimize --config configs/sample_config.json

# Options:
#   -c, --config <FILE>       Config with grid section
#   --sort-by <METRIC>        Sort by: sharpe, calmar, return, win_rate, profit_factor
#   -t, --top <N>             Show top N results
#   --coins <LIST>            Coins to test (e.g., "BTC,ETH,SOL")
#   --timeframes <LIST>       Timeframes to test (e.g., "1h,4h,1d")
#   -O, --override <PARAMS>   Override grid params (e.g., "ema_fast=5,8,13")
```

### Live Trading

```bash
cargo run -- live --config configs/sample_config.json --paper   # Paper trading
cargo run -- live --config configs/sample_config.json --live    # Real trading (CAUTION!)
```

### Download Data

```bash
cargo run -- download --symbols BTC,ETH,SOL --timeframes 1h,4h,1d --days 180
```

## Available Strategies

| Strategy | Description |
|----------|-------------|
| `volatility_regime` | ATR-based regime classification with trend confirmation |
| `momentum_scalper` | EMA crossover with momentum filters |
| `range_breakout` | N-bar high/low breakout |
| `quick_flip` | Range breakout with strong candle confirmation |
| `regime_grid` | Grid trading with volatility regime adaptation |

## Backtest Results
 
**Note**: Results based on available data for each strategy (May 2025 - Jan 2026). Initial Capital: ₹100,000 | Timeframe: 1d
 
### Performance Summary
 
| Strategy | Symbols | Date Range | Return | Sharpe | Max DD | Win Rate | Trades |
|----------|---------|------------|--------|--------|--------|----------|--------|
| **regime_grid** | ETH,SOL | 2025-07-29 to 2026-01-08 | 108.1% | 2.53 | 12.6% | 82.8% | 64 |
| **momentum_scalper** | BTC,ETH,SOL,BNB,XRP | 2025-05-12 to 2025-10-10 | 38.0% | 1.06 | 13.6% | 47.1% | 70 |
| **quick_flip** | BTC,ETH,SOL,BNB,XRP | 2025-05-15 to 2025-10-07 | 26.0% | 1.63 | 5.8% | 63.6% | 22 |
| **range_breakout** | BTC,ETH,SOL,BNB,XRP | 2025-05-15 to 2025-10-10 | 24.8% | 1.50 | 7.4% | 83.3% | 18 |
| **volatility_regime** | BNB,BTC,SOL | 2025-05-30 to 2025-10-10 | 6.4% | 0.28 | 13.6% | 45.5% | 11 |

### Strategy Configurations

<details>
<summary><b>quick_flip</b> - Best risk-adjusted returns (Sharpe 1.63)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.25
    },
    "strategy": {
        "name": "quick_flip",
        "timeframe": "1d",
        "breakout_bars": 10,
        "atr_period": 7,
        "atr_multiplier": 1.5,
        "min_body_pct": 0.6,
        "profit_target_atr": 2.0,
        "max_hold_bars": 5
    }
}
```
</details>

<details>
<summary><b>momentum_scalper</b> - High trade frequency (70 trades)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.25
    },
    "strategy": {
        "name": "momentum_scalper",
        "timeframe": "1d",
        "ema_fast": 8,
        "ema_slow": 21,
        "atr_period": 14,
        "atr_multiplier": 1.5,
        "min_momentum": 0.02,
        "profit_target_atr": 2.0,
        "max_hold_bars": 10
    }
}
```
</details>

<details>
<summary><b>range_breakout</b> - Highest win rate (83.33%)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.25
    },
    "strategy": {
        "name": "range_breakout",
        "timeframe": "1d",
        "breakout_bars": 20,
        "atr_period": 14,
        "atr_multiplier": 2.0,
        "volume_factor": 1.5,
        "profit_target_atr": 3.0,
        "max_hold_bars": 15
    }
}
```
</details>

<details>
<summary><b>volatility_regime</b> - ATR-based regime trading (Sharpe 0.55)</summary>

```json
{
    "trading": {
        "symbols": ["BNBINR", "BTCINR", "SOLINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.20
    },
    "strategy": {
        "name": "volatility_regime",
        "timeframe": "1d",
        "atr_period": 14,
        "regime_lookback": 20,
        "ema_fast": 8,
        "ema_slow": 21,
        "high_vol_threshold": 1.5,
        "low_vol_threshold": 0.7
    }
}
```
</details>

<details>
<summary><b>regime_grid</b> - Best overall (Sharpe 2.78, Return 150.4%)</summary>

```json
{
    "trading": {
        "symbols": ["ETHINR", "SOLINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.20
    },
    "strategy": {
        "name": "regime_grid",
        "timeframe": "1d",
        "adx_period": 14,
        "adx_sideways_threshold": 30,
        "ema_band_pct": 0.05,
        "max_capital_usage_pct": 0.20,
        "max_drawdown_pct": 0.10,
        "max_grids": 7,
        "rsi_bear_threshold": 30,
        "rsi_bull_min": 45,
        "rsi_bull_max": 70,
        "sell_target_pct": 0.03,
        "stop_atr_multiple": 2.0,
        "trailing_activation_pct": 0.025
    }
}
```
</details>

> **Note**: Past performance does not guarantee future results. These configurations are provided as starting points for further optimization.

## Repository Structure

```
├── src/
│   ├── main.rs           # CLI entry point
│   ├── backtest.rs       # Backtesting engine
│   ├── optimizer.rs      # Parameter optimization
│   ├── risk.rs           # Position sizing & drawdown control
│   ├── indicators.rs     # Technical indicators (ATR, EMA, RSI, etc.)
│   ├── commands/         # CLI command handlers
│   ├── strategies/       # Trading strategies
│   ├── oms/              # Order Management System
│   ├── coindcx/          # CoinDCX exchange client
│   ├── zerodha/          # Zerodha Kite client
│   └── binance/          # Binance data client
├── configs/              # Strategy configuration files
├── data/                 # Historical OHLCV data (CSV)
├── tests/                # Integration tests
└── docs/                 # Documentation
```

## Configuration

Strategy configs are JSON files with these sections:

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.20
    },
    "strategy": {
        "name": "volatility_regime",
        "timeframe": "1d",
        "atr_period": 14,
        "ema_fast": 8,
        "ema_slow": 21
    },
    "grid": {
        "ema_fast": [5, 8, 13],
        "ema_slow": [21, 34]
    }
}
```

See `configs/sample_config.json` for a complete example.

## Documentation

- [Creating Strategies](docs/CREATING_STRATEGIES.md) - Step-by-step guide to building custom strategies
- [CLAUDE.md](CLAUDE.md) - AI assistant guidance for development

## License

MIT License - See [LICENSE](LICENSE) for details.

## Author

Prashant Srivastava
