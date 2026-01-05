# Frequently Asked Questions (FAQ)

## General Questions

### What is this project?

A high-performance crypto trading backtester and live trading engine built in Rust. It's designed for serious algorithmic traders who need speed, type safety, and production-ready features.

### Why Rust instead of Python?

- **Speed**: 10-100x faster for backtesting and optimization
- **Type Safety**: Catch bugs at compile time, not in production
- **Memory Efficiency**: 12x less memory usage
- **Concurrency**: True parallelism without GIL limitations
- **Reliability**: No runtime type errors or memory leaks

See the [benchmark comparison](README.md#benchmark-rust-vs-python) in the main README.

### Can I use this for real money trading?

**Yes, but with extreme caution.** The system supports live trading through CoinDCX and Zerodha APIs. However:

- ⚠️ **Start with paper trading** to verify everything works
- ⚠️ **Test extensively** with small amounts first
- ⚠️ **You are responsible** for your trading decisions
- ⚠️ **Past performance ≠ future results**

See [SECURITY.md](SECURITY.md) for production deployment guidelines.

### Is this financial advice?

**No.** This is educational/research software. We provide tools, not investment advice. You are solely responsible for your trading decisions.

---

## Getting Started

### What do I need to get started?

**Minimum requirements:**
- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Historical data (included in `data/` directory)
- Config file (examples in `configs/` directory)

**Optional:**
- API keys for live trading (CoinDCX, Zerodha)
- Python for hybrid usage (coming soon)

### How do I run my first backtest?

```bash
git clone https://github.com/P0W/crypto-strategies.git
cd crypto-strategies/rust
cargo build --release
cargo run --release -- backtest --config ../configs/sample_config.json
```

You'll see backtest results in ~5-10 seconds.

### Where do I get historical data?

**Option 1: Use included data**
- The `data/` directory has sample OHLCV files for BTC, ETH, SOL, BNB, XRP

**Option 2: Download more data**
```bash
cargo run -- download --symbols BTC,ETH --timeframes 1h,1d --days 180
```

**Option 3: Use your own CSV files**
- Format: `datetime,open,high,low,close,volume`
- Place in `data/` directory
- Update config to point to your files

### Can I backtest forex or stocks?

**Yes!** The system is asset-agnostic. It works with:
- Cryptocurrencies (CoinDCX API included)
- Stocks (Zerodha Kite API included)
- Forex (bring your own data + API)
- Commodities (bring your own data + API)

Just ensure your data format matches the expected CSV structure.

---

## Strategy Questions

### How do I create a custom strategy?

See the [10-minute tutorial](examples/custom_strategy/README.md). The basic steps:

1. Create `rust/src/strategies/your_strategy/` directory
2. Implement the `Strategy` trait
3. Register in `strategies/mod.rs`
4. Create a config file
5. Run backtest

### Do I need to modify the core engine to add a strategy?

**No!** The engine uses a trait-based plugin system. You only need to:
- Implement the `Strategy` trait in your module
- Register it in the strategy factory
- Provide a config file

The backtesting engine, risk management, and data handling remain untouched.

### What indicators are available?

25+ technical indicators in `rust/src/indicators.rs`:

**Trend:** SMA, EMA, WMA, DEMA, TEMA, KAMA  
**Momentum:** RSI, MACD, Stochastic, CCI, ROC, Williams %R  
**Volatility:** ATR, Bollinger Bands, Keltner Channels, Standard Deviation  
**Volume:** OBV, VWAP, MFI  
**Other:** ADX, Aroon, Ichimoku Cloud, Parabolic SAR

Missing something? [Open an issue](https://github.com/P0W/crypto-strategies/issues) or submit a PR!

### Can I use multiple timeframes?

**Yes!** Strategies can declare required timeframes:

```rust
impl Strategy for MyStrategy {
    fn required_timeframes(&self) -> Vec<String> {
        vec!["1d".to_string(), "1h".to_string(), "5m".to_string()]
    }
}
```

The engine automatically fetches and aligns data across timeframes.

### How do I optimize strategy parameters?

Use the built-in optimizer with Rayon parallelization:

```bash
cargo run --release -- optimize --config your_config.json
```

Define parameter grid in your config:
```json
{
  "grid": {
    "ema_fast": [8, 13, 21],
    "ema_slow": [21, 34, 55],
    "stop_atr_multiple": [2.0, 2.5, 3.0]
  }
}
```

The optimizer tests all combinations in parallel and reports top results.

---

## Performance Questions

### How fast is it really?

**Backtest benchmarks** (1000 trades, 3 years of data):
- Rust: 0.24s
- Python (Backtrader): 4.8s → **20x faster**
- Python (Zipline): 8.1s → **34x faster**

**Optimization benchmarks** (100 parameter combinations):
- Rust: 8.2s (uses all CPU cores)
- Python: 450s → **55x faster**

Run `cargo bench` to verify on your machine.

### Why is optimization so much faster?

1. **Rayon parallelization** - Uses all CPU cores automatically
2. **Zero-copy data** - No GIL, true parallelism
3. **Compiled code** - No interpreter overhead
4. **Memory efficiency** - Minimal allocations

### Can I make it even faster?

**Yes:**
- Use `--release` flag (already 100x faster than debug)
- Enable native CPU features: `RUSTFLAGS="-C target-cpu=native" cargo build --release`
- Use binary data format instead of CSV (future enhancement)
- Reduce timeframe resolution for faster iteration

---

## Technical Questions

### What exchange APIs are supported?

**Currently supported:**
- **CoinDCX** (India crypto exchange) - Full trading support
- **Zerodha Kite** (India stock exchange) - Full trading support
- **Binance** (Data download only, no trading)

**Coming soon:**
- Binance (trading)
- Bybit
- Others via community contributions

### How does risk management work?

Multi-layer protection:
1. **Position sizing** - Based on risk per trade (default 15%)
2. **Portfolio heat** - Max total exposure (default 30%)
3. **Drawdown limits** - Automatic position size reduction
4. **Consecutive loss protection** - Reduce size after 3 losses
5. **Hard halt** - Stop trading at 20% drawdown

See `rust/src/risk.rs` for implementation.

### What about taxes?

India's crypto tax rules (30% + 1% TDS) are built-in. For other jurisdictions:
- Update `tax.tax_rate` and `tax.tds_rate` in config
- Set `tax.loss_offset_allowed` appropriately
- Consult a tax professional

### How is data stored?

- **OHLCV data** - CSV files in `data/` directory
- **Backtest results** - JSON in `results/` directory
- **Trading state** - SQLite database for live trading
- **Logs** - Text files in `logs/` directory

All data stays local. No cloud dependencies.

### Is the backtester accurate?

We use industry-standard practices:
- **Event-driven simulation** - No lookahead bias
- **T+1 execution** - Orders placed on day T execute at T+1 open
- **Slippage modeling** - Configurable assumed slippage
- **Fee accounting** - Maker/taker fees applied to all trades
- **Tax compliance** - Post-tax returns calculated

**Limitations:**
- Market impact not modeled (assumes small orders)
- Liquidity constraints not enforced
- Partial fills not simulated
- Real execution may vary (see proof/ directory when available)

---

## Python Integration

### Can I use this from Python?

**Coming soon!** We're building PyO3 bindings to expose the Rust engine to Python:

```python
import crypto_strategies

# Write your strategy in Python
class MyStrategy(crypto_strategies.Strategy):
    def generate_signal(self, candles):
        # Your Python logic here
        return "long"

# Execute in Rust for speed
result = crypto_strategies.backtest(MyStrategy(), config)
```

This gives you Python's ease + Rust's speed.

### Should I use the Python version?

**Current Python version:**
- ✅ Good for learning and prototyping
- ✅ Easier for beginners
- ❌ 20-50x slower than Rust
- ❌ Limited to single-core optimization

**Recommendation:** Start with Rust if you're serious about performance. Use Python for quick experiments.

---

## Troubleshooting

### Build fails with "linker error"

**Linux:**
```bash
sudo apt-get install build-essential
```

**macOS:**
```bash
xcode-select --install
```

**Windows:**
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/)
- Or use [rustup](https://rustup.rs/) installer which handles this

### "No such file or directory" error when running backtest

Check your config file paths:
```json
{
  "backtest": {
    "data_dir": "../data",  // Relative to rust/ directory
    "results_dir": "../results"
  }
}
```

Paths are relative to where you run the command from.

### Backtest results don't match docs

Possible reasons:
1. Different data files (prices may vary by source)
2. Different config parameters
3. Different random seed (if applicable)
4. Version mismatch (use latest main branch)

Try the exact config from `configs/sample_config.json` first.

### Out of memory during optimization

Reduce the parameter grid size:
```json
{
  "grid": {
    "param1": [1, 2, 3],     // Instead of [1,2,3,4,5,6,7,8,9]
    "param2": [10, 20, 30]   // Fewer combinations
  }
}
```

Or run on a machine with more RAM.

### Live trading disconnects/errors

Check:
- API keys are valid and have trading permissions
- Network connection is stable
- Exchange API is not down
- You're not hitting rate limits
- Logs in `logs/` directory for detailed errors

Always test with `--paper` flag first!

---

## Contributing

### How can I contribute?

Many ways:
- **Add a strategy** - Share your edge with the community
- **Add indicators** - Implement missing technical indicators
- **Improve docs** - Tutorials, examples, translations
- **Fix bugs** - Check the issues list
- **Add tests** - Improve code coverage
- **Add exchanges** - Integrate new APIs

See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed guidelines.

### What makes a good strategy contribution?

1. **Clear edge hypothesis** - Explain why it should work
2. **Backtested results** - Minimum 2 years of data
3. **Documented parameters** - What each setting does
4. **Tests included** - Verify signal generation
5. **Example config** - Ready to run
6. **Not overfitted** - Works on multiple symbols/timeframes

See [Strategy Contribution Guide](CONTRIBUTING.md#strategy-contribution-guidelines).

### Do I need to know Rust?

**For using:** No, just copy/modify existing configs  
**For simple strategies:** Basic Rust (see [Rust Book](https://doc.rust-lang.org/book/))  
**For core changes:** Intermediate Rust knowledge helps

We welcome all skill levels. "Good first issue" label marks beginner-friendly tasks.

---

## License & Legal

### What license is this under?

MIT License - Free for commercial use, modification, distribution. No warranty provided. See [LICENSE](LICENSE).

### Can I use this commercially?

**Yes!** MIT license allows commercial use. You can:
- Run it for your own trading
- Build a product on top of it
- Sell strategies or services using it
- Fork and monetize it

Just include the original license notice.

### What are the disclaimers?

- **No financial advice** - Educational/research software only
- **No warranty** - Use at your own risk
- **Trading risk** - You can lose money
- **No guaranteed returns** - Past performance ≠ future results
- **Tax compliance** - You are responsible
- **Security** - Secure your API keys

See full disclaimers in [README.md](README.md#important-disclaimers).

---

## Still have questions?

- **Check the docs:** [README.md](README.md), [rust/README.md](rust/README.md)
- **Search issues:** [GitHub Issues](https://github.com/P0W/crypto-strategies/issues)
- **Ask the community:** [GitHub Discussions](https://github.com/P0W/crypto-strategies/discussions)
- **Open a new issue:** Use the [question template](https://github.com/P0W/crypto-strategies/issues/new)

---

**Last updated:** January 2026
