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
cargo run -- optimize --config configs/sample_config.json

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
#   --sort-by <METRIC>        Sort by: sharpe, calmar, return, post_tax_return,
#                             win_rate, profit_factor
#   -t, --top <N>             Show top N results
#   --min-trades <N>          Reject statistically weak candidates (default: 20)
#   --coins <LIST>            Coins to test (e.g., "BTC,ETH,SOL")
#   --timeframes <LIST>       Timeframes to test (e.g., "1h,4h,1d")
#   -O, --override <PARAMS>   Override grid params (e.g., "ema_fast=5,8,13")
#   --no-update               Report results without changing the config
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
| `volatility_regime` | Classifies market into compression/expansion/extreme using ATR ratios. Enters during the quiet, exits before the chaos. |
| `momentum_scalper` | EMA crossover with ADX momentum filter. Catches trends early, exits when momentum fades. |
| `range_breakout` | N-bar high/low breakout with ATR-based stops. Simple and systematic. |
| `quick_flip` | Range breakout with strong candle confirmation. Waits for conviction before entering. |
| `regime_grid` | Grid trading that adapts spacing based on volatility regime. |

## Backtest Results

**Reproduced 2026-07-19** from the committed configs using the current execution,
position valuation, stop/target, lifecycle, and strategy-sizing behavior. These
are not re-optimized results. Initial capital: ₹100,000. Fees and slippage are taken
from each config; post-tax return applies 30% tax to winning trades without loss
offset. Sharpe is calculated from UTC daily closing equity for consistency
across source timeframes. The local crypto files use INR symbol names but may contain
USD/USDT-derived prices, so these results are not proof of CoinDCX profitability.

### Performance Summary

<!-- PERF_TABLE_START -->
| Strategy | Symbols | Return | Post-Tax | Sharpe | Calmar | Max DD | Win Rate | Trades | Expectancy |
|----------|---------|--------|----------|--------|--------|--------|----------|--------|------------|
| **quick_flip** | BTC,ETH,SOL,BNB,XRP | -17.33% | -18.03% | -1.13 | -0.16 | 21.75% | 30.77% | 26 | ₹-666.40 |
| **momentum_scalper** | BTC,ETH,SOL,BNB,XRP | -26.17% | -28.66% | -1.38 | -0.21 | 26.20% | 37.84% | 37 | ₹-707.36 |
| **range_breakout** | BTC,ETH,SOL,BNB,XRP | -15.60% | -17.16% | -0.97 | -0.13 | 23.06% | 29.17% | 24 | ₹-650.18 |
| **volatility_regime** | BNB,BTC,SOL | 3.03% | -8.44% | -0.30 | 0.03 | 21.64% | 46.88% | 32 | ₹94.66 |
| **regime_grid** | ETH,SOL | 3.20% | -5.60% | -1.27 | 0.09 | 6.20% | 44.66% | 562 | ₹3.76 |
| **volatility_regime_4h** | BTC,ETH,SOL | -24.12% | -26.03% | -2.32 | -0.26 | 25.47% | 27.18% | 103 | ₹-234.18 |
<!-- PERF_TABLE_END -->

The quick-start `configs/sample_config.json` baseline returned **-11.98%**
pre-tax and **-18.23%** post-tax over the same daily data range.

### CoinDCX-Native Research Candidate

`configs/sol_bnb_regime_grid_candidate.json` is the only candidate that remained
positive after chronological validation on CoinDCX-native daily candles. It is
**paper-only** and is not a live recommendation.

Assumptions: SOLINR + BNBINR, ₹100,000 initial capital, 1% risk per trade,
20% grid capital cap, 0.1% fee per side, 0.1% slippage, 30% tax on winning
trades without loss offset.

| Period | Dates | Pre-Tax | Post-Tax | Sharpe | Max DD | Profit Factor | Trades |
|--------|-------|---------|----------|--------|--------|---------------|--------|
| Training | 2023-10-24 to 2025-03-31 | 12.29% | 6.18% | 0.66 | 2.18% | 2.52 | 512 |
| Validation | 2025-04-01 to 2025-12-31 | 14.38% | 8.69% | 1.70 | 1.93% | 4.13 | 420 |
| Holdout | 2026-01-01 to 2026-07-19 | 7.21% | 4.30% | 1.47 | 1.29% | 3.88 | 194 |

