"""
CoinDCX Historical Data Fetcher
Fetches OHLCV candle data from CoinDCX public API
"""

import logging
import time
from datetime import datetime, timedelta
from pathlib import Path
from typing import Optional

import pandas as pd
import requests

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


class CoinDCXDataFetcher:
    """Fetch historical OHLCV data from CoinDCX public API"""

    BASE_URL = "https://public.coindcx.com/market_data/candles"
    MARKETS_URL = "https://api.coindcx.com/exchange/v1/markets_details"

    # Valid intervals
    INTERVALS = ["1m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "1d", "3d", "1w", "1M"]

    # Rate limit: be nice to the API
    REQUEST_DELAY = 0.5  # seconds between requests

    def __init__(self, data_dir: str = "data"):
        self.data_dir = Path(data_dir)
        self.data_dir.mkdir(exist_ok=True)

    def get_available_markets(self, base_currency: str = "INR") -> list[dict]:
        """Get list of available trading pairs"""
        try:
            response = requests.get(self.MARKETS_URL, timeout=30)
            response.raise_for_status()
            markets = response.json()

            # Filter by base currency (INR for Indian markets)
            inr_markets = [
                m
                for m in markets
                if m.get("base_currency_short_name") == base_currency
                and m.get("status") == "active"
            ]

            return inr_markets
        except Exception as e:
            logger.error("Failed to fetch markets: %s", e)
            return []

    def list_inr_pairs(self) -> list[str]:
        """List all available INR trading pairs"""
        markets = self.get_available_markets("INR")
        pairs = []
        for m in markets:
            # Format: I-BTC_INR (I = CoinDCX INR exchange)
            pair = m.get("pair", "")
            if pair:
                pairs.append(pair)
        return sorted(pairs)

    def fetch_candles(
        self,
        pair: str,
        interval: str = "1h",
        start_time: Optional[datetime] = None,
        end_time: Optional[datetime] = None,
        limit: int = 1000,
    ) -> pd.DataFrame:
        """
        Fetch candle data for a trading pair

        Args:
            pair: Trading pair (e.g., 'I-BTC_INR', 'I-ETH_INR')
            interval: Candle interval (1m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 1d, 3d, 1w, 1M)
            start_time: Start datetime (optional)
            end_time: End datetime (optional)
            limit: Max candles per request (max 1000)

        Returns:
            DataFrame with OHLCV data
        """
        if interval not in self.INTERVALS:
            raise ValueError(f"Invalid interval. Must be one of: {self.INTERVALS}")

        params = {"pair": pair, "interval": interval, "limit": min(limit, 1000)}

        if start_time:
            params["startTime"] = int(start_time.timestamp() * 1000)
        if end_time:
            params["endTime"] = int(end_time.timestamp() * 1000)

        try:
            response = requests.get(self.BASE_URL, params=params, timeout=30)
            response.raise_for_status()
            data = response.json()

            if not data:
                logger.warning("No data returned for %s", pair)
                return pd.DataFrame()

            # Convert to DataFrame
            df = pd.DataFrame(data)

            # Rename columns to standard OHLCV format
            df = df.rename(
                columns={
                    "time": "datetime",
                    "open": "open",
                    "high": "high",
                    "low": "low",
                    "close": "close",
                    "volume": "volume",
                }
            )

            # Convert timestamp to datetime
            df["datetime"] = pd.to_datetime(df["datetime"], unit="ms")

            # Sort by time (oldest first)
            df = df.sort_values("datetime").reset_index(drop=True)

            # Ensure numeric types
            for col in ["open", "high", "low", "close", "volume"]:
                df[col] = pd.to_numeric(df[col], errors="coerce")

            return df[["datetime", "open", "high", "low", "close", "volume"]]

        except requests.exceptions.RequestException as e:
            logger.error("Request failed for %s: %s", pair, e)
            return pd.DataFrame()

    def fetch_full_history(
        self,
        pair: str,
        interval: str = "1h",
        days_back: int = 365,
        end_time: Optional[datetime] = None,
    ) -> pd.DataFrame:
        """
        Fetch full historical data by making multiple API calls

        Args:
            pair: Trading pair (e.g., 'I-BTC_INR')
            interval: Candle interval
            days_back: Number of days of history to fetch
            end_time: End datetime (defaults to now)

        Returns:
            DataFrame with complete OHLCV history
        """
        if end_time is None:
            end_time = datetime.now()

        start_time = end_time - timedelta(days=days_back)

        logger.info("Fetching %s %s data from %s to %s", pair, interval, start_time, end_time)

        all_data = []
        current_end = end_time

        while current_end > start_time:
            df = self.fetch_candles(pair=pair, interval=interval, end_time=current_end, limit=1000)

            if df.empty:
                logger.warning("No more data available before %s", current_end)
                break

            all_data.append(df)

            # Move end time to oldest candle in this batch
            oldest_time = df["datetime"].min()

            if oldest_time >= current_end:
                # No progress, break to avoid infinite loop
                break

            current_end = oldest_time - timedelta(minutes=1)

            logger.info("  Fetched %d candles, oldest: %s", len(df), oldest_time)

            # Rate limiting
            time.sleep(self.REQUEST_DELAY)

            # Stop if we've gone back far enough
            if oldest_time < start_time:
                break

        if not all_data:
            return pd.DataFrame()

        # Combine all data
        combined = pd.concat(all_data, ignore_index=True)

        # Remove duplicates and sort
        combined = (
            combined.drop_duplicates(subset=["datetime"])
            .sort_values("datetime")
            .reset_index(drop=True)
        )

        # Filter to requested date range
        combined = combined[combined["datetime"] >= start_time]

        logger.info("Total candles fetched: %d", len(combined))

        return combined

    def save_to_csv(self, df: pd.DataFrame, filename: str) -> Path:
        """Save DataFrame to CSV file"""
        filepath = self.data_dir / filename
        df.to_csv(filepath, index=False)
        logger.info("Saved %d rows to %s", len(df), filepath)
        return filepath

    def download_pair(self, symbol: str, interval: str = "1h", days_back: int = 365) -> Path:
        """
        Download historical data for a symbol and save to CSV

        Args:
            symbol: Symbol name (e.g., 'BTC', 'ETH')
            interval: Candle interval
            days_back: Days of history

        Returns:
            Path to saved CSV file
        """
        # Construct pair name for CoinDCX INR market
        pair = f"I-{symbol}_INR"

        # Fetch data
        df = self.fetch_full_history(pair=pair, interval=interval, days_back=days_back)

        if df.empty:
            raise ValueError(f"No data fetched for {symbol}")

        # Save to CSV
        filename = f"{symbol}INR_{interval}.csv"
        return self.save_to_csv(df, filename)


def main():
    """Main function to download historical data"""
    import argparse

    parser = argparse.ArgumentParser(description="Download CoinDCX historical data")
    parser.add_argument("--symbol", "-s", default="BTC", help="Symbol (BTC, ETH, etc.)")
    parser.add_argument(
        "--interval", "-i", default="1h", help="Interval (1m, 5m, 15m, 30m, 1h, 4h, 1d)"
    )
    parser.add_argument("--days", "-d", type=int, default=365, help="Days of history")
    parser.add_argument("--list", "-l", action="store_true", help="List available INR pairs")
    parser.add_argument("--all", "-a", action="store_true", help="Download BTC and ETH")

    args = parser.parse_args()

    fetcher = CoinDCXDataFetcher()

    if args.list:
        print("\nAvailable INR Trading Pairs:")
        print("-" * 40)
        pairs = fetcher.list_inr_pairs()
        for pair in pairs:
            print(f"  {pair}")
        print(f"\nTotal: {len(pairs)} pairs")
        return

    if args.all:
        # Download both BTC and ETH
        symbols = ["BTC", "ETH"]
    else:
        symbols = [args.symbol.upper()]

    for symbol in symbols:
        print(f"\n{'='*50}")
        print(f"Downloading {symbol}INR {args.interval} data ({args.days} days)")
        print("=" * 50)

        try:
            filepath = fetcher.download_pair(
                symbol=symbol, interval=args.interval, days_back=args.days
            )
            print(f"\n✓ Saved to: {filepath}")
        except Exception as e:
            print(f"\n✗ Error downloading {symbol}: {e}")


if __name__ == "__main__":
    main()
