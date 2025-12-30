# Crypto Strategies - Rust Implementation

High-performance Rust implementation of the Volatility Regime Adaptive Trading Strategy for CoinDCX.

## Why Rust?

- **Performance**: 10-100x faster backtests enabling thorough optimization
- **Type Safety**: Compile-time guarantees eliminate runtime type errors
- **Production Resilience**: Memory safety, no null pointer exceptions
- **Concurrency**: Safe parallelization with Rayon for optimization

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (1.70+)
- CoinDCX API credentials (for live trading)

## Setup

```bash
cd rust

# Build
cargo build --release

# Run tests
cargo test
```

### Environment Configuration

```bash
# Create .env from template (in repo root)
copy ..\.env.example ..\.env  # Windows
# cp ../.env.example ../.env  # Linux/Mac

# Add CoinDCX credentials to .env
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
```

## Usage

### Backtesting

```bash
# Run with default config
cargo run --release -- backtest

# With specific config
cargo run --release -- backtest --config ../configs/sample_config.json

# Override parameters
cargo run --release -- backtest --capital 100000 --start 2023-01-01 --end 2024-01-01

# Verbose output
cargo run --release -- backtest -v
```

### Optimization

```bash
# Quick optimization (fewer parameter combinations)
cargo run --release -- optimize --mode quick

# Full optimization (comprehensive grid search)
cargo run --release -- optimize --mode full

# Sort by different metrics
cargo run --release -- optimize --mode quick --sort-by calmar  # return/drawdown
cargo run --release -- optimize --mode quick --sort-by return  # raw return

# Show top N results
cargo run --release -- optimize --mode quick --top 20
```

**Sorting Options:**

| Option | Description | Best For |
|--------|-------------|----------|
| `sharpe` | Risk-adjusted return (default) | Overall performance |
| `calmar` | Return / Max Drawdown | Drawdown-sensitive |
| `return` | Raw total return | Maximum gains |
| `profit_factor` | Gross profits / Gross losses | Trade consistency |
| `win_rate` | Winning trades % | High-probability setups |

### Live Trading

```bash
# Paper trading (safe, simulated)
cargo run --release -- live --paper

# With verbose logging
cargo run --release -- live --paper -v

# Custom cycle interval (seconds)
cargo run --release -- live --paper --interval 300

# Live trading with real money (CAUTION!)
cargo run --release -- live --live
```

## Architecture

### Execution Modes

1. **Backtest** - Historical P&L simulation with comprehensive metrics
2. **Optimize** - Parallel parameter grid search using Rayon
3. **Live** - Real-time trading with state persistence

### Core Components

```
src/
├── main.rs              # CLI dispatch, logging setup
├── commands/            # Command implementations
│   ├── backtest.rs      # Backtest runner
│   ├── optimize.rs      # Parallel optimization
│   ├── live.rs          # Live trading loop
│   └── download.rs      # Data fetcher
├── strategies/          # Strategy implementations
│   └── volatility_regime/
│       ├── strategy.rs  # Signal generation
│       ├── config.rs    # Strategy parameters
│       └── grid_params.rs # Optimization ranges
├── backtest.rs          # Event-driven simulation engine
├── exchange.rs          # CoinDCX API client (circuit breaker, rate limiting)
├── risk.rs              # Position sizing, drawdown management
├── state_manager.rs     # SQLite persistence with JSON backup
├── indicators.rs        # ATR, EMA, ADX calculations
├── config.rs            # JSON configuration parsing
├── types.rs             # Domain model (Candle, Signal, Position, Trade)
└── data.rs              # CSV data loading and alignment
```

### Key Patterns

- **Strategy Trait**: Plugin architecture for multiple strategies
- **Circuit Breaker**: Fail-fast on API errors with auto-recovery
- **Rate Limiting**: Token bucket algorithm for API calls
- **Event-Driven Backtest**: Prevents lookahead bias
- **Parallel Optimization**: Work-stealing via Rayon

## Strategy: Volatility Regime

### Regime Classification

Based on ATR ratio (`current_ATR / median_ATR(lookback)`):

| Regime | ATR Ratio | Action |
|--------|-----------|--------|
| Compression | < 0.6 | Setup for breakout entry |
| Normal | 0.6-1.5 | Standard trend-following |
| Expansion | 1.5-2.5 | No new entries |
| Extreme | > 2.5 | Close all positions |

### Entry Conditions

All must be true:
- Regime is Compression or Normal
- EMA(8) > EMA(21) AND ADX > 30
- Close > (Recent High - 1.5×ATR)
- Risk manager allows entry

### Exit Strategy

- **Stop Loss**: 2.5× ATR below entry
- **Take Profit**: 5.0× ATR above entry (2:1 reward-risk)
- **Trailing Stop**: Activates at 50% profit, trails at 1.5× ATR
- **Regime Exit**: Immediate close if Extreme
- **Trend Exit**: Close if price < EMA(21) when profitable

## Risk Management

| Parameter | Value |
|-----------|-------|
| Risk Per Trade | 0.5% - 2% (dynamic) |
| Max Positions | 2 |
| Max Portfolio Heat | 10% |
| Max Drawdown | 20% (hard halt) |

### Drawdown-Based De-Risking

- 10% drawdown: Reduce position sizes by 50%
- 15% drawdown: Reduce position sizes by 75%
- 20% drawdown: Halt all trading

## State Persistence

SQLite-based with automatic JSON backup:

```
../state/
├── trading_state.db      # SQLite database (primary)
├── trading_state.json    # Auto JSON backup
└── final_state.json      # Export on graceful shutdown
```

**What's Persisted:**
- Open positions with entry details
- Portfolio checkpoints (value, cycle count, drawdown)
- Complete trade history
- Config hash for change detection

## Testing

```bash
cargo test                    # Run all tests
cargo test --release          # With optimizations
cargo test -- --nocapture     # Show output
cargo test risk::             # Test specific module
```

## Performance Tips

1. Always use `--release` for backtesting and optimization
2. Use `--mode quick` for iterative development
3. Limit date range with `--start` and `--end` for faster iteration
4. Check logs in `../logs/` for debugging

## License

MIT License
