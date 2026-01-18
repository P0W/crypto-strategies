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

**Note**: Results from grid search optimization using full available data. Initial Capital: ₹100,000 | Timeframe: 1d

### Performance Summary

<!-- PERF_TABLE_START -->
| Strategy | Symbols | Return | Sharpe | Max DD | Win Rate | Trades | Expectancy |
|----------|---------|--------|--------|--------|----------|--------|------------|
| **quick_flip** | BTC,ETH,SOL,BNB,XRP | 685.9% | 1.37 | 15.8% | 66.2% | 204 | ₹3,359 |
| **momentum_scalper** | BTC,ETH,SOL,BNB,XRP | 680.6% | 0.96 | 29.6% | 53.1% | 375 | ₹1,813 |
| **range_breakout** | BTC,ETH,SOL,BNB,XRP | 356.5% | 0.98 | 18.7% | 56.0% | 168 | ₹2,120 |
| **volatility_regime** | BNB,BTC,SOL | 208.4% | 0.95 | 21.8% | 57.1% | 70 | ₹2,977 |
| **regime_grid** | ETH,SOL | 64.0% | 0.34 | 72.2% | 66.7% | 36 | ₹723 |
<!-- PERF_TABLE_END -->

### Strategy Configurations

<details>
<summary><b>quick_flip</b> - Best risk-adjusted returns (Sharpe 1.37, Calmar 2.92)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.2
    },
    "strategy": {
        "name": "quick_flip",
        "timeframe": "1d",
        "range_bars": 20,
        "atr_period": 14,
        "body_ratio": 0,
        "stop_atr": 1.5,
        "target_atr": 6,
        "cooldown": 3
    }
}
```
</details>

<details>
<summary><b>momentum_scalper</b> - High trade frequency (375 trades)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.1,
        "max_positions": 5,
        "max_drawdown": 0.25
    },
    "strategy": {
        "name": "momentum_scalper",
        "timeframe": "1d",
        "ema_fast": 8,
        "ema_slow": 55,
        "atr_period": 10,
        "adx_threshold": 20,
        "stop_atr_multiple": 1.5,
        "target_atr_multiple": 4,
        "max_hold_bars": 50,
        "trailing_atr_multiple": 1.0
    }
}
```
</details>

<details>
<summary><b>range_breakout</b> - Balanced performance (Sharpe 0.98, Calmar 1.72)</summary>

```json
{
    "trading": {
        "symbols": ["BTCINR", "ETHINR", "SOLINR", "BNBINR", "XRPINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.2
    },
    "strategy": {
        "name": "range_breakout",
        "timeframe": "1d",
        "lookback": 30,
        "atr_period": 14,
        "stop_atr": 2,
        "target_atr": 5,
        "trailing_atr": 1.5,
        "use_trailing": true,
        "cooldown": 3
    }
}
```
</details>

<details>
<summary><b>volatility_regime</b> - ATR-based regime trading (Sharpe 0.95, Calmar 1.06)</summary>

```json
{
    "trading": {
        "symbols": ["BNBINR", "BTCINR", "SOLINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.2
    },
    "strategy": {
        "name": "volatility_regime",
        "timeframe": "1d",
        "atr_period": 14,
        "volatility_lookback": 20,
        "ema_fast": 13,
        "ema_slow": 34,
        "adx_threshold": 20,
        "compression_threshold": 0.6,
        "expansion_threshold": 1.5,
        "stop_atr_multiple": 3,
        "target_atr_multiple": 6
    }
}
```
</details>

<details>
<summary><b>regime_grid</b> - Grid trading with regime adaptation (Sharpe 0.34)</summary>

```json
{
    "trading": {
        "symbols": ["ETHINR", "SOLINR"],
        "initial_capital": 100000,
        "risk_per_trade": 0.15,
        "max_positions": 5,
        "max_drawdown": 0.2
    },
    "strategy": {
        "name": "regime_grid",
        "timeframe": "1d",
        "adx_period": 14,
        "adx_sideways_threshold": 20,
        "ema_band_pct": 0.05,
        "max_capital_usage_pct": 0.1,
        "max_drawdown_pct": 0.1,
        "max_grids": 3,
        "rsi_bear_threshold": 30,
        "rsi_bull_min": 50,
        "rsi_bull_max": 70,
        "sell_target_pct": 0.05,
        "stop_atr_multiple": 1,
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
