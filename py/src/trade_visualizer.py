"""
Trade Visualization Module - Price Charts with Trade Markers and Indicators

Creates detailed charts showing:
- Price action with buy/sell markers
- Entry, exit, stop loss, and target levels
- Technical indicators (EMA, ADX, ATR)
- Trade annotations with profit/loss
"""

import json
import logging
from datetime import datetime, timedelta
from pathlib import Path
from typing import Dict, List, Optional

import matplotlib.dates as mdates
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
from matplotlib.gridspec import GridSpec

# Set up logging
logging.basicConfig(
    level=logging.INFO, format="%(asctime)s %(levelname)-8s [%(funcName)s:%(lineno)d] %(message)s"
)
logger = logging.getLogger(__name__)

# Professional dark theme
plt.rcParams.update(
    {
        "figure.facecolor": "#0d1117",
        "axes.facecolor": "#161b22",
        "axes.edgecolor": "#30363d",
        "axes.labelcolor": "#c9d1d9",
        "text.color": "#c9d1d9",
        "xtick.color": "#8b949e",
        "ytick.color": "#8b949e",
        "grid.color": "#21262d",
        "grid.linestyle": "--",
        "grid.alpha": 0.5,
        "figure.figsize": (20, 12),
        "font.size": 10,
    }
)


def calculate_indicators(df: pd.DataFrame, config: Dict) -> pd.DataFrame:
    """Calculate all technical indicators used by the strategy"""

    # EMA (Common)
    if "ema_fast" in config:
        ema_fast = config.get("ema_fast", 8)
        df["ema_fast"] = df["close"].ewm(span=ema_fast, adjust=False).mean()

    if "ema_slow" in config:
        ema_slow = config.get("ema_slow", 21)
        df["ema_slow"] = df["close"].ewm(span=ema_slow, adjust=False).mean()

    # Bollinger Bands
    if "bb_period" in config:
        bb_period = config.get("bb_period", 20)
        bb_dev = config.get("bb_dev", 2.0)
        df["bb_mid"] = df["close"].rolling(window=bb_period).mean()
        df["bb_std"] = df["close"].rolling(window=bb_period).std()
        df["bb_top"] = df["bb_mid"] + (df["bb_std"] * bb_dev)
        df["bb_bot"] = df["bb_mid"] - (df["bb_std"] * bb_dev)

    # RSI
    if "rsi_period" in config:
        rsi_period = config.get("rsi_period", 14)
        delta = df["close"].diff()
        gain = (delta.where(delta > 0, 0)).rolling(window=rsi_period).mean()
        loss = (-delta.where(delta < 0, 0)).rolling(window=rsi_period).mean()
        rs = gain / loss
        df["rsi"] = 100 - (100 / (1 + rs))

    # ATR (Common)
    if "atr_period" in config:
        atr_period = config.get("atr_period", 14)
        high_low = df["high"] - df["low"]
        high_close = np.abs(df["high"] - df["close"].shift())
        low_close = np.abs(df["low"] - df["close"].shift())
        tr = pd.concat([high_low, high_close, low_close], axis=1).max(axis=1)
        df["atr"] = tr.rolling(window=atr_period).mean()

        # ADX (Depends on ATR calculation)
        if "adx_period" in config:
            adx_period = config.get("adx_period", 14)
            plus_dm = df["high"].diff()
            minus_dm = df["low"].diff().abs() * -1
            plus_dm[plus_dm < 0] = 0
            minus_dm[minus_dm > 0] = 0
            minus_dm = minus_dm.abs()

            tr_smooth = tr.rolling(window=adx_period).sum()
            plus_di = 100 * (plus_dm.rolling(window=adx_period).sum() / tr_smooth)
            minus_di = 100 * (minus_dm.rolling(window=adx_period).sum() / tr_smooth)

            dx = 100 * np.abs(plus_di - minus_di) / (plus_di + minus_di)
            df["adx"] = dx.rolling(window=adx_period).mean()
            df["plus_di"] = plus_di
            df["minus_di"] = minus_di

        # Volatility regime (Depends on ATR)
        if "volatility_lookback" in config:
            volatility_lookback = config.get("volatility_lookback", 20)
            df["atr_ma"] = df["atr"].rolling(window=volatility_lookback).mean()
            df["volatility_ratio"] = df["atr"] / df["atr_ma"]

    return df


