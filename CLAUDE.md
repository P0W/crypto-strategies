# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Crypto Trading Strategy System** - A production-grade automated trading system for CoinDCX (Indian crypto exchange).

This is a **multi-language repository** with two implementations:

| Implementation | Directory | Language | Status |
|---------------|-----------|----------|--------|
| **Rust** | `rust/` | Rust | Production-ready (performance focus) |
| **Python** | `py/` | Python | Production-ready (prototyping focus) |

**Core Strategy**: Volatility Regime Adaptive Strategy (VRAS) exploiting volatility clustering and regime persistence inefficiencies in crypto markets.

## Repository Structure

```
crypto-strategies/
├── rust/                 # Rust implementation
│   ├── src/              # Source code
│   ├── tests/            # Integration tests
│   ├── Cargo.toml        # Rust dependencies
│   └── README.md         # Rust-specific docs
├── py/                   # Python implementation
│   ├── src/              # Source code
│   ├── pyproject.toml    # Python dependencies
│   └── README.md         # Python-specific docs
├── configs/              # Shared configuration files (JSON)
├── data/                 # Shared OHLCV data (CSV)
├── results/              # Backtest results
├── logs/                 # Trading logs
├── .env                  # API credentials
└── README.md             # Project overview
```

## Build & Run Commands

### Rust Implementation

```bash
cd rust

# Build
cargo build --release

# Run backtest
cargo run --release -- backtest --config ../configs/sample_config.json

# Run optimization
cargo run --release -- optimize --mode quick

# Run tests
cargo test
```

### Python Implementation

```bash
cd py

# Setup (using UV)
uv venv
.venv\Scripts\activate  # Windows
uv pip install -e .

# Run backtest
uv run backtest --config ../configs/sample_config.json

# Run optimization
uv run optimize --strategy volatility_regime --mode quick
```

### Environment Configuration
```bash
# Create .env from template (in repo root)
copy .env.example .env  # Windows
# cp .env.example .env  # Linux/Mac

# Add CoinDCX credentials to .env
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
```

## High-Level Architecture (Rust)

All Rust source files are in `rust/src/`.

### Three Execution Modes

1. **Backtest** (`rust/src/commands/backtest.rs`) - Historical P&L simulation
   - Loads OHLCV data → Runs event-driven simulation → Outputs performance metrics

2. **Optimize** (`rust/src/commands/optimize.rs`) - Parameter grid search
   - Generates parameter combinations → Runs parallel backtests → Ranks by Sharpe/Calmar/etc.

3. **Live** (`rust/src/commands/live.rs`) - Real-time trading
   - Paper or live mode → State persistence → Crash recovery

### Core Components

**Strategy Framework** (`rust/src/strategies/`)
- Trait-based plugin architecture: `Strategy` trait defines signal generation interface
- Current implementation: `volatility_regime/` - Exploits GARCH clustering via regime classification
- Easy to add new strategies by implementing the `Strategy` trait

**Risk Management** (`rust/src/risk.rs`)
- Multi-layer protection: position sizing, portfolio heat limits, drawdown-based de-risking
- Consecutive loss protection: reduces size after 3 losses
- Hard halt at 20% drawdown

**Backtesting Engine** (`rust/src/backtest.rs`)
- Event-driven simulation processing each candle chronologically
- Multi-symbol support with automatic data alignment
- Handles stop loss, take profit, and trailing stops
- Calculates comprehensive metrics: Sharpe, Calmar, max drawdown, win rate, profit factor

**Exchange Client** (`rust/src/exchange.rs`)
- Production-ready CoinDCX API wrapper with:
  - Circuit breaker pattern (fails fast after consecutive errors)
  - Exponential backoff retries (3 retries with jitter)
  - Rate limiting (token bucket algorithm via Semaphore)
  - HMAC-SHA256 request signing

**State Persistence** (`rust/src/state_manager.rs`)
- SQLite-based persistence with auto JSON backup
- Stores: open positions, portfolio checkpoints, trade history
- Enables crash recovery and maintains audit trail

**Data Management** (`rust/src/data.rs`)
- CSV-based OHLCV loading
- Multi-symbol alignment (finds common date range)
- Expected format: `datetime,open,high,low,close,volume`

### Key Architectural Patterns

**Type-Driven Design** (`rust/src/types.rs`)
- Core domain model: `Candle` → `Signal` → `Position` → `Trade` → `PerformanceMetrics`
- All types are serializable for persistence
- Strong type safety prevents data corruption

