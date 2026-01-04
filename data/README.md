# Data Directory

Place your historical OHLCV data files here.

## Expected Format

CSV files with the following columns:
- `datetime`: ISO format timestamp (e.g., `2024-01-01 00:00:00`)
- `open`: Opening price
- `high`: Highest price
- `low`: Lowest price  
- `close`: Closing price
- `volume`: Trading volume

## Example

```csv
datetime,open,high,low,close,volume
2024-01-01 00:00:00,5000000,5050000,4980000,5020000,100.5
2024-01-01 04:00:00,5020000,5080000,5010000,5060000,85.3
```

## **IMPORTANT: Currency Handling**

⚠️ **The backtesting system is currency-agnostic** ⚠️

The code does **NOT** perform any currency conversion. All calculations assume that:
1. Your CSV price data is in a specific currency (e.g., USD, INR, EUR)
2. Your `initial_capital` in the config is in the **SAME currency**

### Example:

If your CSV files contain prices in **USD**:
```json
{
  "trading": {
    "initial_capital": 100000  // This should be in USD ($100,000)
  }
}
```

If your CSV files contain prices in **INR**:
```json
{
  "trading": {
    "initial_capital": 100000  // This would be in INR (₹1,00,000)
  }
}
```

### Current Dataset

The CSV files in this repository are currently in **USD**, despite the "INR" suffix in filenames:
- `BTCINR_1d.csv` → Contains BTC prices in USD (e.g., ~$90,000)
- `ETHINR_1d.csv` → Contains ETH prices in USD (e.g., ~$3,200)

The "INR" suffix is a **naming convention** only and does not affect calculations.
Always verify your data source's actual currency denomination.

### Why Currency Doesn't Matter for Results

All performance metrics are **percentage-based**:
- Returns: (final_equity - initial_capital) / initial_capital × 100%
- Sharpe Ratio: Based on return standard deviation (dimensionless)
- Drawdown: (peak_capital - current_capital) / peak_capital × 100%

As long as capital and prices are in the **same currency**, the percentage results
are identical regardless of which currency you use (USD, INR, EUR, etc.).

## Data Sources

You can obtain historical data from:
1. CoinDCX API (limited historical depth)
2. CryptoCompare API
3. CoinGecko API
4. Manual export from trading platforms

## File Naming

Name files after the trading pair:
- `BTCINR.csv` - Bitcoin/INR pair
- `ETHINR.csv` - Ethereum/INR pair
