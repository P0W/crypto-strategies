"""
Strategy Parameter Optimizer

Grid search across symbols, timeframes, and strategy parameters
to find the best performing combinations.

Features:
- Concurrent execution for speed
- Multiple coins (BTC, ETH, SOL, XRP, DOGE, etc.)
- Timeframe permutation (1h, 4h, 1d)
- Seaborn charts for analysis
"""

import itertools
import logging
import multiprocessing
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import pandas as pd

# Configure logging with better format
logging.basicConfig(
    level=logging.INFO, format="%(asctime)s %(levelname)-8s [%(funcName)s:%(lineno)d] %(message)s"
)
logger = logging.getLogger(__name__)
# Suppress backtrader noise
logging.getLogger("backtrader").setLevel(logging.WARNING)


@dataclass
class OptimizationResult:
    """Single optimization run result"""

    params: Dict[str, Any]
    total_return: float
    post_tax_return: float
    win_rate: float
    profit_factor: float
    max_drawdown: float
    total_trades: int
    sharpe_ratio: Optional[float]

    @property
    def calmar_ratio(self) -> float:
        """Return / Max Drawdown - measures return per unit of drawdown risk"""
        if self.max_drawdown <= 0:
            return 0.0
        return self.total_return / self.max_drawdown

    def sort_key(self, metric: str) -> float:
        """Get sort key value for ranking"""
        if metric == "sharpe":
            return self.sharpe_ratio if self.sharpe_ratio is not None else -999
        elif metric == "calmar":
            return self.calmar_ratio
        elif metric == "return":
            return self.total_return
        elif metric == "profit_factor":
            return self.profit_factor
        elif metric == "win_rate":
            return self.win_rate
        else:
            return self.sharpe_ratio if self.sharpe_ratio is not None else -999

    def to_dict(self) -> Dict:
        return {
            **self.params,
            "total_return": self.total_return,
            "post_tax_return": self.post_tax_return,
            "win_rate": self.win_rate,
            "profit_factor": self.profit_factor,
            "max_drawdown": self.max_drawdown,
            "total_trades": self.total_trades,
            "sharpe_ratio": self.sharpe_ratio,
        }


# Worker function must be at module level for multiprocessing
def _run_single_backtest(args: Tuple) -> Optional[Dict]:
    """Worker function for parallel execution"""
    symbols, timeframe, strategy_params, data_dir, initial_capital, base_config_path = args

    try:
        # Suppress ALL logging during optimization for speed
        import logging

        logging.disable(logging.CRITICAL)

        # Import inside worker to avoid pickling issues
        from .backtest import run_backtest
        from .config import Config

        # Load base config if provided, otherwise use defaults
        if base_config_path:
            config = Config.load_from_file(base_config_path)
        else:
            config = Config()

        # Override with optimization parameters
        config.trading.pairs = symbols
        config.trading.initial_capital = initial_capital
        config.backtest.data_dir = data_dir
        config.backtest.timeframe = timeframe

        # Apply strategy params being tested
        for key, value in strategy_params.items():
            if hasattr(config.strategy, key):
                setattr(config.strategy, key, value)

        # Run backtest
        result = run_backtest(config)

        if result is None:
            return None

        return {
            "symbols": ",".join(symbols),
            "timeframe": timeframe,
            **strategy_params,
            "total_return": result.get("total_return", 0),
            "post_tax_return": result.get("post_tax_return", 0),
            "win_rate": result.get("win_rate", 0),
            "profit_factor": result.get("profit_factor", 0),
            "max_drawdown": result.get("max_drawdown", 0),
            "total_trades": result.get("total_trades", 0),
            "sharpe_ratio": result.get("sharpe_ratio"),
        }

    except Exception as e:
        return None
    finally:
        # Re-enable logging
        import logging

        logging.disable(logging.NOTSET)


