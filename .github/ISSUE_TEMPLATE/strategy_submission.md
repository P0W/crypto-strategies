---
name: Strategy Submission
about: Submit a new trading strategy for inclusion
title: '[STRATEGY] '
labels: strategy, enhancement
assignees: ''
---

## Strategy Overview

**Strategy Name**: 

**Brief Description**: 

One-paragraph summary of the strategy.

## Edge Hypothesis

**What market inefficiency does this exploit?**

Explain the theoretical edge:
- Market behavior being exploited
- Why this edge exists
- When/where this edge is strongest

## Strategy Logic

**Entry Conditions**:
1. Condition 1
2. Condition 2
3. ...

**Exit Conditions**:
- Stop Loss: ...
- Take Profit: ...
- Trailing Stop: ...
- Other exits: ...

**Position Sizing**:
- Fixed size / ATR-based / Other: ...

## Configuration Parameters

```json
{
  "strategy": {
    "name": "your_strategy_name",
    "parameter1": "value",
    "parameter2": "value",
    "parameter3": "value"
  }
}
```

**Parameter Descriptions**:
- `parameter1`: Description and recommended range
- `parameter2`: Description and recommended range
- `parameter3`: Description and recommended range

## Backtest Results

**Test Period**: YYYY-MM-DD to YYYY-MM-DD

**Symbols Tested**: BTC, ETH, SOL, etc.

**Timeframe**: 1h / 4h / 1d

**Performance Metrics**:
- **Total Return**: X.X%
- **Sharpe Ratio**: X.XX
- **Max Drawdown**: X.X%
- **Win Rate**: XX%
- **Total Trades**: XX
- **Profit Factor**: X.XX
- **Calmar Ratio**: X.XX
- **Expectancy**: ₹XXX

**Equity Curve** (if available):
<!-- Attach image or link to chart -->

## Robustness Testing

**Different Market Conditions**:
- [ ] Bull market: Result X
- [ ] Bear market: Result Y
- [ ] Sideways market: Result Z

**Multiple Symbols**:
- [ ] BTC: Sharpe X.XX
- [ ] ETH: Sharpe X.XX
- [ ] Other: Sharpe X.XX

**Parameter Sensitivity**:
- [ ] Tested parameter variations
- [ ] Results remain stable (not overfitted)

## Code Implementation

**Branch/Fork**: Link to your implementation

**Files Changed**:
- `src/strategies/your_strategy/mod.rs`
- `src/strategies/your_strategy/config.rs`
- `src/strategies/your_strategy/strategy.rs`
- `tests/your_strategy_tests.rs`
- Config example in `configs/`

**Tests Included**:
- [ ] Unit tests for signal generation
- [ ] Integration test with backtest
- [ ] Edge case handling

## Risk Characteristics

**Maximum Drawdown**: X%

**Typical Holding Period**: X hours/days

**Correlation with Other Strategies**: Low / Medium / High

**Market Conditions**:
- Best in: High volatility / Trending / Ranging
- Avoid in: ...

**Known Limitations**:
- Limitation 1
- Limitation 2

## Documentation

**README Section**: 

Brief description for strategy table in README.md

**Strategy Guide** (optional):

Link to detailed strategy documentation if available.

## Checklist

- [ ] Code compiles without errors (`cargo build`)
- [ ] All tests pass (`cargo test`)
- [ ] Code follows project style (`cargo fmt`, `cargo clippy`)
- [ ] Backtest results are reproducible
- [ ] Configuration example provided
- [ ] Parameter descriptions documented
- [ ] Edge hypothesis explained
- [ ] Risk characteristics documented
- [ ] No overfitting (tested on multiple symbols/timeframes)
- [ ] Results verified independently

## Additional Notes

Any other information that would help evaluate this strategy:
- Inspirations or references
- Known weaknesses
- Future improvement ideas
- Related strategies
