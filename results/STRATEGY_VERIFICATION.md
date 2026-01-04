# Strategy Verification Report

**Generated:** 2026-01-04

This document verifies that all trading strategies in the repository are working correctly based on backtests run against actual OHLCV data from the `data/` folder using verbose logging (`-v` flag).

## Summary

| Strategy | Status | Total Return | Win Rate | Sharpe Ratio | Max Drawdown | Total Trades |
|----------|--------|--------------|----------|--------------|--------------|--------------|
| volatility_regime | ✅ Working | 55.36% | 44.90% | 0.53 | 13.61% | 49 |
| momentum_scalper | ⚠️ Needs Optimization | -27.65% | 32.50% | -0.99 | 27.74% | 40 |
| range_breakout | ✅ Working | 32.20% | 38.36% | 0.30 | 5.05% | 146 |
| quick_flip | ⚠️ Needs Optimization | 4.39% | 12.50% | -0.02 | 39.02% | 8 |

## Detailed Results

### 1. Volatility Regime Strategy

**Status:** ✅ WORKING - Profitable strategy with good risk-adjusted returns

**Configuration:** `configs/volatility_regime_config.json`
- Symbols: BNBINR, BTCINR, SOLINR
- Timeframe: 1d
- ADX Threshold: 20
- Stop ATR Multiple: 3.0
- Target ATR Multiple: 6.0

**Performance Metrics:**
```
Initial Capital:    ₹100,000.00
Total Return:       55.36%
Post-Tax Return:    38.75%
Sharpe Ratio:       0.53
Calmar Ratio:       0.84
Max Drawdown:       13.61%
Win Rate:           44.90%
Profit Factor:      2.18
Expectancy:         ₹1,154.98
Total Trades:       49
Winning Trades:     22
Losing Trades:      27
Average Win:        ₹4,752.61
Average Loss:       ₹1,776.42
```

**Signal Verification (Verbose Logs):** The strategy correctly generates:
- Entry signals when volatility is in compression/normal regime, trend is confirmed (EMA fast > slow, ADX > threshold), and breakout occurs
- Exit signals on stop loss, take profit, or extreme volatility regime

Sample verbose log entries:
```
DEBUG crypto_strategies::backtest: 2023-01-21 Buy SOLINR @ 25.54
DEBUG crypto_strategies::backtest: 2023-01-21 Buy BNBINR @ 305.41
DEBUG crypto_strategies::backtest: 2023-02-08 Buy BTCINR @ 23265.66
DEBUG crypto_strategies::backtest: 2023-02-10 CLOSE BNBINR @ 305.89 PnL=-7.74 (Stop)
DEBUG crypto_strategies::backtest: 2024-02-29 CLOSE BTCINR @ 62369.68 PnL=5166.80 (Target)
DEBUG crypto_strategies::backtest: 2024-03-14 CLOSE BNBINR @ 629.87 PnL=12271.69 (Target)
DEBUG crypto_strategies::backtest: 2023-12-23 CLOSE SOLINR @ 97.81 PnL=12801.66 (Target)
```

### 2. Momentum Scalper Strategy

**Status:** ⚠️ NEEDS OPTIMIZATION - Currently unprofitable with current parameters

**Configuration:** `configs/momentum_scalper_config.json`
- Symbols: All 5 crypto pairs
- Timeframe: 1d
- EMA Fast: 13
- EMA Slow: 21

**Performance Metrics:**
```
Initial Capital:    ₹100,000.00
Total Return:       -27.65%
Post-Tax Return:    -27.65%
Sharpe Ratio:       -0.99
Calmar Ratio:       -0.27
Max Drawdown:       27.74%
Win Rate:           32.50%
Total Trades:       40
```

Sample verbose log entries:
```
DEBUG crypto_strategies::backtest: 2022-04-02 CLOSE SOLINR @ 134.40 PnL=4277.16 (Target)
DEBUG crypto_strategies::backtest: 2022-04-07 CLOSE ETHINR @ 3165.35 PnL=-1455.49 (Stop)
DEBUG crypto_strategies::backtest: 2022-07-25 CLOSE ETHINR @ 1596.10 PnL=2490.78 (Target)
DEBUG crypto_strategies::backtest: 2022-08-09 CLOSE ETHINR @ 1775.27 PnL=3315.64 (Target)
DEBUG crypto_strategies::backtest: 2022-11-09 CLOSE ETHINR @ 1333.45 PnL=-3260.19 (Stop)
```

**Notes:** Strategy is functional but current parameters may not be optimal for the test period. The strategy logic (EMA crossover with ADX confirmation) is implemented correctly.

### 3. Range Breakout Strategy

**Status:** ✅ WORKING - Profitable strategy with low drawdown

**Configuration:** `configs/range_breakout_config.json`
- Symbols: All 5 crypto pairs
- Timeframe: 1d
- Lookback: 30
- Stop ATR: 0.5
- Target ATR: 3.0

**Performance Metrics:**
```
Initial Capital:    ₹100,000.00
Total Return:       32.20%
Post-Tax Return:    22.54%
Sharpe Ratio:       0.30
Calmar Ratio:       1.40
Max Drawdown:       5.05%
Win Rate:           38.36%
Profit Factor:      1.67
Expectancy:         ₹232.07
Total Trades:       146
```

**Signal Verification (Verbose Logs):** The strategy correctly generates:
- Entry signals when price breaks above the highest high of the lookback period
- Uses ATR-based stops and targets
- Maintains low drawdown through tight risk management

