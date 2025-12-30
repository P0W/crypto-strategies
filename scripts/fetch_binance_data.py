#!/usr/bin/env python3
"""
Binance Historical Data Fetcher for Crypto Strategies

Fetches OHLCV candle data from Binance public API and saves to CSV format
compatible with the Rust backtester.

Usage:
    python scripts/fetch_binance_data.py --symbols BTC,ETH,SOL,BNB,XRP --timeframes 1h,4h,1d --days 365

The script converts Binance USDT pairs to INR format for consistency with the existing data structure.
"""

import argparse
import os
import time
from datetime import datetime, timedelta
from pathlib import Path

import requests
import pandas as pd


class BinanceDataFetcher:
    """Fetch historical OHLCV data from Binance public API"""
    
    BASE_URL = "https://api.binance.com/api/v3/klines"
    
    # Map timeframe strings to Binance interval format
    INTERVAL_MAP = {
        "1m": "1m",
        "5m": "5m",
        "15m": "15m",
        "30m": "30m",
        "1h": "1h",
        "4h": "4h",
        "1d": "1d",
    }
    
    # Approximate INR/USDT rate (for price conversion)
    INR_RATE = 83.5
    
    def __init__(self, data_dir: str = "data"):
        self.data_dir = Path(data_dir)
        self.data_dir.mkdir(exist_ok=True)
        
    def fetch_candles(
        self,
        symbol: str,
        interval: str,
        days: int = 365,
    ) -> pd.DataFrame:
        """
        Fetch candle data from Binance
        
        Args:
            symbol: Base symbol (e.g., 'BTC', 'ETH')
            interval: Candle interval (1m, 5m, 15m, 30m, 1h, 4h, 1d)
            days: Number of days of history to fetch
            
        Returns:
            DataFrame with OHLCV data
        """
        if interval not in self.INTERVAL_MAP:
            raise ValueError(f"Invalid interval. Must be one of: {list(self.INTERVAL_MAP.keys())}")
        
        binance_symbol = f"{symbol}USDT"
        binance_interval = self.INTERVAL_MAP[interval]
        
        end_time = datetime.now()
        start_time = end_time - timedelta(days=days)
        
        all_candles = []
        current_start = start_time
        
        print(f"  Fetching {symbol} {interval} data ({days} days)...")
        
        while current_start < end_time:
            params = {
                "symbol": binance_symbol,
                "interval": binance_interval,
                "startTime": int(current_start.timestamp() * 1000),
                "endTime": int(end_time.timestamp() * 1000),
                "limit": 1000,
            }
            
            try:
                response = requests.get(self.BASE_URL, params=params, timeout=30)
                response.raise_for_status()
                data = response.json()
                
                if not data:
                    break
                    
                all_candles.extend(data)
                
                # Move start time forward
                last_time = data[-1][0]
                current_start = datetime.fromtimestamp(last_time / 1000) + timedelta(milliseconds=1)
                
                # Rate limiting
                time.sleep(0.2)
                
            except requests.exceptions.RequestException as e:
                print(f"    Error fetching data: {e}")
                break
        
        if not all_candles:
            return pd.DataFrame()
        
        # Convert to DataFrame
        df = pd.DataFrame(all_candles, columns=[
            "timestamp", "open", "high", "low", "close", "volume",
            "close_time", "quote_volume", "trades", "taker_buy_base",
            "taker_buy_quote", "ignore"
        ])
        
        # Keep only OHLCV columns and convert types
        df = df[["timestamp", "open", "high", "low", "close", "volume"]]
        df["timestamp"] = pd.to_datetime(df["timestamp"], unit="ms")
        
        for col in ["open", "high", "low", "close", "volume"]:
            df[col] = pd.to_numeric(df[col], errors="coerce")
        
        # Convert USDT prices to INR
        for col in ["open", "high", "low", "close"]:
            df[col] = df[col] * self.INR_RATE
        
        # Rename timestamp to datetime for compatibility
        df = df.rename(columns={"timestamp": "datetime"})
        
        # Remove duplicates and sort
        df = df.drop_duplicates(subset=["datetime"]).sort_values("datetime").reset_index(drop=True)
        
        print(f"    Fetched {len(df)} candles")
        return df
    
    def save_data(self, symbol: str, interval: str, df: pd.DataFrame) -> str:
        """Save DataFrame to CSV in the expected format"""
        if df.empty:
            return ""
        
        # Format: BTCINR_1h.csv
        filename = f"{symbol}INR_{interval}.csv"
        filepath = self.data_dir / filename
        
        # Save with datetime format compatible with Rust parser
        df["datetime"] = df["datetime"].dt.strftime("%Y-%m-%d %H:%M:%S")
        df.to_csv(filepath, index=False)
        
        print(f"    Saved to {filepath}")
        return str(filepath)
    
    def fetch_all(
        self,
        symbols: list[str],
        timeframes: list[str],
        days: int = 365,
    ) -> dict:
        """
        Fetch data for multiple symbols and timeframes
        
        Args:
            symbols: List of base symbols (e.g., ['BTC', 'ETH'])
            timeframes: List of intervals (e.g., ['1h', '4h', '1d'])
            days: Number of days of history
            
        Returns:
            Dict mapping (symbol, timeframe) to file paths
        """
        results = {}
        
        print(f"\nFetching {len(symbols)} symbols × {len(timeframes)} timeframes...")
        print(f"Data directory: {self.data_dir.absolute()}\n")
        
        for symbol in symbols:
            for tf in timeframes:
                try:
                    df = self.fetch_candles(symbol, tf, days)
                    if not df.empty:
                        filepath = self.save_data(symbol, tf, df)
                        results[(symbol, tf)] = filepath
                except Exception as e:
                    print(f"    Error: {e}")
                    
        print(f"\nDone! Fetched {len(results)} data files.")
        return results


def main():
    parser = argparse.ArgumentParser(
        description="Fetch historical crypto data from Binance for backtesting"
    )
    parser.add_argument(
        "--symbols",
        type=str,
        default="BTC,ETH,SOL,BNB,XRP",
        help="Comma-separated list of symbols (default: BTC,ETH,SOL,BNB,XRP)"
    )
    parser.add_argument(
        "--timeframes",
        type=str,
        default="1h,4h,1d",
        help="Comma-separated list of timeframes (default: 1h,4h,1d)"
    )
    parser.add_argument(
        "--days",
        type=int,
        default=365,
        help="Number of days of history to fetch (default: 365)"
    )
    parser.add_argument(
        "--data-dir",
        type=str,
        default="data",
        help="Directory to save data files (default: data)"
    )
    
    args = parser.parse_args()
    
    symbols = [s.strip().upper() for s in args.symbols.split(",")]
    timeframes = [t.strip() for t in args.timeframes.split(",")]
    
    fetcher = BinanceDataFetcher(data_dir=args.data_dir)
    fetcher.fetch_all(symbols, timeframes, args.days)


if __name__ == "__main__":
    main()
