#!/usr/bin/env python3
"""
NSE Data Downloader for Rust Backtester
========================================
Downloads Nifty 50, Bank Nifty, and other NSE instruments
and saves in CSV format compatible with the Rust crypto-strategies backtester. 

CSV Format Expected by Rust: 
    datetime,open,high,low,close,volume
    2024-01-02 09:15:00,21700.00,21750.50,21650.25,21725.30,1234567

Usage:
    python download_nse_for_rust.py
    python download_nse_for_rust.py --symbols "NIFTY_50,NIFTY_BANK" --timeframe 1d --days 1000
    python download_nse_for_rust.py --symbols "RELIANCE,TCS,INFY" --timeframe 1h --days 365
"""

import os
import argparse
from datetime import datetime, timedelta
from pathlib import Path
from typing import List, Optional

import pandas as pd
import yfinance as yf


# =============================================================================
# SYMBOL MAPPINGS
# =============================================================================

# Yahoo Finance symbol mapping for NSE instruments
SYMBOL_MAP = {
    # Indices
    "NIFTY_50": "^NSEI",
    "NIFTY_BANK": "^NSEBANK",
    "NIFTY_IT": "^CNXIT",
    "NIFTY_FIN": "^CNXFIN",
    "NIFTY_PHARMA": "^CNXPHARMA",
    "NIFTY_AUTO": "^CNXAUTO",
    "NIFTY_METAL": "^CNXMETAL",
    "NIFTY_REALTY": "^CNXREALTY",
    "NIFTY_ENERGY": "^CNXENERGY",
    "NIFTY_INFRA": "^CNXINFRA",
    "NIFTY_PSE": "^CNXPSE",
    "NIFTY_MIDCAP": "^NSEMDCP50",
    "INDIA_VIX": "^INDIAVIX",
    
    # Large Cap Stocks (F&O)
    "RELIANCE": "RELIANCE. NS",
    "TCS":  "TCS.NS",
    "HDFCBANK": "HDFCBANK.NS",
    "INFY": "INFY.NS",
    "ICICIBANK": "ICICIBANK.NS",
    "HINDUNILVR": "HINDUNILVR.NS",
    "SBIN": "SBIN.NS",
    "BHARTIARTL": "BHARTIARTL.NS",
    "ITC": "ITC.NS",
    "KOTAKBANK":  "KOTAKBANK.NS",
    "LT": "LT.NS",
    "AXISBANK": "AXISBANK.NS",
    "ASIANPAINT": "ASIANPAINT. NS",
    "MARUTI": "MARUTI.NS",
    "TITAN": "TITAN.NS",
    "SUNPHARMA": "SUNPHARMA.NS",
    "BAJFINANCE": "BAJFINANCE.NS",
    "WIPRO": "WIPRO.NS",
    "HCLTECH": "HCLTECH. NS",
    "ULTRACEMCO": "ULTRACEMCO.NS",
    "TATAMOTORS": "TATAMOTORS.NS",
    "TATASTEEL": "TATASTEEL.NS",
    "POWERGRID": "POWERGRID.NS",
    "NTPC": "NTPC.NS",
    "ONGC": "ONGC.NS",
    "COALINDIA": "COALINDIA.NS",
    "JSWSTEEL": "JSWSTEEL.NS",
    "TECHM": "TECHM.NS",
    "HINDALCO": "HINDALCO.NS",
    "ADANIENT": "ADANIENT. NS",
    "ADANIPORTS": "ADANIPORTS.NS",
    "BAJAJFINSV": "BAJAJFINSV.NS",
    "DRREDDY": "DRREDDY.NS",
    "CIPLA": "CIPLA.NS",
    "EICHERMOT": "EICHERMOT.NS",
    "NESTLEIND": "NESTLEIND.NS",
    "DIVISLAB": "DIVISLAB.NS",
    "GRASIM": "GRASIM.NS",
    "HEROMOTOCO": "HEROMOTOCO.NS",
    "INDUSINDBK": "INDUSINDBK.NS",
    "M_M":  "M&M.NS",
    "SBILIFE": "SBILIFE.NS",
    "TATACONSUM": "TATACONSUM.NS",
    "APOLLOHOSP": "APOLLOHOSP.NS",
    "BRITANNIA": "BRITANNIA.NS",
    "HDFCLIFE": "HDFCLIFE. NS",
    "UPL": "UPL.NS",
    "BPCL": "BPCL. NS",
}

# Timeframe mapping (Yahoo Finance intervals)
TIMEFRAME_MAP = {
    "1m": "1m",      # 1 minute (max 7 days)
    "5m": "5m",      # 5 minutes (max 60 days)
    "15m": "15m",    # 15 minutes (max 60 days)
    "30m": "30m",    # 30 minutes (max 60 days)
    "1h": "1h",      # 1 hour (max 730 days)
    "4h": "4h",      # 4 hours (max 730 days) - Note: yfinance may not support
    "1d": "1d",      # 1 day
    "1w": "1wk",     # 1 week
    "1M": "1mo",     # 1 month
}