def parse_trade_log(log_lines: List[str]) -> List[Dict]:
    """Parse trade log entries into structured data

    Log format: '2025-12-29 13:37:28,468 - INFO - 2023-10-24 BUY CREATE for BTCINR: ...'
    The trade date is after 'INFO - ' and before the action.
    """
    trades = []
    pending_trades = {}  # Track pending trades by symbol
    last_sold_symbol = None  # Track the last symbol that was sold

    def extract_trade_date(line: str) -> Optional[datetime]:
        """Extract the trade date from log line (after INFO - )"""
        try:
            # Split by ' - INFO - ' to get the message part
            if " - INFO - " in line:
                msg_part = line.split(" - INFO - ")[1]
                # First word should be the date
                date_str = msg_part.split()[0]
                return datetime.strptime(date_str, "%Y-%m-%d")
        except:
            pass
        return None

    for line in log_lines:
        if "BUY CREATE" in line:
            trade_date = extract_trade_date(line)
            if not trade_date:
                continue

            # Extract symbol - format: "BUY CREATE for BTCINR:"
            if " for " in line and ":" in line:
                try:
                    after_for = line.split(" for ")[1]
                    symbol = after_for.split(":")[0].strip()

                    trade = {"symbol": symbol, "entry_date": trade_date, "type": "BUY"}

                    # Extract levels from "Entry=2882302.22, Stop=2601601.13, Target=3443704.40"
                    if "Entry=" in line:
                        try:
                            entry = float(line.split("Entry=")[1].split(",")[0])
                            trade["entry_price"] = entry
                        except:
                            pass
                    if "Stop=" in line:
                        try:
                            stop = float(line.split("Stop=")[1].split(",")[0])
                            trade["stop_loss"] = stop
                        except:
                            pass
                    if "Target=" in line:
                        try:
                            target_str = (
                                line.split("Target=")[1].split(",")[0].split("\n")[0].strip()
                            )
                            trade["target"] = float(target_str)
                        except:
                            pass

                    pending_trades[symbol] = trade
                except:
                    pass

        elif "BUY EXECUTED" in line:
            # Format: "BUY EXECUTED for BTCINR: Price=2882302.22, Size=0.0087"
            if " for " in line:
                try:
                    symbol = line.split(" for ")[1].split(":")[0].strip()
                    if symbol in pending_trades:
                        price = float(line.split("Price=")[1].split(",")[0])
                        pending_trades[symbol]["executed_price"] = price
                        # Update entry date to execution date
                        exec_date = extract_trade_date(line)
                        if exec_date:
                            pending_trades[symbol]["entry_date"] = exec_date
                except:
                    pass

        elif "TARGET hit" in line or "STOP LOSS triggered" in line or "TREND EXIT" in line:
            # Format: "TARGET hit for SOLINR"
            if " for " in line:
                try:
                    symbol = line.split(" for ")[1].split("\n")[0].strip()
                    if symbol in pending_trades:
                        exit_date = extract_trade_date(line)
                        if exit_date:
                            pending_trades[symbol]["exit_date"] = exit_date

                        if "TARGET hit" in line:
                            pending_trades[symbol]["exit_reason"] = "TARGET"
                        elif "STOP LOSS" in line:
                            pending_trades[symbol]["exit_reason"] = "STOP LOSS"
                        elif "TREND EXIT" in line:
                            pending_trades[symbol]["exit_reason"] = "TREND EXIT"
                except:
                    pass

        elif "CLOSING position" in line:
            # Format: "CLOSING position for BTCINR at 3080100.00"
            if " for " in line and " at " in line:
                try:
                    symbol = line.split(" for ")[1].split(" at ")[0].strip()
                    price = float(line.split(" at ")[1].split("\n")[0].strip())
                    if symbol in pending_trades:
                        pending_trades[symbol]["close_price"] = price
                        exit_date = extract_trade_date(line)
                        if exit_date:
                            pending_trades[symbol]["exit_date"] = exit_date
                except:
                    pass

        elif "SELL EXECUTED" in line:
            # Format: "SELL EXECUTED for BTCINR: Price=3120674.60, Size=-0.0087"
            if " for " in line:
                try:
                    symbol = line.split(" for ")[1].split(":")[0].strip()
                    if symbol in pending_trades:
                        price = float(line.split("Price=")[1].split(",")[0])
                        pending_trades[symbol]["exit_price"] = price
                        last_sold_symbol = symbol

                        # Get exit date if not already set
                        if "exit_date" not in pending_trades[symbol]:
                            exit_date = extract_trade_date(line)
                            if exit_date:
                                pending_trades[symbol]["exit_date"] = exit_date
                except:
                    pass

        elif "TRADE CLOSED" in line:
            # Format: "TRADE CLOSED: Gross=2067.55, Comm=0.52, Net=2067.03, Post-Tax=1446.92"
            # This comes right after SELL EXECUTED
            try:
                pnl_str = line.split("Net=")[1].split(",")[0]
                pnl = float(pnl_str)

                # Use the last sold symbol to identify the trade
                if last_sold_symbol and last_sold_symbol in pending_trades:
                    trade = pending_trades[last_sold_symbol]
                    trade["pnl"] = pnl

                    # Ensure entry_price is set
                    if "entry_price" not in trade:
                        trade["entry_price"] = trade.get("executed_price", 0)

                    # Trade is complete
                    trades.append(trade.copy())
                    del pending_trades[last_sold_symbol]
                    last_sold_symbol = None
            except Exception as e:
                logger.debug("Error parsing TRADE CLOSED: %s", e)

    logger.info("Parsed %d trades from log", len(trades))
    return trades


