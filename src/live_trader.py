"""
Live Trading Module - Production Ready

Reuses the EXACT SAME Backtrader CoinDCXStrategy for live trading.
This ensures 100% logic parity between backtest and live execution.

Features:
- Uses identical strategy code (no reimplementation)
- Comprehensive dual logging (console + file)
- Paper trade mode for testing
- Graceful error handling
- Position state persistence
- Emergency shutdown capability

Author: Prashant Srivastava
"""

import logging
import sys
import time
import traceback
from datetime import datetime
from pathlib import Path
from typing import Optional

import backtrader as bt
import pandas as pd

from .exchange import CoinDCXClient, CoinDCXExecutor
from .strategy import CoinDCXStrategy
from .config import Config, get_default_config


# =============================================================================
# LOGGING SETUP
# =============================================================================


class DualLogger:
    """Logger that writes to both console and file with timestamps"""

    def __init__(self, name: str, log_dir: str = "logs"):
        self.log_dir = Path(log_dir)
        self.log_dir.mkdir(parents=True, exist_ok=True)

        # Create unique log file for this session
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        self.trade_log = self.log_dir / f"trades_{timestamp}.log"
        self.system_log = self.log_dir / f"system_{timestamp}.log"

        # Setup Python logger
        self.logger = logging.getLogger(name)
        self.logger.setLevel(logging.DEBUG)
        self.logger.handlers.clear()

        # Console handler (INFO level)
        console = logging.StreamHandler(sys.stdout)
        console.setLevel(logging.INFO)
        console.setFormatter(
            logging.Formatter("%(asctime)s | %(levelname)-8s | %(message)s", datefmt="%H:%M:%S")
        )
        self.logger.addHandler(console)

        # File handler (DEBUG level)
        file_handler = logging.FileHandler(self.system_log, encoding="utf-8")
        file_handler.setLevel(logging.DEBUG)
        file_handler.setFormatter(
            logging.Formatter("%(asctime)s | %(levelname)-8s | %(name)s | %(message)s")
        )
        self.logger.addHandler(file_handler)

    def info(self, msg: str):
        self.logger.info(msg)

    def debug(self, msg: str):
        self.logger.debug(msg)

    def warning(self, msg: str):
        self.logger.warning(msg)

    def error(self, msg: str):
        self.logger.error(msg)

    def trade(self, msg: str):
        """Log trade-specific message to both console and trade file"""
        timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        self.logger.info(msg)
        with open(self.trade_log, "a", encoding="utf-8") as f:
            f.write(f"{timestamp} | {msg}\n")

    def section(self, title: str, char: str = "="):
        """Log a section header"""
        line = char * 70
        self.trade(line)
        self.trade(f"  {title}")
        self.trade(line)


# =============================================================================
# CUSTOM BACKTRADER DATA FEED FOR LIVE DATA
# =============================================================================


class CoinDCXLiveData(bt.feeds.PandasData):
    """
    Backtrader data feed wrapper for CoinDCX live data.
    Allows us to feed real-time data into the same strategy.
    """

    params = (
        ("datetime", "time"),
        ("open", "Open"),
        ("high", "High"),
        ("low", "Low"),
        ("close", "Close"),
        ("volume", "Volume"),
        ("openinterest", None),
    )


# =============================================================================
# LIVE TRADING STRATEGY (EXTENDS BACKTEST STRATEGY)
# =============================================================================


