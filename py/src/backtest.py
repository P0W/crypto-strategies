"""
Backtest Runner

Main entry point for running strategy backtests using Backtrader.
"""

import argparse
import logging
import sys
from pathlib import Path
from typing import Optional

import backtrader as bt
import pandas as pd

from .strategies import get_strategy_class
from .config import Config, get_default_config, setup_logging
from .risk import calculate_minimum_edge

logger = logging.getLogger(__name__)


class TaxAwareCommission(bt.CommInfoBase):
    """
    Commission scheme that accounts for Indian crypto taxes.

    - 0.1% fee per trade (maker/taker)
    - 30% tax on profits (applied at portfolio level, not per-trade)
    
    NOTE: Backtrader divides COMM_PERC by 100, so we use 0.1 to get 0.1%
    """

    params = (
        ("commission", 0.1),  # 0.1% per trade (Backtrader divides by 100)
        ("stocklike", True),
        ("commtype", bt.CommInfoBase.COMM_PERC),
    )

    def _getcommission(self, size, price, pseudoexec):
        return abs(size) * price * self.p.commission


class BacktestAnalyzer(bt.Analyzer):
    """Custom analyzer for comprehensive backtest metrics"""

    def __init__(self):
        super().__init__()
        self.trades = []
        self.equity_curve = []

    def start(self):
        self.start_value = self.strategy.broker.getvalue()

    def notify_trade(self, trade):
        if trade.isclosed:
            self.trades.append(
                {
                    "pnl": trade.pnl,
                    "pnlcomm": trade.pnlcomm,
                    "commission": trade.commission,
                    "size": trade.size,
                    "price": trade.price,
                    "value": trade.value,
                    "baropen": trade.baropen,
                    "barclose": trade.barclose,
                }
            )

    def next(self):
        self.equity_curve.append(
            {
                "datetime": self.strategy.datetime.datetime(),
                "value": self.strategy.broker.getvalue(),
            }
        )

    def stop(self):
        self.end_value = self.strategy.broker.getvalue()

    def get_analysis(self):
        if not self.trades:
            return {
                "total_trades": 0,
                "winning_trades": 0,
                "losing_trades": 0,
                "win_rate": 0,
                "total_pnl": 0,
                "total_commission": 0,
                "net_pnl": 0,
                "avg_trade_pnl": 0,
                "profit_factor": 0,
                "max_drawdown": 0,
            }

        total_trades = len(self.trades)
        winning_trades = sum(1 for t in self.trades if t["pnlcomm"] > 0)
        losing_trades = total_trades - winning_trades

        gross_profits = sum(t["pnlcomm"] for t in self.trades if t["pnlcomm"] > 0)
        gross_losses = abs(sum(t["pnlcomm"] for t in self.trades if t["pnlcomm"] < 0))

        total_pnl = sum(t["pnl"] for t in self.trades)
        total_commission = sum(t["commission"] for t in self.trades)
        net_pnl = sum(t["pnlcomm"] for t in self.trades)

        # Calculate max drawdown from equity curve
        if self.equity_curve:
            values = [e["value"] for e in self.equity_curve]
            peak = values[0]
            max_dd = 0
            for v in values:
                peak = max(peak, v)
                dd = (peak - v) / peak if peak > 0 else 0
                max_dd = max(max_dd, dd)
        else:
            max_dd = 0

        # Apply tax to profits
        pre_tax_profit = net_pnl
        if pre_tax_profit > 0:
            tax = pre_tax_profit * 0.30
            post_tax_profit = pre_tax_profit - tax
        else:
            tax = 0
            post_tax_profit = pre_tax_profit

        return {
            "total_trades": total_trades,
            "winning_trades": winning_trades,
            "losing_trades": losing_trades,
            "win_rate": winning_trades / total_trades if total_trades > 0 else 0,
            "gross_profits": gross_profits,
            "gross_losses": gross_losses,
            "total_pnl": total_pnl,
            "total_commission": total_commission,
            "net_pnl": net_pnl,
            "avg_trade_pnl": net_pnl / total_trades if total_trades > 0 else 0,
            "profit_factor": gross_profits / gross_losses if gross_losses > 0 else float("inf"),
            "max_drawdown": max_dd,
            "pre_tax_profit": pre_tax_profit,
            "tax_amount": tax,
            "post_tax_profit": post_tax_profit,
            "start_value": self.start_value,
            "end_value": self.end_value,
            "total_return": (self.end_value - self.start_value) / self.start_value,
            "post_tax_return": post_tax_profit / self.start_value,
        }


