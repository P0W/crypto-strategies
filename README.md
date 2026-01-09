# Crypto Strategies

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

**Test Period**: 2021-12-04 to 2026-01-04 | **Initial Capital**: ₹100,000 | **Timeframe**: 1d

### Performance Summary

| Strategy | Symbols | Return | Sharpe | Calmar | Max DD | Win Rate | Profit Factor | Trades | Expectancy |
|----------|---------|--------|--------|--------|--------|----------|---------------|--------|------------|
| **regime_grid** | ETH,SOL | 155.80% | 2.21 | 17.90 | 8.70% | 83.70% | - | 92 | ₹1,668 |
| **quick_flip** | BTC,ETH,SOL,BNB,XRP | 26.01% | 1.63 | 4.51 | 5.76% | 63.64% | 2.78 | 22 | ₹1,182 |
| **momentum_scalper** | BTC,ETH,SOL,BNB,XRP | 38.00% | 1.06 | 2.79 | 13.61% | 47.14% | 1.77 | 70 | ₹543 |
| **range_breakout** | BTC,ETH,SOL,BNB,XRP | 24.75% | 1.50 | 3.33 | 7.43% | 83.33% | 6.96 | 18 | ₹1,375 |
| **volatility_regime** | BTC,ETH,SOL,BNB,XRP | 6.38% | 0.19 | 1.20 | 5.31% | 45.45% | 1.27 | 11 | ₹580 |

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
<summary><b>volatility_regime</b> - Conservative approach (5.31% max DD)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
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
<summary><b>regime_grid</b> - Best overall (Sharpe 2.21, Calmar 17.90, Return 155.80%)</summary>

```json
{
    "trading": {
        "symbols": ["ETHINR", "SOLINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.15
    },
    "strategy": {
        "name": "regime_grid",
        "timeframe": "1d",
        "adx_period": 14,
        "adx_sideways_threshold": 30,
        "ema_band_pct": 0.10,
        "max_capital_usage_pct": 0.25,
        "max_drawdown_pct": 0.10,
        "max_grids": 7,
        "rsi_bear_threshold": 30,
        "rsi_bull_min": 45,
        "rsi_bull_max": 70,
        "sell_target_pct": 0.03,
        "stop_atr_multiple": 1.0,
        "trailing_activation_pct": 0.015
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