# =============================================================================
# DATA DOWNLOADER
# =============================================================================

def get_yahoo_symbol(symbol: str) -> str:
    """Convert our symbol to Yahoo Finance symbol."""
    # Check if it's in our mapping
    if symbol. upper() in SYMBOL_MAP:
        return SYMBOL_MAP[symbol.upper()]
    
    # If it already looks like a Yahoo symbol, use as is
    if symbol.startswith("^") or symbol.endswith(". NS"):
        return symbol
    
    # Assume it's an NSE stock, append .NS
    return f"{symbol. upper()}.NS"


def get_rust_symbol_name(symbol: str) -> str:
    """
    Convert symbol to the format expected by Rust backtester. 
    
    The Rust code expects filenames like:
        NIFTY_50_1d.csv
        NIFTY_BANK_1d.csv
        RELIANCE_1d.csv
    """
    # If it's a Yahoo symbol, convert back
    for rust_sym, yahoo_sym in SYMBOL_MAP.items():
        if yahoo_sym == symbol:
            return rust_sym
    
    # Remove .NS suffix if present
    if symbol.endswith(".NS"):
        return symbol[:-3]. upper()
    
    # Remove ^ prefix if present (indices)
    if symbol.startswith("^"):
        return symbol[1:].upper()
    
    return symbol.upper()


def download_data(
    symbol: str,
    timeframe:  str = "1d",
    days:  int = 1000,
    start_date: Optional[str] = None,
    end_date: Optional[str] = None,
) -> Optional[pd.DataFrame]:
    """
    Download OHLCV data from Yahoo Finance. 
    
    Args:
        symbol: Symbol to download (e.g., "NIFTY_50", "RELIANCE")
        timeframe: Timeframe (1m, 5m, 15m, 30m, 1h, 1d, 1w, 1M)
        days: Number of days of history (if start_date not provided)
        start_date:  Start date (YYYY-MM-DD format)
        end_date: End date (YYYY-MM-DD format)
    
    Returns:
        DataFrame with columns: datetime, open, high, low, close, volume
    """
    yahoo_symbol = get_yahoo_symbol(symbol)
    yf_interval = TIMEFRAME_MAP. get(timeframe, timeframe)
    
    # Calculate date range
    if end_date:
        end_dt = datetime.strptime(end_date, "%Y-%m-%d")
    else:
        end_dt = datetime.now()
    
    if start_date:
        start_dt = datetime.strptime(start_date, "%Y-%m-%d")
    else:
        start_dt = end_dt - timedelta(days=days)
    
    print(f"  Downloading {yahoo_symbol} ({timeframe}).. .", end=" ")
    
    try:
        ticker = yf.Ticker(yahoo_symbol)
        df = ticker.history(
            start=start_dt. strftime("%Y-%m-%d"),
            end=end_dt. strftime("%Y-%m-%d"),
            interval=yf_interval,
        )
        
        if df.empty:
            print("✗ No data returned")
            return None
        
        # Reset index to make datetime a column
        df = df.reset_index()
        
        # Rename columns to match Rust expected format
        # Handle both 'Date' and 'Datetime' column names
        date_col = 'Datetime' if 'Datetime' in df.columns else 'Date'
        
        df = df.rename(columns={
            date_col: 'datetime',
            'Open': 'open',
            'High': 'high',
            'Low': 'low',
            'Close': 'close',
            'Volume': 'volume',
        })
        
        # Select only required columns
        df = df[['datetime', 'open', 'high', 'low', 'close', 'volume']]
        
        # Convert datetime to string format expected by Rust
        # Format: "2024-01-02 09:15:00" or "2024-01-02 00:00:00+00:00"
        df['datetime'] = pd.to_datetime(df['datetime']).dt.strftime('%Y-%m-%d %H:%M:%S')
        
        # Ensure numeric types
        df['open'] = pd.to_numeric(df['open'], errors='coerce')
        df['high'] = pd.to_numeric(df['high'], errors='coerce')
        df['low'] = pd.to_numeric(df['low'], errors='coerce')
        df['close'] = pd.to_numeric(df['close'], errors='coerce')
        df['volume'] = pd.to_numeric(df['volume'], errors='coerce').fillna(0).astype(int)
        
        # Drop any rows with NaN values
        df = df.dropna()
        
        print(f"✓ {len(df)} bars")
        return df
        
    except Exception as e:
        print(f"✗ Error:  {e}")
        return None


