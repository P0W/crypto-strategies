"""
Optimizer script for Volatility Regime Strategy
"""
import argparse
import sys
from pathlib import Path
from typing import List, Dict, Any, Optional

# Add project root to path to allow imports
project_root = Path(__file__).resolve().parents[3]
sys.path.append(str(project_root))

from src.optimizer import (
    grid_search_parallel, 
    grid_search_sequential, 
    print_results, 
    save_results, 
    generate_top_results_charts,
    fetch_data_for_coins,
    parse_float_list,
    parse_symbols_list,
    generate_coin_combinations
)

def quick_optimize(
    parallel: bool = True,
    generate_charts: bool = True,
    base_config_path: Optional[str] = None,
    sort_by: str = "sharpe",
):
    """Quick optimization with common parameter ranges"""

    # Available coins on CoinDCX INR market
    coins = ["BTC", "ETH", "SOL", "XRP", "DOGE"]
    timeframes = ["4h"]  # Start with 4h only for quick

    # Fetch any missing data
    fetch_data_for_coins(coins, timeframes)

    param_grid = {
        "adx_threshold": [20.0, 25.0, 30.0],
        "stop_atr_multiple": [2.5, 3.0, 3.5],
        "target_atr_multiple": [5.0, 6.0, 7.0],
    }

    # Test each coin individually and some combos
    symbols_list = [[f"{c}INR"] for c in coins] + [
        ["BTCINR", "ETHINR"],
        ["SOLINR", "XRPINR"],
    ]

    if parallel:
        results = grid_search_parallel(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir="data",
            min_trades=3,
            base_config_path=base_config_path,
        )
    else:
        results = grid_search_sequential(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir="data",
            min_trades=3,
            base_config_path=base_config_path,
        )

    # Sort by chosen metric
    results.sort(key=lambda x: x.sort_key(sort_by), reverse=True)

    print_results(results, top_n=15, sort_by=sort_by)
    save_results(results, "results/optimization_results.csv")

    # Generate charts for top results
    if generate_charts and results:
        generate_top_results_charts(results[:5], "results/charts")

    return results


def full_optimize(
    parallel: bool = True,
    generate_charts: bool = True,
    base_config_path: Optional[str] = None,
    sort_by: str = "sharpe",
):
    """Full optimization with more coins, timeframes, and parameters"""

    # More coins to test
    coins = ["BTC", "ETH", "SOL", "XRP", "DOGE", "MATIC", "ADA", "AVAX"]
    timeframes = ["1h", "4h", "1d"]

    # Fetch any missing data
    fetch_data_for_coins(coins, timeframes)

    param_grid = {
        "adx_threshold": [20.0, 25.0, 30.0],
        "stop_atr_multiple": [2.0, 2.5, 3.0, 3.5],
        "target_atr_multiple": [4.0, 5.0, 6.0, 7.0],
        "compression_threshold": [0.6, 0.7],
    }

    # Test each coin individually
    symbols_list = [[f"{c}INR"] for c in coins]
    # Add some combos
    symbols_list.extend(
        [
            ["BTCINR", "ETHINR"],
            ["SOLINR", "XRPINR", "DOGEINR"],
        ]
    )

    if parallel:
        results = grid_search_parallel(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir="data",
            min_trades=3,
            base_config_path=base_config_path,
        )
    else:
        results = grid_search_sequential(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir="data",
            min_trades=3,
            base_config_path=base_config_path,
        )

    # Sort by chosen metric
    results.sort(key=lambda x: x.sort_key(sort_by), reverse=True)

    print_results(results, top_n=25, sort_by=sort_by)
    save_results(results, "results/full_optimization_results.csv")

    # Generate charts for top results
    if generate_charts and results:
        generate_top_results_charts(results[:5], "results/charts")

    return results

def custom_optimize(
    symbols_list: List[List[str]],
    timeframes: List[str],
    param_grid: Dict[str, List[Any]],
    data_dir: str = "data",
    parallel: bool = True,
    generate_charts: bool = True,
    base_config_path: Optional[str] = None,
    output_file: str = "results/custom_optimization_results.csv",
    sort_by: str = "sharpe",
):
    """Custom optimization with user-specified parameters"""

    # Extract unique coins for data fetching
    all_symbols = set()
    for symbols in symbols_list:
        all_symbols.update(symbols)
    coins = [s.replace("INR", "") for s in all_symbols]

    # Fetch any missing data
    fetch_data_for_coins(coins, timeframes, data_dir)

    if parallel:
        results = grid_search_parallel(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir=data_dir,
            min_trades=3,
            base_config_path=base_config_path,
        )
    else:
        results = grid_search_sequential(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir=data_dir,
            min_trades=3,
            base_config_path=base_config_path,
        )

    # Sort by chosen metric
    results.sort(key=lambda x: x.sort_key(sort_by), reverse=True)

    print_results(results, top_n=15, sort_by=sort_by)
    save_results(results, output_file)

    if generate_charts and results:
        generate_top_results_charts(results[:5], "results/charts")

    return results

