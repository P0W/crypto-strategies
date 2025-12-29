"""
Strategy Parameter Optimizer

Grid search across symbols, timeframes, and strategy parameters
to find the best performing combinations.

Features:
- Concurrent execution for speed
- Multiple coins (BTC, ETH, SOL, XRP, DOGE, etc.)
- Timeframe permutation (1h, 4h, 1d)
- Seaborn charts for analysis
- Strategy-agnostic optimization
"""

import argparse
import itertools
import logging
import multiprocessing
import sys
from concurrent.futures import ProcessPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import pandas as pd

from src.strategies import (
    get_strategy_optimization_grid,
    get_strategy_defaults,
    get_strategy_aliases,
)

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
    (
        symbols,
        timeframe,
        strategy_params,
        data_dir,
        initial_capital,
        base_config_path,
        strategy_name,
    ) = args

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
        if strategy_name:
            # If strategy changed or we are using default config (which defaults to volatility_regime),
            # we must reset params to avoid pollution from the default strategy
            if config.strategy.name != strategy_name or not base_config_path:
                config.strategy.name = strategy_name
                config.strategy.params = get_strategy_defaults(strategy_name)

        # Apply strategy params being tested
        for key, value in strategy_params.items():
            config.strategy.params[key] = value

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
        return {"error": str(e)}
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
    strategy_name: Optional[str] = None,
) -> Optional[OptimizationResult]:
    """Run a single backtest with given parameters (non-parallel version)"""

    result = _run_single_backtest(
        (
            symbols,
            timeframe,
            strategy_params,
            data_dir,
            initial_capital,
            base_config_path,
            strategy_name,
        )
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
    strategy_name: Optional[str] = None,
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
                    (
                        symbols,
                        timeframe,
                        params,
                        data_dir,
                        initial_capital,
                        base_config_path,
                        strategy_name,
                    )
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
            symbols, timeframe, params, _, _, _, _ = task

            try:
                result = future.result()

                if result and "error" not in result and result.get("total_trades", 0) >= min_trades:
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
                    if result and "error" in result:
                        status = f"✗ worker error: {result['error']}"
                    else:
                        trades = result.get("total_trades", 0) if result else 0
                        status = f"✗ skip ({trades})"

            except Exception as e:
                status = f"✗ error: {str(e)}"

            # Clean progress bar - no status details
            pct = completed / total_runs * 100
            bar_len = 40
            filled = int(bar_len * completed / total_runs)
            bar = "█" * filled + "░" * (bar_len - filled)
            valid = len(results)
            print(
                f"\r⚡ [{bar}] {pct:5.1f}% | {completed}/{total_runs} | ✓ {valid} valid | Last: {status}",
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
    strategy_name: Optional[str] = None,
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
                    strategy_name=strategy_name,
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
    print(
        f"🏆 TOP {min(top_n, len(results))} PARAMETER COMBINATIONS (by {metric_labels.get(sort_by, sort_by)})"
    )
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


def generate_top_results_charts(
    results: List[OptimizationResult], output_dir: str = "results/charts", strategy_name: str = None
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

            if strategy_name:
                if config.strategy.name != strategy_name:
                    config.strategy.name = strategy_name
                    config.strategy.params = get_strategy_defaults(strategy_name)

            symbols_str = result.params.get("symbols", "")
            symbols = symbols_str.split(",") if symbols_str else ["BTCINR"]
            config.trading.pairs = symbols
            config.backtest.timeframe = result.params.get("timeframe", "4h")

            # Apply strategy params
            # Note: We need to apply all params from the result, not just hardcoded ones
            for key, value in result.params.items():
                if key not in ["symbols", "timeframe"]:
                    config.strategy.params[key] = value

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
    parser = argparse.ArgumentParser(description="Strategy Parameter Optimizer")
    parser.add_argument(
        "--strategy",
        type=str,
        required=True,
        help="Strategy to optimize (e.g., volatility_regime, bollinger_reversion)",
    )
    parser.add_argument(
        "--mode",
        type=str,
        choices=["quick", "full", "custom"],
        default="quick",
        help="Optimization mode",
    )
    parser.add_argument(
        "--parallel", action="store_true", default=True, help="Run in parallel (default: True)"
    )
    parser.add_argument(
        "--no-parallel", action="store_false", dest="parallel", help="Run sequentially"
    )
    parser.add_argument(
        "--charts", action="store_true", default=True, help="Generate charts (default: True)"
    )
    parser.add_argument(
        "--no-charts", action="store_false", dest="charts", help="Skip chart generation"
    )
    parser.add_argument(
        "--sort", "--sort-by", dest="sort", type=str, default="sharpe", help="Metric to sort by"
    )
    parser.add_argument("--config", type=str, help="Base config file path")
    parser.add_argument("--coins", type=str, help="Comma-separated list of coins")
    parser.add_argument("--timeframes", type=str, help="Comma-separated list of timeframes")
    parser.add_argument("--min-combo", type=int, default=1, help="Minimum coin combination size")
    parser.add_argument("--max-combo", type=int, help="Maximum coin combination size")

    args, unknown = parser.parse_known_args()

    # Get strategy-specific optimization grid
    try:
        grid_config = get_strategy_optimization_grid(args.strategy, args.mode)
    except ValueError as e:
        print(f"Error: {e}")
        sys.exit(1)

    # Override with custom args
    if args.coins:
        grid_config["coins"] = [c.strip() for c in args.coins.split(",")]

    if args.timeframes:
        grid_config["timeframes"] = [t.strip() for t in args.timeframes.split(",")]

    # Parse unknown args as strategy params
    # Common parameter aliases for CLI convenience
    try:
        PARAM_ALIASES = get_strategy_aliases(args.strategy)
    except Exception:
        PARAM_ALIASES = {}

    if unknown:
        param_grid = grid_config.get("param_grid", {})
        i = 0
        while i < len(unknown):
            arg = unknown[i]
            if arg.startswith("--"):
                raw_key = arg.lstrip("-").replace("-", "_")  # Convert kebab-case to snake_case
                # Map alias to actual parameter name if exists, else use raw key
                key = PARAM_ALIASES.get(raw_key, raw_key)

                if i + 1 < len(unknown) and not unknown[i + 1].startswith("--"):
                    val = unknown[i + 1]
                    # Parse comma-separated values
                    parsed_vals = []
                    for p in val.split(","):
                        p = p.strip()
                        try:
                            if "." in p:
                                parsed_vals.append(float(p))
                            else:
                                parsed_vals.append(int(p))
                        except ValueError:
                            parsed_vals.append(p)
                    param_grid[key] = parsed_vals
                    i += 2
                else:
                    print(f"Warning: No value provided for {arg}")
                    i += 1
            else:
                i += 1
        grid_config["param_grid"] = param_grid

    print(f"Optimizing strategy: {args.strategy} (Mode: {args.mode})")

    # Extract grid components
    param_grid = grid_config.get("param_grid", {})
    timeframes = grid_config.get("timeframes", ["4h"])
    coins = grid_config.get("coins", ["BTC", "ETH"])

    # Generate symbol combinations if not provided explicitly
    if "symbols_list" in grid_config:
        symbols_list = grid_config["symbols_list"]
    else:
        # Default combination logic
        if args.mode == "quick":
            # Individual coins + one big combo
            symbols_list = [[f"{c}INR"] for c in coins]
            if len(coins) > 1:
                symbols_list.append([f"{c}INR" for c in coins])
        elif args.mode == "custom":
            # Custom combination logic based on min/max combo
            max_c = args.max_combo if args.max_combo is not None else len(coins)
            symbols_list = generate_coin_combinations(
                coins, min_size=args.min_combo, max_size=max_c
            )
        else:
            # More combinations for full mode
            symbols_list = generate_coin_combinations(coins, min_size=1, max_size=1)
            if len(coins) >= 2:
                symbols_list.extend(generate_coin_combinations(coins, min_size=2, max_size=2))
            if len(coins) >= 3:
                symbols_list.append([f"{c}INR" for c in coins])

    # Fetch data
    fetch_data_for_coins(coins, timeframes)

    # Run optimization
    if args.parallel:
        results = grid_search_parallel(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir="data",
            min_trades=3,
            base_config_path=args.config,
            strategy_name=args.strategy,
        )
    else:
        results = grid_search_sequential(
            param_grid=param_grid,
            symbols_list=symbols_list,
            timeframes=timeframes,
            data_dir="data",
            min_trades=3,
            base_config_path=args.config,
            strategy_name=args.strategy,
        )

    # Sort and save
    results.sort(key=lambda x: x.sort_key(args.sort), reverse=True)

    output_file = f"results/{args.strategy}_{args.mode}_optimization.csv"
    print_results(results, top_n=15, sort_by=args.sort)
    save_results(results, output_file)

    if args.charts and results:
        chart_dir = f"results/charts/{args.strategy}_{args.mode}"
        generate_top_results_charts(results[:5], chart_dir, strategy_name=args.strategy)


if __name__ == "__main__":
    # Support running without args for backward compatibility or testing
    if len(sys.argv) == 1:
        print("Usage: python -m src.optimizer --strategy <name> --mode <quick|full>")
        sys.exit(1)
    main()