def load_price_data(symbol: str, timeframe: str, data_dir: str = "data") -> pd.DataFrame:
    """Load price data for a symbol"""
    filepath = Path(data_dir) / f"{symbol}_{timeframe}.csv"
    if not filepath.exists():
        filepath = Path(data_dir) / f"{symbol}.csv"

    if not filepath.exists():
        logger.warning("Data file not found: %s", filepath)
        return pd.DataFrame()

    df = pd.read_csv(filepath, parse_dates=["datetime"])
    df.set_index("datetime", inplace=True)
    df = df.sort_index()
    return df


def create_trade_chart(
    symbol: str,
    trades: List[Dict],
    config: Dict,
    data_dir: str = "data",
    output_dir: str = "results",
    show_all_data: bool = False,
) -> Optional[plt.Figure]:
    """
    Create detailed price chart with trade markers and indicators.

    Args:
        symbol: Trading symbol (e.g., 'BTCINR')
        trades: List of trade dictionaries for this symbol
        config: Strategy configuration
        data_dir: Directory containing price data
        output_dir: Directory to save charts
        show_all_data: Show full data or just around trades

    Returns:
        Matplotlib figure
    """
    timeframe = config.get("timeframe", "1d")
    df = load_price_data(symbol, timeframe, data_dir)

    if df.empty:
        logger.warning("No data available for %s", symbol)
        return None

    # Calculate indicators
    df = calculate_indicators(df, config)

    # Filter trades for this symbol
    symbol_trades = [t for t in trades if t.get("symbol") == symbol]

    if not symbol_trades and not show_all_data:
        logger.info("No trades for %s", symbol)
        return None

    # Determine panels to show based on available data
    panels = []
    if "adx" in df.columns:
        panels.append("adx")
    if "atr" in df.columns:
        panels.append("atr")
    if "volatility_ratio" in df.columns:
        panels.append("vol_ratio")
    if "rsi" in df.columns:
        panels.append("rsi")

    # Height ratios: Main chart gets 3, others get 1
    height_ratios = [3] + [1] * len(panels)
    total_height = 10 + 2 * len(panels)

    # Create figure with subplots
    fig = plt.figure(figsize=(24, total_height))
    gs = GridSpec(len(height_ratios), 1, figure=fig, height_ratios=height_ratios, hspace=0.1)

    # === Main Price Chart ===
    ax1 = fig.add_subplot(gs[0])

    # Plot price line with fill
    ax1.fill_between(df.index, df["low"], df["high"], alpha=0.1, color="#58a6ff")
    ax1.plot(df.index, df["close"], color="#58a6ff", linewidth=1.5, label="Close", alpha=0.8)

    # Plot EMAs if available
    if "ema_fast" in df.columns:
        ema_fast = config.get("ema_fast", 8)
        ax1.plot(
            df.index,
            df["ema_fast"],
            color="#f0883e",
            linewidth=1.2,
            linestyle="-",
            label=f"EMA {ema_fast}",
            alpha=0.9,
        )
    if "ema_slow" in df.columns:
        ema_slow = config.get("ema_slow", 21)
        ax1.plot(
            df.index,
            df["ema_slow"],
            color="#a371f7",
            linewidth=1.2,
            linestyle="-",
            label=f"EMA {ema_slow}",
            alpha=0.9,
        )

    # Plot Bollinger Bands if available
    if "bb_top" in df.columns:
        ax1.plot(
            df.index,
            df["bb_top"],
            color="#8b949e",
            linewidth=1,
            linestyle="--",
            alpha=0.5,
            label="BB Top",
        )
        ax1.plot(
            df.index,
            df["bb_bot"],
            color="#8b949e",
            linewidth=1,
            linestyle="--",
            alpha=0.5,
            label="BB Bot",
        )
        ax1.fill_between(df.index, df["bb_top"], df["bb_bot"], color="#8b949e", alpha=0.05)

    # Plot trades
    for trade in symbol_trades:
        entry_date = trade.get("entry_date")
        exit_date = trade.get("exit_date")
        entry_price = trade.get("entry_price") or trade.get("executed_price")
        exit_price = trade.get("exit_price")
        stop_loss = trade.get("stop_loss")
        target = trade.get("target")
        pnl = trade.get("pnl", 0)
        exit_reason = trade.get("exit_reason", "Unknown")

        if entry_date and entry_price:
            # Entry marker (green triangle up)
            ax1.scatter(
                entry_date,
                entry_price,
                marker="^",
                s=200,
                c="#3fb950",
                edgecolors="white",
                linewidths=2,
                zorder=5,
                label="_nolegend_",
            )
            ax1.annotate(
                f"BUY\nRs{entry_price:,.0f}",
                xy=(entry_date, entry_price),
                xytext=(10, 30),
                textcoords="offset points",
                fontsize=9,
                color="#3fb950",
                fontweight="bold",
                bbox={
                    "boxstyle": "round,pad=0.3",
                    "facecolor": "#161b22",
                    "edgecolor": "#3fb950",
                    "alpha": 0.9,
                },
                arrowprops={"arrowstyle": "->", "color": "#3fb950", "lw": 1.5},
            )

            # Draw horizontal lines for stop and target if we have exit date
            if exit_date:
                line_end = exit_date
            else:
                line_end = entry_date + timedelta(days=30)

            # Stop loss line (red dashed)
            if stop_loss:
                ax1.hlines(
                    y=stop_loss,
                    xmin=entry_date,
                    xmax=line_end,
                    colors="#f85149",
                    linestyles="--",
                    linewidth=1.5,
                    alpha=0.7,
                )
                ax1.annotate(
                    f"SL: Rs{stop_loss:,.0f}",
                    xy=(entry_date, stop_loss),
                    xytext=(5, -15),
                    textcoords="offset points",
                    fontsize=8,
                    color="#f85149",
                    alpha=0.8,
                )

            # Target line (green dashed)
            if target:
                ax1.hlines(
                    y=target,
                    xmin=entry_date,
                    xmax=line_end,
                    colors="#3fb950",
                    linestyles="--",
                    linewidth=1.5,
                    alpha=0.7,
                )
                ax1.annotate(
                    f"TGT: Rs{target:,.0f}",
                    xy=(entry_date, target),
                    xytext=(5, 10),
                    textcoords="offset points",
                    fontsize=8,
                    color="#3fb950",
                    alpha=0.8,
                )

        if exit_date and exit_price:
            # Exit marker
            marker_color = "#3fb950" if pnl > 0 else "#f85149"
            marker = "v"  # Triangle down for sell

            ax1.scatter(
                exit_date,
                exit_price,
                marker=marker,
                s=200,
                c=marker_color,
                edgecolors="white",
                linewidths=2,
                zorder=5,
                label="_nolegend_",
            )

            # P&L annotation
            pnl_text = f"{exit_reason}\nRs{exit_price:,.0f}\nP&L: Rs{pnl:+,.0f}"
            ax1.annotate(
                pnl_text,
                xy=(exit_date, exit_price),
                xytext=(10, -40),
                textcoords="offset points",
                fontsize=9,
                color=marker_color,
                fontweight="bold",
                bbox={
                    "boxstyle": "round,pad=0.3",
                    "facecolor": "#161b22",
                    "edgecolor": marker_color,
                    "alpha": 0.9,
                },
                arrowprops={"arrowstyle": "->", "color": marker_color, "lw": 1.5},
            )

            # Draw trade connection line
            if entry_date and entry_price:
                ax1.plot(
                    [entry_date, exit_date],
                    [entry_price, exit_price],
                    color=marker_color,
                    linewidth=2,
                    linestyle=":",
                    alpha=0.5,
                )

    ax1.set_title(
        f"{symbol} - Price Chart with Trades ({timeframe})",
        fontsize=16,
        fontweight="bold",
        color="#58a6ff",
        pad=20,
    )
    ax1.set_ylabel("Price (Rs)", fontsize=12)
    ax1.legend(loc="upper left", facecolor="#161b22", edgecolor="#30363d")
    ax1.grid(True, alpha=0.3)
    ax1.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))

    # === Dynamic Subplots ===
    axes = [ax1]

    for i, panel in enumerate(panels):
        ax = fig.add_subplot(gs[i + 1], sharex=ax1)
        axes.append(ax)

        if panel == "adx":
            adx_threshold = config.get("adx_threshold", 30)
            ax.plot(df.index, df["adx"], color="#f0883e", linewidth=1.5, label="ADX")
            if "plus_di" in df.columns:
                ax.plot(
                    df.index, df["plus_di"], color="#3fb950", linewidth=1, alpha=0.7, label="+DI"
                )
                ax.plot(
                    df.index, df["minus_di"], color="#f85149", linewidth=1, alpha=0.7, label="-DI"
                )
            ax.axhline(
                y=adx_threshold,
                color="#8b949e",
                linestyle="--",
                linewidth=1,
                label=f"Threshold ({adx_threshold})",
            )
            ax.fill_between(
                df.index,
                adx_threshold,
                df["adx"],
                where=df["adx"] >= adx_threshold,
                color="#3fb950",
                alpha=0.2,
            )
            ax.set_ylabel("ADX", fontsize=11)
            ax.set_ylim(0, 60)
            ax.legend(loc="upper left", facecolor="#161b22", edgecolor="#30363d", ncol=4)
            ax.grid(True, alpha=0.3)

        elif panel == "atr":
            ax.plot(df.index, df["atr"], color="#a371f7", linewidth=1.5, label="ATR")
            if "atr_ma" in df.columns:
                ax.plot(
                    df.index,
                    df["atr_ma"],
                    color="#8b949e",
                    linewidth=1,
                    linestyle="--",
                    label="ATR MA",
                    alpha=0.7,
                )
            ax.set_ylabel("ATR", fontsize=11)
            ax.legend(loc="upper left", facecolor="#161b22", edgecolor="#30363d")
            ax.grid(True, alpha=0.3)

        elif panel == "vol_ratio":
            compression_threshold = config.get("compression_threshold", 0.6)
            expansion_threshold = config.get("expansion_threshold", 1.5)

            ax.plot(
                df.index, df["volatility_ratio"], color="#58a6ff", linewidth=1.5, label="Vol Ratio"
            )
            ax.axhline(y=1.0, color="#8b949e", linestyle="-", linewidth=1, alpha=0.5)
            ax.axhline(
                y=compression_threshold,
                color="#3fb950",
                linestyle="--",
                linewidth=1,
                label=f"Compression ({compression_threshold})",
            )
            ax.axhline(
                y=expansion_threshold,
                color="#f85149",
                linestyle="--",
                linewidth=1,
                label=f"Expansion ({expansion_threshold})",
            )
            ax.fill_between(
                df.index,
                0,
                df["volatility_ratio"],
                where=df["volatility_ratio"] <= compression_threshold,
                color="#3fb950",
                alpha=0.2,
                label="_nolegend_",
            )
            ax.fill_between(
                df.index,
                df["volatility_ratio"],
                3,
                where=df["volatility_ratio"] >= expansion_threshold,
                color="#f85149",
                alpha=0.2,
                label="_nolegend_",
            )
            ax.set_ylabel("Vol Ratio", fontsize=11)
            ax.set_ylim(0, 3)
            ax.legend(loc="upper left", facecolor="#161b22", edgecolor="#30363d", ncol=3)
            ax.grid(True, alpha=0.3)

        elif panel == "rsi":
            rsi_oversold = config.get("rsi_oversold", 30)
            rsi_overbought = config.get("rsi_overbought", 70)

            ax.plot(df.index, df["rsi"], color="#a371f7", linewidth=1.5, label="RSI")
            ax.axhline(
                y=rsi_oversold, color="#3fb950", linestyle="--", linewidth=1, label="Oversold"
            )
            ax.axhline(
                y=rsi_overbought, color="#f85149", linestyle="--", linewidth=1, label="Overbought"
            )
            ax.fill_between(df.index, 0, rsi_oversold, color="#3fb950", alpha=0.1)
            ax.fill_between(df.index, rsi_overbought, 100, color="#f85149", alpha=0.1)
            ax.set_ylabel("RSI", fontsize=11)
            ax.set_ylim(0, 100)
            ax.legend(loc="upper left", facecolor="#161b22", edgecolor="#30363d")
            ax.grid(True, alpha=0.3)

    # Hide x-axis labels for upper panels
    for ax in axes[:-1]:
        plt.setp(ax.get_xticklabels(), visible=False)

    # Format x-axis for the last panel
    last_ax = axes[-1]
    last_ax.set_xlabel("Date", fontsize=12)
    last_ax.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m-%d"))
    plt.setp(last_ax.xaxis.get_majorticklabels(), rotation=45, ha="right")

    plt.tight_layout()

    # Save chart
    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)
    chart_file = output_path / f"{symbol}_{timeframe}_trades.png"
    fig.savefig(chart_file, dpi=150, bbox_inches="tight", facecolor="#0d1117")
    logger.info("Saved chart: %s", chart_file)

    return fig


