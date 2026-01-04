# Show r/rust: I built a crypto backtester that's 50x faster than Python

**TL;DR:** Rewrote my Python crypto trading backtester in Rust. Got 20-50x speedups, eliminated runtime errors, and can now optimize strategies in seconds instead of minutes. Open source, MIT licensed.

**GitHub:** https://github.com/P0W/crypto-strategies

---

## Motivation

I've been backtesting crypto strategies in Python (Backtrader) for 2 years. The workflow was painful:

- **Slow optimization:** Testing 100 parameter combinations = 7 minutes
- **Runtime errors:** Type errors only appear during execution
- **Memory hungry:** 300MB+ for a simple backtest
- **GIL bottleneck:** Can't truly parallelize

I wanted:
- ⚡ Fast iteration during strategy development
- 🛡️ Compile-time safety (no runtime surprises)
- 💻 Efficient memory usage
- 🔧 Production-ready live trading

So I rewrote it in Rust.

---

## What I Built

A complete trading system with:

1. **Backtesting engine** - Event-driven simulation (no lookahead bias)
2. **Strategy framework** - Trait-based plugins (add strategies without touching core)
3. **Optimizer** - Rayon-powered parallel grid search
4. **Live trading** - CoinDCX + Zerodha integration with circuit breakers
5. **Risk management** - Position sizing, drawdown limits, consecutive loss protection
6. **Multiple strategies** - Volatility regime, momentum scalper, range breakout, etc.

**Key architectural decision:** Make strategies pure trait implementations. Users shouldn't need to understand the engine to contribute.

```rust
pub trait Strategy {
    fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal;
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;
}
```

That's it. Implement these 3 methods, register in the factory, and you're done.

---

## Benchmarks

Compared to my Python implementation (Backtrader):

| Operation | Rust | Python | Speedup |
|-----------|------|--------|---------|
| Single backtest (1000 trades) | 0.24s | 4.8s | **20x** |
| Optimization (100 combinations) | 8.2s | 450s | **55x** |
| Memory usage | 28 MB | 340 MB | **12x less** |

Ran on M1 Mac, BTC+ETH+SOL data (2022-2025).

The optimization speedup is insane because Rayon just works - wrap your iterator in `.par_iter()` and boom, all cores utilized.

---

## Interesting Rust Learnings

**1. Trait-based plugins are elegant**

Python: Dynamic dispatch everywhere, runtime errors  
Rust: `Box<dyn Strategy>` gives you runtime polymorphism with compile-time safety

**2. Rayon makes parallelization trivial**

```rust
grid_combinations
    .par_iter()
    .map(|params| run_backtest(params))
    .collect()
```

That's it. 8 cores working in parallel. No GIL. No threading hell.

**3. Type safety catches bugs early**

Python: "Oh, I'm dividing by None" → Runtime crash during optimization  
Rust: Won't compile until you handle the `Option<f64>`

**4. Memory efficiency matters for large optimizations**

Python version would OOM on 1000+ parameter combinations. Rust handles it easily with minimal allocations.

**5. Async + sync hybrid works well**

Live trading uses `tokio` for API calls, but backtesting is pure sync. No async tax where you don't need it.

---

## Challenges

**CSV parsing is verbose**
- `csv` crate works but requires lots of boilerplate
- Ended up writing helper functions to reduce duplication

**Indicators library landscape**
- `ta-rs` exists but limited
- Had to implement some indicators manually
- Would love a comprehensive TA library like `pandas-ta`

**Error handling ergonomics**
- `anyhow` helped a lot
- Still learning when to use `Result` vs `panic!`
- Trading systems need graceful degradation, not crashes

**Testing floating-point math**
- Sharpe ratios, returns, etc. need `approx` crate
- Learning curve for proper FP testing

---

## What's Next

- **PyO3 bindings** - Write strategies in Python, execute in Rust
- **More exchanges** - Binance, Bybit integrations
- **Benchmark suite** - Criterion.rs for performance regression testing
- **Walk-forward validation** - Prove strategies aren't overfitted
- **Community strategies** - Accept PRs for new trading ideas

---

## Code Highlights

**Plugin registration** (strategies/mod.rs):
```rust
pub fn get_registry() -> HashMap<&'static str, StrategyFactory> {
    let mut map = HashMap::new();
    map.insert("volatility_regime", volatility_regime::create);
    map.insert("momentum_scalper", momentum_scalper::create);
    // Users add their own here
    map
}
```

**Parallel optimization** (optimizer.rs):
```rust
results = grid_combinations
    .par_iter()
    .progress_count(total_combinations)
    .map(|params| {
        let mut config = base_config.clone();
        apply_params(&mut config, params);
        run_backtest(config)
    })
    .collect();
```

**Risk management** (risk.rs):
```rust
pub fn calculate_position_size(&self, equity: f64) -> f64 {
    let base_size = equity * self.config.risk_per_trade;
    let dd_multiplier = self.drawdown_multiplier();
    let streak_multiplier = self.streak_multiplier();
    base_size * dd_multiplier * streak_multiplier
}
```

---

## Try It

```bash
git clone https://github.com/P0W/crypto-strategies.git
cd crypto-strategies/rust
cargo build --release
cargo run --release -- backtest --config ../configs/sample_config.json
```

You'll see backtest results in ~5 seconds. Try the optimizer to see Rayon in action.

---

## Feedback Welcome

This is my first serious Rust project. I'd love feedback on:

- Architecture decisions (trait-based strategies good? better alternatives?)
- Error handling patterns (am I over-using `unwrap()`?)
- Performance optimizations (where can I improve?)
- API ergonomics (is the config structure sane?)
- Idiomatic Rust (where am I fighting the borrow checker unnecessarily?)

Also happy to discuss trading strategy ideas, backtesting methodology, or anything quant-related.

**Repo:** https://github.com/P0W/crypto-strategies  
**License:** MIT  
**Disclaimer:** Educational/research software. Not financial advice. Trade at your own risk.

---

## Stats

- 🦀 ~6000 lines of Rust
- 📈 4 strategies implemented
- 🧪 Integration tests passing
- 📊 Backtested on 3+ years of crypto data
- 🚀 Production-ready live trading

Thanks for reading! Always happy to discuss Rust, trading, or how to make backtesting less painful.
