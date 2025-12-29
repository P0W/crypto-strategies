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