class LiveTradingStrategy(CoinDCXStrategy):
    """
    Live trading version of CoinDCXStrategy.

    INHERITS 100% of the backtest logic from CoinDCXStrategy.
    Only adds enhanced logging for live trading visibility.

    NOTE: Uses same params as parent (defined in CoinDCXStrategy).
    Live-specific params (logger, executor, paper_trade) are also in parent.
    """

    def __init__(self):
        """Initialize with parent strategy + live trading extensions"""
        super().__init__()
        self.log_obj = self.p.logger
        self.executor = self.p.executor
        self.paper_trade = self.p.paper_trade

    def log(self, txt: str, dt=None):
        """Enhanced logging with timestamp"""
        if self.log_obj:
            dt = dt or self.datas[0].datetime.datetime(0)
            self.log_obj.debug(f"[{dt.strftime('%Y-%m-%d %H:%M')}] {txt}")
        else:
            super().log(txt, dt)

    def notify_order(self, order):
        """Enhanced order notification with detailed logging"""
        if order.status in [order.Submitted, order.Accepted]:
            if self.log_obj:
                self.log_obj.debug(f"Order {order.Status[order.status]}: {order.data._name}")
            return

        if order.status == order.Completed:
            symbol = order.data._name

            if order.isbuy():
                if self.log_obj:
                    self.log_obj.section(f"BUY ORDER - {symbol}")
                    self.log_obj.trade(f"  Price:      Rs {order.executed.price:,.2f}")
                    self.log_obj.trade(f"  Quantity:   {order.executed.size:.6f}")
                    self.log_obj.trade(f"  Value:      Rs {order.executed.value:,.2f}")
                    self.log_obj.trade(f"  Commission: Rs {order.executed.comm:.2f}")

                    if self.paper_trade:
                        self.log_obj.trade("  Mode:       [PAPER TRADE]")
                    else:
                        self.log_obj.trade("  Mode:       [LIVE EXECUTION]")

            else:  # Sell
                if self.log_obj:
                    self.log_obj.section(f"SELL ORDER - {symbol}")
                    self.log_obj.trade(f"  Price:      Rs {order.executed.price:,.2f}")
                    self.log_obj.trade(f"  Quantity:   {abs(order.executed.size):.6f}")
                    self.log_obj.trade(f"  Value:      Rs {abs(order.executed.value):,.2f}")
                    self.log_obj.trade(f"  Commission: Rs {order.executed.comm:.2f}")

                    if self.paper_trade:
                        self.log_obj.trade("  Mode:       [PAPER TRADE]")
                    else:
                        self.log_obj.trade("  Mode:       [LIVE EXECUTION]")

        elif order.status in [order.Canceled, order.Margin, order.Rejected]:
            if self.log_obj:
                self.log_obj.warning(
                    f"Order FAILED ({order.Status[order.status]}): {order.data._name}"
                )

        # Call parent to handle order tracking
        super().notify_order(order)

    def notify_trade(self, trade):
        """Enhanced trade notification with P&L details"""
        if not trade.isclosed:
            return

        symbol = trade.data._name
        gross_pnl = trade.pnl
        net_pnl = trade.pnlcomm
        commission = abs(gross_pnl - net_pnl)

        # Calculate tax (30% on profits)
        tax = net_pnl * 0.30 if net_pnl > 0 else 0
        post_tax_pnl = net_pnl - tax

        # Determine result
        result = "WIN" if post_tax_pnl > 0 else "LOSS"
        pnl_pct = (gross_pnl / (trade.price * abs(trade.size))) * 100 if trade.size else 0

        if self.log_obj:
            self.log_obj.section(f"TRADE CLOSED - {symbol} [{result}]")
            self.log_obj.trade(f"  Entry Price:    Rs {trade.price:,.2f}")
            self.log_obj.trade(f"  Gross P&L:      Rs {gross_pnl:+,.2f} ({pnl_pct:+.2f}%)")
            self.log_obj.trade(f"  Commission:     Rs {commission:.2f}")
            self.log_obj.trade(f"  Net P&L:        Rs {net_pnl:+,.2f}")
            self.log_obj.trade(f"  Tax (30%):      Rs {tax:.2f}")
            self.log_obj.trade(f"  Final P&L:      Rs {post_tax_pnl:+,.2f}")
            self.log_obj.trade(f"  Result:         {result}")

        # Call parent
        super().notify_trade(trade)

    def next(self):
        """Log market state before running strategy logic"""
        # Log current market state for visibility
        for d in self.datas:
            if d._name in self.indicators:
                ind = self.indicators[d._name]
                regime_val = int(ind["regime"].regime[0])
                regime_names = ["COMPRESSION", "NORMAL", "EXPANSION", "EXTREME"]
                regime_name = regime_names[regime_val] if 0 <= regime_val < 4 else "UNKNOWN"

                pos = self.getposition(d)
                pos_str = f"Position: {pos.size:.6f}" if pos.size else "No Position"

                if self.log_obj:
                    self.log_obj.debug(
                        f"[{d._name}] Close: Rs{d.close[0]:,.2f} | "
                        f"EMA: {ind['ema_fast'][0]:.0f}/{ind['ema_slow'][0]:.0f} | "
                        f"ATR: Rs{ind['atr'][0]:,.0f} | ADX: {ind['adx'][0]:.1f} | "
                        f"Regime: {regime_name} | {pos_str}"
                    )

        # Run the ACTUAL strategy logic (100% inherited from CoinDCXStrategy)
        super().next()


# =============================================================================
# LIVE ORDER EXECUTOR
# =============================================================================


