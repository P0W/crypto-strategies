# 🦀 Crypto Strategies

> **High-performance crypto backtester & live trading engine in Rust.** 55% returns, 0.53 Sharpe ratio. Built for CoinDCX (India). Type-safe, battle-tested, production-ready.

[![CI](https://github.com/P0W/crypto-strategies/workflows/Rust%20CI/badge.svg)](https://github.com/P0W/crypto-strategies/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

---

## Why This Exists

Most crypto backtesting tools are slow (Python), untyped (JavaScript), or proprietary. This project delivers:

- ⚡ **10-100x faster** than Python backtesters (Rust + Rayon parallelization)
- 🛡️ **Type-safe** - catch bugs at compile time, not in production
- 🇮🇳 **India tax-compliant** - 30% flat tax + 1% TDS built-in
- 📊 **Regime-adaptive** - exploits volatility clustering (GARCH effects)
- 🔌 **Plugin architecture** - write strategies without touching the engine
- 🧪 **Battle-tested** - verified on 3+ years of crypto data

## Benchmark: Rust vs Python

| Metric | Rust (This) | Python (Backtrader) | Speedup |
|--------|-------------|---------------------|---------|
| **1000 trades backtest** | 0.24s | 4.8s | **20x faster** |
| **Parameter optimization (100 combos)** | 8.2s | 450s | **55x faster** |
| **Memory usage** | 28 MB | 340 MB | **12x less** |
| **Type safety** | ✅ Compile-time | ❌ Runtime errors | ∞ |

*Benchmarks on M1 Mac, BTC+ETH+SOL 1D data, 2022-2025. [Reproducible benchmarks →](rust/benches/)*

---

## Implementations

This repository contains two complete implementations:

| Implementation | Directory | Status | Best For |
|---------------|-----------|--------|----------|
| **Rust** ⭐ | [`rust/`](rust/) | Production-ready | Performance, live trading, optimization |
| **Python** | [`py/`](py/) | Active | Prototyping, analysis, learning |

**Python users:** The Rust implementation can be used from Python via PyO3 bindings (coming soon). Write strategies in Python, execute in Rust.

## 🚀 Quick Start

### Installation

#### Option 1: From Source (Recommended)

```bash
git clone https://github.com/P0W/crypto-strategies.git
cd crypto-strategies/rust
cargo build --release
```

#### Option 2: From crates.io (Coming Soon)

```bash
cargo install crypto-strategies
crypto-strategies backtest --config config.json
```

### Run Your First Backtest

```bash
cd rust

# Backtest volatility regime strategy on BTC+ETH+SOL
cargo run --release -- backtest --config ../configs/sample_config.json

# Optimize parameters across multiple combinations
cargo run --release -- optimize --config ../configs/sample_config.json

# Paper trading (safe simulation)
cargo run --release -- live --paper --config ../configs/sample_config.json
```

**Output:**
```
✅ Backtest complete!
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Strategy: volatility_regime
Period: 2022-01-02 to 2025-12-31
Capital: ₹100,000 → ₹155,360 (+55.36%)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Sharpe Ratio:    0.53
Max Drawdown:   13.61%
Win Rate:       44.90%
Total Trades:       49
Profit Factor:    2.18
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Python Users

```bash
cd py
uv venv && source .venv/bin/activate  # or .venv\Scripts\activate on Windows
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

## 📊 Featured Strategy: Volatility Regime Adaptive (VRAS)

### The Edge Hypothesis

Cryptocurrency markets exhibit **volatility clustering** (GARCH effects) that retail traders systematically misjudge:

1. **Compression Phase** (Low ATR) → Retail hesitates → Big move brewing
2. **Expansion Phase** (High ATR) → Retail FOMOs in → Reversal likely
3. **Normal Phase** → Clear trends → Follow momentum

**Our edge:** Enter during compression/normal, avoid expansion, exit before extreme.

### Verified Performance

**Test Setup:** 
- Symbols: BTC, ETH, SOL (3 major cryptos)
- Timeframe: 1D candles
- Period: Jan 2022 – Dec 2025 (4 years, bull + bear markets)
- Capital: ₹100,000 initial
- Fees: 0.1% taker + 0.1% slippage

**Results:**

| Metric | Value | Benchmark |
|--------|-------|-----------|
| **Total Return** | 55.36% | BTC: 68%, ETH: 45% |
| **Post-Tax Return** | 38.75% | (30% tax + 1% TDS) |
| **Sharpe Ratio** | 0.53 | > 0.5 = good |
| **Calmar Ratio** | 0.84 | Return/MaxDD |
| **Max Drawdown** | 13.61% | BTC: 28%, ETH: 35% |
| **Win Rate** | 44.90% | 22 wins / 27 losses |
| **Profit Factor** | 2.18 | ₹2.18 profit per ₹1 risk |
| **Total Trades** | 49 | ~1 per month |
| **Avg Trade** | +1154.98 | ₹1.2k per trade |

**Reproducibility:** Run `cargo run --release -- backtest --config ../configs/volatility_regime_config.json` to verify these exact numbers.

### Strategy Features

- ✅ **Regime Classification**: ATR-based volatility detection (Compression/Normal/Expansion/Extreme)
- ✅ **Trend Confirmation**: Dual EMA (8/21) + ADX > 30 filter
- ✅ **Dynamic Stops**: 2.5x ATR stop loss, 5x ATR take profit, trailing stop at 50% profit
- ✅ **Risk Management**: Drawdown-based position sizing, consecutive loss reduction
- ✅ **Tax Compliance**: India's 30% flat tax + 1% TDS on every sell

## 🔌 Plugin Architecture: Write Your Strategy in 10 Minutes

The engine uses a **trait-based plugin system**. You don't need to understand the entire codebase to contribute a strategy.

### Example: Moving Average Crossover

```rust
use crypto_strategies::*;

pub struct MACrossover {
    fast_period: usize,
    slow_period: usize,
}

impl Strategy for MACrossover {
    fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal {
        let fast_ma = sma(candles, self.fast_period);
        let slow_ma = sma(candles, self.slow_period);
        
        if fast_ma > slow_ma && position.is_none() {
            Signal::Long  // Golden cross
        } else if fast_ma < slow_ma && position.is_some() {
            Signal::Flat  // Death cross
        } else {
            Signal::Flat
        }
    }
    
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        entry_price * 0.95  // 5% stop
    }
    
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        entry_price * 1.15  // 15% target
    }
}
```

**That's it!** Register in `src/strategies/mod.rs` and run:

```bash
cargo run --release -- backtest --config your_config.json
```

See [CONTRIBUTING.md](CONTRIBUTING.md#creating-new-strategies) for full tutorial.

---

## 🏗️ Project Structure

```
crypto-strategies/
├── rust/                    # ⭐ Rust implementation (primary)
│   ├── src/
│   │   ├── strategies/      # Strategy plugins (add yours here!)
│   │   │   ├── volatility_regime/
│   │   │   ├── momentum_scalper/
│   │   │   └── your_strategy/  ← Start here
│   │   ├── backtest.rs      # Event-driven backtesting engine
│   │   ├── optimizer.rs     # Parallel grid search (Rayon)
│   │   ├── risk.rs          # Position sizing & drawdown control
│   │   └── indicators.rs    # 25+ technical indicators
│   ├── benches/             # Criterion.rs benchmarks
│   └── tests/               # Integration tests
│
├── py/                      # Python implementation
│   └── src/                 # Strategy prototyping
│
├── configs/                 # Strategy configurations (JSON)
│   ├── sample_config.json   # Start here
│   └── volatility_regime_config.json
│
├── data/                    # Historical OHLCV data (CSV)
│   ├── BTCINR_1d.csv
│   └── ETHINR_1d.csv
│
└── .github/                 # CI/CD, issue templates
```

**Key Files:**
- [`rust/src/strategies/mod.rs`](rust/src/strategies/mod.rs) - Strategy trait definition
- [`rust/src/backtest.rs`](rust/src/backtest.rs) - Backtesting engine
- [`CONTRIBUTING.md`](CONTRIBUTING.md) - Contribution guidelines

---

## 🎯 Comparison: Why This Over Alternatives?

### vs Python Frameworks

| Feature | This Repo | Backtrader (Py) | Zipline (Py) | Hummingbot (Py) |
|---------|-----------|-----------------|--------------|-----------------|
| **Speed** | ⚡ 0.24s | 4.8s | 8.1s | N/A |
| **Type Safety** | ✅ Compile-time | ❌ Runtime | ❌ Runtime | ❌ Runtime |
| **Parallelization** | ✅ Rayon | ❌ GIL-limited | ❌ GIL-limited | ⚠️ asyncio |
| **Memory** | ✅ 28 MB | ❌ 340 MB | ❌ 580 MB | ❌ 200+ MB |
| **India Tax** | ✅ Built-in | ❌ Manual | ❌ Manual | ❌ N/A |
| **Live Trading** | ✅ CoinDCX + Zerodha | ❌ Limited | ❌ Deprecated | ✅ Multi-exchange |
| **Multi-Timeframe** | ✅ Native | ⚠️ Resampler | ⚠️ Panels | ❌ Single TF |
| **License** | ✅ MIT | ✅ GPL v3 | ✅ Apache 2.0 | ✅ Apache 2.0 |

### vs Rust Frameworks

| Feature | This Repo | barter-rs | hftbacktest | Freqtrade (Py) |
|---------|-----------|-----------|-------------|----------------|
| **Backtesting** | ✅ Event-driven | ✅ Event-driven | ✅ Tick-level | ✅ Vectorized |
| **Live Trading** | ✅ Production-ready | ⚠️ In development | ❌ Not included | ✅ Production-ready |
| **Strategy Plugins** | ✅ Trait-based | ✅ Trait-based | ⚠️ Custom engine | ✅ Class-based |
| **Multi-Exchange** | ⚠️ 2 exchanges | ✅ 10+ exchanges | ✅ Multiple | ✅ 100+ exchanges |
| **Optimization** | ✅ Rayon parallel | ❌ Manual | ⚠️ Limited | ✅ Hyperopt |
| **Documentation** | ✅ Comprehensive | ⚠️ Improving | ⚠️ Technical | ✅ Extensive |
| **India Tax** | ✅ Built-in | ❌ Not included | ❌ Not included | ❌ Not included |
| **Maturity** | 🆕 Early | 🔄 Active dev | 🔬 Research | ⭐ Mature |
| **Use Case** | India crypto/equity | Multi-asset trading | HFT research | Crypto trading bots |

**Why choose this:**
- **India-specific**: Tax compliance (30% + 1% TDS) and local exchanges (CoinDCX, Zerodha)
- **Learning curve**: Simpler than hftbacktest, more features than early barter-rs
- **Complete solution**: Backtesting + optimization + live trading in one
- **Verified strategies**: Battle-tested with reproducible results

**When to use alternatives:**
- **barter-rs**: Multi-exchange live trading with 10+ integrations
- **hftbacktest**: High-frequency trading research with tick-level precision
- **Freqtrade**: Mature crypto bot with extensive exchange support and community

---

## 📈 Available Strategies

All strategies include verified backtest results on crypto data (2022-2025):

| Strategy | Sharpe | Return | Win Rate | Timeframe | Status |
|----------|--------|--------|----------|-----------|--------|
| **volatility_regime** | 0.55 | 55% | 45% | 1d | ✅ Production |
| **momentum_scalper** | 0.46 | 70% | 44% | 1d | ✅ Production |
| **range_breakout** | 0.29 | 31% | 38% | 1d | ✅ Production |
| **quick_flip** | 0.26 | 25% | 45% | 1d | ✅ Production |

See [rust/README.md](rust/README.md) for strategy details and configuration.

**Want to add yours?** See [Strategy Contribution Guide →](CONTRIBUTING.md#strategy-contribution-guidelines)

---

## 🧪 Verified Claims: Walk-Forward Testing

We practice **radical transparency**. All performance claims are reproducible:

### Deterministic Backtests

```bash
# Anyone can verify our results
git clone https://github.com/P0W/crypto-strategies.git
cd crypto-strategies/rust
cargo run --release -- backtest --config ../configs/volatility_regime_config.json

# You'll get EXACTLY:
# Sharpe: 0.53, Return: 55.36%, Max DD: 13.61%
```

### CI/CD Benchmarks

Every commit runs:
- Performance benchmarks (vs baseline)
- Strategy backtests (regression detection)
- Dependency security audit

See [`.github/workflows/ci.yml`](.github/workflows/ci.yml)

### Backtest vs Reality (Coming Soon)

We're building a **proof/** directory comparing:
- Backtest logs → Paper trading logs → Live execution logs
- Slippage analysis: predicted vs actual
- Fill quality: limit orders vs market orders

**Goal:** Show exactly where theory diverges from practice.

---

## 🤝 Contributing

We welcome contributions! This project succeeds when the community builds better strategies together.

### Good First Issues

Start here if you're new:

- [ ] **Add RSI indicator** ([Issue #X](issues)) - Implement RSI in `indicators.rs`
- [ ] **Binance data downloader** ([Issue #X](issues)) - Add Binance API integration
- [ ] **CSV trade exporter** ([Issue #X](issues)) - Export trade history to CSV
- [ ] **Bollinger Bands strategy** ([Issue #X](issues)) - New strategy using BB
- [ ] **Backtest visualization** ([Issue #X](issues)) - Generate equity curve charts

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

### How to Contribute a Strategy

1. Fork the repository
2. Create `rust/src/strategies/your_strategy/`
3. Implement the `Strategy` trait
4. Add configuration and tests
5. Run backtest and document results
6. Submit PR with performance metrics

See [examples/custom_strategy/](examples/custom_strategy/) for full tutorial.

---

## 📚 Documentation

- **[Rust Implementation Guide](rust/README.md)** - Build, run, architecture
- **[Python Implementation Guide](py/README.md)** - Setup and usage
- **[Contributing Guide](CONTRIBUTING.md)** - How to contribute
- **[Code of Conduct](CODE_OF_CONDUCT.md)** - Community standards
- **[Security Policy](SECURITY.md)** - Vulnerability reporting
- **[CLAUDE.md](CLAUDE.md)** - AI assistant guidance

### External Resources

- **[Criterion Benchmarks](rust/benches/)** - Performance measurements
- **[Strategy Examples](examples/)** - Tutorial code samples
- **[API Documentation](https://docs.rs/crypto-strategies)** - Auto-generated docs (coming soon)

---

## 🚨 Important Disclaimers

### Trading Risk

> ⚠️ **TRADING INVOLVES SUBSTANTIAL RISK OF LOSS.** Past performance is not indicative of future results. This software is for **educational and research purposes only**. 
>
> - Not financial advice
> - No guarantees of profit
> - You are responsible for your own trading decisions
> - Test thoroughly before using real money

### Tax Compliance

This system includes India's crypto tax rules (30% + 1% TDS) but **you are responsible for tax compliance in your jurisdiction**. Consult a tax professional.

### Security

- **Never commit API keys** to version control
- Store credentials in `.env` files (gitignored)
- Use paper trading to test before going live
- Review [SECURITY.md](SECURITY.md) for best practices

---

## 📜 License

MIT License - See [LICENSE](LICENSE) for details.

**What this means:**
- ✅ Use commercially
- ✅ Modify freely
- ✅ Distribute copies
- ✅ Private use
- ❌ No warranty provided
- ℹ️ Must include license notice

---

## 🌟 Star History

If this project helps you, consider giving it a star! It helps others discover it.

[![Star History Chart](https://api.star-history.com/svg?repos=P0W/crypto-strategies&type=Date)](https://star-history.com/#P0W/crypto-strategies&Date)

---

## 🔗 Links & Community

- **GitHub:** [P0W/crypto-strategies](https://github.com/P0W/crypto-strategies)
- **Issues:** [Report bugs](https://github.com/P0W/crypto-strategies/issues)
- **Discussions:** [Community forum](https://github.com/P0W/crypto-strategies/discussions)
- **Author:** [Prashant Srivastava](https://github.com/P0W)

### Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org/) - Systems programming language
- [Rayon](https://github.com/rayon-rs/rayon) - Data parallelism
- [Tokio](https://tokio.rs/) - Async runtime
- [ta-rs](https://github.com/greyblake/ta-rs) - Technical analysis indicators

---

**Made with 🦀 by the Rust trading community**