def save_csv(df: pd.DataFrame, symbol: str, timeframe: str, output_dir: str) -> str:
    """
    Save DataFrame to CSV in format expected by Rust backtester. 
    
    Filename format: {SYMBOL}_{timeframe}.csv
    Example: NIFTY_50_1d.csv, RELIANCE_1h.csv
    """
    rust_symbol = get_rust_symbol_name(symbol)
    filename = f"{rust_symbol}_{timeframe}.csv"
    filepath = os.path.join(output_dir, filename)
    
    # Create output directory if it doesn't exist
    Path(output_dir).mkdir(parents=True, exist_ok=True)
    
    # Save without index
    df.to_csv(filepath, index=False)
    
    return filepath


def download_multiple(
    symbols: List[str],
    timeframes: List[str],
    days: int = 1000,
    output_dir: str = "data",
    start_date: Optional[str] = None,
    end_date: Optional[str] = None,
) -> dict:
    """
    Download data for multiple symbols and timeframes. 
    
    Returns:
        Dictionary with results:  {symbol: {timeframe: filepath}}
    """
    results = {}
    total = len(symbols) * len(timeframes)
    current = 0
    
    print("\n" + "=" * 60)
    print("DOWNLOADING NSE DATA FOR RUST BACKTESTER")
    print("=" * 60)
    print(f"  Symbols:     {', '.join(symbols)}")
    print(f"  Timeframes: {', '.join(timeframes)}")
    print(f"  Days:       {days}")
    print(f"  Output:      {output_dir}/")
    print("=" * 60 + "\n")
    
    for symbol in symbols:
        results[symbol] = {}
        print(f"\n{symbol}:")
        
        for timeframe in timeframes:
            current += 1
            
            df = download_data(
                symbol=symbol,
                timeframe=timeframe,
                days=days,
                start_date=start_date,
                end_date=end_date,
            )
            
            if df is not None and not df.empty:
                filepath = save_csv(df, symbol, timeframe, output_dir)
                results[symbol][timeframe] = filepath
    
    return results


def create_config_file(
    symbols: List[str],
    timeframe: str = "1d",
    output_dir: str = "configs",
    config_name: str = "nse_config. json",
    initial_capital: float = 500000.0,
) -> str:
    """
    Create a JSON config file compatible with the Rust backtester.
    """
    import json
    
    # Convert symbols to Rust format
    rust_symbols = [get_rust_symbol_name(s) for s in symbols]
    
    config = {
        "exchange": {
            "name": "zerodha",
            "maker_fee": 0.0003,
            "taker_fee": 0.0003,
            "assumed_slippage": 0.001,
            "rate_limit": 3
        },
        "trading":  {
            "pairs": rust_symbols,
            "initial_capital": initial_capital,
            "risk_per_trade": 0.02,
            "max_positions": min(len(rust_symbols), 5),
            "max_portfolio_heat": 0.10,
            "max_position_pct": 0.30,
            "max_drawdown": 0.20,
            "drawdown_warning":  0.10,
            "drawdown_critical": 0.15,
            "drawdown_warning_multiplier": 0.5,
            "drawdown_critical_multiplier": 0.25,
            "consecutive_loss_limit": 3,
            "consecutive_loss_multiplier": 0.5
        },
        "strategy": {
            "name": "volatility_regime",
            "timeframe": timeframe,
            "params": {
                "atr_period": 14,
                "ema_fast": 9,
                "ema_slow": 21,
                "adx_period": 14,
                "adx_threshold": 25,
                "volatility_lookback": 20,
                "compression_threshold": 0.75,
                "expansion_threshold": 1.5,
                "extreme_threshold": 2.0,
                "stop_atr_multiple": 1.5,
                "target_atr_multiple": 2.5,
                "breakout_atr_multiple": 1.5,
                "trailing_activation":  1.5,
                "trailing_atr_multiple": 1.5
            }
        },
        "tax": {
            "tax_rate": 0.15,  # 15% STCG for equity
            "tds_rate": 0.0,
            "loss_offset_allowed": True
        },
        "backtest": {
            "data_dir": "data",
            "results_dir": "results",
            "start_date": "2022-01-01",
            "end_date": "2025-12-31",
            "commission":  0.0003,
            "timeframe": timeframe
        },
        "grid": {
            "ema_fast": [8, 9, 13],
            "ema_slow":  [21, 34],
            "adx_threshold": [22, 25, 30],
            "stop_atr_multiple": [1.5, 2.0, 2.5],
            "target_atr_multiple": [2.0, 2.5, 3.0]
        }
    }
    
    # Create output directory
    Path(output_dir).mkdir(parents=True, exist_ok=True)
    
    filepath = os.path.join(output_dir, config_name)
    with open(filepath, 'w') as f:
        json.dump(config, f, indent=2)
    
    print(f"\n✓ Config saved to:  {filepath}")
    return filepath