def main():
    """Run optimization from command line"""
    parser = argparse.ArgumentParser(
        description="Volatility Regime Strategy Optimizer",
        formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--mode",
        choices=["quick", "full", "custom"],
        default="quick",
        help="Optimization mode: quick, full, or custom",
    )
    parser.add_argument(
        "--sequential", action="store_true", help="Run sequentially instead of parallel"
    )
    parser.add_argument("--config", type=str, help="Base config file for shared settings")
    parser.add_argument("--data-dir", default="data", help="Data directory")
    parser.add_argument(
        "--output", default="results/optimization_results.csv", help="Output CSV file"
    )
    parser.add_argument(
        "--sort-by",
        choices=["sharpe", "calmar", "return", "profit_factor", "win_rate"],
        default="sharpe",
        help="Metric to rank results by (default: sharpe). "
    )

    # Custom mode parameters
    parser.add_argument(
        "--symbols",
        type=str,
        help="Symbols to test. Comma-separated for single group, semicolon for multiple groups. "
        "E.g., 'BTCINR,ETHINR' or 'BTCINR;ETHINR;BTCINR,ETHINR'",
    )
    parser.add_argument(
        "--coins",
        type=str,
        help="Coins to generate combinations from, comma-separated. "
        "E.g., 'BTC,ETH,SOL' generates all combinations: BTC, ETH, SOL, BTC+ETH, BTC+SOL, etc.",
    )
    parser.add_argument(
        "--min-combo",
        type=int,
        default=1,
        help="Minimum combination size when using --coins (default: 1 = singles)",
    )
    parser.add_argument(
        "--max-combo",
        type=int,
        default=None,
        help="Maximum combination size when using --coins (default: all coins together)",
    )
    parser.add_argument(
        "--timeframes",
        type=str,
        default="4h",
        help="Timeframes to test, comma-separated. E.g., '1h,4h,1d' (default: 4h)",
    )
    parser.add_argument(
        "--adx",
        type=str,
        help="ADX threshold values to test, comma-separated. E.g., '20,25,30'",
    )
    parser.add_argument(
        "--stop-atr",
        type=str,
        help="Stop ATR multiples to test, comma-separated. E.g., '2.0,2.5,3.0'",
    )
    parser.add_argument(
        "--target-atr",
        type=str,
        help="Target ATR multiples to test, comma-separated. E.g., '5.0,6.0,7.0'",
    )
    parser.add_argument(
        "--compression",
        type=str,
        help="Compression threshold values to test, comma-separated. E.g., '0.6,0.7'",
    )

    args = parser.parse_args()

    # Ensure directories exist
    Path("results").mkdir(exist_ok=True)
    Path(args.data_dir).mkdir(exist_ok=True)

    parallel = not args.sequential

    if args.config:
        print(f"   Base config: {args.config}")

    print(f"   Sort by: {args.sort_by}")

    if args.mode == "quick":
        print("🚀 Running QUICK optimization...")
        print(f"   Parallel: {parallel}")
        results = quick_optimize(
            parallel=parallel, base_config_path=args.config, sort_by=args.sort_by
        )

    elif args.mode == "full":
        print("🚀 Running FULL optimization...")
        print(f"   Parallel: {parallel}")
        print("   ⚠️  This may take several minutes!")
        results = full_optimize(
            parallel=parallel, base_config_path=args.config, sort_by=args.sort_by
        )

    elif args.mode == "custom":
        # Validate required args for custom mode
        if not args.symbols and not args.coins:
            parser.error("--symbols or --coins is required for custom mode")

        # Parse symbols - either from explicit list or generate combinations
        if args.coins:
            coins = [c.strip() for c in args.coins.split(",")]
            symbols_list = generate_coin_combinations(
                coins, min_size=args.min_combo, max_size=args.max_combo
            )
            print(f"   Generated {len(symbols_list)} coin combinations from {coins}")
        else:
            symbols_list = parse_symbols_list(args.symbols)

        # Parse timeframes
        timeframes = [t.strip() for t in args.timeframes.split(",")]

        # Build param grid from provided args
        param_grid = {}
        if args.adx:
            param_grid["adx_threshold"] = parse_float_list(args.adx)
        if args.stop_atr:
            param_grid["stop_atr_multiple"] = parse_float_list(args.stop_atr)
        if args.target_atr:
            param_grid["target_atr_multiple"] = parse_float_list(args.target_atr)
        if args.compression:
            param_grid["compression_threshold"] = parse_float_list(args.compression)

        if not param_grid:
            # Default to testing a few ADX values if nothing specified
            param_grid = {"adx_threshold": [25.0, 30.0]}
            print("   ⚠️  No parameters specified, defaulting to ADX: 25, 30")

        print("🚀 Running CUSTOM optimization...")
        print(f"   Symbols:    {symbols_list}")
        print(f"   Timeframes: {timeframes}")
        print(f"   Parameters: {param_grid}")
        print(f"   Parallel:   {parallel}")

        results = custom_optimize(
            symbols_list=symbols_list,
            timeframes=timeframes,
            param_grid=param_grid,
            data_dir=args.data_dir,
            parallel=parallel,
            base_config_path=args.config,
            output_file=args.output,
            sort_by=args.sort_by,
        )

    return 0

if __name__ == "__main__":
    main()