def run_single_optimization(
    symbols: List[str],
    timeframe: str,
    data_dir: str,
    strategy_params: Dict[str, Any],
    initial_capital: float = 100_000,
    base_config_path: Optional[str] = None,
) -> Optional[OptimizationResult]:
    """Run a single backtest with given parameters (non-parallel version)"""

    result = _run_single_backtest(
        (symbols, timeframe, strategy_params, data_dir, initial_capital, base_config_path)
    )

    if result is None:
        return None

    return OptimizationResult(
        params={
            k: v
            for k, v in result.items()
            if k
            not in [
                "total_return",
                "post_tax_return",
                "win_rate",
                "profit_factor",
                "max_drawdown",
                "total_trades",
                "sharpe_ratio",
            ]
        },
        total_return=result["total_return"],
        post_tax_return=result["post_tax_return"],
        win_rate=result["win_rate"],
        profit_factor=result["profit_factor"],
        max_drawdown=result["max_drawdown"],
        total_trades=result["total_trades"],
        sharpe_ratio=result["sharpe_ratio"],
    )


def grid_search_parallel(
    param_grid: Dict[str, List[Any]],
    symbols_list: List[List[str]],
    timeframes: List[str],
    data_dir: str = "data",
    min_trades: int = 5,
    initial_capital: float = 100_000,
    max_workers: int = None,
    base_config_path: Optional[str] = None,
) -> List[OptimizationResult]:
    """
    Perform parallel grid search over parameter combinations.

    Args:
        param_grid: Dict of parameter names to list of values to test
        symbols_list: List of symbol combinations to test
        timeframes: List of timeframes to test ('1h', '4h', '1d')
        data_dir: Directory containing CSV data files
        min_trades: Minimum trades required for valid result
        initial_capital: Starting capital for backtest
        max_workers: Max parallel processes (default: CPU count)
        base_config_path: Path to base config file for shared settings
    """

    if max_workers is None:
        max_workers = max(1, multiprocessing.cpu_count() - 1)

    # Generate all parameter combinations
    param_names = list(param_grid.keys())
    param_values = list(param_grid.values())
    all_param_combos = list(itertools.product(*param_values))

    # Build full task list: symbols × timeframes × params
    tasks = []
    for symbols in symbols_list:
        for timeframe in timeframes:
            for combo in all_param_combos:
                params = dict(zip(param_names, combo))
                tasks.append(
                    (symbols, timeframe, params, data_dir, initial_capital, base_config_path)
                )

    total_runs = len(tasks)

    print(f"\n{'='*70}")
    print(f"🚀 PARALLEL PARAMETER OPTIMIZATION")
    print(f"{'='*70}")
    print(f"  Symbols:     {len(symbols_list)} combinations")
    print(f"  Timeframes:  {timeframes}")
    print(f"  Parameters:  {param_names}")
    print(f"  Total tests: {total_runs}")
    print(f"  Workers:     {max_workers} parallel processes")
    print(f"{'='*70}\n")

    results = []
    completed = 0

    # Use ProcessPoolExecutor for true parallelism
    with ProcessPoolExecutor(max_workers=max_workers) as executor:
        # Submit all tasks
        future_to_task = {executor.submit(_run_single_backtest, task): task for task in tasks}

        # Process results as they complete
        for future in as_completed(future_to_task):
            completed += 1
            task = future_to_task[future]
            symbols, timeframe, params, _, _, _ = task

            try:
                result = future.result()

                if result and result.get("total_trades", 0) >= min_trades:
                    opt_result = OptimizationResult(
                        params={
                            k: v
                            for k, v in result.items()
                            if k
                            not in [
                                "total_return",
                                "post_tax_return",
                                "win_rate",
                                "profit_factor",
                                "max_drawdown",
                                "total_trades",
                                "sharpe_ratio",
                            ]
                        },
                        total_return=result["total_return"],
                        post_tax_return=result["post_tax_return"],
                        win_rate=result["win_rate"],
                        profit_factor=result["profit_factor"],
                        max_drawdown=result["max_drawdown"],
                        total_trades=result["total_trades"],
                        sharpe_ratio=result["sharpe_ratio"],
                    )
                    results.append(opt_result)
                    status = (
                        f"✓ PF:{result['profit_factor']:.2f} R:{result['total_return']*100:.1f}%"
                    )
                else:
                    status = "✗ skip"

            except Exception as e:
                status = f"✗ error"

            # Clean progress bar - no status details
            pct = completed / total_runs * 100
            bar_len = 40
            filled = int(bar_len * completed / total_runs)
            bar = "█" * filled + "░" * (bar_len - filled)
            valid = len(results)
            print(
                f"\r⚡ [{bar}] {pct:5.1f}% | {completed}/{total_runs} | ✓ {valid} valid",
                end="",
                flush=True,
            )

    print("\n")

    return results