def load_data(
    symbol: str,
    data_dir: str,
    timeframe: str = "4h",
    start_date: Optional[str] = None,
    end_date: Optional[str] = None,
) -> Optional[bt.feeds.PandasData]:
    """
    Load OHLCV data for a symbol.

    Expected CSV format: datetime,open,high,low,close,volume

    Args:
        symbol: Trading pair symbol
        data_dir: Directory containing data files
        timeframe: Timeframe for data (1h, 4h, 1d)
        start_date: Start date (YYYY-MM-DD)
        end_date: End date (YYYY-MM-DD)

    Returns:
        Backtrader data feed or None
    """
    # Try timeframe-specific file first, then fall back to symbol-only
    data_path = Path(data_dir) / f"{symbol}_{timeframe}.csv"
    if not data_path.exists():
        data_path = Path(data_dir) / f"{symbol}.csv"

    if not data_path.exists():
        logger.warning("Data file not found: %s", data_path)
        return None

    try:
        df = pd.read_csv(data_path, parse_dates=["datetime"], index_col="datetime")

        # Filter by date range
        if start_date:
            df = df[df.index >= start_date]
        if end_date:
            df = df[df.index <= end_date]

        if df.empty:
            logger.warning("No data in date range for %s", symbol)
            return None

        data = bt.feeds.PandasData(
            dataname=df,
            datetime=None,  # Use index
            open="open",
            high="high",
            low="low",
            close="close",
            volume="volume",
            openinterest=-1,
        )
        data._name = symbol

        logger.info("Loaded %d bars for %s", len(df), symbol)
        return data

    except Exception as e:
        logger.error("Error loading data for %s: %s", symbol, e)
        return None