# =============================================================================
# MAIN
# =============================================================================

def main():
    parser = argparse.ArgumentParser(
        description="Download NSE data for Rust backtester",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Download Nifty 50 and Bank Nifty daily data
  python download_nse_for_rust.py
  
  # Download specific symbols
  python download_nse_for_rust.py --symbols "NIFTY_50,NIFTY_BANK,RELIANCE"
  
  # Download multiple timeframes
  python download_nse_for_rust.py --symbols "NIFTY_50" --timeframes "1h,4h,1d"
  
  # Download with specific date range
  python download_nse_for_rust.py --start 2020-01-01 --end 2025-01-01
  
  # Also generate config file
  python download_nse_for_rust.py --create-config

Available Symbols:
  Indices:  NIFTY_50, NIFTY_BANK, NIFTY_IT, NIFTY_FIN, INDIA_VIX
  Stocks:   RELIANCE, TCS, HDFCBANK, INFY, ICICIBANK, SBIN, etc.
        """
    )
    
    parser.add_argument(
        "--symbols",
        type=str,
        default="NIFTY_50,NIFTY_BANK",
        help="Comma-separated list of symbols (default: NIFTY_50,NIFTY_BANK)"
    )
    
    parser.add_argument(
        "--timeframes",
        type=str,
        default="1d",
        help="Comma-separated list of timeframes:  1m,5m,15m,30m,1h,1d,1w (default: 1d)"
    )
    
    parser.add_argument(
        "--days",
        type=int,
        default=1000,
        help="Number of days of history to download (default: 1000)"
    )
    
    parser.add_argument(
        "--start",
        type=str,
        default=None,
        help="Start date (YYYY-MM-DD format)"
    )
    
    parser.add_argument(
        "--end",
        type=str,
        default=None,
        help="End date (YYYY-MM-DD format)"
    )
    
    parser.add_argument(
        "--output",
        type=str,
        default="data",
        help="Output directory for CSV files (default: data)"
    )
    
    parser.add_argument(
        "--create-config",
        action="store_true",
        help="Also create a JSON config file for the Rust backtester"
    )
    
    parser.add_argument(
        "--capital",
        type=float,
        default=500000.0,
        help="Initial capital for config file (default: 500000)"
    )
    
    parser.add_argument(
        "--list-symbols",
        action="store_true",
        help="List all available symbols and exit"
    )
    
    args = parser.parse_args()
    
    # List symbols if requested
    if args. list_symbols:
        print("\nAvailable Symbols:")
        print("-" * 40)
        print("\nINDICES:")
        for sym in sorted([s for s in SYMBOL_MAP.keys() if s.startswith("NIFTY") or s == "INDIA_VIX"]):
            print(f"  {sym}")
        print("\nSTOCKS:")
        for sym in sorted([s for s in SYMBOL_MAP.keys() if not s.startswith("NIFTY") and s != "INDIA_VIX"]):
            print(f"  {sym}")
        return
    
    # Parse arguments
    symbols = [s.strip() for s in args.symbols. split(",")]
    timeframes = [t.strip() for t in args.timeframes.split(",")]
    
    # Download data
    results = download_multiple(
        symbols=symbols,
        timeframes=timeframes,
        days=args. days,
        output_dir=args.output,
        start_date=args.start,
        end_date=args.end,
    )
    
    # Print summary
    print("\n" + "=" * 60)
    print("DOWNLOAD COMPLETE")
    print("=" * 60)
    
    total_files = sum(len(tfs) for tfs in results. values())
    print(f"  Total files: {total_files}")
    print(f"  Output directory: {args.output}/")
    print("\nFiles created:")
    for symbol, tfs in results.items():
        for tf, filepath in tfs.items():
            print(f"  ✓ {filepath}")
    
    # Create config if requested
    if args.create_config:
        primary_tf = timeframes[0]  # Use first timeframe as primary
        config_name = f"nse_{'_'.join([get_rust_symbol_name(s).lower() for s in symbols[:3]])}_{primary_tf}.json"
        create_config_file(
            symbols=symbols,
            timeframe=primary_tf,
            output_dir="configs",
            config_name=config_name,
            initial_capital=args.capital,
        )
    
    print("\n" + "=" * 60)
    print("NEXT STEPS")
    print("=" * 60)
    print(f"""
  1. Build the Rust backtester:
     cd rust && cargo build --release

  2. Run backtest:
     cargo run --release -- backtest --config ../configs/{config_name if args.create_config else 'your_config. json'}

  3. Run optimization:
     cargo run --release -- optimize --config ../configs/{config_name if args.create_config else 'your_config.json'}
    """)


if __name__ == "__main__":
    main()