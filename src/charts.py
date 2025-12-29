"""
Comprehensive Charting Module for Strategy Analysis

Uses Seaborn and Matplotlib for professional trading charts including:
- Equity curves with drawdowns
- Underwater (drawdown) charts
- Trade analysis
- Monthly returns heatmap
- Win/Loss distribution
- Rolling metrics
"""

import logging
from pathlib import Path
from typing import Any, Dict, List, Optional

import matplotlib.dates as mdates
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns
from matplotlib.gridspec import GridSpec

# Set up logging
logging.basicConfig(
    level=logging.INFO, format="%(asctime)s %(levelname)-8s [%(funcName)s:%(lineno)d] %(message)s"
)
logger = logging.getLogger(__name__)

# Set seaborn style
sns.set_theme(style="darkgrid", palette="husl")
plt.rcParams["figure.facecolor"] = "#1a1a2e"
plt.rcParams["axes.facecolor"] = "#16213e"
plt.rcParams["axes.edgecolor"] = "#e94560"
plt.rcParams["axes.labelcolor"] = "#eaeaea"
plt.rcParams["text.color"] = "#eaeaea"
plt.rcParams["xtick.color"] = "#eaeaea"
plt.rcParams["ytick.color"] = "#eaeaea"
plt.rcParams["grid.color"] = "#0f3460"
plt.rcParams["figure.figsize"] = (16, 10)


def calculate_drawdown(equity_curve: pd.Series) -> pd.DataFrame:
    """Calculate drawdown series from equity curve"""
    rolling_max = equity_curve.expanding().max()
    drawdown = (equity_curve - rolling_max) / rolling_max
    return pd.DataFrame(
        {
            "equity": equity_curve,
            "peak": rolling_max,
            "drawdown": drawdown,
            "drawdown_pct": drawdown * 100,
        }
    )


def calculate_rolling_metrics(equity_curve: pd.Series, window: int = 20) -> pd.DataFrame:
    """Calculate rolling performance metrics"""
    returns = equity_curve.pct_change().dropna()

    rolling_return = returns.rolling(window).mean() * 252  # Annualized
    rolling_vol = returns.rolling(window).std() * np.sqrt(252)
    rolling_sharpe = rolling_return / rolling_vol

    return pd.DataFrame(
        {
            "rolling_return": rolling_return,
            "rolling_vol": rolling_vol,
            "rolling_sharpe": rolling_sharpe,
        }
    )