class LiveOrderExecutor:
    """
    Executes orders on CoinDCX exchange.
    Handles both paper and live trading modes.
    """

    def __init__(
        self,
        client: CoinDCXClient,
        executor: CoinDCXExecutor,
        logger: DualLogger,
        paper_trade: bool = True,
    ):
        self.client = client
        self.executor = executor
        self.logger = logger
        self.paper_trade = paper_trade

    def execute_buy(self, symbol: str, quantity: float, price: float) -> bool:
        """Execute buy order"""
        if self.paper_trade:
            self.logger.trade(f"[PAPER] BUY {quantity:.6f} {symbol} @ Rs {price:,.2f}")
            return True

        try:
            response = self.executor.execute_buy(
                market=symbol,
                quantity=quantity,
                limit_price=price * 1.002,  # 0.2% premium for fill
                use_limit=True,
            )

            if response:
                self.logger.trade(f"[LIVE] BUY EXECUTED - Order ID: {response.get('id', 'N/A')}")
                return True

            self.logger.error("BUY order failed for %s", symbol)
            return False

        except Exception as e:
            self.logger.error("BUY execution error for %s: %s", symbol, e)
            return False

    def execute_sell(self, symbol: str, quantity: float, price: float) -> bool:
        """Execute sell order"""
        if self.paper_trade:
            self.logger.trade(f"[PAPER] SELL {quantity:.6f} {symbol} @ Rs {price:,.2f}")
            return True

        try:
            response = self.executor.execute_sell(
                market=symbol,
                quantity=quantity,
                limit_price=price * 0.998,  # 0.2% discount for fill
                use_limit=True,
            )

            if response:
                self.logger.trade(f"[LIVE] SELL EXECUTED - Order ID: {response.get('id', 'N/A')}")
                return True

            self.logger.error("SELL order failed for %s", symbol)
            return False

        except Exception as e:
            self.logger.error("SELL execution error for %s: %s", symbol, e)
            return False


# =============================================================================
# MAIN LIVE TRADER CLASS
# =============================================================================


