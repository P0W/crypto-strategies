# Strategy Organization

This directory contains all trading strategies organized in a modular structure.

## Structure

Each strategy is contained in its own subdirectory with the following components:

```
strategies/
└── <strategy_name>/
    ├── mod.rs          # Module exports
    ├── strategy.rs     # Strategy implementation
    ├── config.rs       # Strategy-specific configuration
    ├── grid_params.rs  # Grid search parameter ranges
    └── utils.rs        # Helper functions for instantiation
```

## Available Strategies

### Volatility Regime (`volatility_regime/`)

Exploits volatility clustering and regime persistence in crypto markets.

**Key Features:**
- Regime classification (Compression, Normal, Expansion, Extreme)
- Trend confirmation with EMA and ADX
- Breakout detection
- Adaptive position sizing based on volatility

**Configuration:**
- ATR period and lookback
- EMA fast/slow periods
- ADX threshold
- Stop loss and take profit ATR multiples
- Trailing stop parameters

**Grid Search:**
- Quick mode: ~16 combinations
- Full mode: ~500+ combinations

## Adding a New Strategy

To add a new strategy:

1. Create a new directory: `strategies/<strategy_name>/`

2. Implement required files:
   - `strategy.rs`: Implement the `Strategy` trait
   - `config.rs`: Define strategy-specific configuration
   - `grid_params.rs`: Define parameter ranges for optimization
   - `utils.rs`: Helper functions for instantiation
   - `mod.rs`: Export public components

3. Add the module to `strategies/mod.rs`:
   ```rust
   pub mod <strategy_name>;
   ```

4. Use in binaries via the strategy's utility functions:
   ```rust
   use crypto_strategies::strategies::<strategy_name>;
   
   let strategy = <strategy_name>::create_strategy_from_config(&config);
   ```

## Strategy Trait

All strategies must implement:

```rust
pub trait Strategy: Send + Sync {
    fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal;
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn update_trailing_stop(&self, position: &Position, current_price: f64, candles: &[Candle]) -> Option<f64>;
}
```

## Optimization

Each strategy provides:
- `GridParams` for defining parameter ranges
- `generate_configs()` for creating all parameter combinations
- `config_to_params()` for result reporting

The generic optimizer in the root handles parallel execution.

## Example Usage

### Backtesting

```bash
./target/release/backtest --config configs/btc_eth_sol_bnb_xrp_1d.json
```

### Optimization

```bash
./target/release/optimize --mode quick
./target/release/optimize --mode full --sort-by calmar
```

The optimizer automatically uses the strategy specified in the config file.