def create_comprehensive_chart(
    equity_curve: List[Dict],
    trades: List[Dict],
    config_params: Dict[str, Any],
    metrics: Dict[str, Any],
    output_path: Optional[str] = None,
    title: str = "Strategy Performance Analysis",
) -> plt.Figure:
    """
    Create comprehensive multi-panel performance chart.

    Args:
        equity_curve: List of {'datetime': dt, 'value': float}
        trades: List of trade dictionaries
        config_params: Strategy configuration parameters
        metrics: Performance metrics dictionary
        output_path: Path to save chart
        title: Chart title

    Returns:
        Matplotlib figure
    """
    logger.info("Creating comprehensive chart: %s", title)

    # Convert to DataFrame
    if not equity_curve:
        logger.warning("No equity curve data provided")
        return None

    eq_df = pd.DataFrame(equity_curve)
    eq_df["datetime"] = pd.to_datetime(eq_df["datetime"])
    eq_df.set_index("datetime", inplace=True)
    eq_df = eq_df.sort_index()

    # Calculate drawdown
    dd_df = calculate_drawdown(eq_df["value"])

    # Calculate rolling metrics
    if len(eq_df) > 20:
        roll_df = calculate_rolling_metrics(eq_df["value"])
    else:
        roll_df = None

    # Create figure with subplots
    fig = plt.figure(figsize=(20, 14))
    gs = GridSpec(4, 3, figure=fig, hspace=0.3, wspace=0.25)

    # === Panel 1: Equity Curve with Trades ===
    ax1 = fig.add_subplot(gs[0, :2])
    ax1.fill_between(eq_df.index, eq_df["value"], alpha=0.3, color="#00d9ff")
    ax1.plot(eq_df.index, eq_df["value"], color="#00d9ff", linewidth=2, label="Portfolio Value")
    ax1.plot(
        eq_df.index,
        dd_df["peak"],
        color="#ff6b6b",
        linewidth=1,
        linestyle="--",
        alpha=0.7,
        label="Peak",
    )

    # Mark trades
    if trades:
        trade_df = pd.DataFrame(trades)
        wins = trade_df[trade_df["pnlcomm"] > 0]
        losses = trade_df[trade_df["pnlcomm"] <= 0]

        # We don't have exact trade dates, so skip marking for now

    ax1.set_title("📈 Equity Curve", fontsize=14, fontweight="bold", color="#00d9ff")
    ax1.set_ylabel("Portfolio Value (₹)", fontsize=11)
    ax1.legend(loc="upper left", facecolor="#16213e", edgecolor="#e94560")
    ax1.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))
    ax1.tick_params(axis="x", rotation=45)

    # === Panel 2: Key Metrics Box ===
    ax2 = fig.add_subplot(gs[0, 2])
    ax2.axis("off")

    metrics_text = f"""
╔══════════════════════════════════╗
║     PERFORMANCE METRICS          ║
╠══════════════════════════════════╣
║  Total Return:    {metrics.get('total_return', 0)*100:>8.2f}%    ║
║  Post-Tax Return: {metrics.get('post_tax_return', 0)*100:>8.2f}%    ║
║  Win Rate:        {metrics.get('win_rate', 0)*100:>8.1f}%    ║
║  Profit Factor:   {metrics.get('profit_factor', 0):>8.2f}     ║
║  Max Drawdown:    {metrics.get('max_drawdown', 0)*100:>8.2f}%    ║
║  Total Trades:    {metrics.get('total_trades', 0):>8d}     ║
║  Sharpe Ratio:    {metrics.get('sharpe_ratio', 0) or 0:>8.2f}     ║
╚══════════════════════════════════╝

╔══════════════════════════════════╗
║     CONFIGURATION                ║
╠══════════════════════════════════╣
║  Symbols:     {str(config_params.get('symbols', 'N/A')):<18} ║
║  Timeframe:   {str(config_params.get('timeframe', '4h')):<18} ║
║  ADX Thresh:  {config_params.get('adx_threshold', 25):<18} ║
║  Stop ATR:    {config_params.get('stop_atr_multiple', 3.0):<18} ║
║  Target ATR:  {config_params.get('target_atr_multiple', 6.0):<18} ║
╚══════════════════════════════════╝
"""
    ax2.text(
        0.1,
        0.95,
        metrics_text,
        transform=ax2.transAxes,
        fontsize=10,
        verticalalignment="top",
        fontfamily="monospace",
        color="#00ff88",
        bbox={"boxstyle": "round", "facecolor": "#0f3460", "alpha": 0.9},
    )

    # === Panel 3: Underwater (Drawdown) Chart ===
    ax3 = fig.add_subplot(gs[1, :2])
    ax3.fill_between(
        eq_df.index,
        dd_df["drawdown_pct"],
        0,
        where=dd_df["drawdown_pct"] < 0,
        color="#ff6b6b",
        alpha=0.7,
    )
    ax3.plot(eq_df.index, dd_df["drawdown_pct"], color="#ff6b6b", linewidth=1)
    ax3.axhline(y=0, color="#eaeaea", linestyle="-", linewidth=0.5)
    ax3.axhline(
        y=-10, color="#ffaa00", linestyle="--", linewidth=1, alpha=0.7, label="Warning (-10%)"
    )
    ax3.axhline(
        y=-20, color="#ff0000", linestyle="--", linewidth=1, alpha=0.7, label="Critical (-20%)"
    )
    ax3.set_title("🔻 Underwater Chart (Drawdown)", fontsize=14, fontweight="bold", color="#ff6b6b")
    ax3.set_ylabel("Drawdown (%)", fontsize=11)
    ax3.set_ylim(min(dd_df["drawdown_pct"].min() * 1.1, -25), 5)
    ax3.legend(loc="lower left", facecolor="#16213e", edgecolor="#e94560")
    ax3.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))
    ax3.tick_params(axis="x", rotation=45)

    # === Panel 4: Trade PnL Distribution ===
    ax4 = fig.add_subplot(gs[1, 2])
    if trades:
        trade_pnls = [t["pnlcomm"] for t in trades]
        # Colors determined per-bar in histogram

        # Histogram
        ax4.hist(
            trade_pnls,
            bins=min(20, len(trade_pnls)),
            color="#00d9ff",
            alpha=0.7,
            edgecolor="#eaeaea",
        )
        ax4.axvline(x=0, color="#ffaa00", linestyle="--", linewidth=2)
        ax4.axvline(
            x=np.mean(trade_pnls),
            color="#00ff88",
            linestyle="-",
            linewidth=2,
            label=f"Mean: ₹{np.mean(trade_pnls):.0f}",
        )
    ax4.set_title("📊 Trade PnL Distribution", fontsize=14, fontweight="bold", color="#00d9ff")
    ax4.set_xlabel("PnL (₹)", fontsize=11)
    ax4.set_ylabel("Frequency", fontsize=11)
    if trades:
        ax4.legend(facecolor="#16213e", edgecolor="#e94560")

    # === Panel 5: Rolling Sharpe ===
    ax5 = fig.add_subplot(gs[2, :2])
    if roll_df is not None and not roll_df["rolling_sharpe"].isna().all():
        ax5.plot(roll_df.index, roll_df["rolling_sharpe"], color="#ff9f43", linewidth=2)
        ax5.axhline(y=0, color="#ff6b6b", linestyle="-", linewidth=1, alpha=0.7)
        ax5.axhline(y=1, color="#00ff88", linestyle="--", linewidth=1, alpha=0.7, label="Good (>1)")
        ax5.axhline(
            y=2, color="#00d9ff", linestyle="--", linewidth=1, alpha=0.7, label="Excellent (>2)"
        )
        ax5.legend(loc="upper left", facecolor="#16213e", edgecolor="#e94560")
    ax5.set_title(
        "📉 Rolling Sharpe Ratio (20-period)", fontsize=14, fontweight="bold", color="#ff9f43"
    )
    ax5.set_ylabel("Sharpe Ratio", fontsize=11)
    ax5.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))
    ax5.tick_params(axis="x", rotation=45)

    # === Panel 6: Win/Loss Breakdown ===
    ax6 = fig.add_subplot(gs[2, 2])
    if trades:
        wins = sum(1 for t in trades if t["pnlcomm"] > 0)
        losses = len(trades) - wins
        sizes = [wins, losses]
        labels = [f"Wins\n{wins}", f"Losses\n{losses}"]
        colors_pie = ["#00ff88", "#ff6b6b"]
        explode = (0.05, 0)

        ax6.pie(
            sizes,
            labels=labels,
            colors=colors_pie,
            explode=explode,
            autopct="%1.1f%%",
            shadow=True,
            startangle=90,
            textprops={"color": "#eaeaea", "fontsize": 11},
        )
        ax6.set_title("🎯 Win/Loss Ratio", fontsize=14, fontweight="bold", color="#00d9ff")

    # === Panel 7: Trade Sequence ===
    ax7 = fig.add_subplot(gs[3, :2])
    if trades:
        trade_pnls = [t["pnlcomm"] for t in trades]
        cumulative_pnl = np.cumsum(trade_pnls)
        colors_bar = ["#00ff88" if p > 0 else "#ff6b6b" for p in trade_pnls]

        x = range(len(trade_pnls))
        ax7.bar(x, trade_pnls, color=colors_bar, alpha=0.8, edgecolor="#eaeaea", linewidth=0.5)

        # Overlay cumulative line
        ax7_twin = ax7.twinx()
        ax7_twin.plot(
            x,
            cumulative_pnl,
            color="#00d9ff",
            linewidth=2,
            marker="o",
            markersize=4,
            label="Cumulative PnL",
        )
        ax7_twin.set_ylabel("Cumulative PnL (₹)", color="#00d9ff", fontsize=11)
        ax7_twin.tick_params(axis="y", labelcolor="#00d9ff")

    ax7.set_title(
        "📈 Trade Sequence & Cumulative PnL", fontsize=14, fontweight="bold", color="#00ff88"
    )
    ax7.set_xlabel("Trade #", fontsize=11)
    ax7.set_ylabel("Trade PnL (₹)", fontsize=11)
    ax7.axhline(y=0, color="#ffaa00", linestyle="-", linewidth=1, alpha=0.7)

    # === Panel 8: Monthly Returns (if enough data) ===
    ax8 = fig.add_subplot(gs[3, 2])
    if len(eq_df) > 30:
        try:
            monthly_returns = eq_df["value"].resample("ME").last().pct_change().dropna() * 100
            if len(monthly_returns) > 0:
                colors_monthly = ["#00ff88" if r > 0 else "#ff6b6b" for r in monthly_returns]
                ax8.bar(
                    range(len(monthly_returns)), monthly_returns, color=colors_monthly, alpha=0.8
                )
                ax8.set_xticks(range(len(monthly_returns)))
                ax8.set_xticklabels([d.strftime("%b") for d in monthly_returns.index], rotation=45)
                ax8.axhline(y=0, color="#ffaa00", linestyle="-", linewidth=1)
        except Exception as e:
            logger.warning("Could not calculate monthly returns: %s", e)
    ax8.set_title("📅 Monthly Returns", fontsize=14, fontweight="bold", color="#00d9ff")
    ax8.set_ylabel("Return (%)", fontsize=11)

    # Main title
    fig.suptitle(title, fontsize=18, fontweight="bold", color="#00d9ff", y=0.98)

    # Save
    if output_path:
        Path(output_path).parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(
            output_path, dpi=150, bbox_inches="tight", facecolor="#1a1a2e", edgecolor="none"
        )
        logger.info("Chart saved to: %s", output_path)

    return fig