def create_trade_summary_chart(
    trades: List[Dict], config: Dict, backtest_metrics: Dict, output_dir: str = "results"
) -> plt.Figure:
    """Create summary chart of all trades across symbols"""

    fig = plt.figure(figsize=(20, 14))
    gs = GridSpec(3, 3, figure=fig, hspace=0.3, wspace=0.3)

    # === Trade Log Table ===
    ax1 = fig.add_subplot(gs[0, :])
    ax1.axis("off")

    if trades:
        # Create table data
        table_data = []
        for i, t in enumerate(trades[:20], 1):  # Show first 20 trades
            symbol = t.get("symbol", "N/A")
            entry_date = t.get("entry_date", "")
            if isinstance(entry_date, datetime):
                entry_date = entry_date.strftime("%Y-%m-%d")
            exit_date = t.get("exit_date", "")
            if isinstance(exit_date, datetime):
                exit_date = exit_date.strftime("%Y-%m-%d")
            entry_price = t.get("entry_price", 0)
            exit_price = t.get("exit_price", 0)
            pnl = t.get("pnl", 0)
            exit_reason = t.get("exit_reason", "N/A")

            table_data.append(
                [
                    i,
                    symbol,
                    entry_date,
                    f"Rs{entry_price:,.0f}" if entry_price else "N/A",
                    exit_date,
                    f"Rs{exit_price:,.0f}" if exit_price else "N/A",
                    exit_reason,
                    f"Rs{pnl:+,.0f}" if pnl else "N/A",
                ]
            )

        columns = [
            "#",
            "Symbol",
            "Entry Date",
            "Entry Price",
            "Exit Date",
            "Exit Price",
            "Exit Reason",
            "P&L",
        ]

        table = ax1.table(
            cellText=table_data,
            colLabels=columns,
            cellLoc="center",
            loc="center",
            colColours=["#30363d"] * len(columns),
        )
        table.auto_set_font_size(False)
        table.set_fontsize(9)
        table.scale(1, 1.5)

        # Color P&L cells
        for i, t in enumerate(trades[:20]):
            pnl = t.get("pnl", 0)
            cell = table[(i + 1, 7)]
            if pnl > 0:
                cell.set_facecolor("#238636")
            elif pnl < 0:
                cell.set_facecolor("#da3633")
            else:
                cell.set_facecolor("#30363d")
            cell.set_text_props(color="white")

    ax1.set_title(
        "Trade Log (First 20 Trades)", fontsize=14, fontweight="bold", color="#58a6ff", pad=20
    )

    # === P&L by Symbol ===
    ax2 = fig.add_subplot(gs[1, 0])
    if trades:
        symbol_pnl = {}
        for t in trades:
            sym = t.get("symbol", "Unknown")
            pnl = t.get("pnl", 0)
            symbol_pnl[sym] = symbol_pnl.get(sym, 0) + pnl

        symbols = list(symbol_pnl.keys())
        pnls = list(symbol_pnl.values())
        colors = ["#3fb950" if p > 0 else "#f85149" for p in pnls]

        h_bars = ax2.barh(symbols, pnls, color=colors, edgecolor="white", linewidth=0.5)
        ax2.axvline(x=0, color="#8b949e", linewidth=1)

        for h_bar, pnl in zip(h_bars, pnls):
            width = h_bar.get_width()
            ax2.text(
                width + (500 if width > 0 else -500),
                h_bar.get_y() + h_bar.get_height() / 2,
                f"Rs{pnl:+,.0f}",
                va="center",
                ha="left" if width > 0 else "right",
                fontsize=9,
                color="white",
            )

    ax2.set_title("P&L by Symbol", fontsize=12, fontweight="bold", color="#58a6ff")
    ax2.set_xlabel("P&L (Rs)")

    # === Trades by Exit Reason ===
    ax3 = fig.add_subplot(gs[1, 1])
    if trades:
        exit_counts = {}
        for t in trades:
            reason = t.get("exit_reason", "Unknown")
            exit_counts[reason] = exit_counts.get(reason, 0) + 1

        reasons = list(exit_counts.keys())
        counts = list(exit_counts.values())
        colors = {
            "TARGET": "#3fb950",
            "STOP LOSS": "#f85149",
            "TREND EXIT": "#f0883e",
            "Unknown": "#8b949e",
        }
        bar_colors = [colors.get(r, "#8b949e") for r in reasons]

        ax3.bar(reasons, counts, color=bar_colors, edgecolor="white", linewidth=0.5)

        for i, (r, c) in enumerate(zip(reasons, counts)):
            ax3.text(i, c + 0.3, str(c), ha="center", fontsize=10, fontweight="bold", color="white")

    ax3.set_title("Trades by Exit Reason", fontsize=12, fontweight="bold", color="#58a6ff")
    ax3.set_ylabel("Count")

    # === Win Rate by Symbol ===
    ax4 = fig.add_subplot(gs[1, 2])
    if trades:
        symbol_stats = {}
        for t in trades:
            sym = t.get("symbol", "Unknown")
            pnl = t.get("pnl", 0)
            if sym not in symbol_stats:
                symbol_stats[sym] = {"wins": 0, "total": 0}
            symbol_stats[sym]["total"] += 1
            if pnl > 0:
                symbol_stats[sym]["wins"] += 1

        symbols = list(symbol_stats.keys())
        win_rates = [
            s["wins"] / s["total"] * 100 if s["total"] > 0 else 0 for s in symbol_stats.values()
        ]

        colors = ["#3fb950" if wr >= 50 else "#f85149" for wr in win_rates]
        v_bars = ax4.bar(symbols, win_rates, color=colors, edgecolor="white", linewidth=0.5)
        ax4.axhline(y=50, color="#8b949e", linestyle="--", linewidth=1, label="50%")

        for v_bar, wr in zip(v_bars, win_rates):
            ax4.text(
                v_bar.get_x() + v_bar.get_width() / 2,
                v_bar.get_height() + 2,
                f"{wr:.0f}%",
                ha="center",
                fontsize=10,
                fontweight="bold",
                color="white",
            )

    ax4.set_title("Win Rate by Symbol", fontsize=12, fontweight="bold", color="#58a6ff")
    ax4.set_ylabel("Win Rate (%)")
    ax4.set_ylim(0, 110)

    # === Metrics Summary ===
    ax5 = fig.add_subplot(gs[2, :2])
    ax5.axis("off")

    metrics_text = f"""
    +===========================================================================+
    |                       BACKTEST RESULTS SUMMARY                            |
    +===========================================================================+
    |                                                                           |
    |   RETURNS                            TRADE STATISTICS                     |
    |   ---------------------              ---------------------                |
    |   Total Return:     {backtest_metrics.get('total_return', 0)*100:>8.2f}%           Total Trades:    {backtest_metrics.get('total_trades', 0):>8}    |
    |   Post-Tax Return:  {backtest_metrics.get('post_tax_return', 0)*100:>8.2f}%           Win Rate:        {backtest_metrics.get('win_rate', 0)*100:>7.1f}%    |
    |   Pre-Tax Profit:   Rs{backtest_metrics.get('pre_tax_profit', 0):>9,.0f}           Profit Factor:   {backtest_metrics.get('profit_factor', 0):>8.2f}    |
    |   Post-Tax Profit:  Rs{backtest_metrics.get('post_tax_profit', 0):>9,.0f}           Avg Trade P&L:   Rs{backtest_metrics.get('avg_trade_pnl', 0):>6,.0f}    |
    |                                                                           |
    |   RISK METRICS                       FEES & TAXES                         |
    |   ---------------------              ---------------------                |
    |   Sharpe Ratio:     {backtest_metrics.get('sharpe_ratio', 0) or 0:>8.2f}           Commission:      Rs{backtest_metrics.get('total_commission', 0):>6,.0f}    |
    |   Max Drawdown:     {backtest_metrics.get('max_drawdown', 0)*100:>7.2f}%           Tax (30%):       Rs{backtest_metrics.get('tax_amount', 0):>6,.0f}    |
    |                                                                           |
    +===========================================================================+
    """

    ax5.text(
        0.05,
        0.95,
        metrics_text,
        transform=ax5.transAxes,
        fontsize=11,
        verticalalignment="top",
        fontfamily="monospace",
        color="#58a6ff",
        bbox={"boxstyle": "round", "facecolor": "#161b22", "edgecolor": "#30363d", "alpha": 0.95},
    )

    # === Configuration ===
    ax6 = fig.add_subplot(gs[2, 2])
    ax6.axis("off")

    config_text = f"""
    +===============================+
    |     STRATEGY CONFIG           |
    +===============================+
    | Timeframe:    {config.get('timeframe', '1d'):<15} |
    | ADX Threshold:{config.get('adx_threshold', 30):<15} |
    | Stop ATR:     {config.get('stop_atr_multiple', 2.5):<15} |
    | Target ATR:   {config.get('target_atr_multiple', 5.0):<15} |
    | EMA Fast:     {config.get('ema_fast', 8):<15} |
    | EMA Slow:     {config.get('ema_slow', 21):<15} |
    | Compression:  {config.get('compression_threshold', 0.6):<15} |
    +===============================+
    """

    ax6.text(
        0.1,
        0.95,
        config_text,
        transform=ax6.transAxes,
        fontsize=10,
        verticalalignment="top",
        fontfamily="monospace",
        color="#3fb950",
        bbox={"boxstyle": "round", "facecolor": "#161b22", "edgecolor": "#30363d", "alpha": 0.95},
    )

    fig.suptitle(
        "Strategy Backtest Analysis", fontsize=18, fontweight="bold", color="#58a6ff", y=0.98
    )

    plt.tight_layout()

    # Save
    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)
    chart_file = output_path / "trade_summary.png"
    fig.savefig(chart_file, dpi=150, bbox_inches="tight", facecolor="#0d1117")
    logger.info("Saved summary chart: %s", chart_file)

    return fig