def run_backtest(
    config: Optional[Config] = None,
    data_feeds: Optional[list] = None,
    return_full_data: bool = False,
) -> dict:
    """
    Run strategy backtest.

    Args:
        config: Configuration object
        data_feeds: Optional list of data feeds
        return_full_data: If True, include equity_curve and trades in output

    Returns:
        Dictionary with backtest results
    """
    config = config or get_default_config()

    # Initialize Cerebro
    cerebro = bt.Cerebro()

    # Set initial capital
    cerebro.broker.setcash(config.trading.initial_capital)

    # Set commission
    cerebro.broker.addcommissioninfo(TaxAwareCommission())
    
    # Set slippage (0.1% assumed slippage on each trade)
    # This models realistic market impact and execution costs
    cerebro.broker.set_slippage_perc(
        perc=config.exchange.assumed_slippage,
        slip_open=True,   # Apply to market orders at open
        slip_limit=False,  # Don't apply to limit orders
        slip_match=True,   # Apply when price matches
        slip_out=True      # Apply on exit orders
    )

    # Add strategy with params from unified config
    strategy_params = config.get_strategy_params()

    # Dynamically load strategy class
    strategy_class = get_strategy_class(config.strategy.name)
    cerebro.addstrategy(strategy_class, **strategy_params)

    # Add data feeds
    if data_feeds:
        for data in data_feeds:
            cerebro.adddata(data)
    else:
        # Load data from files
        for symbol in config.trading.pairs:
            data = load_data(
                symbol,
                config.backtest.data_dir,
                config.backtest.timeframe,
                config.backtest.start_date,
                config.backtest.end_date,
            )
            if data is not None:
                cerebro.adddata(data)

    # Add analyzers
    cerebro.addanalyzer(BacktestAnalyzer, _name="custom")
    # Sharpe Ratio: annualized, using 365 trading days for crypto (24/7 market)
    cerebro.addanalyzer(
        bt.analyzers.SharpeRatio,
        _name="sharpe",
        timeframe=bt.TimeFrame.Days,
        riskfreerate=0.05,  # 5% annual risk-free rate
        annualize=True,
        factor=365,  # Crypto trades 365 days/year
    )
    cerebro.addanalyzer(bt.analyzers.Returns, _name="returns")
    cerebro.addanalyzer(bt.analyzers.DrawDown, _name="drawdown")
    cerebro.addanalyzer(bt.analyzers.TradeAnalyzer, _name="trades")

    # Run backtest
    logger.info("Starting backtest...")
    logger.info("Initial capital: Rs%s", f"{config.trading.initial_capital:,.2f}")

    results = cerebro.run()
    strat = results[0]

    # Collect results
    custom_analysis = strat.analyzers.custom.get_analysis()
    sharpe = strat.analyzers.sharpe.get_analysis()
    _ = strat.analyzers.returns.get_analysis()  # Required for analyzer
    drawdown = strat.analyzers.drawdown.get_analysis()

    final_value = cerebro.broker.getvalue()

    # Compile report
    report = {
        "initial_capital": config.trading.initial_capital,
        "final_value": final_value,
        "total_return": (final_value - config.trading.initial_capital)
        / config.trading.initial_capital,
        "post_tax_return": custom_analysis.get("post_tax_return", 0),
        "sharpe_ratio": sharpe.get("sharperatio", 0),
        "max_drawdown": drawdown.get("max", {}).get("drawdown", 0) / 100,
        "total_trades": custom_analysis.get("total_trades", 0),
        "win_rate": custom_analysis.get("win_rate", 0),
        "profit_factor": custom_analysis.get("profit_factor", 0),
        "avg_trade_pnl": custom_analysis.get("avg_trade_pnl", 0),
        "total_commission": custom_analysis.get("total_commission", 0),
        "pre_tax_profit": custom_analysis.get("pre_tax_profit", 0),
        "tax_amount": custom_analysis.get("tax_amount", 0),
        "post_tax_profit": custom_analysis.get("post_tax_profit", 0),
    }

    # Calculate Calmar Ratio
    try:
        # Get start and end dates from the first data feed
        n_bars = len(strat.datas[0])
        if n_bars > 0:
            end_date = strat.datas[0].datetime.datetime(0)
            start_date = strat.datas[0].datetime.datetime(-n_bars + 1)
            duration_days = (end_date - start_date).days

            if duration_days > 0:
                duration_years = duration_days / 365.0
                annualized_return = (1 + report["total_return"]) ** (1 / duration_years) - 1
            else:
                annualized_return = 0
        else:
            annualized_return = 0

        if report["max_drawdown"] > 0:
            report["calmar_ratio"] = annualized_return / report["max_drawdown"]
        else:
            report["calmar_ratio"] = 0 if annualized_return <= 0 else float("inf")
    except Exception:
        report["calmar_ratio"] = 0

    # Include full data for charting if requested
    if return_full_data:
        report["equity_curve"] = strat.analyzers.custom.equity_curve
        report["trades"] = strat.analyzers.custom.trades

    return report