def grid_search_sequential(
    param_grid: Dict[str, List[Any]],
    symbols_list: List[List[str]],
    timeframes: List[str],
    data_dir: str = "data",
    min_trades: int = 5,
    initial_capital: float = 100_000,
    base_config_path: Optional[str] = None,
) -> List[OptimizationResult]:
    """Sequential grid search (fallback if parallel fails)"""

    param_names = list(param_grid.keys())
    param_values = list(param_grid.values())
    all_combos = list(itertools.product(*param_values))

    total_runs = len(all_combos) * len(symbols_list) * len(timeframes)
    print(f"\n{'='*60}")
    print(f"SEQUENTIAL PARAMETER OPTIMIZATION")
    print(f"{'='*60}")
    print(f"Total backtests to run: {total_runs}")
    print(f"{'='*60}\n")

    results = []
    run_count = 0

    # Suppress logging for speed
    import logging

    logging.disable(logging.CRITICAL)

    for symbols in symbols_list:
        for timeframe in timeframes:
            for combo in all_combos:
                run_count += 1
                params = dict(zip(param_names, combo))

                result = run_single_optimization(
                    symbols=symbols,
                    timeframe=timeframe,
                    data_dir=data_dir,
                    strategy_params=params,
                    initial_capital=initial_capital,
                    base_config_path=base_config_path,
                )

                if result and result.total_trades >= min_trades:
                    results.append(result)

                # Clean progress bar
                pct = run_count / total_runs * 100
                bar_len = 40
                filled = int(bar_len * run_count / total_runs)
                bar = "█" * filled + "░" * (bar_len - filled)
                print(
                    f"\r⚡ [{bar}] {pct:5.1f}% | {run_count}/{total_runs} | ✓ {len(results)} valid",
                    end="",
                    flush=True,
                )

    # Re-enable logging
    logging.disable(logging.NOTSET)
    print()  # New line after progress

    return results


def print_results(results: List[OptimizationResult], top_n: int = 10, sort_by: str = "sharpe"):
    """Print top optimization results"""

    metric_labels = {
        "sharpe": "Sharpe Ratio",
        "calmar": "Calmar Ratio (Return/Drawdown)",
        "return": "Total Return",
        "profit_factor": "Profit Factor",
        "win_rate": "Win Rate",
    }

    print(f"\n{'='*80}")
    print(f"🏆 TOP {min(top_n, len(results))} PARAMETER COMBINATIONS (by {metric_labels.get(sort_by, sort_by)})")
    print(f"{'='*80}\n")

    if not results:
        print("No valid results found!")
        return

    for i, r in enumerate(results[:top_n], 1):
        print(f"#{i} ─────────────────────────────────────────────────────")

        # Print all params nicely
        for key, value in r.params.items():
            if key == "symbols":
                print(f"   {'Symbols':<20} {value}")
            elif key == "timeframe":
                print(f"   {'Timeframe':<20} {value}")
            else:
                print(f"   {key:<20} {value}")

        print(f"   ─────────────────────────────────────")
        print(f"   {'Return':<20} {r.total_return*100:>8.2f}%")
        print(f"   {'Post-Tax':<20} {r.post_tax_return*100:>8.2f}%")
        print(f"   {'Win Rate':<20} {r.win_rate*100:>8.1f}%")
        print(f"   {'Profit Factor':<20} {r.profit_factor:>8.2f}")
        print(f"   {'Max Drawdown':<20} {r.max_drawdown*100:>8.2f}%")
        print(f"   {'Sharpe Ratio':<20} {r.sharpe_ratio:>8.2f}" if r.sharpe_ratio else "")
        print(f"   {'Calmar Ratio':<20} {r.calmar_ratio:>8.2f}")
        print(f"   {'Total Trades':<20} {r.total_trades:>8d}")
        print()