def generate_all_charts(
    log_file: str,
    config_file: str,
    backtest_metrics: Dict,
    data_dir: str = "data",
    output_dir: str = "results",
):
    """
    Generate all trade visualization charts.

    Args:
        log_file: Path to backtest log file
        config_file: Path to config JSON file
        backtest_metrics: Backtest metrics dictionary
        data_dir: Directory with price data
        output_dir: Directory to save charts
    """
    # Load config
    with open(config_file, "r", encoding="utf-8") as f:
        config_data = json.load(f)

    # Flatten config if nested
    config = {}
    if "strategy" in config_data:
        config.update(config_data.get("strategy", {}))
        config.update(config_data.get("trading", {}))
        config.update(config_data.get("backtest", {}))
    else:
        config = config_data

    # Get symbols
    symbols = config.get("pairs", config.get("symbols", []))
    timeframe = config.get("timeframe", "1d")
    config["timeframe"] = timeframe

    # Parse log file
    with open(log_file, "r", encoding="utf-8") as f:
        log_lines = f.readlines()

    trades = parse_trade_log(log_lines)
    logger.info("Parsed %d trades from log", len(trades))

    # Generate chart for each symbol
    for symbol in symbols:
        create_trade_chart(symbol, trades, config, data_dir, output_dir)

    # Generate summary chart
    create_trade_summary_chart(trades, config, backtest_metrics, output_dir)

    logger.info("All charts saved to %s/", output_dir)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Generate trade visualization charts")
    parser.add_argument("--log", type=str, required=True, help="Path to backtest log file")
    parser.add_argument("--config", type=str, required=True, help="Path to config JSON file")
    parser.add_argument("--data-dir", type=str, default="data", help="Data directory")
    parser.add_argument("--output-dir", type=str, default="results", help="Output directory")

    args = parser.parse_args()

    # Mock metrics for standalone use
    metrics = {
        "total_return": 0,
        "post_tax_return": 0,
        "win_rate": 0,
        "profit_factor": 0,
        "max_drawdown": 0,
        "total_trades": 0,
        "sharpe_ratio": 0,
        "avg_trade_pnl": 0,
        "total_commission": 0,
        "tax_amount": 0,
        "pre_tax_profit": 0,
        "post_tax_profit": 0,
    }

    generate_all_charts(args.log, args.config, metrics, args.data_dir, args.output_dir)