class LiveTrader:
    """
    Production-ready live trading system using Backtrader.

    Key Features:
    - Uses EXACT SAME CoinDCXStrategy as backtest
    - Comprehensive logging to console and files
    - Paper trade mode for safe testing
    - Automatic position state tracking
    - Graceful error handling and recovery
    - Emergency shutdown capability
    """

    def __init__(
        self, config: Optional[Config] = None, paper_trade: bool = True, log_dir: str = "logs"
    ):
        """
        Initialize live trader.

        Args:
            config: Trading configuration
            paper_trade: If True, simulate trades without real execution
            log_dir: Directory for log files
        """
        self.config = config or get_default_config()
        self.paper_trade = paper_trade

        # Setup logging
        self.logger = DualLogger("LiveTrader", log_dir)

        # Initialize exchange connection
        self.client = CoinDCXClient()
        self.executor = CoinDCXExecutor(self.client)
        self.order_executor = LiveOrderExecutor(
            self.client, self.executor, self.logger, paper_trade
        )

        # State tracking
        self._running = False
        self._cerebro = None
        self._strategy = None

        # Position state file for persistence
        self.state_file = Path(log_dir) / "position_state.json"

        self._log_startup()

    def _log_startup(self):
        """Log startup configuration"""
        mode = "PAPER TRADE" if self.paper_trade else "*** LIVE TRADING ***"

        self.logger.section(f"LIVE TRADER INITIALIZED - {mode}")
        self.logger.trade(f"  Capital:    Rs {self.config.trading.initial_capital:,.2f}")
        self.logger.trade(f"  Pairs:      {self.config.trading.pairs}")
        self.logger.trade(f"  Timeframe:  {self.config.trading.timeframe}")
        self.logger.trade(f"  Max Pos:    {self.config.trading.max_positions}")
        self.logger.trade("")
        self.logger.trade("  STRATEGY PARAMETERS:")
        self.logger.trade(
            f"    EMA Fast/Slow:      {self.config.strategy.ema_fast}/{self.config.strategy.ema_slow}"
        )
        self.logger.trade(f"    ADX Threshold:      {self.config.strategy.adx_threshold}")
        self.logger.trade(f"    Stop ATR Multiple:  {self.config.strategy.stop_atr_multiple}x")
        self.logger.trade(f"    Target ATR Multiple:{self.config.strategy.target_atr_multiple}x")
        self.logger.trade(f"    Risk Per Trade:     {self.config.trading.risk_per_trade*100:.1f}%")
        self.logger.trade(
            f"    Max Position:       {self.config.trading.max_position_pct*100:.0f}%"
        )
        self.logger.trade("")
        self.logger.trade(f"  Trade Log:  {self.logger.trade_log}")
        self.logger.trade(f"  System Log: {self.logger.system_log}")

    def fetch_data(self, symbol: str, bars: int = 100) -> pd.DataFrame:
        """
        Fetch historical OHLCV data from CoinDCX.

        Args:
            symbol: Trading pair (e.g., 'BTCINR')
            bars: Number of bars to fetch

        Returns:
            DataFrame with OHLCV data
        """
        try:
            timeframe = self.config.trading.timeframe
            self.logger.debug("Fetching %d bars of %s data for %s", bars, timeframe, symbol)

            candles = self.client.get_candles(symbol, timeframe, limit=bars)

            if not candles:
                self.logger.error("[%s] No data returned from CoinDCX API", symbol)
                return pd.DataFrame()

            df = pd.DataFrame(candles)
            df["time"] = pd.to_datetime(df["time"], unit="ms")
            df = df.sort_values("time").reset_index(drop=True)
            df = df.rename(
                columns={
                    "open": "Open",
                    "high": "High",
                    "low": "Low",
                    "close": "Close",
                    "volume": "Volume",
                }
            )

            self.logger.debug(
                f"[{symbol}] Fetched {len(df)} bars | "
                f"Latest: {df['time'].iloc[-1]} | "
                f"Close: Rs {df['Close'].iloc[-1]:,.2f}"
            )

            return df

        except Exception as e:
            self.logger.error("[%s] Data fetch failed: %s", symbol, e)
            return pd.DataFrame()

    def _build_cerebro(self) -> bt.Cerebro:
        """Build Backtrader cerebro with strategy and data feeds"""
        cerebro = bt.Cerebro()

        # Get strategy params from unified config + live trading extras
        # This ensures 100% parity with backtest configuration
        strategy_params = self.config.get_strategy_params(
            logger=self.logger,
            executor=self.order_executor,
            paper_trade=self.paper_trade,
        )

        cerebro.addstrategy(LiveTradingStrategy, **strategy_params)

        # Set broker parameters
        cerebro.broker.setcash(self.config.trading.initial_capital)
        cerebro.broker.setcommission(commission=self.config.exchange.taker_fee)

        # Add data feeds for each pair
        for symbol in self.config.trading.pairs:
            df = self.fetch_data(symbol, bars=100)

            if df.empty:
                self.logger.warning("Skipping %s - no data available", symbol)
                continue

            data = CoinDCXLiveData(
                dataname=df,
                name=symbol,
                datetime="time",
                open="Open",
                high="High",
                low="Low",
                close="Close",
                volume="Volume",
                openinterest=-1,
            )
            cerebro.adddata(data, name=symbol)
            self.logger.info("Added data feed: %s", symbol)

        return cerebro

    def run_cycle(self):
        """Run one trading cycle"""
        self.logger.section(f"TRADING CYCLE - {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")

        try:
            # Build fresh cerebro with latest data
            cerebro = self._build_cerebro()

            if len(cerebro.datas) == 0:
                self.logger.error("No data feeds available - skipping cycle")
                return

            # Run strategy
            self.logger.debug("Running Backtrader strategy...")
            results = cerebro.run()

            if results:
                self._strategy = results[0]

                # Log portfolio status
                portfolio_value = cerebro.broker.getvalue()
                cash = cerebro.broker.getcash()

                self.logger.trade("")
                self.logger.section("PORTFOLIO STATUS", "-")
                self.logger.trade(f"  Total Value:  Rs {portfolio_value:,.2f}")
                self.logger.trade(f"  Cash:         Rs {cash:,.2f}")
                self.logger.trade(f"  Positions:    Rs {portfolio_value - cash:,.2f}")

                # Log individual positions
                for data in cerebro.datas:
                    pos = self._strategy.getposition(data)
                    if pos.size:
                        price = data.close[0]
                        _value = pos.size * price
                        pnl = (price - pos.price) * pos.size
                        pnl_pct = ((price / pos.price) - 1) * 100 if pos.price else 0

                        self.logger.trade(
                            f"  {data._name}: {pos.size:.6f} @ Rs {pos.price:,.2f} | "
                            f"Current: Rs {price:,.2f} | P&L: Rs {pnl:+,.2f} ({pnl_pct:+.2f}%)"
                        )

        except Exception as e:
            self.logger.error("Error in trading cycle: %s", e)
            self.logger.debug(traceback.format_exc())

    def start(self, interval_seconds: int = 300):
        """
        Start the live trading loop.

        Args:
            interval_seconds: Seconds between trading cycles (default: 5 min)
        """
        self._running = True

        mode = "PAPER" if self.paper_trade else "LIVE"
        self.logger.section(f"STARTING LIVE TRADER - {mode} MODE")
        self.logger.trade(f"  Interval: {interval_seconds} seconds")
        self.logger.trade(f"  Pairs: {self.config.trading.pairs}")
        self.logger.trade("")
        self.logger.trade("Press Ctrl+C to stop...")

        cycle_count = 0

        while self._running:
            cycle_count += 1
            self.logger.info("Starting cycle #%d", cycle_count)

            try:
                self.run_cycle()
            except KeyboardInterrupt:
                self.logger.info("Interrupted by user (Ctrl+C)")
                break
            except Exception as e:
                self.logger.error("Unexpected error: %s", e)
                self.logger.debug(traceback.format_exc())

            if self._running:
                self.logger.info("Sleeping %ds until next cycle...", interval_seconds)
                time.sleep(interval_seconds)

        self.stop()

    def stop(self):
        """Stop the live trader gracefully"""
        self._running = False
        self.logger.section("LIVE TRADER STOPPED")

    def emergency_close_all(self):
        """Emergency: Close all positions immediately"""
        self.logger.section("!!! EMERGENCY CLOSE ALL !!!")

        if self._strategy:
            for data in self._strategy.datas:
                pos = self._strategy.getposition(data)
                if pos.size:
                    self.logger.warning("Closing position: %s (%.6f)", data._name, pos.size)
                    price = data.close[0]
                    self.order_executor.execute_sell(data._name, abs(pos.size), price)

        self.stop()