def save_results(results: List[OptimizationResult], filepath: str = "optimization_results.csv"):
    """Save results to CSV"""
    if not results:
        return

    df = pd.DataFrame([r.to_dict() for r in results])
    df.to_csv(filepath, index=False)
    print(f"📁 Results saved to: {filepath}")


def fetch_data_for_coins(coins: List[str], timeframes: List[str], data_dir: str = "data"):
    """Fetch data for all coin/timeframe combinations"""
    from .data_fetcher import CoinDCXDataFetcher

    fetcher = CoinDCXDataFetcher(data_dir=data_dir)

    print(f"\n📥 Fetching data for {len(coins)} coins × {len(timeframes)} timeframes...")

    for coin in coins:
        pair = f"I-{coin}_INR"
        for tf in timeframes:
            filename = f"{coin}INR_{tf}.csv"
            filepath = Path(data_dir) / filename

            if filepath.exists():
                print(f"  ✓ {filename} already exists")
                continue

            print(f"  ⬇ Fetching {pair} {tf}...", end=" ")
            try:
                df = fetcher.fetch_candles(pair=pair, interval=tf, limit=1000)
                if not df.empty:
                    df.to_csv(filepath, index=False)
                    print(f"✓ {len(df)} bars")
                else:
                    print("✗ no data")
            except Exception as e:
                print(f"✗ {e}")


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


def generate_top_results_charts(
    results: List[OptimizationResult], output_dir: str = "results/charts"
):
    """Generate detailed charts for top optimization results"""
    from .charts import create_comprehensive_chart, create_comparison_chart
    from .backtest import run_backtest
    from .config import Config

    Path(output_dir).mkdir(parents=True, exist_ok=True)

    print(f"\n📊 Generating charts for top {len(results)} configurations...")

    # Create comparison chart first
    comparison_data = [
        {
            "symbols": r.params.get("symbols", "N/A"),
            "timeframe": r.params.get("timeframe", "4h"),
            "total_return": r.total_return,
            "win_rate": r.win_rate,
            "profit_factor": r.profit_factor,
            "max_drawdown": r.max_drawdown,
        }
        for r in results
    ]

    try:
        create_comparison_chart(
            comparison_data,
            output_path=f"{output_dir}/top5_comparison.png",
            title="Top 5 Strategy Configurations Comparison",
        )
    except Exception as e:
        logger.warning("Failed to create comparison chart: %s", e)

    # Run detailed backtest for each top result to get full data
    for i, result in enumerate(results, 1):
        print(f"\n  [{i}/{len(results)}] Running detailed backtest for config #{i}...")

        try:
            # Recreate the config and run backtest with full details
            config = Config()

            symbols_str = result.params.get("symbols", "")
            symbols = symbols_str.split(",") if symbols_str else ["BTCINR"]
            config.trading.pairs = symbols
            config.backtest.timeframe = result.params.get("timeframe", "4h")

            # Apply strategy params
            for key in [
                "adx_threshold",
                "stop_atr_multiple",
                "target_atr_multiple",
                "compression_threshold",
            ]:
                if key in result.params:
                    setattr(config.strategy, key, result.params[key])

            # Run backtest
            backtest_result = run_backtest(config, return_full_data=True)

            if backtest_result:
                equity_curve = backtest_result.get("equity_curve", [])
                trades = backtest_result.get("trades", [])

                metrics = {
                    "total_return": result.total_return,
                    "post_tax_return": result.post_tax_return,
                    "win_rate": result.win_rate,
                    "profit_factor": result.profit_factor,
                    "max_drawdown": result.max_drawdown,
                    "total_trades": result.total_trades,
                    "sharpe_ratio": result.sharpe_ratio,
                }

                title = f"Config #{i}: {symbols_str} ({result.params.get('timeframe', '4h')})"

                create_comprehensive_chart(
                    equity_curve=equity_curve,
                    trades=trades,
                    config_params=result.params,
                    metrics=metrics,
                    output_path=f"{output_dir}/config_{i}_detailed.png",
                    title=title,
                )
                print(f"    ✓ Chart saved: config_{i}_detailed.png")
        except Exception as e:
            logger.warning("Failed to generate chart for config #%d: %s", i, e)

    print(f"\n✅ All charts saved to: {output_dir}/")

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


