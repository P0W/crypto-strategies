"""
Live Trading Module - Production Ready

Reuses the EXACT SAME Backtrader CoinDCXStrategy for live trading.
This ensures 100% logic parity between backtest and live execution.

Features:
- Uses identical strategy code (no reimplementation)
- Comprehensive dual logging (console + file)
- Paper trade mode for testing
- Graceful error handling
- Position state persistence (SQLite + JSON backup)
- Automatic crash recovery
- Emergency shutdown capability

Author: Prashant Srivastava
"""

import hashlib
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
from .strategies import get_strategy_class
from .config import Config, get_default_config
from .risk import RiskManager, RiskLimits, RiskAction, Position as RiskPosition
from .state_manager import (
    StateManager,
    Position,
    Checkpoint,
    create_state_manager,
    get_open_positions,
    close_position,
)


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

    def info(self, msg: str, *args):
        if args:
            msg = msg % args
        self.logger.info(msg)

    def debug(self, msg: str, *args):
        if args:
            msg = msg % args
        self.logger.debug(msg)

    def warning(self, msg: str, *args):
        if args:
            msg = msg % args
        self.logger.warning(msg)

    def error(self, msg: str, *args):
        if args:
            msg = msg % args
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


class LiveTradingMixin:
    """
    Live trading mixin.

    Adds enhanced logging for live trading visibility.
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

                # Execute via executor if available
                if self.executor:
                    # Try to extract stop/target from strategy if available
                    stop_loss = getattr(self, "stop_prices", {}).get(symbol, 0.0)
                    take_profit = getattr(self, "target_prices", {}).get(symbol, 0.0)

                    self.executor.execute_buy(
                        symbol=symbol,
                        quantity=order.executed.size,
                        price=order.executed.price,
                        stop_loss=stop_loss,
                        take_profit=take_profit,
                    )

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

                # Execute via executor if available
                if self.executor:
                    self.executor.execute_sell(
                        symbol=symbol,
                        quantity=abs(order.executed.size),
                        price=order.executed.price,
                        exit_reason="Strategy Signal",
                    )

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

                # Generic indicator logging
                ind_str_parts = []
                for name, indicator in ind.items():
                    try:
                        # Try to get the first line value
                        val = indicator[0]
                        if isinstance(val, (int, float)):
                            ind_str_parts.append(f"{name}: {val:.2f}")
                    except:
                        pass

                ind_str = " | ".join(ind_str_parts)
                pos = self.getposition(d)
                pos_str = f"Position: {pos.size:.6f}" if pos.size else "No Position"

                if self.log_obj:
                    self.log_obj.debug(
                        f"[{d._name}] Close: Rs{d.close[0]:,.2f} | " f"{ind_str} | {pos_str}"
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
    Integrates with state manager for persistence.
    """

    def __init__(
        self,
        client: CoinDCXClient,
        executor: CoinDCXExecutor,
        logger: DualLogger,
        paper_trade: bool = True,
        state_manager: Optional[StateManager] = None,
        risk_manager: Optional[RiskManager] = None,
    ):
        self.client = client
        self.executor = executor
        self.logger = logger
        self.paper_trade = paper_trade
        self.state_manager = state_manager
        self.risk_manager = risk_manager

    def execute_buy(
        self,
        symbol: str,
        quantity: float,
        price: float,
        stop_loss: float = 0.0,
        take_profit: float = 0.0,
        atr: float = 0.0,
        market_state: str = "",
    ) -> bool:
        """Execute buy order with state persistence and full context"""
        
        # --- RISK MANAGEMENT CHECKS ---
        if self.risk_manager:
            # 1. Check if trading is allowed
            action = self.risk_manager.can_trade()
            if action == RiskAction.BLOCK:
                self.logger.warning(f"RiskManager BLOCKED buy order for {symbol}")
                return False
            
            # 2. Calculate safe size
            # If stop_loss is 0, assume 5% risk for calculation safety
            stop_distance = price - stop_loss if stop_loss > 0 else price * 0.05
            
            safe_quantity_value = self.risk_manager.calculate_position_size(
                price=price,
                stop_distance=stop_distance,
                volatility_adjustment=1.0 
            )
            safe_quantity = safe_quantity_value / price if price > 0 else 0
            
            # 3. Cap quantity if strategy requests more than risk manager allows
            if safe_quantity < quantity:
                self.logger.warning(f"RiskManager reduced quantity for {symbol}: {quantity:.6f} -> {safe_quantity:.6f}")
                quantity = safe_quantity
                
            if quantity <= 0:
                self.logger.warning(f"RiskManager calculated 0 quantity for {symbol}")
                return False

        if self.paper_trade:
            self.logger.trade(f"[PAPER] BUY {quantity:.6f} {symbol} @ Rs {price:,.2f}")
            self.logger.trade(
                f"  Stop: Rs {stop_loss:,.2f} | Target: Rs {take_profit:,.2f} | ATR: Rs {atr:,.2f} | State: {market_state}"
            )

            # Record position in state manager
            if self.state_manager:
                try:
                    position = Position(
                        symbol=symbol,
                        side="buy",
                        quantity=quantity,
                        entry_price=price,
                        entry_time=datetime.now().isoformat(),
                        stop_loss=stop_loss,
                        take_profit=take_profit,
                        status="open",
                        order_id=f"PAPER-{datetime.now().strftime('%Y%m%d%H%M%S')}",
                        metadata={
                            "paper_trade": True,
                            "execution_type": "market",
                            "atr_at_entry": atr,
                            "market_state_entry": market_state,
                        },
                    )
                    self.state_manager.save_position(position)
                except Exception as e:
                    self.logger.warning("Failed to persist position: %s", e)
            
            # Register with Risk Manager
            if self.risk_manager:
                try:
                    risk_pos = RiskPosition(
                        symbol=symbol,
                        entry_price=price,
                        quantity=quantity,
                        stop_price=stop_loss,
                        target_price=take_profit,
                        entry_time=datetime.now(),
                        risk_amount=(price - stop_loss) * quantity if stop_loss > 0 else 0
                    )
                    self.risk_manager.add_position(risk_pos)
                except Exception as e:
                    self.logger.warning("Failed to register position with RiskManager: %s", e)

            return True

        try:
            response = self.executor.execute_buy(
                market=symbol,
                quantity=quantity,
                limit_price=price * 1.002,  # 0.2% premium for fill
                use_limit=True,
            )

            if response:
                order_id = response.get("id", "N/A")
                self.logger.trade(f"[LIVE] BUY EXECUTED - Order ID: {order_id}")
                self.logger.trade(
                    f"  Stop: Rs {stop_loss:,.2f} | Target: Rs {take_profit:,.2f} | ATR: Rs {atr:,.2f}"
                )

                # Record position in state manager
                if self.state_manager:
                    try:
                        position = Position(
                            symbol=symbol,
                            side="buy",
                            quantity=quantity,
                            entry_price=price,
                            entry_time=datetime.now().isoformat(),
                            stop_loss=stop_loss,
                            take_profit=take_profit,
                            status="open",
                            order_id=str(order_id),
                            metadata={
                                "paper_trade": False,
                                "execution_type": "limit",
                                "atr_at_entry": atr,
                                "market_state_entry": market_state,
                                "raw_response": response,
                            },
                        )
                        self.state_manager.save_position(position)
                    except Exception as e:
                        self.logger.warning("Failed to persist position: %s", e)
                
                # Register with Risk Manager
                if self.risk_manager:
                    try:
                        risk_pos = RiskPosition(
                            symbol=symbol,
                            entry_price=price,
                            quantity=quantity,
                            stop_price=stop_loss,
                            target_price=take_profit,
                            entry_time=datetime.now(),
                            risk_amount=(price - stop_loss) * quantity if stop_loss > 0 else 0
                        )
                        self.risk_manager.add_position(risk_pos)
                    except Exception as e:
                        self.logger.warning("Failed to register position with RiskManager: %s", e)

                return True

            self.logger.error("BUY order failed for %s", symbol)
            return False

        except Exception as e:
            self.logger.error("BUY execution error for %s: %s", symbol, e)
            return False

    def execute_sell(
        self,
        symbol: str,
        quantity: float,
        price: float,
        gross_pnl: float = 0.0,
        fees: float = 0.0,
        tax: float = 0.0,
        exit_reason: str = "",
        market_state: str = "",
    ) -> bool:
        """Execute sell order with state persistence and full trade details"""
        if self.paper_trade:
            net_pnl = gross_pnl - fees - tax
            result = "WIN" if net_pnl > 0 else "LOSS"
            self.logger.trade(f"[PAPER] SELL {quantity:.6f} {symbol} @ Rs {price:,.2f}")
            self.logger.trade(
                f"  Gross P&L: Rs {gross_pnl:+,.2f} | Fees: Rs {fees:.2f} | Tax: Rs {tax:.2f} | Net: Rs {net_pnl:+,.2f} [{result}]"
            )
            self.logger.trade(f"  Exit Reason: {exit_reason} | State: {market_state}")

            # Close position in state manager with full details
            if self.state_manager:
                try:
                    close_position(
                        self.state_manager,
                        symbol,
                        exit_price=price,
                        gross_pnl=gross_pnl,
                        fees=fees,
                        tax=tax,
                        exit_reason=exit_reason,
                        market_state_exit=market_state,
                    )
                except Exception as e:
                    self.logger.warning("Failed to close position in state: %s", e)
            
            # Update Risk Manager
            if self.risk_manager:
                try:
                    is_win = net_pnl > 0
                    self.risk_manager.record_trade_result(
                        symbol=symbol,
                        gross_pnl=gross_pnl,
                        fees=fees + tax,
                        is_win=is_win
                    )
                    self.risk_manager.remove_position(symbol)
                except Exception as e:
                    self.logger.warning("Failed to update RiskManager: %s", e)

            return True

        try:
            response = self.executor.execute_sell(
                market=symbol,
                quantity=quantity,
                limit_price=price * 0.998,  # 0.2% discount for fill
                use_limit=True,
            )

            if response:
                order_id = response.get("id", "N/A")
                net_pnl = gross_pnl - fees - tax
                self.logger.trade(f"[LIVE] SELL EXECUTED - Order ID: {order_id}")
                self.logger.trade(
                    f"  Gross P&L: Rs {gross_pnl:+,.2f} | Fees: Rs {fees:.2f} | Tax: Rs {tax:.2f} | Net: Rs {net_pnl:+,.2f}"
                )

                # Close position in state manager with full details
                if self.state_manager:
                    try:
                        close_position(
                            self.state_manager,
                            symbol,
                            exit_price=price,
                            gross_pnl=gross_pnl,
                            fees=fees,
                            tax=tax,
                            exit_reason=exit_reason,
                            market_state_exit=market_state,
                        )
                    except Exception as e:
                        self.logger.warning("Failed to close position in state: %s", e)
                
                # Update Risk Manager
                if self.risk_manager:
                    try:
                        is_win = net_pnl > 0
                        self.risk_manager.record_trade_result(
                            symbol=symbol,
                            gross_pnl=gross_pnl,
                            fees=fees + tax,
                            is_win=is_win
                        )
                        self.risk_manager.remove_position(symbol)
                    except Exception as e:
                        self.logger.warning("Failed to update RiskManager: %s", e)

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
    - State persistence with SQLite + JSON backup
    - Automatic crash recovery
    - Graceful error handling and recovery
    - Emergency shutdown capability
    """

    def __init__(
        self,
        config: Optional[Config] = None,
        paper_trade: bool = True,
        log_dir: str = "logs",
        state_dir: str = "state",
        state_backend: str = "sqlite",
    ):
        """
        Initialize live trader.

        Args:
            config: Trading configuration
            paper_trade: If True, simulate trades without real execution
            log_dir: Directory for log files
            state_dir: Directory for state persistence
            state_backend: State backend ('sqlite' or 'json')
        """
        self.config = config or get_default_config()
        self.paper_trade = paper_trade

        # Setup logging
        self.logger = DualLogger("LiveTrader", log_dir)

        # Initialize state manager (SQLite + JSON backup)
        self.state_manager = create_state_manager(
            backend=state_backend,
            state_dir=state_dir,
            auto_backup=True,
        )
        self.logger.info("State manager initialized: %s backend", state_backend)

        # Initialize Risk Manager
        self.risk_manager = RiskManager(
            initial_equity=self.config.trading.initial_capital,
            limits=RiskLimits(
                max_risk_per_trade=self.config.risk.risk_per_trade,
                max_positions=self.config.risk.max_open_positions,
                max_portfolio_heat=self.config.risk.max_portfolio_heat,
                max_position_pct=self.config.risk.max_position_size_pct,
                drawdown_halt=self.config.risk.max_drawdown,
            ),
        )
        self.logger.info("Risk Manager initialized with capital: Rs %s", self.config.trading.initial_capital)

        # Initialize exchange connection
        self.client = CoinDCXClient()
        self.executor = CoinDCXExecutor(self.client)
        self.order_executor = LiveOrderExecutor(
            self.client,
            self.executor,
            self.logger,
            paper_trade,
            state_manager=self.state_manager,  # Pass state manager
            risk_manager=self.risk_manager,    # Pass risk manager
        )

        # State tracking
        self._running = False
        self._cerebro = None
        self._strategy = None
        self._cycle_count = 0
        self._peak_value = self.config.trading.initial_capital
        self._consecutive_losses = 0

        # Generate config hash for change detection
        self._config_hash = self._compute_config_hash()

        # Check for recovery from previous session
        self._attempt_recovery()

        self._log_startup()

    def _compute_config_hash(self) -> str:
        """Compute hash of config for change detection."""
        # Use params dict for hashing
        params = self.config.strategy.params
        # Sort params to ensure consistent hashing
        sorted_params = sorted(params.items())
        params_str = "|".join([f"{k}:{v}" for k, v in sorted_params])

        config_str = (
            f"{self.config.trading.pairs}|" f"{self.config.trading.timeframe}|" f"{params_str}"
        )
        return hashlib.md5(config_str.encode()).hexdigest()[:8]

    def _attempt_recovery(self):
        """
        Attempt to recover from a previous session.

        Loads checkpoint and positions from state manager.
        Warns if config has changed since last session.
        """
        checkpoint = self.state_manager.load_checkpoint()

        if checkpoint:
            self.logger.section("RECOVERY: Previous session detected", "-")

            # Check config compatibility
            if checkpoint.config_hash and checkpoint.config_hash != self._config_hash:
                self.logger.warning(
                    "CONFIG CHANGED since last session! " "Previous: %s, Current: %s",
                    checkpoint.config_hash,
                    self._config_hash,
                )
                self.logger.warning(
                    "Positions from previous session may not be compatible. "
                    "Consider manual review."
                )

            # Check paper mode consistency
            if checkpoint.paper_mode != self.paper_trade:
                self.logger.warning(
                    "TRADING MODE CHANGED! Previous: %s, Current: %s",
                    "PAPER" if checkpoint.paper_mode else "LIVE",
                    "PAPER" if self.paper_trade else "LIVE",
                )

            # Restore state
            self._cycle_count = checkpoint.cycle_count
            self._consecutive_losses = checkpoint.consecutive_losses
            self._peak_value = checkpoint.portfolio_value

            # Sync RiskManager state
            if self.risk_manager:
                self.risk_manager.update_equity(checkpoint.portfolio_value)
                self.risk_manager.state.consecutive_losses = checkpoint.consecutive_losses
                # Ensure peak equity is at least what we had in checkpoint
                self.risk_manager.state.peak_equity = max(self.risk_manager.state.peak_equity, checkpoint.portfolio_value)

            # Load open positions
            open_positions = get_open_positions(self.state_manager)

            # Sync positions to RiskManager
            if self.risk_manager and open_positions:
                for pos in open_positions:
                    try:
                        risk_pos = RiskPosition(
                            symbol=pos.symbol,
                            entry_price=pos.entry_price,
                            quantity=pos.quantity,
                            stop_price=pos.stop_loss,
                            target_price=pos.take_profit,
                            entry_time=datetime.fromisoformat(pos.entry_time),
                            risk_amount=(pos.entry_price - pos.stop_loss) * pos.quantity if pos.stop_loss > 0 else 0
                        )
                        self.risk_manager.add_position(risk_pos)
                    except Exception as e:
                        self.logger.warning(f"Failed to sync position {pos.symbol} to RiskManager: {e}")

            self.logger.trade(f"  Last checkpoint:    {checkpoint.timestamp}")
            self.logger.trade(f"  Cycle count:        {checkpoint.cycle_count}")
            self.logger.trade(f"  Portfolio value:    Rs {checkpoint.portfolio_value:,.2f}")
            self.logger.trade(f"  Cash:               Rs {checkpoint.cash:,.2f}")
            self.logger.trade(f"  Open positions:     {len(open_positions)}")
            self.logger.trade(f"  Drawdown:           {checkpoint.drawdown_pct:.2f}%")

            if open_positions:
                self.logger.trade("")
                self.logger.trade("  RECOVERED POSITIONS:")
                for pos in open_positions:
                    self.logger.trade(
                        f"    {pos.symbol}: {pos.quantity:.6f} @ Rs {pos.entry_price:,.2f} "
                        f"[SL: Rs {pos.stop_loss:,.0f}, TP: Rs {pos.take_profit:,.0f}]"
                    )
        else:
            self.logger.debug("No previous session to recover from")

    def _save_checkpoint(self, portfolio_value: float, cash: float):
        """Save current state as checkpoint."""
        try:
            open_positions = get_open_positions(self.state_manager)
            positions_value = portfolio_value - cash

            # Calculate drawdown
            if portfolio_value > self._peak_value:
                self._peak_value = portfolio_value
            drawdown_pct = ((self._peak_value - portfolio_value) / self._peak_value) * 100

            checkpoint = Checkpoint(
                timestamp=datetime.now().isoformat(),
                cycle_count=self._cycle_count,
                portfolio_value=portfolio_value,
                cash=cash,
                positions_value=positions_value,
                open_positions=len(open_positions),
                last_processed_symbols=self.config.trading.pairs,
                drawdown_pct=drawdown_pct,
                consecutive_losses=self._consecutive_losses,
                paper_mode=self.paper_trade,
                config_hash=self._config_hash,
                metadata={
                    "timeframe": self.config.trading.timeframe,
                    "strategy_version": "1.0",
                },
            )

            self.state_manager.save_checkpoint(checkpoint)
            self.logger.debug(
                "Checkpoint saved: cycle=%d, value=Rs %.2f",
                self._cycle_count,
                portfolio_value,
            )

        except Exception as e:
            self.logger.error("Failed to save checkpoint: %s", e)

    def _log_startup(self):
        """Log startup configuration"""
        mode = "PAPER TRADE" if self.paper_trade else "*** LIVE TRADING ***"
        params = self.config.strategy.params

        self.logger.section(f"LIVE TRADER INITIALIZED - {mode}")
        self.logger.trade(f"  Capital:    Rs {self.config.trading.initial_capital:,.2f}")
        self.logger.trade(f"  Pairs:      {self.config.trading.pairs}")
        self.logger.trade(f"  Timeframe:  {self.config.trading.timeframe}")
        self.logger.trade(f"  Max Pos:    {self.config.trading.max_positions}")
        self.logger.trade(f"  Config:     {self._config_hash}")
        self.logger.trade("")
        self.logger.trade("  STRATEGY PARAMETERS:")
        for key, value in params.items():
            self.logger.trade(f"    {key:<20}: {value}")

        self.logger.trade(f"    Risk Per Trade:     {self.config.trading.risk_per_trade*100:.1f}%")
        self.logger.trade(
            f"    Max Position:       {self.config.trading.max_position_pct*100:.0f}%"
        )
        self.logger.trade("")
        self.logger.trade("  STATE PERSISTENCE:")
        self.logger.trade(f"    Backend:    {type(self.state_manager).__name__}")
        self.logger.trade(
            f"    Recovery:   {'Enabled' if self._cycle_count > 0 else 'Fresh start'}"
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

        # Dynamically load base strategy
        base_strategy_class = get_strategy_class(self.config.strategy.name)

        # Create dynamic LiveTradingStrategy inheriting from Mixin and Base
        # Mixin first to ensure its methods override/extend base
        LiveStrategy = type("LiveTradingStrategy", (LiveTradingMixin, base_strategy_class), {})

        cerebro.addstrategy(LiveStrategy, **strategy_params)

        # Set broker parameters
        cerebro.broker.setcash(self.config.trading.initial_capital)
        cerebro.broker.setcommission(commission=self.config.exchange.taker_fee)

        # Add data feeds for each pair
        for symbol in self.config.trading.pairs:
            df = self.fetch_data(symbol, bars=100)

            if df.empty:
                self.logger.warning("Skipping %s - no data available", symbol)
                continue

            # pylint: disable=unexpected-keyword-arg
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
        """Run one trading cycle with state persistence"""
        self._cycle_count += 1
        self.logger.section(
            f"TRADING CYCLE #{self._cycle_count} - {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"
        )

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

                # Log individual positions and sync state
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

                        # Sync position to state manager
                        self._sync_position(data._name, pos, price)

                # Save checkpoint after each cycle
                self._save_checkpoint(portfolio_value, cash)

        except Exception as e:
            self.logger.error("Error in trading cycle: %s", e)
            self.logger.debug(traceback.format_exc())

    def _sync_position(self, symbol: str, bt_position, current_price: float):
        """
        Sync Backtrader position with state manager.

        Args:
            symbol: Trading pair symbol
            bt_position: Backtrader position object
            current_price: Current market price
        """
        try:
            if bt_position.size > 0:
                # Position exists - update or create in state
                existing = self.state_manager.get_position(symbol)

                if existing and existing.status == "open":
                    # Update existing position metadata
                    existing.metadata["last_price"] = current_price
                    existing.metadata["last_update"] = datetime.now().isoformat()
                    self.state_manager.save_position(existing)
                else:
                    # Create new position record
                    position = Position(
                        symbol=symbol,
                        side="buy",
                        quantity=bt_position.size,
                        entry_price=bt_position.price,
                        entry_time=datetime.now().isoformat(),
                        status="open",
                        metadata={
                            "current_price": current_price,
                            "cycle_opened": self._cycle_count,
                        },
                    )
                    self.state_manager.save_position(position)
                    self.logger.debug("Position synced to state: %s", symbol)

            else:
                # No position in Backtrader - check if we need to close in state
                existing = self.state_manager.get_position(symbol)
                if existing and existing.status == "open":
                    # Position was closed by strategy - record it
                    close_position(
                        self.state_manager,
                        symbol,
                        exit_price=current_price,
                        gross_pnl=0.0,  # Will be calculated from actual trade
                    )
                    self.logger.debug("Position closed in state: %s", symbol)

        except Exception as e:
            self.logger.warning("Failed to sync position %s: %s", symbol, e)

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
        self.logger.trade(f"  Starting cycle: {self._cycle_count + 1}")
        self.logger.trade("")
        self.logger.trade("Press Ctrl+C to stop...")

        while self._running:
            self.logger.info("Starting cycle #%d", self._cycle_count + 1)

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
        """Stop the live trader gracefully with state persistence"""
        self._running = False

        # Save final checkpoint
        try:
            if self._strategy:
                # Try to get final portfolio value
                portfolio_value = self.config.trading.initial_capital
                cash = portfolio_value

                self.logger.info("Saving final state...")
                self._save_checkpoint(portfolio_value, cash)

                # Export JSON backup
                self.state_manager.export_json("state/final_state.json")
                self.logger.info("Final state exported to state/final_state.json")

        except Exception as e:
            self.logger.warning("Failed to save final state: %s", e)

        # Close state manager
        self.state_manager.close()

        self.logger.section("LIVE TRADER STOPPED")
        self.logger.trade(f"  Total cycles completed: {self._cycle_count}")
        self.logger.trade("  State saved for recovery")

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
  python -m src.live_trader --state-backend json       # Use JSON state backend
  python -m src.live_trader --reset-state              # Clear previous state
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

    # State management options
    parser.add_argument(
        "--state-backend",
        type=str,
        default="sqlite",
        choices=["sqlite", "json"],
        help="State persistence backend (default: sqlite)",
    )
    parser.add_argument(
        "--state-dir",
        type=str,
        default="state",
        help="Directory for state files (default: state)",
    )
    parser.add_argument(
        "--reset-state",
        action="store_true",
        help="Clear previous state and start fresh",
    )

    args = parser.parse_args()

    # Setup root logging level
    if args.verbose:
        logging.getLogger().setLevel(logging.DEBUG)
    else:
        logging.getLogger().setLevel(logging.INFO)

    # Handle state reset if requested
    if args.reset_state:
        print("\n[!] Clearing previous state...")
        state_dir = Path(args.state_dir)
        if state_dir.exists():
            import shutil

            shutil.rmtree(state_dir)
            print(f"    Removed {state_dir}")
        print("    State cleared. Starting fresh.\n")

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

    # Create and start trader with state management
    trader = LiveTrader(
        config=config,
        paper_trade=paper_trade,
        state_dir=args.state_dir,
        state_backend=args.state_backend,
    )

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
