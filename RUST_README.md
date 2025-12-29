# Crypto Trading Strategies - Rust Implementation

A production-grade automated trading system for cryptocurrency markets, rewritten in Rust for maximum performance, correctness, and reliability.

## Features

- **High Performance**: Written in Rust for speed and efficiency
- **Type Safety**: Compile-time guarantees for correctness
- **Volatility Regime Strategy**: Exploits volatility clustering and regime persistence
- **Comprehensive Backtesting**: Event-driven backtest engine with realistic commission/slippage modeling
- **Parameter Optimization**: Parallel grid search using Rayon
- **Well-Maintained Dependencies**: Uses stable, popular crates (serde, polars, tokio, clap, etc.)
- **Extensible Design**: Trait-based strategy framework for easy customization

## Architecture

The system is built with modularity and extensibility in mind:

- `Config`: JSON-based configuration with environment variable support
- `Strategy` trait: Define custom strategies by implementing this trait
- `RiskManager`: Portfolio-level risk controls and position sizing
- `Backtester`: Event-driven backtesting engine
- `Optimizer`: Parallel parameter optimization

## Installation

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs/))
- Historical OHLCV data in CSV format

### Build

```bash
# Debug build
cargo build

# Optimized release build (recommended for backtesting/optimization)
cargo build --release
```

## Usage

### Backtesting

```bash
# Run backtest with default config
./target/release/backtest

# Use custom config file
./target/release/backtest --config configs/btc_eth_sol_bnb_xrp_1d.json

# Set custom initial capital
./target/release/backtest --capital 200000

# Verbose output showing all trades
./target/release/backtest --verbose

# Custom date range
./target/release/backtest --start 2023-01-01 --end 2024-01-01
```

### Parameter Optimization

```bash
# Quick optimization (fewer combinations, faster)
./target/release/optimize --mode quick

# Full optimization (comprehensive grid search)
./target/release/optimize --mode full

# Sort by different metrics
./target/release/optimize --mode quick --sort-by calmar
./target/release/optimize --mode quick --sort-by return

# Show top N results
./target/release/optimize --mode quick --top 20
```

### Live Trading

```bash
# Paper trading (safe, no real money)
./target/release/live --paper

# Live trading (CAUTION - REAL MONEY!)
./target/release/live --live

# Custom check interval
./target/release/live --paper --interval 300
```

## Configuration

Configuration files are in JSON format. See `configs/btc_eth_sol_bnb_xrp_1d.json` for an example.

### Environment Variables

Create a `.env` file for API credentials:

```env
COINDCX_API_KEY=your_api_key_here
COINDCX_API_SECRET=your_api_secret_here
```

## Data Format

Place OHLCV CSV files in the `data/` directory with the format:

```
{SYMBOL}_{TIMEFRAME}.csv
```

Example: `BTCINR_1d.csv`

CSV format:
```csv
datetime,open,high,low,close,volume
2024-01-01 00:00:00,5000000,5050000,4980000,5020000,100
```

## Performance Metrics

The backtest engine calculates:

- Total Return
- Sharpe Ratio (risk-adjusted return)
- Calmar Ratio (return/max drawdown)
- Maximum Drawdown
- Win Rate
- Profit Factor
- Average Win/Loss
- Largest Win/Loss

## Strategy Customization

To create a new strategy:

1. Implement the `Strategy` trait:

```rust
pub trait Strategy: Send + Sync {
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal;

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn update_trailing_stop(&self, position: &Position, current_price: f64, candles: &[Candle]) -> Option<f64>;
}
```

2. Add your strategy to the binary files
3. Rebuild and test

## Technical Indicators

The system includes implementations of:

- SMA (Simple Moving Average)
- EMA (Exponential Moving Average)
- ATR (Average True Range)
- ADX (Average Directional Index)
- Bollinger Bands
- RSI (Relative Strength Index)

## Risk Management

Built-in risk controls:

- Per-trade risk limits
- Position size limits
- Portfolio heat management
- Drawdown-based de-risking
- Consecutive loss protection

## Crates Used

- **serde/serde_json**: Configuration and serialization
- **polars**: High-performance data processing
- **tokio**: Async runtime for API calls
- **reqwest**: HTTP client for exchange API
- **clap**: Command-line argument parsing
- **chrono**: Date/time handling
- **rayon**: Parallel computation for optimization
- **anyhow**: Error handling
- **statrs**: Statistical functions

All crates are well-maintained and widely used in the Rust ecosystem.

## Advantages Over Python Version

1. **Performance**: 10-100x faster backtesting and optimization
2. **Memory Safety**: No runtime errors from null pointers or data races
3. **Type Safety**: Catch errors at compile time
4. **Concurrency**: Safe parallel processing with Rayon
5. **Small Binary**: Single executable with no dependencies
6. **Cross-Platform**: Compile for Linux, macOS, Windows easily

## Building for Production

```bash
# Optimized release build with all optimizations
cargo build --release

# Strip symbols for smaller binary
strip target/release/backtest
strip target/release/optimize
strip target/release/live

# Binary is ready to deploy
ls -lh target/release/{backtest,optimize,live}
```

## Disclaimer

This software is for educational purposes only. Cryptocurrency trading involves substantial risk of loss. Past performance does not guarantee future results. Always conduct your own research and consult with financial professionals before trading.

## License

MIT License - See LICENSE file for details.

## Author

Prashant Srivastava

---

## Migration from Python

The Rust version maintains feature parity with the Python version while offering:

- Faster execution
- Better resource utilization
- Compile-time safety guarantees
- No runtime dependencies
- Easier deployment

The configuration format and data format remain the same for easy migration.
