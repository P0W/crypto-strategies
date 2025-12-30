"""
Strategy: Volatility Regime Adaptive Strategy (VRAS)
Author: Prashant Srivastava
Description: Exploits volatility clustering and regime persistence in crypto markets
Timeframe: 4H (primary), 1D (regime filter)
Universe: BTC/INR, ETH/INR (high liquidity pairs on CoinDCX)

=============================================================================
EDGE HYPOTHESIS
=============================================================================

**Core Inefficiency: Volatility Clustering & Regime Persistence**

Cryptocurrency markets exhibit strong volatility clustering (GARCH effects) where:
1. High volatility periods tend to persist (volatility begets volatility)
2. Low volatility periods compress before explosive moves (volatility mean-reversion)
3. Retail traders systematically misjudge regime transitions

**Why This Edge Exists:**
- Crypto markets are dominated by retail participants who:
  - Overtrade during high volatility (exhaustion trades)
  - Undertrade during low volatility (miss breakouts)
  - Use static position sizing (don't adapt to regime)

- Institutional crypto adoption in India is limited, leaving inefficiencies unexploited

**Why It Persists After Costs:**
- The strategy trades infrequently (4-8 trades/month)
- Targets asymmetric payoffs (2:1+ reward:risk minimum)
- Volatility-adjusted sizing reduces risk during unfavorable regimes
- Fee impact: ~0.2% round-trip on ~3-5% average win = ~6% fee drag on profits
- Post-30% tax, requires 1.5x edge to break even; our target is 2.5x+

**CoinDCX Liquidity Validation:**
- BTC/INR and ETH/INR have sufficient depth for ₹50k-₹1L positions
- 4H timeframe avoids microstructure noise and slippage issues
- Strategy avoids market orders during low-liquidity hours (2 AM - 6 AM IST)

=============================================================================
STRATEGY CLASS: Volatility Regime + Trend Confirmation Hybrid
=============================================================================

This strategy combines:
1. **Volatility Regime Detection** - Identify market state (compression vs expansion)
2. **Trend Confirmation** - Enter only with momentum alignment
3. **Adaptive Position Sizing** - Scale exposure based on regime quality

"""

import logging
from enum import Enum
from typing import TYPE_CHECKING

import backtrader as bt

if TYPE_CHECKING:
    from src.config import Config

# Configure logging
logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s")
logger = logging.getLogger(__name__)


class SignalType(Enum):
    """Trading signal types"""

    LONG = 1
    SHORT = -1  # Not used in spot-only strategy but kept for completeness
    FLAT = 0


class VolatilityRegime(Enum):
    """Market volatility regime classification"""

    COMPRESSION = "compression"  # Low vol, potential breakout setup
    NORMAL = "normal"  # Average volatility
    EXPANSION = "expansion"  # High vol, trend continuation or exhaustion
    EXTREME = "extreme"  # Very high vol, reduce exposure


class VolatilityRegimeIndicator(bt.Indicator):
    """
    Custom indicator to classify market volatility regime.

    Uses ATR percentile rank over lookback period to determine:
    - Compression: Low volatility, potential breakout setup
    - Normal: Average market conditions
    - Expansion: Trending/volatile conditions
    - Extreme: Risk-off, reduce exposure
    """

    lines = ("regime", "atr_percentile", "regime_score")
    params = (
        ("atr_period", 14),
        ("lookback", 20),
        ("compression_thresh", 0.6),
        ("expansion_thresh", 1.4),
        ("extreme_thresh", 2.0),
    )

    def __init__(self):
        self.atr = bt.indicators.ATR(self.data, period=self.p.atr_period)

    def next(self):
        if len(self.atr) < self.p.lookback:
            self.lines.regime[0] = 1  # Normal
            self.lines.atr_percentile[0] = 0.5
            self.lines.regime_score[0] = 1.0
            return

        # Calculate ATR ratio to moving average
        atr_values = [self.atr[-i] for i in range(self.p.lookback)]
        atr_mean = sum(atr_values) / len(atr_values)

        if atr_mean == 0:
            self.lines.regime[0] = 1
            self.lines.atr_percentile[0] = 0.5
            self.lines.regime_score[0] = 1.0
            return

        atr_ratio = self.atr[0] / atr_mean
        self.lines.atr_percentile[0] = atr_ratio

        # Classify regime
        if atr_ratio < self.p.compression_thresh:
            self.lines.regime[0] = 0  # Compression
            self.lines.regime_score[0] = 1.5  # Higher conviction for breakouts
        elif atr_ratio > self.p.extreme_thresh:
            self.lines.regime[0] = 3  # Extreme
            self.lines.regime_score[0] = 0.5  # Reduce position size
        elif atr_ratio > self.p.expansion_thresh:
            self.lines.regime[0] = 2  # Expansion
            self.lines.regime_score[0] = 0.8  # Slightly reduced size
        else:
            self.lines.regime[0] = 1  # Normal
            self.lines.regime_score[0] = 1.0