**Strategy Trait + Factory Pattern**
```rust
pub trait Strategy: Send + Sync {
    fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal;
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn update_trailing_stop(&self, position: &Position, current_price: f64, candles: &[Candle]) -> Option<f64>;
    fn notify_order(&mut self, order: &Order);
    fn notify_trade(&mut self, trade: &Trade);
    fn init(&mut self);
}
```

**Risk Manager as Central Arbiter**
- Before any trade entry, validates: trading not halted, within position limits, portfolio heat OK
- Returns position size adjusted for current drawdown and losing streaks

**Configuration Hierarchy** (`rust/src/config.rs`)
- JSON-based config structure:
  - `exchange`: fees, slippage, rate limits
  - `trading`: pairs, capital, risk limits, drawdown thresholds
  - `strategy_name`: which strategy to use
  - `strategy`: strategy-specific parameters (parsed into concrete types)
  - `tax`: India-specific tax rules (30% flat tax, 1% TDS)
  - `backtest`: data paths, date range

### Data Flow (Backtest Mode)

```
1. Load config from JSON + .env credentials
2. Load multi-symbol OHLCV data → HashMap<Symbol, Vec<Candle>>
3. Create strategy via factory (e.g., VolatilityRegimeStrategy)
4. Create Backtester with config + strategy
5. Backtester::run():
   a. Align data to common date range
   b. For each candle (chronologically):
      - For each symbol:
        * Query strategy for signal (Long/Short/Flat)
        * Check risk manager constraints
        * If signal == Long && no position: open position
        * If position exists: check stops, update trailing stop
      - Update equity curve
   c. Calculate performance metrics
6. Output BacktestResult with all stats
```

### Parallelization Strategy

**Optimizer** uses Rayon for parallel backtests:
- Each parameter combination runs independently
- Distributes across all CPU cores
- Use `--mode quick` for faster iteration, `--mode full` for comprehensive search

## Strategy: Volatility Regime

**Core Logic** (`strategies/volatility_regime/strategy.rs`):

1. **Regime Classification** (based on ATR ratio):
   - `atr_ratio = current_ATR / median_ATR(lookback)`
   - Compression: < 0.6 (volatility squeeze, setup for breakout)
   - Normal: 0.6-1.5 (standard trend-following)
   - Expansion: 1.5-2.5 (high volatility, no new entries)
   - Extreme: > 2.5 (danger zone, close positions)

2. **Entry Conditions** (ALL must be true):
   - Regime is Compression or Normal
   - Trend confirmation: EMA(8) > EMA(21) AND ADX > 30
   - Breakout trigger: Close > (Recent High - 1.5×ATR)
   - Risk manager allows entry

3. **Exit Strategy**:
   - Stop Loss: 2.5× ATR below entry
   - Take Profit: 5.0× ATR above entry (2:1 reward-risk)
   - Trailing Stop: Activates at 50% profit, trails at 1.5× ATR
   - Regime Exit: Immediate close if Extreme regime
   - Trend Exit: Close if price < EMA(21) (only if profitable)

**Configuration** (`rust/src/strategies/volatility_regime/config.rs`):
- All parameters are configurable via JSON `strategy` section
- Key params: `atr_period`, `volatility_lookback`, thresholds, EMA/ADX settings

## India-Specific Tax Compliance

The system includes India's crypto tax regime:
- 30% flat tax on all gains (no slab benefits)
- 1% TDS on every sell transaction
- No loss offset allowed

This ensures backtest results reflect post-tax reality. Target is 2:1 reward-risk with >50% win rate to overcome 30% tax drag.

## Multi-Language Repository Structure

This repository contains both Python and Rust implementations:
- `py/` - Original Python implementation
- `rust/` - Rust rewrite for performance

**Why Rust:**
- Type safety eliminates entire categories of bugs
- Performance: 10-100x faster backtests enable more thorough optimization
- Production resilience: No runtime errors from type mismatches
- Memory safety: No segfaults or use-after-free bugs

**Current Status:**
- ✅ Backtest mode: Production-ready
- ✅ Optimize mode: Production-ready
- ⚠️ Live mode: Core components ready (exchange client, state manager, risk controls), awaiting async event loop implementation

## Important Implementation Notes

### When Working on Strategies

1. All strategies must implement the `Strategy` trait
2. Strategies receive slices of candles (newest last) - use `.last()` for current bar
3. Signal generation should be stateless - all state in candles/position
4. Use `notify_order()` and `notify_trade()` hooks for logging/adaptation
5. Stop/target calculations use historical candles + entry price

