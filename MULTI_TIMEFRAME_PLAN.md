# Multi-Timeframe Backtest Engine - Implementation Plan

## Overview
Design and implement multi-timeframe support while maintaining single-timeframe backward compatibility.

## Phase 1: Core Data Structures ✅ COMPLETE
- [x] Create `MultiTimeframeData` struct for per-symbol multi-TF data
- [x] Create `MultiTimeframeCandles` for windowed strategy access
- [x] Create alignment functions
- [x] Add to lib.rs exports

## Phase 2: Data Loading Enhancement (IN PROGRESS)
- [ ] Update `data::load_multi_symbol_with_range()` to support multiple timeframes
- [ ] Add `data::load_multi_timeframe()` function
- [ ] Support timeframe list in config: `"timeframes": ["1d", "15m", "5m"]`
- [ ] Auto-detection: if single `"timeframe"` provided, use single-TF mode
- [ ] Parallel loading of timeframes (use rayon)

## Phase 3: Strategy Trait Update
- [ ] Add optional method to Strategy trait:
  ```rust
  fn required_timeframes(&self) -> Vec<&'static str> {
      vec![] // Empty = use primary only (backward compat)
  }
  ```
- [ ] Update `generate_signal()` signature (breaking change):
  ```rust
  // Old:
  fn generate_signal(&self, symbol: &Symbol, candles: &[Candle], position: Option<&Position>) -> Signal;
  
  // New:
  fn generate_signal(&self, symbol: &Symbol, mtf_candles: &MultiTimeframeCandles, position: Option<&Position>) -> Signal;
  ```
- [ ] Add backward-compat helper: `mtf_candles.primary()` returns primary TF candles

## Phase 4: Backtest Engine Update
- [ ] Update `Backtester::run()` to accept `MultiSymbolMultiTimeframeData`
- [ ] Create windowed `MultiTimeframeCandles` views in main loop
- [ ] Pass MTF views to strategy instead of single candle slice
- [ ] Maintain current O(n*k) performance (MAX_LOOKBACK windowing)

## Phase 5: Update All Existing Strategies
All strategies need minimal changes:
- [ ] volatility_regime
- [ ] mean_reversion
- [ ] momentum_scalper
- [ ] range_breakout
- [ ] vwap_scalper
- [ ] quick_flip

Change pattern for each:
```rust
// Change signature
fn generate_signal(&self, symbol: &Symbol, mtf_candles: &MultiTimeframeCandles, position: Option<&Position>) -> Signal {
    // Get primary timeframe candles (backward compat)
    let candles = mtf_candles.primary();
    
    // Rest of code unchanged...
}
```

## Phase 6: Update Commands
- [ ] Update `commands/backtest.rs` to:
  - Load multi-timeframe data if config has `"timeframes"` array
  - Otherwise load single timeframe (backward compat)
  - Pass correct data type to Backtester
- [ ] Update `optimizer.rs` similarly

## Phase 7: Quick Flip Multi-Timeframe Implementation
Update Quick Flip to use true multi-TF:
```rust
fn required_timeframes(&self) -> Vec<&'static str> {
    vec!["1d", "15m"] // Declare dependencies
}

fn generate_signal(&self, symbol: &Symbol, mtf_candles: &MultiTimeframeCandles, position: Option<&Position>) -> Signal {
    // Get daily candles for ATR
    let daily = mtf_candles.get("1d").unwrap();
    let daily_atr = calculate_atr(daily, 14);
    
    // Get 15m candles for range box
    let candles_15m = mtf_candles.get("15m").unwrap();
    let (range_high, range_low) = get_opening_range(candles_15m);
    
    // Get 5m (primary) for pattern detection
    let candles_5m = mtf_candles.primary();
    
    // ... rest of logic
}
```

## Phase 8: Configuration Examples
### Single-Timeframe (Backward Compatible)
```json
{
  "strategy": {
    "timeframe": "5m"
  }
}
```

### Multi-Timeframe
```json
{
  "strategy": {
    "timeframe": "5m",          // Primary (for iteration)
    "timeframes": ["1d", "15m", "5m"]  // All required TFs
  }
}
```

## Performance Considerations
- Lazy loading: Only load TFs that strategy actually requests
- Parallel loading: Use rayon for simultaneous TF downloads
- Memory: Each TF stored once, windowed views are zero-copy slices
- Current O(n*k) complexity maintained (k=MAX_LOOKBACK=300)

## Testing Strategy
1. Unit tests for MultiTimeframeData
2. Integration test: Load multi-TF data for BTC
3. Backtest test: Run single-TF strategy (ensure no regression)
4. Backtest test: Run multi-TF Quick Flip strategy
5. Performance benchmark: Compare single-TF vs multi-TF overhead

## Migration Path
1. Phases 1-4: Core engine changes (breaking for strategies)
2. Phase 5: Update all strategies (one commit per strategy)
3. Phase 6: Update commands
4. Phase 7: Implement Quick Flip multi-TF
5. Phase 8: Documentation and examples

## Estimated Effort
- Phase 1: ✅ Done (30 min)
- Phase 2: 45 min
- Phase 3: 30 min
- Phase 4: 60 min
- Phase 5: 90 min (15 min × 6 strategies)
- Phase 6: 30 min
- Phase 7: 45 min
- Phase 8: 15 min

**Total: ~5.5 hours of focused development**

## Current Status
- ✅ Phase 1 Complete: Core data structures created and compiling
- 🔄 Next: Phase 2 - Data loading enhancement