class CoinDCXStrategy(bt.Strategy):
    """
    Volatility Regime Adaptive Strategy for CoinDCX

    =======================================================================
    ENTRY LOGIC (from config.py):
    =======================================================================
    1. Regime Filter: Only trade in COMPRESSION or NORMAL regimes
       - Compression: ATR ratio < 0.6 (volatility squeeze, breakout setup)
       - Normal: ATR ratio 0.6-1.5 (standard trend-following)
       - Expansion/Extreme: No new entries (ATR ratio > 1.5)

    2. Trend Confirmation:
       - EMA(8) > EMA(21) for bullish trend
       - ADX > 30 confirms trend strength
       - Price must close above recent consolidation high

    3. Breakout Confirmation:
       - Price closes above (Recent High - 1.5 * ATR)
       - Anticipatory entry (buy the dip / early breakout)
       - Current close above level AND previous close at/below level

    =======================================================================
    EXIT LOGIC (from config.py):
    =======================================================================
    1. Stop Loss: 2.5x ATR below entry (volatility-adjusted)
    2. Take Profit: 5.0x ATR above entry (2:1 reward-risk target)
    3. Trailing Stop: Activates at 50% of target (2.5 ATR), trails at 1.5x ATR
    4. Regime Exit: Close if regime shifts to EXTREME
    5. Trend Exit: Close if price closes below EMA(21) (only when in profit)

    =======================================================================
    RISK MANAGEMENT:
    =======================================================================
    1. Position Size: Volatility-adjusted Kelly fraction
       - Base risk: 15% of equity
       - Adjusted by regime score (0.5x to 1.5x)
       - Capped at 20% of equity per position

    2. Portfolio Heat: Max 30% total risk across all positions

    3. Drawdown Protection:
       - Reduce size by 50% if drawdown > 10%
       - Stop trading if drawdown > 20%

    4. Consecutive Loss Protection:
       - Reduce size by 25% after 3 consecutive losses
       - Reset after 2 consecutive wins
    """

    params = {
        # =====================================================================
        # NO DEFAULT VALUES - Configuration MUST be provided via:
        #   config.get_strategy_params() from src/config.py
        #
        # This ensures SINGLE SOURCE OF TRUTH for all parameters.
        # Running strategy without proper config will raise an error.
        # =====================================================================
        # Risk parameters (from config.trading)
        "risk_per_trade": None,
        "max_positions": None,
        "max_portfolio_heat": None,
        "max_position_pct": None,
        "max_drawdown": None,
        # Drawdown protection (from config.trading)
        "drawdown_warning": None,
        "drawdown_critical": None,
        "drawdown_warning_multiplier": None,
        "drawdown_critical_multiplier": None,
        # Consecutive loss protection (from config.trading)
        "consecutive_loss_limit": None,
        "consecutive_loss_multiplier": None,
        # Volatility regime (from config.strategy)
        "atr_period": None,
        "volatility_lookback": None,
        "compression_threshold": None,
        "expansion_threshold": None,
        "extreme_threshold": None,
        # Trend confirmation (from config.strategy)
        "ema_fast": None,
        "ema_slow": None,
        "adx_period": None,
        "adx_threshold": None,
        # Entry/Exit (from config.strategy)
        "breakout_atr_multiple": None,
        "stop_atr_multiple": None,
        "target_atr_multiple": None,
        "trailing_activation": None,
        "trailing_atr_multiple": None,
        # Fees (from config.exchange)
        "maker_fee": None,
        "taker_fee": None,
        "slippage": None,
        # Execution (has sensible default)
        "min_bars_between_trades": 2,
        # Live trading extras (optional - only used by LiveTradingStrategy)
        "logger": None,
        "executor": None,
        "paper_trade": True,
    }

    # Required parameters that MUST be provided via config
    REQUIRED_PARAMS = [
        "risk_per_trade",
        "max_positions",
        "max_portfolio_heat",
        "max_position_pct",
        "max_drawdown",
        "drawdown_warning",
        "drawdown_critical",
        "drawdown_warning_multiplier",
        "drawdown_critical_multiplier",
        "consecutive_loss_limit",
        "consecutive_loss_multiplier",
        "atr_period",
        "volatility_lookback",
        "compression_threshold",
        "expansion_threshold",
        "extreme_threshold",
        "ema_fast",
        "ema_slow",
        "adx_period",
        "adx_threshold",
        "breakout_atr_multiple",
        "stop_atr_multiple",
        "target_atr_multiple",
        "trailing_activation",
        "trailing_atr_multiple",
        "maker_fee",
        "taker_fee",
        "slippage",
    ]

    def __init__(self):
        """Initialize indicators and tracking variables"""
        # Validate that all required params are provided (not None)
        self._validate_params()

        self.orders = {}  # Track pending orders per data
        self.pending_close = {}  # Track pending close orders to prevent duplicates
        self.entry_prices = {}  # Track entry prices
        self.stop_prices = {}  # Track stop prices
        self.target_prices = {}  # Track target prices
        self.entry_bars = {}  # Track bar count at entry
        self.trailing_active = {}  # Track trailing stop activation
        self.consecutive_losses = 0
        self.consecutive_wins = 0
        self.peak_value = self.broker.getvalue()
        self.trade_count = 0
        self.winning_trades = 0
        self.losing_trades = 0

        # Initialize indicators for each data feed
        self.indicators = {}
        for d in self.datas:
            self.indicators[d._name] = {
                "atr": bt.indicators.ATR(d, period=self.p.atr_period),
                "ema_fast": bt.indicators.EMA(d.close, period=self.p.ema_fast),
                "ema_slow": bt.indicators.EMA(d.close, period=self.p.ema_slow),
                "adx": bt.indicators.ADX(d, period=self.p.adx_period),
                "highest": bt.indicators.Highest(d.high, period=self.p.volatility_lookback),
                "lowest": bt.indicators.Lowest(d.low, period=self.p.volatility_lookback),
                "regime": VolatilityRegimeIndicator(
                    d,
                    atr_period=self.p.atr_period,
                    lookback=self.p.volatility_lookback,
                    compression_thresh=self.p.compression_threshold,
                    expansion_thresh=self.p.expansion_threshold,
                    extreme_thresh=self.p.extreme_threshold,
                ),
            }

    def _validate_params(self):
        """
        Validate that all required parameters were provided via config.

        Raises:
            ValueError: If any required parameter is None (not configured)
        """
        missing = []
        for param_name in self.REQUIRED_PARAMS:
            value = getattr(self.p, param_name, None)
            if value is None:
                missing.append(param_name)

        if missing:
            raise ValueError(
                f"Strategy configuration incomplete! Missing parameters: {missing}\n"
                f"You MUST provide configuration via config.get_strategy_params().\n"
                f"Example:\n"
                f"  from src.config import Config\n"
                f"  config = Config.load_from_file('configs/your_config.json')\n"
                f"  cerebro.addstrategy(CoinDCXStrategy, **config.get_strategy_params())"
            )

    def log(self, txt: str, dt=None):
        """Logging function"""
        dt = dt or self.datas[0].datetime.date(0)
        logger.info("%s %s", dt.isoformat(), txt)

    def get_current_drawdown(self) -> float:
        """Calculate current drawdown from peak"""
        current_value = self.broker.getvalue()
        self.peak_value = max(self.peak_value, current_value)
        if self.peak_value == 0:
            return 0.0
        return (self.peak_value - current_value) / self.peak_value

    def get_position_size(self, data, regime_score: float) -> float:
        """
        Calculate position size using volatility-adjusted fractional sizing.

        Formula:
        1. Base risk amount = equity * risk_per_trade
        2. Adjust for regime score (0.5 to 1.5)
        3. Adjust for drawdown (reduce if > 10%)
        4. Adjust for consecutive losses
        5. Calculate shares based on ATR stop distance
        6. Cap at max_position_pct

        Returns position size in currency units
        """
        equity = self.broker.getvalue()
        atr = self.indicators[data._name]["atr"][0]
        price = data.close[0]

        if atr == 0 or price == 0:
            return 0.0

        # Base risk amount
        base_risk = equity * self.p.risk_per_trade

        # Regime adjustment
        regime_adjusted_risk = base_risk * regime_score

        # Drawdown adjustment (configurable thresholds)
        drawdown = self.get_current_drawdown()
        if drawdown > self.p.drawdown_critical:
            regime_adjusted_risk *= self.p.drawdown_critical_multiplier
        elif drawdown > self.p.drawdown_warning:
            regime_adjusted_risk *= self.p.drawdown_warning_multiplier

        # Consecutive loss adjustment (configurable)
        if self.consecutive_losses >= self.p.consecutive_loss_limit:
            regime_adjusted_risk *= self.p.consecutive_loss_multiplier

        # Calculate position size based on stop distance
        stop_distance = atr * self.p.stop_atr_multiple
        shares = regime_adjusted_risk / stop_distance
        position_value = shares * price

        # Cap at max position percentage
        max_position_value = equity * self.p.max_position_pct
        position_value = min(position_value, max_position_value)

        # Check portfolio heat
        current_heat = self._calculate_portfolio_heat()
        remaining_heat = self.p.max_portfolio_heat - current_heat
        max_risk = remaining_heat * equity

        if max_risk < regime_adjusted_risk:
            position_value = (max_risk / stop_distance) * price

        return position_value

    def _calculate_portfolio_heat(self) -> float:
        """Calculate current portfolio heat (total risk across positions)"""
        equity = self.broker.getvalue()
        if equity == 0:
            return 0.0

        total_risk = 0.0
        for d in self.datas:
            pos = self.getposition(d)
            if pos.size > 0 and d._name in self.stop_prices:
                risk = (d.close[0] - self.stop_prices[d._name]) * pos.size
                total_risk += max(0, risk)

        return total_risk / equity

    def check_entry_conditions(self, data) -> SignalType:
        """
        Check if entry conditions are met for LONG signal.

        Conditions (MORE SELECTIVE):
        1. No existing position
        2. No pending order
        3. Regime is COMPRESSION (strongest setups)
        4. Trend is bullish (EMA fast > EMA slow)
        5. ADX > threshold (trend strength)
        6. Price breaks above recent high with momentum
        7. Price above EMA slow (additional trend confirm)
        8. Portfolio heat allows new position
        9. Drawdown is acceptable
        """
        ind = self.indicators[data._name]

        # Check position and orders
        if self.getposition(data).size > 0:
            return SignalType.FLAT
        if data._name in self.orders and self.orders[data._name]:
            return SignalType.FLAT

        # Check max positions
        active_positions = sum(1 for d in self.datas if self.getposition(d).size > 0)
        if active_positions >= self.p.max_positions:
            return SignalType.FLAT

        # Check drawdown
        if self.get_current_drawdown() > self.p.max_drawdown:
            return SignalType.FLAT

        # Regime filter - trade in compression OR normal (not expansion/extreme)
        regime = int(ind["regime"].regime[0])
        if regime >= 2:  # Expansion or Extreme - avoid these
            return SignalType.FLAT

        # Trend confirmation - EMA alignment
        if ind["ema_fast"][0] <= ind["ema_slow"][0]:
            return SignalType.FLAT

        # ADX strength - require decent trend (not too strict)
        if ind["adx"][0] < self.p.adx_threshold:
            return SignalType.FLAT

        # Breakout confirmation - price breaks above recent high
        atr = ind["atr"][0]
        breakout_level = ind["highest"][-1] - atr * self.p.breakout_atr_multiple

        # Check for breakout: current close above level, previous close at/below level
        current_above = data.close[0] > breakout_level
        previous_at_or_below = data.close[-1] <= breakout_level
        if current_above and previous_at_or_below:
            return SignalType.LONG

        return SignalType.FLAT

    def check_exit_conditions(self, data) -> bool:
        """
        Check if exit conditions are met for existing position.

        Exit if:
        1. Stop loss hit
        2. Target hit
        3. Trailing stop hit (when activated)
        4. Regime shifts to EXTREME
        5. Price closes below slow EMA (trend broken)
        """
        if self.getposition(data).size == 0:
            return False

        # Skip if there's already a pending order (prevents duplicate sells)
        name = data._name
        if name in self.orders and self.orders[name]:
            return False

        ind = self.indicators[data._name]

        if name not in self.entry_prices:
            return False

        entry_price = self.entry_prices[name]
        current_price = data.close[0]
        atr = ind["atr"][0]

        # Calculate profit in ATR terms
        profit_atr = (current_price - entry_price) / atr if atr > 0 else 0
        
        # Production behavior: Activate AND update trailing stop immediately
        # This provides real-time downside protection once profit threshold is reached
        if profit_atr >= self.p.trailing_activation:
            self.trailing_active[name] = True
            new_stop = current_price - atr * self.p.trailing_atr_multiple
            # Ratchet up only - never lower the stop
            if new_stop > self.stop_prices.get(name, 0):
                self.stop_prices[name] = new_stop

        # Stop loss check - PRIORITY 1 (using close price to match Rust behavior)
        # Note: In production, you'd want to check LOW for more accurate stop loss triggers
        if current_price <= self.stop_prices.get(name, 0):
            self.log(f"STOP LOSS triggered for {name}")
            return True

        # Target check - PRIORITY 2 (check against HIGH for production correctness)
        # Targets should trigger if intraday high reaches target, not just close
        current_high = data.high[0]
        if current_high >= self.target_prices.get(name, float("inf")):
            self.log(f"TARGET hit for {name} (high={current_high:.2f})")
            return True

        # Regime exit - extreme volatility
        regime = int(ind["regime"].regime[0])
        if regime == 3:  # Extreme
            self.log(f"REGIME EXIT - Extreme volatility for {name}")
            return True

        # Trend exit - price closes below slow EMA (less sensitive than EMA cross)
        # Only exit on trend break if we're in profit or break-even
        if current_price < ind["ema_slow"][0]:
            if current_price >= entry_price:  # In profit or break-even
                self.log(f"TREND EXIT - Price below EMA slow for {name}")
                return True
            # If in loss, let the stop handle it (don't exit prematurely)

        return False

    def next(self):
        """Main strategy logic executed on each bar"""
        for data in self.datas:
            name = data._name
            ind = self.indicators[name]

            # Skip if not enough data
            if len(data) < self.p.volatility_lookback + self.p.adx_period:
                continue

            # Check exits first (skip if already pending close)
            if name not in self.pending_close or not self.pending_close[name]:
                if self.check_exit_conditions(data):
                    self.log(f"CLOSING position for {name} at {data.close[0]:.2f}")
                    self.close(data=data)
                    self.pending_close[name] = True  # Mark as pending close
                    continue

            # Check entries
            signal = self.check_entry_conditions(data)

            if signal == SignalType.LONG:
                regime_score = ind["regime"].regime_score[0]
                position_value = self.get_position_size(data, regime_score)

                if position_value <= 0:
                    continue

                size = position_value / data.close[0]
                atr = ind["atr"][0]

                # Calculate stops and targets
                entry_price = data.close[0]
                stop_price = entry_price - atr * self.p.stop_atr_multiple
                target_price = entry_price + atr * self.p.target_atr_multiple

                # Place order
                self.log(
                    f"BUY CREATE for {name}: Size={size:.4f}, Entry={entry_price:.2f}, "
                    f"Stop={stop_price:.2f}, Target={target_price:.2f}"
                )

                order = self.buy(data=data, size=size)
                self.orders[name] = order
                self.entry_prices[name] = entry_price
                self.stop_prices[name] = stop_price
                self.target_prices[name] = target_price
                self.entry_bars[name] = len(data)
                self.trailing_active[name] = False

    def notify_order(self, order):
        """Handle order notifications"""
        if order.status in [order.Submitted, order.Accepted]:
            return

        data_name = order.data._name

        if order.status in [order.Completed]:
            if order.isbuy():
                self.log(
                    f"BUY EXECUTED for {data_name}: Price={order.executed.price:.2f}, "
                    f"Size={order.executed.size:.4f}, Cost={order.executed.value:.2f}, "
                    f"Comm={order.executed.comm:.2f}"
                )
                self.trade_count += 1
            else:
                self.log(
                    f"SELL EXECUTED for {data_name}: Price={order.executed.price:.2f}, "
                    f"Size={order.executed.size:.4f}, Comm={order.executed.comm:.2f}"
                )

        elif order.status in [order.Canceled, order.Margin, order.Rejected]:
            self.log(f"Order {order.status} for {data_name}")

        # Clear order reference
        if data_name in self.orders:
            self.orders[data_name] = None

    def notify_trade(self, trade):
        """Handle trade notifications"""
        if not trade.isclosed:
            return

        # Calculate net P&L after fees
        gross_pnl = trade.pnl
        comm = trade.commission
        net_pnl = gross_pnl - comm

        # Apply tax impact (simplified)
        if net_pnl > 0:
            # tax = net_pnl * 0.30 (calculated inline below)
            net_pnl_post_tax = net_pnl * (1 - 0.30)  # 30% tax
        else:
            net_pnl_post_tax = net_pnl  # No tax on losses

        self.log(
            f"TRADE CLOSED: Gross={gross_pnl:.2f}, Comm={comm:.2f}, "
            f"Net={net_pnl:.2f}, Post-Tax={net_pnl_post_tax:.2f}"
        )

        # Track win/loss streaks
        if net_pnl > 0:
            self.winning_trades += 1
            self.consecutive_wins += 1
            self.consecutive_losses = 0
        else:
            self.losing_trades += 1
            self.consecutive_losses += 1
            self.consecutive_wins = 0

        # Clean up tracking
        data_name = trade.data._name
        for d in [
            self.entry_prices,
            self.stop_prices,
            self.target_prices,
            self.entry_bars,
            self.trailing_active,
            self.pending_close,  # Clear pending close flag
        ]:
            if data_name in d:
                del d[data_name]

    def stop(self):
        """Called at end of backtest"""
        total_trades = self.winning_trades + self.losing_trades
        win_rate = self.winning_trades / total_trades if total_trades > 0 else 0

        self.log("=== STRATEGY RESULTS ===")
        self.log(f"Final Value: {self.broker.getvalue():.2f}")
        self.log(f"Total Trades: {total_trades}")
        self.log(f"Win Rate: {win_rate:.2%}")
        self.log(f"Max Drawdown: {self.get_current_drawdown():.2%}")