Cost stress:

- At 0.2% fee and 0.2% slippage, the holdout remained positive at **0.85% post-tax**.
- At a 0.5% fee and 0.25% slippage, full-period post-tax return was **-2.15%**.

Therefore the candidate must not trade live unless the actual CoinDCX account fee
tier is low enough, current native data reproduces the result, and paper execution
passes the gates in [`docs/LIVE_TRADING_REVIEW.md`](docs/LIVE_TRADING_REVIEW.md).

### Strategy Configurations

<details>
<summary><b>quick_flip</b> - Baseline: -17.33% return, Sharpe -1.13</summary>

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
<summary><b>momentum_scalper</b> - Baseline: -26.17% return, 37 trades</summary>

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
<summary><b>range_breakout</b> - Baseline: -15.60% return, Sharpe -0.97</summary>

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
<summary><b>volatility_regime</b> - Baseline: 3.03% pre-tax, -8.44% post-tax</summary>

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
<summary><b>regime_grid</b> - Baseline: 3.20% pre-tax, -5.60% post-tax</summary>

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

> **Note**: None of the committed strategies currently has positive post-tax
> performance in this run. Do not deploy them with real funds without
> CoinDCX-native data, walk-forward validation, realistic fee tiers, and paper
> trading.

## Repository Structure

```
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library exports
│   ├── backtest.rs          # Event-driven backtesting engine
│   ├── optimizer.rs         # Parallel parameter grid search
│   ├── risk.rs              # Position sizing, drawdown control, portfolio heat
│   ├── indicators.rs        # Technical indicators (ATR, EMA, RSI, ADX, etc.)
│   ├── config.rs            # Configuration parsing and validation
│   ├── data.rs              # OHLCV data loading and alignment
│   ├── grid.rs              # Grid parameter generation for optimization
│   ├── multi_timeframe.rs   # Multi-timeframe data handling
│   ├── state_manager.rs     # SQLite state persistence and crash recovery
│   ├── types.rs             # Core domain types (Candle, Position, Trade, etc.)
│   ├── commands/            # CLI command handlers (backtest, optimize, live, download)
│   ├── strategies/          # Trading strategy implementations
│   │   ├── volatility_regime/   # ATR-based regime classification
│   │   ├── momentum_scalper/    # EMA crossover with momentum filter
│   │   ├── range_breakout/      # N-bar high/low breakout
│   │   ├── quick_flip/          # Range breakout with candle confirmation
│   │   └── regime_grid/         # Grid trading with volatility adaptation
│   ├── oms/                 # Order Management System
│   │   ├── orderbook.rs         # Order storage and matching
│   │   ├── execution.rs         # Fill simulation with slippage
│   │   ├── position_manager.rs  # FIFO position accounting
│   │   ├── order_sizer.rs       # Risk-based position sizing
│   │   └── types.rs             # Order, Fill, Position types
│   ├── analysis/            # Trade analysis utilities
│   │   ├── monthly.rs           # Monthly P&L breakdown
│   │   ├── day_of_week.rs       # Day-of-week performance
│   │   └── streaks.rs           # Win/loss streak analysis
│   ├── common/              # Shared utilities
│   │   ├── circuit_breaker.rs   # Fault tolerance pattern
│   │   └── rate_limiter.rs      # API rate limiting
│   ├── coindcx/             # CoinDCX exchange client (crypto)
│   ├── zerodha/             # Zerodha Kite client (equity)
│   └── binance/             # Binance client (data only)
├── configs/                 # Strategy configuration files (JSON)
├── data/                    # Historical OHLCV data (CSV)
├── tests/                   # Integration tests
├── docs/                    # Documentation
├── logs/                    # Trading and backtest logs
└── results/                 # Backtest results output
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

- [Architecture](docs/ARCHITECTURE.md) - System design with mermaid diagrams
- [Creating Strategies](docs/CREATING_STRATEGIES.md) - Step-by-step guide to building custom strategies
- [Live Trading Review](docs/LIVE_TRADING_REVIEW.md) - Production deployment notes
- [CLAUDE.md](CLAUDE.md) - AI assistant guidance for development

## License

MIT License - See [LICENSE](LICENSE) for details.

## Author

Prashant Srivastava
