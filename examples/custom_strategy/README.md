# Creating Your First Trading Strategy

This tutorial shows you how to create a custom trading strategy in 10 minutes without modifying the core engine.

## The Strategy Trait

All strategies implement the `Strategy` trait:

```rust
pub trait Strategy {
    fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal;
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;
}
```

## Example: Moving Average Crossover

Let's build a simple MA crossover strategy step-by-step.

### Step 1: Create your strategy files in `rust/src/strategies/ma_crossover/`

### Step 2: Implement the Strategy trait

```rust
impl Strategy for MACrossoverStrategy {
    fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal {
        let fast_ma = sma(candles, self.config.fast_period);
        let slow_ma = sma(candles, self.config.slow_period);
        
        if fast_ma > slow_ma && position.is_none() {
            Signal::Long  // Golden cross
        } else if fast_ma < slow_ma && position.is_some() {
            Signal::Flat  // Death cross
        } else {
            Signal::Flat
        }
    }
}
```

### Step 3: Register and Run

Register in `strategies/mod.rs`, create a config file, and run:

```bash
cargo run --release -- backtest --config ../configs/ma_crossover.json
```

See the [full tutorial](README_FULL.md) for complete implementation details.

## Resources

- [Full Tutorial](README_FULL.md) - Complete step-by-step guide
- [Strategy Trait Docs](../../rust/src/strategies/mod.rs)
- [Example Strategies](../../rust/src/strategies/)
- [Contributing Guide](../../CONTRIBUTING.md)
