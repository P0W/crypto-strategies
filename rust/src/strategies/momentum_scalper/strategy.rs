//! Momentum Scalper Strategy Implementation
//!
//! Performance optimized: Indicators are calculated once per generate_signal call.
//!
//! ## Entry Logic
//! 1. EMA fast crosses above EMA slow (bullish crossover)
//! 2. MACD histogram positive (momentum confirmation)
//! 3. ADX above threshold (trend strength)
//!
//! ## Exit Logic
//! 1. Take profit at target ATR multiple
//! 2. Stop loss at entry - stop ATR multiple
//! 3. Trailing stop after activation threshold
//! 4. Exit on EMA cross back (fast below slow)
//! 5. Max hold bars exceeded

use crate::indicators::{adx, atr, ema, macd};
use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side, Trade};

use super::config::MomentumScalperConfig;
use super::MomentumState;

/// Pre-calculated indicators to avoid redundant computation
struct Indicators {
    current_ema_fast: Option<f64>,
    current_ema_slow: Option<f64>,
    current_adx: Option<f64>,
    hist_curr: f64,
    hist_prev: f64,
    macd_curr: f64,
    signal_curr: f64,
}

impl Indicators {
    fn new(candles: &[Candle], config: &MomentumScalperConfig) -> Self {
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();

        // EMA calculations
        let ema_fast = ema(&close, config.ema_fast);
        let ema_slow_vals = ema(&close, config.ema_slow);

        // ADX calculation
        let adx_values = adx(&high, &low, &close, config.adx_period);

        // MACD calculation
        let (macd_line, signal_line, histogram) = macd(
            &close,
            config.macd_fast,
            config.macd_slow,
            config.macd_signal,
        );

        let hist_curr = histogram.last().and_then(|&x| x).unwrap_or(0.0);
        let hist_prev = if histogram.len() >= 2 {
            histogram[histogram.len() - 2].unwrap_or(0.0)
        } else {
            0.0
        };

        Self {
            current_ema_fast: ema_fast.last().and_then(|&x| x),
            current_ema_slow: ema_slow_vals.last().and_then(|&x| x),
            current_adx: adx_values.last().and_then(|&x| x),
            hist_curr,
            hist_prev,
            macd_curr: macd_line.last().and_then(|&x| x).unwrap_or(0.0),
            signal_curr: signal_line.last().and_then(|&x| x).unwrap_or(0.0),
        }
    }

    /// Calculate ATR only (for stop/target/trailing methods)
    fn atr_only(candles: &[Candle], atr_period: usize) -> Option<f64> {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        atr(&high, &low, &close, atr_period).last().and_then(|&x| x)
    }
}

/// Momentum Scalper Strategy
pub struct MomentumScalperStrategy {
    config: MomentumScalperConfig,
    bars_in_position: usize,
    cooldown_counter: usize,
}

impl MomentumScalperStrategy {
    pub fn new(config: MomentumScalperConfig) -> Self {
        MomentumScalperStrategy {
            config,
            bars_in_position: 0,
            cooldown_counter: 0,
        }
    }

    /// Get EMA alignment signal from pre-calculated indicators
    fn get_ema_alignment(&self, ind: &Indicators) -> Option<Side> {
        let fast = ind.current_ema_fast?;
        let slow = ind.current_ema_slow?;

        if fast > slow {
            Some(Side::Buy)
        } else if fast < slow {
            Some(Side::Sell)
        } else {
            None
        }
    }

    /// Get MACD momentum state from pre-calculated indicators
    fn get_momentum_state(&self, ind: &Indicators) -> MomentumState {
        if !self.config.use_macd {
            return MomentumState::Neutral;
        }

        if ind.hist_curr > 0.0 && ind.hist_curr > ind.hist_prev && ind.macd_curr > ind.signal_curr {
            MomentumState::StrongBullish
        } else if ind.hist_curr > 0.0 {
            MomentumState::WeakBullish
        } else if ind.hist_curr < 0.0
            && ind.hist_curr < ind.hist_prev
            && ind.macd_curr < ind.signal_curr
        {
            MomentumState::StrongBearish
        } else if ind.hist_curr < 0.0 {
            MomentumState::WeakBearish
        } else {
            MomentumState::Neutral
        }
    }

    /// Check ADX strength from pre-calculated indicators
    fn is_adx_strong(&self, ind: &Indicators) -> bool {
        ind.current_adx.unwrap_or(0.0) >= self.config.adx_threshold
    }