def print_report(report: dict):
    """Print formatted backtest report"""
    print("\n" + "=" * 60)
    print("BACKTEST RESULTS - Volatility Regime Adaptive Strategy")
    print("=" * 60)

    print("\nPERFORMANCE SUMMARY")
    print(f"  Initial Capital:    Rs{report['initial_capital']:>14,.2f}")
    print(f"  Final Value:        Rs{report['final_value']:>14,.2f}")
    print(f"  Total Return:       {report['total_return']:>15.2%}")
    print(f"  Post-Tax Return:    {report['post_tax_return']:>15.2%}")

    print("\nRISK METRICS")
    sharpe = report.get("sharpe_ratio")
    sharpe_str = f"{sharpe:.2f}" if sharpe is not None else "N/A"
    print(f"  Sharpe Ratio:       {sharpe_str:>15}")
    calmar = report.get("calmar_ratio", 0)
    print(f"  Calmar Ratio:       {calmar:>15.2f}")
    print(f"  Max Drawdown:       {report['max_drawdown']:>15.2%}")

    print("\nTRADE STATISTICS")
    print(f"  Total Trades:       {report['total_trades']:>15}")
    print(f"  Win Rate:           {report['win_rate']:>15.2%}")
    print(f"  Profit Factor:      {report['profit_factor']:>15.2f}")
    print(f"  Avg Trade P&L:      Rs{report['avg_trade_pnl']:>13,.2f}")

    print("\nFEE & TAX IMPACT")
    print(f"  Total Commission:   Rs{report['total_commission']:>13,.2f}")
    print(f"  Pre-Tax Profit:     Rs{report['pre_tax_profit']:>13,.2f}")
    print(f"  Tax (30%):          Rs{report['tax_amount']:>13,.2f}")
    print(f"  Post-Tax Profit:    Rs{report['post_tax_profit']:>13,.2f}")

    # Edge analysis
    edge_info = calculate_minimum_edge()
    print("\nMINIMUM EDGE REQUIREMENT")
    print(f"  {edge_info['recommendation']}")

    print("\n" + "=" * 60)


def main():
    """Main entry point"""
    parser = argparse.ArgumentParser(description="Run backtest for CoinDCX strategy")
    parser.add_argument("--config", type=str, help="Path to config file")
    parser.add_argument("--data-dir", type=str, default=None, help="Data directory (overrides config)")
    parser.add_argument("--start", type=str, help="Start date (YYYY-MM-DD)")
    parser.add_argument("--end", type=str, help="End date (YYYY-MM-DD)")
    parser.add_argument("--capital", type=float, default=None, help="Initial capital (overrides config)")
    parser.add_argument("-v", "--verbose", action="store_true", help="Verbose output")
    parser.add_argument("--chart", action="store_true", help="Generate trade visualization charts")
    parser.add_argument(
        "--output-dir", type=str, default="results", help="Output directory for charts"
    )

    args = parser.parse_args()

    # Setup logging - save to file if charting
    log_file = None
    if args.chart:
        Path(args.output_dir).mkdir(parents=True, exist_ok=True)
        log_file = Path(args.output_dir) / "backtest.log"

        # Clear any existing handlers
        root_logger = logging.getLogger()
        for handler in root_logger.handlers[:]:
            root_logger.removeHandler(handler)

        # Configure with file handler
        root_logger.setLevel(logging.DEBUG if args.verbose else logging.INFO)
        formatter = logging.Formatter("%(asctime)s - %(levelname)s - %(message)s")

        # Console handler
        console_handler = logging.StreamHandler()
        console_handler.setFormatter(formatter)
        root_logger.addHandler(console_handler)

        # File handler
        file_handler = logging.FileHandler(log_file, mode="w")
        file_handler.setFormatter(formatter)
        root_logger.addHandler(file_handler)
    else:
        setup_logging(level=logging.DEBUG if args.verbose else logging.INFO)

    # Load config
    if args.config:
        config = Config.load_from_file(args.config)
    else:
        config = get_default_config()

    # Apply CLI overrides
    if args.data_dir:
        config.backtest.data_dir = args.data_dir
    if args.start:
        config.backtest.start_date = args.start
    if args.end:
        config.backtest.end_date = args.end
    if args.capital:
        config.trading.initial_capital = args.capital

    # Run backtest
    report = run_backtest(config)

    # Print results
    print_report(report)

    print(report)

    # Generate charts if requested
    if args.chart and args.config and log_file:
        try:
            from .trade_visualizer import generate_all_charts

            logger.info("Generating trade visualization charts...")
            charts_dir = Path(args.output_dir) / "charts"
            charts_dir.mkdir(parents=True, exist_ok=True)
            
            generate_all_charts(
                log_file=str(log_file),
                config_file=args.config,
                backtest_metrics=report,
                data_dir=args.data_dir,
                output_dir=str(charts_dir),
            )
            logger.info("Charts saved to %s", charts_dir)
        except Exception as e:
            logger.error("Error generating charts: %s", e)

    return 0  # Success exit code


if __name__ == "__main__":
    sys.exit(main())
