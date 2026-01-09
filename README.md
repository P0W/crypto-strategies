# Crypto Strategies

[![Strategy Regression Tests](https://github.com/P0W/crypto-strategies/actions/workflows/regression-tests.yml/badge.svg)](https://github.com/P0W/crypto-strategies/actions/workflows/regression-tests.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

High-performance Rust trading system with **production-grade Order Management System (OMS)** for backtesting and live trading on CoinDCX (Indian crypto exchange) and Zerodha (Indian equity).

> **Note**: A legacy Python implementation exists in the [`python`](https://github.com/P0W/crypto-strategies/tree/python) branch but is deprecated and unmaintained.

## 🎯 OMS Architecture

The system features a complete Order Management System enabling:
- **Order lifecycle management**: Pending → Submitted → Open → Filled/Cancelled
- **Intra-candle fill detection**: Buy limit fills if `candle.low <= limit_price`
- **FIFO position accounting**: Multiple fills per position with weighted average entry
- **Grid trading support**: Place multiple simultaneous limit orders per symbol
- **Multi-timeframe strategies**: Access multiple timeframes in strategy logic
- **Order-based execution**: Strategies generate orders, not just signals

## Quick Start

```bash
# Build (debug for development)
cargo build

# Build (release for production/optimization)
cargo build --release

# Run backtest
cargo run -- backtest --config configs/sample_config.json

# Run optimization
cargo run --release -- optimize --config configs/sample_config.json

# Run tests
cargo test
```

### Environment Configuration

```bash
# Create .env from template
copy .env.example .env  # Windows

# Add credentials
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
ZERODHA_API_KEY=your_kite_api_key
ZERODHA_ACCESS_TOKEN=your_access_token
```

## Repository Structure

```
├── src/              # Rust source code
│   ├── commands/     # CLI commands (backtest, optimize, live, download)
│   ├── oms/          # Order Management System
│   ├── strategies/   # Trading strategies
│   ├── binance/      # Binance API (data only)
│   ├── coindcx/      # CoinDCX API (trading)
│   ├── zerodha/      # Zerodha Kite API (equity)
│   └── common/       # Shared utilities
├── tests/            # Integration tests
├── configs/          # Strategy configuration files (JSON)
├── data/             # Historical OHLCV data (CSV)
├── results/          # Backtest results and charts
├── logs/             # Trading logs
├── .env              # API credentials (create from .env.example)
└── Cargo.toml        # Rust dependencies
```

### ⚠️ Important: Currency Handling

The system is **currency-agnostic** - all calculations work with dimensionless numbers. No currency conversion is performed. Simply ensure your `initial_capital` (in config) and price data (in CSV files) are in the **same currency**.

**Current data files contain USD prices** despite the "INR" suffix in filenames.

## Verified Backtest Results

All strategies backtested with **₹100,000 initial capital** on crypto pairs (BTC, ETH, SOL, BNB, XRP) with INR.
Data period: 2022-01 to 2026-01 (~1493 daily candles per symbol).

| Strategy | Timeframe | Sharpe | Calmar | Return | Post-Tax | Win Rate | Trades | Max DD | Profit Factor |
|----------|-----------|--------|--------|--------|----------|----------|--------|--------|---------------|
| **quick_flip** | 1d | 1.08 | 2.00 | 166.08% | 116.31% | 56.55% | 145 | 13.54% | 2.73 |
| **momentum_scalper** | 1d | 0.55 | 0.77 | 104.13% | 72.95% | 45.65% | 276 | 24.69% | 1.41 |
| **range_breakout** | 1d | 0.71 | 1.09 | 92.74% | 64.92% | 48.28% | 116 | 15.93% | 2.44 |
| **volatility_regime_4h** | 4h | -0.36 | 0.94 | 92.25% | 64.59% | 54.09% | 281 | 18.50% | 1.72 |
| **volatility_regime** | 1d | 0.35 | 0.76 | 42.38% | 29.66% | 52.00% | 50 | 11.92% | 2.04 |
| **regime_grid** | 1d | 0.42 | 0.44 | 80.19% | 58.92% | 83.02% | 53 | 35.16% | 83.21 |

**Tax Calculation**: 30% flat tax on profits (Indian crypto tax), no loss offset allowed.

*Results verified on 2026-01-09 using OMS-based backtest engine with optimized parameters.*

## Commands

### Download Historical Data

```bash
# Download from Binance (default, no auth required)
cargo run -- download --symbols BTC,ETH,SOL --timeframes 5m,15m,1h,1d --days 180

# Download from CoinDCX
cargo run -- download --symbols BTC,ETH --timeframes 1h,1d --days 90 --source coindcx
```

### Backtesting

```bash
# Run backtest
cargo run -- backtest --config configs/sample_config.json

# With date range filter
cargo run -- backtest --config configs/sample_config.json --start 2024-01-01 --end 2024-12-31

# Override capital
cargo run -- backtest --config configs/sample_config.json --capital 50000
```

### Optimization

```bash
# Run optimization (uses grid from config)
cargo run --release -- optimize --config configs/sample_config.json

# Test multiple coin combinations
cargo run --release -- optimize --coins BTC,ETH,SOL,BNB --min-combo 2

# Sort by different metrics
cargo run --release -- optimize --sort-by calmar
```

### Live Trading

```bash
# Paper trading (safe, simulated)
cargo run -- live --config configs/sample_config.json --paper

# Live trading with real money (CAUTION!)
cargo run -- live --live
```

## Available Strategies

| Strategy | Description | Best Timeframe | Key Feature |
|----------|-------------|----------------|-------------|
| `volatility_regime` | ATR-based regime classification | 1d | Volatility clustering |
| `regime_grid` | Grid trading with regime adaptation | 1d | High win rate (83%) |
| `range_breakout` | N-bar high/low breakout | 1d | Lowest drawdown |
| `momentum_scalper` | EMA crossover momentum | 1d | High trade count |
| `quick_flip` | Range reversal/breakout | 1d | Best Sharpe (1.08) |

## Features

- **🎯 Order Management System**: Production-grade OMS with order lifecycle, FIFO P&L, grid trading
- **⚡ Performance**: 10-100x faster backtests than Python enabling thorough optimization
- **🔒 Type Safety**: Compile-time guarantees eliminate runtime type errors
- **📊 Multi-Timeframe**: Strategies access multiple timeframes (e.g., 1d + 4h + 1h)
- **⚙️ Parallel Optimization**: Rayon-based grid search across all CPU cores
- **🏭 Production Ready**: Circuit breakers, rate limiting, state persistence
- **🌐 Multiple Exchanges**: CoinDCX (crypto) and Zerodha Kite (equity)

## Documentation

- [CLAUDE.md](CLAUDE.md) - AI coding assistant guidance
- [REPORT.md](REPORT.md) - Detailed strategy analysis
- [OMS Design](docs/OMS_DESIGN.md) - Order Management System architecture

## License

MIT License - See [LICENSE](LICENSE) for details.

## Author

Prashant Srivastava
