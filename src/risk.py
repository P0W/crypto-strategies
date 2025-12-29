"""
Risk Management Module

Portfolio-level risk framework implementing:
- Position sizing
- Portfolio heat management
- Drawdown-based de-risking
- Consecutive loss protection
- Daily loss cutoff
"""

from dataclasses import dataclass, field
from typing import Dict, List, Optional
from datetime import datetime, date
from enum import Enum
import logging

logger = logging.getLogger(__name__)


class RiskAction(Enum):
    """Risk management actions"""

    ALLOW = "allow"
    REDUCE = "reduce"
    BLOCK = "block"


@dataclass
class RiskLimits:
    """
    Risk limit configuration.

    All values are justified for Indian crypto market conditions.
    """

    # Per-trade limits
    max_risk_per_trade: float = 0.02  # 2% max risk per trade
    min_risk_per_trade: float = 0.005  # 0.5% min risk per trade

    # Position limits
    max_position_pct: float = 0.40  # 40% max single position
    max_positions: int = 2  # Max concurrent positions

    # Portfolio limits
    max_portfolio_heat: float = 0.10  # 10% max total portfolio risk

    # Drawdown limits
    drawdown_warning: float = 0.10  # 10% - reduce position sizes
    drawdown_critical: float = 0.15  # 15% - further reduction
    drawdown_halt: float = 0.20  # 20% - halt trading

    # Daily limits
    max_daily_loss_pct: float = 0.05  # 5% max daily loss
    max_daily_trades: int = 5  # Max trades per day

    # Consecutive loss limits
    consecutive_loss_threshold: int = 3  # Reduce after 3 losses
    consecutive_loss_reduction: float = 0.25  # Reduce by 25%
    max_consecutive_losses: int = 5  # Halt after 5 losses

    # Recovery settings
    consecutive_wins_for_reset: int = 2  # Wins to reset loss counter


@dataclass
class Position:
    """Active position data"""

    symbol: str
    entry_price: float
    quantity: float
    stop_price: float
    target_price: float
    entry_time: datetime
    risk_amount: float  # INR at risk

    @property
    def current_value(self) -> float:
        return self.quantity * self.entry_price

    @property
    def risk_pct(self) -> float:
        if self.entry_price == 0:
            return 0.0
        return (self.entry_price - self.stop_price) / self.entry_price


@dataclass
class DailyStats:
    """Daily trading statistics"""

    date: date
    starting_equity: float
    current_equity: float
    trades_count: int = 0
    winning_trades: int = 0
    losing_trades: int = 0
    gross_pnl: float = 0.0
    fees_paid: float = 0.0

    @property
    def net_pnl(self) -> float:
        return self.gross_pnl - self.fees_paid

    @property
    def daily_return(self) -> float:
        if self.starting_equity == 0:
            return 0.0
        return (self.current_equity - self.starting_equity) / self.starting_equity


@dataclass
class PortfolioState:
    """Current portfolio state"""

    equity: float
    peak_equity: float
    positions: Dict[str, Position] = field(default_factory=dict)
    daily_stats: Optional[DailyStats] = None
    consecutive_losses: int = 0
    consecutive_wins: int = 0

    @property
    def drawdown(self) -> float:
        if self.peak_equity == 0:
            return 0.0
        return (self.peak_equity - self.equity) / self.peak_equity

    @property
    def total_risk(self) -> float:
        return sum(p.risk_amount for p in self.positions.values())

    @property
    def portfolio_heat(self) -> float:
        if self.equity == 0:
            return 0.0
        return self.total_risk / self.equity

    @property
    def position_count(self) -> int:
        return len(self.positions)