def create_comparison_chart(
    results: List[Dict],
    output_path: Optional[str] = None,
    title: str = "Top Configurations Comparison",
) -> plt.Figure:
    """
    Create comparison chart for multiple configurations.

    Args:
        results: List of optimization results with metrics
        output_path: Path to save chart
        title: Chart title
    """
    logger.info("Creating comparison chart for %d configurations", len(results))

    if not results:
        logger.warning("No results to compare")
        return None

    fig, axes = plt.subplots(2, 2, figsize=(16, 12))

    # Prepare data
    labels = [
        f"#{i+1}\n{r.get('symbols', 'N/A')}\n{r.get('timeframe', '4h')}"
        for i, r in enumerate(results)
    ]
    returns = [r.get("total_return", 0) * 100 for r in results]
    win_rates = [r.get("win_rate", 0) * 100 for r in results]
    profit_factors = [min(r.get("profit_factor", 0), 10) for r in results]  # Cap at 10 for viz
    max_dds = [r.get("max_drawdown", 0) * 100 for r in results]

    x = np.arange(len(labels))

    # Returns comparison
    ax1 = axes[0, 0]
    colors = ["#00ff88" if r > 0 else "#ff6b6b" for r in returns]
    bars = ax1.bar(x, returns, color=colors, alpha=0.8, edgecolor="#eaeaea")
    ax1.axhline(y=0, color="#ffaa00", linestyle="-", linewidth=1)
    ax1.set_title("📈 Total Return (%)", fontsize=14, fontweight="bold", color="#00d9ff")
    ax1.set_xticks(x)
    ax1.set_xticklabels(labels, fontsize=9)
    for bar, val in zip(bars, returns):
        ax1.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height() + 0.5,
            f"{val:.1f}%",
            ha="center",
            va="bottom",
            fontsize=10,
            color="#eaeaea",
        )

    # Win Rate comparison
    ax2 = axes[0, 1]
    bars = ax2.bar(x, win_rates, color="#00d9ff", alpha=0.8, edgecolor="#eaeaea")
    ax2.axhline(y=50, color="#ffaa00", linestyle="--", linewidth=1, label="Break-even")
    ax2.set_title("🎯 Win Rate (%)", fontsize=14, fontweight="bold", color="#00d9ff")
    ax2.set_xticks(x)
    ax2.set_xticklabels(labels, fontsize=9)
    ax2.set_ylim(0, 110)
    ax2.legend(facecolor="#16213e")
    for bar, val in zip(bars, win_rates):
        ax2.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height() + 2,
            f"{val:.0f}%",
            ha="center",
            va="bottom",
            fontsize=10,
            color="#eaeaea",
        )

    # Profit Factor comparison
    ax3 = axes[1, 0]
    colors = ["#00ff88" if pf > 1 else "#ff6b6b" for pf in profit_factors]
    bars = ax3.bar(x, profit_factors, color=colors, alpha=0.8, edgecolor="#eaeaea")
    ax3.axhline(y=1, color="#ffaa00", linestyle="--", linewidth=1, label="Break-even")
    ax3.axhline(y=1.5, color="#00ff88", linestyle="--", linewidth=1, alpha=0.7, label="Good (>1.5)")
    ax3.set_title("💰 Profit Factor", fontsize=14, fontweight="bold", color="#00ff88")
    ax3.set_xticks(x)
    ax3.set_xticklabels(labels, fontsize=9)
    ax3.legend(facecolor="#16213e")
    for bar, val in zip(bars, profit_factors):
        display_val = "∞" if val >= 10 else f"{val:.2f}"
        ax3.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height() + 0.1,
            display_val,
            ha="center",
            va="bottom",
            fontsize=10,
            color="#eaeaea",
        )

    # Max Drawdown comparison
    ax4 = axes[1, 1]
    bars = ax4.bar(x, max_dds, color="#ff6b6b", alpha=0.8, edgecolor="#eaeaea")
    ax4.axhline(y=10, color="#ffaa00", linestyle="--", linewidth=1, label="Warning (10%)")
    ax4.axhline(y=20, color="#ff0000", linestyle="--", linewidth=1, label="Critical (20%)")
    ax4.set_title("🔻 Max Drawdown (%)", fontsize=14, fontweight="bold", color="#ff6b6b")
    ax4.set_xticks(x)
    ax4.set_xticklabels(labels, fontsize=9)
    ax4.legend(facecolor="#16213e")
    ax4.invert_yaxis()  # Lower is better
    for bar, val in zip(bars, max_dds):
        ax4.text(
            bar.get_x() + bar.get_width() / 2,
            bar.get_height() + 0.5,
            f"{val:.1f}%",
            ha="center",
            va="bottom",
            fontsize=10,
            color="#eaeaea",
        )

    fig.suptitle(title, fontsize=18, fontweight="bold", color="#00d9ff", y=0.98)
    plt.tight_layout()

    if output_path:
        Path(output_path).parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(
            output_path, dpi=150, bbox_inches="tight", facecolor="#1a1a2e", edgecolor="none"
        )
        logger.info("Comparison chart saved to: %s", output_path)

    return fig
