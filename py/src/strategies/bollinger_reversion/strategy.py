"""
Strategy: Bollinger Band Mean Reversion (Scalping)
Timeframe: 5m / 15m
Description: Classic mean reversion strategy for choppy/sideways markets.
             Buys when price is oversold (Lower BB + Low RSI).
             Sells when price is overbought (Upper BB).
"""

import backtrader as bt
import logging

logger = logging.getLogger(__name__)

class BollingerReversionStrategy(bt.Strategy):
    """
    Bollinger Band Mean Reversion Strategy
    
    Entry: Close < Lower Band AND RSI < 30
    Exit: Close > Upper Band OR Stop Loss
    """
    
    params = (
        # Standard Indicators
        ("bb_period", 20),
        ("bb_dev", 2.0),
        ("rsi_period", 14),
        ("rsi_oversold", 30),
        ("rsi_overbought", 70),
        
        # Risk Management (Required by RiskManager/Config)
        ("risk_per_trade", 0.01),
        ("stop_loss_pct", 0.01),  # 1% tight stop for scalping
        
        # These are passed by the config system but might not be used directly 
        # if we don't implement the full RiskManager here for simplicity,
        # but we should accept them to avoid errors.
        ("max_positions", 1),
        ("max_portfolio_heat", 1.0),
        ("max_position_pct", 1.0),
        ("max_drawdown", 0.2),
        ("drawdown_warning", 0.1),
        ("drawdown_critical", 0.15),
        ("drawdown_warning_multiplier", 0.5),
        ("drawdown_critical_multiplier", 0.25),
        ("consecutive_loss_limit", 3),
        ("consecutive_loss_multiplier", 0.75),
        ("maker_fee", 0.001),
        ("taker_fee", 0.001),
        ("slippage", 0.001),
        ("logger", None),
        ("executor", None),
        ("paper_trade", True),
    )

    def __init__(self):
        self.bb = bt.indicators.BollingerBands(
            self.data.close, 
            period=self.p.bb_period, 
            devfactor=self.p.bb_dev
        )
        self.rsi = bt.indicators.RSI(
            self.data.close, 
            period=self.p.rsi_period
        )
        self.order = None

    def next(self):
        # Skip if order pending
        if self.order:
            return

        # Check for open position
        if not self.position:
            # Entry Logic: Price below lower band AND RSI oversold
            if self.data.close[0] < self.bb.lines.bot[0] and self.rsi[0] < self.p.rsi_oversold:
                
                # Simple sizing for demonstration: 95% of cash / price
                size = (self.broker.getcash() * 0.95) / self.data.close[0]
                
                self.log(f"ENTRY SIGNAL: Close {self.data.close[0]:.2f} < BB Bot {self.bb.lines.bot[0]:.2f} & RSI {self.rsi[0]:.1f}")
                self.order = self.buy(size=size)
                
        else:
            # Exit Logic: Price above upper band
            if self.data.close[0] > self.bb.lines.top[0]:
                self.log(f"EXIT SIGNAL: Close {self.data.close[0]:.2f} > BB Top {self.bb.lines.top[0]:.2f}")
                self.order = self.close()
                
            # Stop Loss Logic (Simple % based)
            elif self.data.close[0] < self.position.price * (1 - self.p.stop_loss_pct):
                self.log(f"STOP LOSS: Close {self.data.close[0]:.2f} < Stop {self.position.price * (1 - self.p.stop_loss_pct):.2f}")
                self.order = self.close()

    def notify_order(self, order):
        if order.status in [order.Submitted, order.Accepted]:
            return

        if order.status in [order.Completed]:
            if order.isbuy():
                self.log(f'BUY EXECUTED, Price: {order.executed.price:.2f}, Cost: {order.executed.value:.2f}, Comm: {order.executed.comm:.2f}')
            else:
                self.log(f'SELL EXECUTED, Price: {order.executed.price:.2f}, Cost: {order.executed.value:.2f}, Comm: {order.executed.comm:.2f}')
            self.order = None

        elif order.status in [order.Canceled, order.Margin, order.Rejected]:
            self.log('Order Canceled/Margin/Rejected')
            self.order = None

    def log(self, txt, dt=None):
        dt = dt or self.datas[0].datetime.datetime(0)
        logger.info(f'{dt.isoformat()} {txt}')