class RiskManager:
    """
    Portfolio-level risk management.

    Implements comprehensive risk controls:
    1. Position sizing with volatility adjustment
    2. Portfolio heat management
    3. Drawdown-based de-risking
    4. Consecutive loss protection
    5. Daily loss limits

    Usage:
        risk_mgr = RiskManager(initial_equity=100000)

        # Check if new trade allowed
        action = risk_mgr.can_trade()

        # Calculate position size
        size = risk_mgr.calculate_position_size(
            price=5000000,
            stop_distance=100000,
            regime_score=1.2
        )

        # Register position
        risk_mgr.add_position(position)

        # Record trade result
        risk_mgr.record_trade_result(pnl=5000, fees=100)
    """

    def __init__(self, initial_equity: float, limits: Optional[RiskLimits] = None):
        """
        Initialize risk manager.

        Args:
            initial_equity: Starting capital in INR
            limits: Risk limit configuration
        """
        self.limits = limits or RiskLimits()
        self.state = PortfolioState(
            equity=initial_equity,
            peak_equity=initial_equity,
            daily_stats=DailyStats(
                date=date.today(), starting_equity=initial_equity, current_equity=initial_equity
            ),
        )
        self._trade_history: List[Dict] = []

    def update_equity(self, new_equity: float):
        """
        Update portfolio equity.

        Args:
            new_equity: Current portfolio value
        """
        self.state.equity = new_equity
        self.state.peak_equity = max(self.state.peak_equity, new_equity)

        if self.state.daily_stats:
            self.state.daily_stats.current_equity = new_equity

    def can_trade(self) -> RiskAction:
        """
        Check if new trades are allowed.

        Returns:
            RiskAction indicating whether to allow, reduce, or block
        """
        # Check drawdown
        if self.state.drawdown >= self.limits.drawdown_halt:
            logger.warning(
                "Trading HALTED: Drawdown %.1f%% >= %.1f%%",
                self.state.drawdown * 100,
                self.limits.drawdown_halt * 100,
            )
            return RiskAction.BLOCK

        # Check consecutive losses
        if self.state.consecutive_losses >= self.limits.max_consecutive_losses:
            logger.warning("Trading HALTED: %d consecutive losses", self.state.consecutive_losses)
            return RiskAction.BLOCK

        # Check daily limits
        if self.state.daily_stats:
            if self.state.daily_stats.trades_count >= self.limits.max_daily_trades:
                logger.warning("Daily trade limit reached: %d", self.limits.max_daily_trades)
                return RiskAction.BLOCK

            if abs(self.state.daily_stats.daily_return) >= self.limits.max_daily_loss_pct:
                if self.state.daily_stats.daily_return < 0:
                    logger.warning(
                        "Daily loss limit reached: %.1f%%",
                        self.state.daily_stats.daily_return * 100,
                    )
                    return RiskAction.BLOCK

        # Check position limits
        if self.state.position_count >= self.limits.max_positions:
            logger.info("Max positions reached: %d", self.limits.max_positions)
            return RiskAction.BLOCK

        # Check portfolio heat
        if self.state.portfolio_heat >= self.limits.max_portfolio_heat:
            logger.info("Max portfolio heat reached: %.1f%%", self.state.portfolio_heat * 100)
            return RiskAction.BLOCK

        # Check for reduced sizing conditions
        if self.state.drawdown >= self.limits.drawdown_warning:
            logger.info("Drawdown warning: %.1f%% - reducing size", self.state.drawdown * 100)
            return RiskAction.REDUCE

        if self.state.consecutive_losses >= self.limits.consecutive_loss_threshold:
            logger.info("Consecutive losses: %d - reducing size", self.state.consecutive_losses)
            return RiskAction.REDUCE

        return RiskAction.ALLOW

    def get_size_multiplier(self) -> float:
        """
        Get position size multiplier based on risk conditions.

        Returns:
            Multiplier between 0.25 and 1.0
        """
        multiplier = 1.0

        # Drawdown-based reduction
        if self.state.drawdown >= self.limits.drawdown_critical:
            multiplier *= 0.25
        elif self.state.drawdown >= self.limits.drawdown_warning:
            multiplier *= 0.5

        # Consecutive loss reduction
        if self.state.consecutive_losses >= self.limits.consecutive_loss_threshold:
            multiplier *= 1 - self.limits.consecutive_loss_reduction

        return max(0.25, multiplier)

    def calculate_position_size(
        self,
        price: float,
        stop_distance: float,
        regime_score: float = 1.0,
        base_risk_pct: Optional[float] = None,
    ) -> float:
        """
        Calculate position size using volatility-adjusted fractional sizing.

        Formula:
        1. risk_amount = equity * risk_per_trade * size_multiplier * regime_score
        2. shares = risk_amount / stop_distance
        3. position_value = shares * price
        4. Apply caps

        Args:
            price: Current price
            stop_distance: Distance to stop loss in price units
            regime_score: Volatility regime adjustment (0.5 to 1.5)
            base_risk_pct: Override base risk percentage

        Returns:
            Position size in quote currency (INR)
        """
        if stop_distance <= 0 or price <= 0:
            return 0.0

        # Check if trading allowed
        action = self.can_trade()
        if action == RiskAction.BLOCK:
            return 0.0

        # Base risk
        risk_pct = base_risk_pct or self.limits.max_risk_per_trade
        risk_pct = min(risk_pct, self.limits.max_risk_per_trade)
        risk_pct = max(risk_pct, self.limits.min_risk_per_trade)

        # Calculate base risk amount
        risk_amount = self.state.equity * risk_pct

        # Apply regime score
        risk_amount *= min(max(regime_score, 0.5), 1.5)

        # Apply size multiplier (drawdown/loss protection)
        risk_amount *= self.get_size_multiplier()

        # Calculate shares based on stop distance
        shares = risk_amount / stop_distance
        position_value = shares * price

        # Cap at max position percentage
        max_value = self.state.equity * self.limits.max_position_pct
        position_value = min(position_value, max_value)

        # Check remaining portfolio heat
        remaining_heat = self.limits.max_portfolio_heat - self.state.portfolio_heat
        if remaining_heat < risk_pct:
            risk_amount = remaining_heat * self.state.equity
            shares = risk_amount / stop_distance
            position_value = min(position_value, shares * price)

        return position_value

    def add_position(self, position: Position):
        """
        Register a new position.

        Args:
            position: Position to add
        """
        self.state.positions[position.symbol] = position
        logger.info(
            f"Position added: {position.symbol} - "
            f"Qty: {position.quantity:.6f}, Entry: {position.entry_price:.2f}, "
            f"Risk: ₹{position.risk_amount:.2f}"
        )

    def remove_position(self, symbol: str) -> Optional[Position]:
        """
        Remove a position.

        Args:
            symbol: Position symbol to remove

        Returns:
            Removed position or None
        """
        return self.state.positions.pop(symbol, None)

    def record_trade_result(self, symbol: str, gross_pnl: float, fees: float, is_win: bool):
        """
        Record trade result for tracking.

        Args:
            symbol: Traded symbol
            gross_pnl: Gross P&L before fees
            fees: Total fees paid
            is_win: Whether trade was profitable
        """
        net_pnl = gross_pnl - fees

        # Update consecutive counters
        if is_win:
            self.state.consecutive_wins += 1
            self.state.consecutive_losses = 0

            # Reset after recovery
            if self.state.consecutive_wins >= self.limits.consecutive_wins_for_reset:
                logger.info("Win streak - risk parameters reset")
        else:
            self.state.consecutive_losses += 1
            self.state.consecutive_wins = 0

        # Update daily stats
        if self.state.daily_stats:
            self.state.daily_stats.trades_count += 1
            self.state.daily_stats.gross_pnl += gross_pnl
            self.state.daily_stats.fees_paid += fees

            if is_win:
                self.state.daily_stats.winning_trades += 1
            else:
                self.state.daily_stats.losing_trades += 1

        # Log result
        self._trade_history.append(
            {
                "symbol": symbol,
                "gross_pnl": gross_pnl,
                "fees": fees,
                "net_pnl": net_pnl,
                "is_win": is_win,
                "timestamp": datetime.now(),
                "equity": self.state.equity,
                "drawdown": self.state.drawdown,
            }
        )

        logger.info(
            f"Trade closed: {symbol} - "
            f"Gross: ₹{gross_pnl:.2f}, Fees: ₹{fees:.2f}, Net: ₹{net_pnl:.2f}"
        )

    def new_day(self, new_equity: float):
        """
        Reset daily statistics for new trading day.

        Args:
            new_equity: Current portfolio value
        """
        self.state.daily_stats = DailyStats(
            date=date.today(), starting_equity=new_equity, current_equity=new_equity
        )
        logger.info("New trading day started. Equity: ₹%.2f", new_equity)

    def get_risk_report(self) -> Dict:
        """
        Generate comprehensive risk report.

        Returns:
            Dictionary with risk metrics
        """
        return {
            "equity": self.state.equity,
            "peak_equity": self.state.peak_equity,
            "drawdown": self.state.drawdown,
            "drawdown_pct": f"{self.state.drawdown:.2%}",
            "portfolio_heat": f"{self.state.portfolio_heat:.2%}",
            "position_count": self.state.position_count,
            "total_risk": self.state.total_risk,
            "consecutive_losses": self.state.consecutive_losses,
            "consecutive_wins": self.state.consecutive_wins,
            "size_multiplier": self.get_size_multiplier(),
            "can_trade": self.can_trade().value,
            "daily_pnl": self.state.daily_stats.net_pnl if self.state.daily_stats else 0,
            "daily_trades": self.state.daily_stats.trades_count if self.state.daily_stats else 0,
        }

    def apply_indian_tax(self, gross_profit: float) -> Dict:
        """
        Calculate Indian tax impact on profits.

        India Crypto Tax (as of 2024):
        - 30% flat tax on gains
        - 1% TDS on transactions
        - No loss offset allowed

        Args:
            gross_profit: Gross profit amount

        Returns:
            Dictionary with tax breakdown
        """
        if gross_profit <= 0:
            return {
                "gross_profit": gross_profit,
                "tax": 0,
                "tds": 0,
                "net_profit": gross_profit,
                "effective_rate": 0,
            }

        tax = gross_profit * 0.30  # 30% flat tax
        tds = gross_profit * 0.01  # 1% TDS (adjustable against tax)
        net_profit = gross_profit - tax

        return {
            "gross_profit": gross_profit,
            "tax": tax,
            "tds": tds,  # TDS is adjusted, not additional
            "net_profit": net_profit,
            "effective_rate": 0.30,
        }