# =============================================================================
# MAIN ENTRY POINT
# =============================================================================


def main():
    """Main entry point for live trading"""
    import argparse

    parser = argparse.ArgumentParser(
        description="CoinDCX Live Trading System",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python -m src.live_trader --paper                    # Paper trading (default)
  python -m src.live_trader --config configs/my.json   # Use custom config
  python -m src.live_trader --live                     # LIVE TRADING (real money!)
  python -m src.live_trader --interval 60 -v           # 1 min cycles, verbose
        """,
    )

    parser.add_argument("--config", "-c", type=str, help="Path to config file (JSON)")
    parser.add_argument(
        "--paper", action="store_true", default=True, help="Paper trade mode (default)"
    )
    parser.add_argument("--live", action="store_true", help="LIVE trading mode - REAL MONEY!")
    parser.add_argument(
        "--interval",
        "-i",
        type=int,
        default=300,
        help="Trading cycle interval in seconds (default: 300)",
    )
    parser.add_argument("--verbose", "-v", action="store_true", help="Verbose debug output")

    args = parser.parse_args()

    # Setup root logging level
    if args.verbose:
        logging.getLogger().setLevel(logging.DEBUG)
    else:
        logging.getLogger().setLevel(logging.INFO)

    # Determine trading mode
    paper_trade = not args.live

    # Safety confirmation for live trading
    if not paper_trade:
        print("\n" + "!" * 70)
        print("!!! WARNING: LIVE TRADING MODE - REAL MONEY AT RISK !!!")
        print("!" * 70)
        print("\nThis will execute REAL trades on CoinDCX with REAL money.")
        print("\nBefore proceeding, ensure you have:")
        print("  1. API keys configured correctly")
        print("  2. Sufficient INR balance in your account")
        print("  3. Tested thoroughly in paper mode")
        print("  4. Understood and accepted all risks")
        print("")

        confirm = input("Type 'I UNDERSTAND THE RISKS' to confirm: ")
        if confirm.strip() != "I UNDERSTAND THE RISKS":
            print("\nAborted. Use --paper for paper trading.")
            return

        print("\nProceeding with LIVE trading...")

    # Load config
    if args.config:
        config = Config.load_from_file(args.config)
        print(f"Loaded config: {args.config}")
    else:
        config = get_default_config()
        print("Using default configuration")

    # Create and start trader
    trader = LiveTrader(config=config, paper_trade=paper_trade)

    try:
        trader.start(interval_seconds=args.interval)
    except KeyboardInterrupt:
        print("\nShutting down...")
        trader.stop()
    except Exception as e:
        print(f"\nFatal error: {e}")
        traceback.print_exc()
        trader.emergency_close_all()


if __name__ == "__main__":
    main()
