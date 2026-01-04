# Currency Handling in Crypto Trading System

## Summary

**The backtesting and optimization system is completely currency-agnostic.**

Currency does **NOT** play any role in calculations. The code treats all monetary values as dimensionless numbers and requires only that `initial_capital` and price data are denominated in the **same currency**.

## How It Works

### Position Sizing Formula

```rust
// From risk.rs
let base_risk = current_capital * risk_per_trade;
let stop_distance = entry_price - stop_price;
let position_size = base_risk / stop_distance;
```

This formula works identically whether values are in USD, INR, EUR, or any other currency.

**Example:**

| Scenario | Capital | BTC Price | Stop Distance | Position Size |
|----------|---------|-----------|---------------|---------------|
| USD | $100,000 | $90,000 | $7,500 | 2.0 BTC |
| INR | ₹8,500,000 | ₹7,650,000 | ₹637,500 | 2.0 BTC |
| EUR | €85,000 | €76,500 | €6,375 | 2.0 BTC |

The **position size in BTC is identical** because the ratios are preserved.

### PnL Calculation

```rust
// From backtest.rs
let pnl = (exit_price - entry_price) * quantity;
let commission = (entry_value + exit_value) * fee_rate;
let net_pnl = pnl - commission;
```

Again, currency unit doesn't matter as long as entry/exit prices are in the same currency.

### Performance Metrics

All metrics are **percentage-based** or **ratio-based**:

```rust
// From backtest.rs
let total_return = (final_equity - initial_capital) / initial_capital * 100.0;
let drawdown = (peak_capital - current_capital) / peak_capital;
let sharpe_ratio = excess_return / std_dev * sqrt(periods_per_year);
```

These are **dimensionless** - currency cancels out in the division.

## Current Data Reality

### CSV Files

Files like `BTCINR_1d.csv` contain **USD prices**, not INR prices:

```
datetime,open,high,low,close,volume
2026-01-04 00:00:00,90628.01,91610.17,90628,91215.93,2238.34943
```

Bitcoin at ~$91,000 USD is correct. If this were INR, BTC would be ~$1,072 USD (clearly incorrect).

### Config Files

```json
{
  "trading": {
    "initial_capital": 100000
  }
}
```

With USD price data, this `100000` is treated as **$100,000 USD**, not ₹1,00,000 INR.

## Verification

Let's prove currency doesn't affect results:

### Scenario A: Both in USD

- Initial Capital: $100,000
- Final Equity: $150,000
- **Return: 50%**
- BTC Price: $90,000
- Position: 0.1667 BTC
- Position Value: $15,000 (15% of capital)

### Scenario B: Both in INR (@ 85 INR/USD)

- Initial Capital: ₹8,500,000
- Final Equity: ₹12,750,000
- **Return: 50%** ← Same!
- BTC Price: ₹7,650,000
- Position: 0.1667 BTC ← Same quantity!
- Position Value: ₹1,275,000 (15% of capital)

The **percentage return and position sizing are identical** regardless of currency!

## No Currency Conversion

The code has **ZERO** currency conversion logic:

```bash
# Search the entire Rust codebase
$ grep -r "exchange_rate\|currency\|conversion\|USD\|INR" rust/src/*.rs

# Result: No matches for exchange rates or conversion logic
```

The only mentions of "currency" are in comments and documentation, not in actual calculations.

## Common Misconceptions

### ❌ Misconception 1: "My CSV says BTCINR so prices must be in INR"

**Reality:** Filename is just a label. The actual data determines currency. Current CSV files contain USD prices.

### ❌ Misconception 2: "I need to convert INR capital to USD for backtesting"

**Reality:** No conversion needed. Just ensure capital and prices are in the same unit. If your data is USD, set capital in USD. If data is INR, set capital in INR.

### ❌ Misconception 3: "Exchange rate changes will affect my backtest"

**Reality:** There is no exchange rate in the code. All calculations are in a single currency unit determined by your data.

## Best Practices

1. **Verify your data source's currency** - Don't trust filenames alone
2. **Match config to data** - Set `initial_capital` in the same currency as your CSV prices
3. **Document your choice** - Add a comment in config files noting the currency
4. **Interpret results correctly** - A 50% return is 50% regardless of currency

## Example Config with Documentation

```json
{
  "trading": {
    "symbols": ["BTCINR", "ETHINR"],
    "initial_capital": 100000,
    "_comment": "Capital in USD ($100k) to match CSV price data which is in USD"
  }
}
```

or

```json
{
  "trading": {
    "symbols": ["BTCINR", "ETHINR"],
    "initial_capital": 8500000,
    "_comment": "Capital in INR (₹85 lakhs) to match CSV price data which is in INR"
  }
}
```

## Conclusion

**Currency plays NO role** in the optimizer or backtest calculations.

The system is currency-agnostic by design:
- No currency conversion
- No exchange rate handling
- No currency symbols or units in code
- All metrics are percentage/ratio-based

As long as your `initial_capital` and price data are in the **same currency**, the calculations are mathematically correct and produce identical percentage results regardless of which currency you choose.

The current repository uses **USD** for both capital and prices, despite "INR" in filenames.
