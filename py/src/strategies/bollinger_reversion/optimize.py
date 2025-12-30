"""
Optimizer script for Bollinger Reversion Strategy
"""
import logging
import sys
from pathlib import Path

# Add project root to path to allow imports
# File is in src/strategies/bollinger_reversion/optimize.py
# Root is ../../../..
project_root = Path(__file__).resolve().parents[3]
sys.path.append(str(project_root))

from src.optimizer import grid_search_parallel

def main():
    # Define parameter grid for Bollinger Reversion
    param_grid = {
        "bb_period": [15, 20, 25],
        "bb_dev": [1.5, 2.0],
        "rsi_period": [10, 14],
        "rsi_oversold": [30, 40, 45],
        # "rsi_overbought": [60, 70] # Keep it simple for now
    }

    print("🚀 Starting Bollinger Reversion Optimization...")
    
    # Paths need to be relative to project root or absolute
    data_dir = project_root / "data"
    config_path = project_root / "configs" / "bollinger_5m.json"

    results = grid_search_parallel(
        param_grid=param_grid,
        symbols_list=[["SOLINR"]],
        timeframes=["5m"],
        data_dir=str(data_dir),
        initial_capital=100_000,
        base_config_path=str(config_path),
        max_workers=4 # Adjust based on CPU
    )

    print(f"\n✅ Optimization Complete. Found {len(results)} results.")
    
    if results:
        # Sort by Sharpe Ratio
        results.sort(key=lambda x: x.sharpe_ratio if x.sharpe_ratio is not None else -999, reverse=True)
        
        print("\n🏆 Top 5 Configurations:")
        for i, res in enumerate(results[:5]):
            print(f"\nRank {i+1}:")
            print(f"  Params: {res.params}")
            print(f"  Return: {res.total_return:.2%}")
            print(f"  Sharpe: {res.sharpe_ratio:.2f}")
            print(f"  Trades: {res.total_trades}")
            print(f"  Win Rate: {res.win_rate:.2%}")

if __name__ == "__main__":
    main()