    /// Check if should exit on EMA cross
    fn should_exit_on_cross(&self, ind: &Indicators, is_long: bool) -> bool {
        if !self.config.exit_on_cross {
            return false;
        }

        if let Some(alignment) = self.get_ema_alignment(ind) {
            if is_long && alignment == Side::Sell {
                return true;
            }
            if !is_long && alignment == Side::Buy {
                return true;
            }
        }
        false
    }
}

impl Strategy for MomentumScalperStrategy {
    fn name(&self) -> &'static str {
        "momentum_scalper"
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        let min_bars = self
            .config
            .ema_slow
            .max(self.config.ema_trend)
            .max(self.config.macd_slow + self.config.macd_signal)
            .max(self.config.adx_period * 2);

        if ctx.candles.len() < min_bars + 5 {
            return orders;
        }

        if self.cooldown_counter > 0 && ctx.current_position.is_none() {
            return orders;
        }

        // Calculate all indicators ONCE
        let ind = Indicators::new(ctx.candles, &self.config);

        // If in position, check exit conditions
        if let Some(pos) = ctx.current_position {
            if self.should_exit_on_cross(&ind, true) {
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            if self.bars_in_position >= self.config.max_hold_bars {
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            let momentum = self.get_momentum_state(&ind);
            if matches!(
                momentum,
                MomentumState::WeakBearish | MomentumState::StrongBearish
            ) {
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            // Hold position
            return orders;
        }

        // Entry logic using pre-calculated indicators
        let should_buy = match self.get_ema_alignment(&ind) {
            Some(side) => side == Side::Buy,
            None => false,
        };

        if !should_buy {
            return orders;
        }

        if self.config.adx_threshold > 0.0 && !self.is_adx_strong(&ind) {
            return orders;
        }

        if self.config.use_macd {
            let momentum = self.get_momentum_state(&ind);
            if !matches!(
                momentum,
                MomentumState::StrongBullish | MomentumState::WeakBullish | MomentumState::Neutral
            ) {
                return orders;
            }
        }

        // Generate buy order
        orders.push(OrderRequest::market_buy(ctx.symbol.clone(), 1.0));
        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(entry_price * 0.01);
        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(entry_price * 0.01);
        entry_price + self.config.target_atr_multiple * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let current_atr =
            Indicators::atr_only(candles, self.config.atr_period).unwrap_or(current_price * 0.01);

        let profit_atr = if current_atr > 0.0 {
            (current_price - position.average_entry_price) / current_atr
        } else {
            0.0
        };

        if profit_atr >= self.config.trailing_activation {
            let new_stop = current_price - self.config.trailing_atr_multiple * current_atr;
            Some(new_stop)
        } else {
            None
        }
    }

    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        let ind = Indicators::new(candles, &self.config);
        match self.get_momentum_state(&ind) {
            MomentumState::StrongBullish => 1.3,
            MomentumState::WeakBullish => 1.1,
            MomentumState::Neutral => 0.8,
            MomentumState::WeakBearish => 0.6,
            MomentumState::StrongBearish => 0.5,
        }
    }

    fn on_order_filled(&mut self, _fill: &Fill, _position: &Position) {
        self.bars_in_position = 0;
        self.cooldown_counter = 0;
    }

    fn on_trade_closed(&mut self, trade: &Trade) {
        self.cooldown_counter = self.config.cooldown_bars;
        self.bars_in_position = 0;

        // Return% calculation must account for position side
        let return_pct = match trade.side {
            Side::Buy => ((trade.exit_price - trade.entry_price) / trade.entry_price) * 100.0,
            Side::Sell => ((trade.entry_price - trade.exit_price) / trade.entry_price) * 100.0,
        };
        tracing::info!(
            symbol = %trade.symbol,
            return_pct = format!("{:.2}%", return_pct),
            net_pnl = format!("{:.2}", trade.net_pnl),
            "Momentum Scalper trade closed"
        );
    }

    fn on_bar(&mut self, ctx: &StrategyContext) {
        // NOTE: bars_in_position is NOT incremented here to match main branch behavior.
        // Main branch's backtest never called anything that incremented bars_in_position,
        // so max_hold_bars exits were effectively disabled.
        // The counter is also shared across all symbols which would cause bugs if used.
        // TODO: Implement per-symbol state tracking if max_hold_bars is needed.
        if ctx.current_position.is_none() && self.cooldown_counter > 0 {
            self.cooldown_counter -= 1;
        }
    }

    fn init(&mut self) {
        self.bars_in_position = 0;
        self.cooldown_counter = 0;
        tracing::info!("Momentum Scalper strategy initialized");
    }
}