Sample verbose log entries:
```
DEBUG crypto_strategies::backtest: 2023-01-05 Buy BTCINR @ 16918.00
DEBUG crypto_strategies::backtest: 2023-01-14 CLOSE BTCINR @ 19270.18 PnL=3129.67 (Target)
DEBUG crypto_strategies::backtest: 2025-07-12 CLOSE XRPINR @ 2.73 PnL=1677.94 (Target)
DEBUG crypto_strategies::backtest: 2025-07-14 CLOSE BTCINR @ 118967.56 PnL=842.29 (Target)
DEBUG crypto_strategies::backtest: 2025-07-17 CLOSE ETHINR @ 3367.98 PnL=1749.69 (Target)
DEBUG crypto_strategies::backtest: 2025-07-18 CLOSE XRPINR @ 3.48 PnL=1838.56 (Target)
```

### 4. Quick Flip Strategy

**Status:** ⚠️ NEEDS OPTIMIZATION - Marginally profitable but low trade count

**Configuration:** `configs/quick_flip_config.json`
- Symbols: All 5 crypto pairs
- Timeframe: 1d
- Opening Bars: 50
- Cooldown: 1

**Performance Metrics:**
```
Initial Capital:    ₹100,000.00
Total Return:       4.39%
Post-Tax Return:    3.07%
Sharpe Ratio:       -0.02
Calmar Ratio:       0.03
Max Drawdown:       39.02%
Win Rate:           12.50%
Total Trades:       8
```

Sample verbose log entries:
```
DEBUG crypto_strategies::backtest: 2022-02-26 Buy BNBINR @ 375.37
DEBUG crypto_strategies::backtest: 2022-03-01 Buy SOLINR @ 99.77
DEBUG crypto_strategies::backtest: 2022-03-02 CLOSE SOLINR @ 98.55 PnL=-211.84 (Stop)
DEBUG crypto_strategies::backtest: 2022-03-03 Sell BTCINR @ 43849.10
DEBUG crypto_strategies::backtest: 2022-03-31 CLOSE SOLINR @ 120.70 PnL=9052.34 (Target)
DEBUG crypto_strategies::backtest: 2022-05-09 CLOSE BNBINR @ 355.54 PnL=-822.47 (Stop)
```

**Notes:** Strategy is functional but generates very few trades. May need parameter tuning or different market conditions to be more active.

## Data Verification

All strategies successfully loaded and processed data from the `data/` folder:

| Symbol | Data Points | Date Range |
|--------|-------------|------------|
| BTCINR_1d | 1493 candles | 2021-12 to 2025 |
| ETHINR_1d | 1493 candles | 2021-12 to 2025 |
| SOLINR_1d | 1493 candles | 2021-12 to 2025 |
| BNBINR_1d | 1493 candles | 2021-12 to 2025 |
| XRPINR_1d | 1493 candles | 2021-12 to 2025 |

## Test Execution

All 86 unit tests passed:
- Backtest engine tests
- Indicator tests (ATR, EMA, MACD, RSI, Bollinger Bands, etc.)
- Risk manager tests
- Strategy-specific tests
- Exchange client tests

3 integration tests were skipped due to network restrictions (expected in sandboxed environment).

## Log Verification

Backtest logs generated in `rust/logs/` directory with verbose mode (`-v` flag) showing:
- Strategy initialization
- Data loading confirmation
- **Detailed trade execution (Buy/Sell actions with prices)**
- **Position closures with PnL and exit reasons (Stop, Target, Signal)**
- Performance metrics calculation

### Log File Locations
```
rust/logs/backtest_2026-01-04_03-43-03.log  # volatility_regime
rust/logs/backtest_2026-01-04_03-45-16.log  # momentum_scalper, range_breakout, quick_flip
```

### Log Format
The verbose logs show each trade execution in the format:
```
TIMESTAMP DEBUG crypto_strategies::backtest: YYYY-MM-DD ACTION SYMBOL @ PRICE
TIMESTAMP DEBUG crypto_strategies::backtest: YYYY-MM-DD CLOSE SYMBOL @ PRICE PnL=VALUE (REASON)
```

Exit reasons:
- `Stop` - Stop loss triggered
- `Target` - Take profit target reached
- `Signal` - Strategy signal changed to exit

## Recommendations

1. **Volatility Regime:** Production-ready. Best performer in terms of risk-adjusted returns.
2. **Range Breakout:** Production-ready. Best for low-drawdown conservative trading.
3. **Momentum Scalper:** Requires parameter optimization. Consider different timeframes or symbol selection.
4. **Quick Flip:** Requires further tuning. Consider adjusting opening_bars or min_range_pct parameters.

## How to Reproduce

```bash
cd rust

# Build
cargo build

# Run individual backtests with VERBOSE LOGGING (-v flag)
cargo run -- -v backtest --config ../configs/volatility_regime_config.json
cargo run -- -v backtest --config ../configs/momentum_scalper_config.json
cargo run -- -v backtest --config ../configs/range_breakout_config.json
cargo run -- -v backtest --config ../configs/quick_flip_config.json

# Run without verbose (summary only)
cargo run -- backtest --config ../configs/volatility_regime_config.json

# Run tests
cargo test
```

## Verbose Logging Details

The `-v` flag enables DEBUG level logging which shows:
1. **Data loading**: Exact candle counts and symbol information
2. **Trade entries**: Date, action (Buy/Sell), symbol, and price
3. **Trade exits**: Date, symbol, exit price, PnL, and exit reason

Example verbose output:
```
2026-01-04T03:45:04.056084Z DEBUG crypto_strategies::backtest: 2024-02-29 CLOSE BNBINR @ 414.29 PnL=7513.90 (Target)
2026-01-04T03:45:04.056084Z DEBUG crypto_strategies::backtest: 2024-02-29 CLOSE BTCINR @ 62369.68 PnL=5166.80 (Target)
2026-01-04T03:45:04.057559Z DEBUG crypto_strategies::backtest: 2024-03-02 Buy BNBINR @ 407.81
```
