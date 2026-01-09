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