### When Working on Risk Management

1. `RiskManager` is called BEFORE position entry - it can veto any trade
2. Drawdown is calculated as `(peak_capital - current_capital) / peak_capital`
3. Position sizing formula: `base_size × drawdown_multiplier × streak_multiplier`
4. Always check `should_halt_trading()` before allowing new positions

### When Working on Backtesting

1. Multi-symbol data MUST be aligned via `align_data()` - prevents lookahead bias
2. Candles are processed chronologically - no peeking ahead
3. Positions are stored in `HashMap<Symbol, Position>` - one position per symbol
4. Trade history accumulates in `Vec<Trade>` for metrics calculation
5. Equity curve tracks total portfolio value (cash + positions) at each bar

### When Working on Exchange Integration

1. `RobustCoinDCXClient` includes circuit breaker - failures trigger "open" state
2. All API calls go through rate limiter (Semaphore with permits/second)
3. Retries use exponential backoff with jitter to avoid thundering herd
4. HMAC-SHA256 signing required for all authenticated endpoints
5. Use `.await` properly - all methods are async (tokio runtime)

### When Working on State Persistence

1. SQLite is primary backend, JSON is backup
2. State includes: positions, checkpoints (cycle count, portfolio value), trade records
3. Config hash stored in checkpoint - warns on mismatch during recovery
4. Use transactions for atomicity when updating multiple tables

## Testing Conventions

Run tests with:
```bash
cargo test
cargo test --release  # With optimizations
cargo test -- --nocapture  # Show println! output
```

## Common Development Workflows

### Adding a New Strategy

1. Create module under `src/strategies/your_strategy/`
2. Implement `Strategy` trait
3. Add config struct with parameters
4. Create factory function in `mod.rs`
5. Update `main_backtest_cmd.rs` to recognize new strategy name
6. Add grid params in `grid_params.rs` for optimization

### Modifying Risk Rules (Rust)

1. Edit `rust/src/risk.rs::RiskManager`
2. Update `calculate_position_size()` or `should_halt_trading()` logic
3. Ensure config changes reflected in `rust/src/config.rs::TradingConfig`
4. Test with: `cd rust && cargo test risk::`

### Adding New Indicators (Rust)

1. Add function to `rust/src/indicators.rs`
2. Return Vec<f64> or single f64 value
3. Test edge cases (empty data, NaN handling)

### Debugging Backtests

1. Run with `-v` flag for debug logging
2. Check logs in `logs/backtest_{timestamp}.log`
3. Add `tracing::debug!()` statements in critical paths
4. Use `--start` and `--end` to isolate specific date ranges

## Architecture Decision Records

**Why Event-Driven Backtest?**
- Prevents lookahead bias (only current/historical data available)
- Mirrors live trading logic (same code path)
- Easy to add slippage/latency simulation

**Why Trait-Based Strategies?**
- Enables runtime polymorphism via `Box<dyn Strategy>`
- Allows config-driven strategy selection
- Easy to A/B test multiple strategies

**Why Circuit Breaker Pattern?**
- Fails fast during API outages
- Prevents cascading failures
- Auto-recovery via half-open state

**Why SQLite + JSON State?**
- SQLite for fast queries and transactions
- JSON backup for portability and debugging
- Dual persistence increases durability

**Why Rayon for Parallelization?**
- Dead-simple parallel iterators
- Work-stealing scheduler for load balance
- Integrates with Indicatif for progress bars

## Module Dependency Graph (Rust)

```
rust/src/main.rs (CLI dispatch, logging)
  ├─→ main_backtest_cmd.rs
  │     ├─→ config.rs
  │     ├─→ data.rs
  │     ├─→ backtest.rs
  │     │     ├─→ strategies/* (via Strategy trait)
  │     │     ├─→ risk.rs
  │     │     ├─→ indicators.rs
  │     │     └─→ types.rs
  │     └─→ strategies/volatility_regime/*
  │
  ├─→ main_optimize_cmd.rs
  │     ├─→ optimizer.rs
  │     │     └─→ backtest.rs (via parallel iter)
  │     └─→ strategies/volatility_regime/grid_params.rs
  │
  └─→ main_live_cmd.rs
        ├─→ exchange.rs (CoinDCX API client)
        ├─→ state_manager.rs (SQLite persistence)
        └─→ risk.rs

Shared Core:
  types.rs (domain model)
  config.rs (JSON parsing)
  indicators.rs (ATR, EMA, ADX, etc.)
```
