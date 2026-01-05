# Show HN: Fast crypto backtester in Rust (50x faster than Python, open source)

**Link:** https://github.com/P0W/crypto-strategies

I've been algo trading crypto for 2 years using Python (Backtrader). The optimization loop was painfully slow - testing 100 parameter combinations took 7+ minutes. So I rewrote everything in Rust.

**Results:**
- 20-50x faster backtesting
- Parallel optimization uses all cores (Rayon)
- Compile-time safety eliminated entire classes of bugs
- 28 MB memory vs 340 MB in Python
-94% returns on volatility regime strategy (reproducible, code + data included)

**Key features:**
- Event-driven backtesting (no lookahead bias)
- Trait-based strategy plugins (add strategies without touching core)
- Live trading support (CoinDCX, Zerodha)
- Multi-timeframe analysis
- India crypto tax compliance (30% + 1% TDS)

**Tech decisions:**
- `Box<dyn Strategy>` for runtime strategy selection
- Rayon's `.par_iter()` for zero-effort parallelization
- Circuit breakers + rate limiting for production
- SQLite for state persistence

**Interesting challenge:** Balancing type safety with plugin flexibility. Ended up with trait objects for strategies, which works well but has some dynamic dispatch cost. Worth it for the ergonomics.

The backtest results are fully reproducible - clone the repo, run one command, get exact same numbers. Includes 3 years of crypto data.

Built this because I was frustrated with Python's GIL, runtime errors during long optimizations, and memory bloat. Rust solved all three.

**Open source (MIT).** Would love feedback on architecture, performance optimizations, or trading strategy ideas.