def parse_float_list(value: str) -> List[float]:
    """Parse comma-separated float values"""
    return [float(x.strip()) for x in value.split(",")]


def parse_symbols_list(value: str) -> List[List[str]]:
    """
    Parse symbols argument. Supports:
    - Single group: "BTCINR,ETHINR" -> [["BTCINR", "ETHINR"]]
    - Multiple groups: "BTCINR;ETHINR;BTCINR,ETHINR" -> [["BTCINR"], ["ETHINR"], ["BTCINR", "ETHINR"]]
    """
    groups = value.split(";")
    return [[s.strip().upper() for s in group.split(",")] for group in groups]


def generate_coin_combinations(
    coins: List[str], min_size: int = 1, max_size: int = None
) -> List[List[str]]:
    """
    Generate all combinations of coins from min_size to max_size.
    
    Args:
        coins: List of coin symbols (e.g., ["BTC", "ETH", "SOL"])
        min_size: Minimum combination size (default: 1 = singles)
        max_size: Maximum combination size (default: len(coins) = all together)
    
    Returns:
        List of symbol lists, e.g., [["BTCINR"], ["ETHINR"], ["BTCINR", "ETHINR"], ...]
    """
    if max_size is None:
        max_size = len(coins)
    
    # Normalize coins to INR pairs
    symbols = [f"{c.upper().replace('INR', '')}INR" for c in coins]
    
    combinations = []
    for size in range(min_size, max_size + 1):
        for combo in itertools.combinations(symbols, size):
            combinations.append(list(combo))
    
    return combinations


def main():
    """Run optimization from command line"""
    import argparse

    parser = argparse.ArgumentParser(
        description="Strategy Parameter Optimizer",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Quick preset optimization
  uv run optimize --mode quick

  # Full preset optimization  
  uv run optimize --mode full

  # Custom: test all combinations of 3 coins (7 combos: 3 singles + 3 pairs + 1 triple)
  uv run optimize --mode custom --coins "BTC,ETH,SOL" --timeframes "1d"

  # Custom: only test pairs and triples (skip singles)
  uv run optimize --mode custom --coins "BTC,ETH,SOL,XRP" --min-combo 2 --timeframes "1d"

  # Custom: only test pairs (no singles, no 3+ groups)
  uv run optimize --mode custom --coins "BTC,ETH,SOL,XRP" --min-combo 2 --max-combo 2

  # Custom with parameter sweep
  uv run optimize --mode custom --coins "BTC,ETH,SOL" --adx "25,30" --stop-atr "2.0,2.5,3.0"

  # Custom with base config
  uv run optimize --mode custom --config configs/btc_eth_sol_bnb_xrp_1d.json --coins "BTC,ETH" --stop-atr "2.0,2.5,3.0"

  # Explicit symbol groups (alternative to --coins)
  uv run optimize --mode custom --symbols "BTCINR;ETHINR;BTCINR,ETHINR" --timeframes "1d"
        """,
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
        "sharpe=risk-adjusted, calmar=return/drawdown, return=total return",
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
        results = quick_optimize(parallel=parallel, base_config_path=args.config, sort_by=args.sort_by)

    elif args.mode == "full":
        print("🚀 Running FULL optimization...")
        print(f"   Parallel: {parallel}")
        print("   ⚠️  This may take several minutes!")
        results = full_optimize(parallel=parallel, base_config_path=args.config, sort_by=args.sort_by)

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
    exit(main())
