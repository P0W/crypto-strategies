# Crypto Strategies

A production-grade automated trading system for CoinDCX (Indian crypto exchange), implementing the **Volatility Regime Adaptive Strategy (VRAS)** - exploiting volatility clustering and regime persistence inefficiencies in cryptocurrency markets.

## Implementations

This repository contains two complete implementations:

| Implementation | Directory | Status | Best For |
|---------------|-----------|--------|----------|
| **Rust** | [`rust/`](rust/) | Production-ready | Performance, live trading |
| **Python** | [`py/`](py/) | Production-ready | Prototyping, analysis |

## Quick Start

### Rust (Recommended for Production)

```bash
cd rust
cargo build --release
cargo run --release -- backtest --config ../configs/sample_config.json
```

### Python

```bash
cd py
uv venv && .venv\Scripts\activate  # Windows
uv pip install -e .
uv run backtest --config ../configs/sample_config.json
```

## Shared Resources

Both implementations share common resources in the root directory:

```
├── configs/          # Strategy configuration files (JSON)
├── data/             # Historical OHLCV data (CSV)
├── results/          # Backtest results and charts
├── logs/             # Trading logs
├── .env              # API credentials (create from .env.example)
└── .env.example      # Environment template
```

### ⚠️ Important: Currency Handling

The system is **currency-agnostic** - all calculations work with dimensionless numbers. No currency conversion is performed. Simply ensure your `initial_capital` (in config) and price data (in CSV files) are in the **same currency**.

**Current data files contain USD prices** despite the "INR" suffix in filenames.

See [CURRENCY.md](CURRENCY.md) for detailed explanation of currency handling.

## Core Strategy: Volatility Regime Adaptive Strategy

### Edge Hypothesis

Cryptocurrency markets exhibit strong volatility clustering (GARCH effects):
- High volatility periods persist
- Low volatility periods compress before explosive moves
- Retail traders misjudge regime transitions

### Key Features

- **Regime-Based Entry**: Only trades in Compression or Normal volatility regimes
- **Trend Confirmation**: EMA crossover + ADX filter
- **Dynamic Risk Management**: ATR-based stops, trailing stops, drawdown de-risking
- **India Tax Compliant**: Accounts for 30% flat tax + 1% TDS

### Backtest Performance

| Metric | Result |
|--------|--------|
| Total Return | 94.67% |
| Post-Tax Return | 64.57% |
| Sharpe Ratio | 1.60 |
| Max Drawdown | 13.25% |
| Win Rate | 79.31% |

*BTC+ETH+SOL+BNB+XRP, 1D timeframe, Oct 2022 – Dec 2025*

## Documentation

- [Rust Implementation](rust/README.md) - Build, run, and architecture details
- [Python Implementation](py/README.md) - Setup, usage, and strategy details
- [CLAUDE.md](CLAUDE.md) - AI coding assistant guidance

## License

MIT License - See [LICENSE](LICENSE) for details.

## Author

Prashant Srivastava