# ===========================================================================
# FEE & TAX REALITY CHECK
# ===========================================================================


def calculate_minimum_edge(
    fee_rate: float = 0.001,
    slippage: float = 0.001,
    tax_rate: float = 0.30,
    avg_trades_per_month: int = 6,
    avg_holding_days: int = 3,
) -> Dict:
    """
    Calculate minimum edge required to be profitable post-tax.

    Args:
        fee_rate: Exchange fee rate (0.1% = 0.001)
        slippage: Expected slippage per trade
        tax_rate: Tax rate on gains (30% = 0.30)
        avg_trades_per_month: Expected trades per month
        avg_holding_days: Average holding period

    Returns:
        Dictionary with edge requirements
    """
    # Round-trip cost
    round_trip_cost = 2 * (fee_rate + slippage)  # ~0.4%

    # Monthly cost from trading
    monthly_trading_cost = round_trip_cost * avg_trades_per_month  # ~2.4%

    # To break even after costs, need returns > costs
    # But gains are taxed, losses aren't offset
    # If win_rate = 50%, need:
    # (avg_win * 0.5 * 0.7) - (avg_loss * 0.5) - monthly_cost > 0
    # Assuming avg_win = avg_loss = X:
    # 0.35X - 0.5X - monthly_cost > 0
    # -0.15X - monthly_cost > 0 (impossible with symmetric returns)

    # Need asymmetric payoff:
    # With 2:1 reward-risk (win = 2X, loss = X):
    # (2X * win_rate * 0.7) - (X * loss_rate) - monthly_cost > 0
    # At 40% win rate:
    # (2X * 0.4 * 0.7) - (X * 0.6) - monthly_cost > 0
    # 0.56X - 0.6X - monthly_cost > 0
    # -0.04X - monthly_cost > 0 (still negative)

    # At 45% win rate with 2:1:
    # (2X * 0.45 * 0.7) - (X * 0.55) - monthly_cost > 0
    # 0.63X - 0.55X - monthly_cost > 0
    # 0.08X > monthly_cost
    # X > monthly_cost / 0.08

    # Minimum win rate for 2:1 payoff to be profitable
    min_win_rate_2to1 = 0.45  # ~45%

    # With 3:1 payoff at 35% win rate:
    # (3X * 0.35 * 0.7) - (X * 0.65) - monthly_cost > 0
    # 0.735X - 0.65X - monthly_cost > 0
    # 0.085X > monthly_cost

    min_win_rate_3to1 = 0.35  # ~35%

    return {
        "round_trip_cost": f"{round_trip_cost:.2%}",
        "monthly_trading_cost": f"{monthly_trading_cost:.2%}",
        "annual_trading_cost": f"{monthly_trading_cost * 12:.2%}",
        "min_win_rate_2to1_payoff": f"{min_win_rate_2to1:.0%}",
        "min_win_rate_3to1_payoff": f"{min_win_rate_3to1:.0%}",
        "tax_impact": f"{tax_rate:.0%} on gains, no loss offset",
        "recommendation": (
            "Target 2:1 reward-risk with >45% win rate, or "
            "3:1 reward-risk with >35% win rate to be profitable post-tax"
        ),
    }
