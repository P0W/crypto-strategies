//! Momentum Scalper Strategy Implementation
//!
//! Production-grade with batch indicator calculation and per-symbol state tracking.
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

use crate::indicators::{adx, ema, macd};
use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::{
    atr_stop_loss, atr_take_profit, close_position_order, current_atr_or_default,
    volume_ratio_confirmed, OhlcVectors, PositionLifecycleManager, Strategy,
};
use crate::{Candle, Position, Side, Trade};

use super::config::MomentumScalperConfig;
use super::MomentumState;

/// Pre-calculated indicators using batch functions
struct Indicators {
    current_ema_fast: Option<f64>,
    current_ema_slow: Option<f64>,
    previous_ema_fast: Option<f64>,
    previous_ema_slow: Option<f64>,
    current_ema_trend: Option<f64>,
    current_adx: Option<f64>,
    hist_curr: f64,
    hist_prev: f64,
    macd_curr: f64,
    signal_curr: f64,
}

impl Indicators {
    fn new(candles: &[Candle], config: &MomentumScalperConfig) -> Self {
        let ohlc = OhlcVectors::from_candles(candles);

        // Batch EMA calculations
        let ema_fast = ema(&ohlc.close, config.ema_fast);
        let ema_slow_vals = ema(&ohlc.close, config.ema_slow);
        let ema_trend = ema(&ohlc.close, config.ema_trend);

        // Batch ADX calculation
        let adx_values = adx(&ohlc.high, &ohlc.low, &ohlc.close, config.adx_period);

        // Batch MACD calculation
        let (macd_line, signal_line, histogram) = macd(
            &ohlc.close,
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
            previous_ema_fast: ema_fast
                .get(ema_fast.len().saturating_sub(2))
                .and_then(|&x| x),
            previous_ema_slow: ema_slow_vals
                .get(ema_slow_vals.len().saturating_sub(2))
                .and_then(|&x| x),
            current_ema_trend: ema_trend.last().and_then(|&x| x),
            current_adx: adx_values.last().and_then(|&x| x),
            hist_curr,
            hist_prev,
            macd_curr: macd_line.last().and_then(|&x| x).unwrap_or(0.0),
            signal_curr: signal_line.last().and_then(|&x| x).unwrap_or(0.0),
        }
    }
}

/// Momentum Scalper Strategy - Production Grade
pub struct MomentumScalperStrategy {
    config: MomentumScalperConfig,
    lifecycle: PositionLifecycleManager,
}

impl MomentumScalperStrategy {
    pub fn new(config: MomentumScalperConfig) -> Self {
        Self {
            config,
            lifecycle: PositionLifecycleManager::default(),
        }
    }

    /// Get EMA alignment signal from pre-calculated indicators
    fn get_ema_alignment(ind: &Indicators) -> Option<Side> {
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

    fn get_ema_cross(ind: &Indicators) -> Option<Side> {
        let fast = ind.current_ema_fast?;
        let slow = ind.current_ema_slow?;
        let previous_fast = ind.previous_ema_fast?;
        let previous_slow = ind.previous_ema_slow?;

        if fast > slow && previous_fast <= previous_slow {
            Some(Side::Buy)
        } else if fast < slow && previous_fast >= previous_slow {
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

        if let Some(alignment) = Self::get_ema_alignment(ind) {
            if is_long && alignment == Side::Sell {
                return true;
            }
            if !is_long && alignment == Side::Buy {
                return true;
            }
        }
        false
    }

    fn is_volume_confirmed(&self, candles: &[Candle]) -> bool {
        if !self.config.require_volume {
            return true;
        }
        volume_ratio_confirmed(
            candles,
            self.config.volume_period,
            self.config.volume_threshold,
        )
    }
}

impl Strategy for MomentumScalperStrategy {
    fn name(&self) -> &'static str {
        "momentum_scalper"
    }

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(MomentumScalperStrategy::new(self.config.clone()))
    }

    fn on_bar(&mut self, ctx: &StrategyContext) {
        self.lifecycle
            .on_bar(ctx.symbol, ctx.current_position.is_some());
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

        // Check cooldown (per-symbol)
        if self.lifecycle.is_cooling_down(ctx.symbol) && ctx.current_position.is_none() {
            return orders;
        }

        // Calculate all indicators ONCE using batch functions
        let ind = Indicators::new(ctx.candles, &self.config);

        // If in position, check exit conditions
        if let Some(pos) = ctx.current_position {
            let is_long = pos.side == Side::Buy;
            let exit_order = || close_position_order(ctx.symbol, pos);

            // Exit on EMA cross
            if self.should_exit_on_cross(&ind, is_long) {
                orders.push(exit_order());
                return orders;
            }

            // Exit on max hold bars
            if self.lifecycle.bars_in_position(ctx.symbol) >= self.config.max_hold_bars {
                orders.push(exit_order());
                return orders;
            }

            // Exit on momentum reversal
            let momentum = self.get_momentum_state(&ind);
            let momentum_reversed = if is_long {
                matches!(
                    momentum,
                    MomentumState::WeakBearish | MomentumState::StrongBearish
                )
            } else {
                matches!(
                    momentum,
                    MomentumState::WeakBullish | MomentumState::StrongBullish
                )
            };
            if momentum_reversed {
                orders.push(exit_order());
                return orders;
            }

            // Hold position
            return orders;
        }

        // Entry logic using pre-calculated batch indicators
        let alignment = match Self::get_ema_cross(&ind) {
            Some(side) => side,
            None => return orders,
        };

        if alignment == Side::Sell && !self.config.allow_short {
            return orders;
        }

        if self.config.trade_with_trend {
            let current_close = ctx.candles.last().map(|c| c.close).unwrap_or(0.0);
            let trend = match ind.current_ema_trend {
                Some(value) => value,
                None => return orders,
            };
            let aligned_with_trend = match alignment {
                Side::Buy => current_close > trend,
                Side::Sell => current_close < trend,
            };
            if !aligned_with_trend {
                return orders;
            }
        }

        // ADX filter
        if self.config.adx_threshold > 0.0 && !self.is_adx_strong(&ind) {
            return orders;
        }

        if !self.is_volume_confirmed(ctx.candles) {
            return orders;
        }

        // MACD momentum filter
        if self.config.use_macd {
            let momentum = self.get_momentum_state(&ind);
            let confirmed = match alignment {
                Side::Buy => matches!(
                    momentum,
                    MomentumState::StrongBullish | MomentumState::WeakBullish
                ),
                Side::Sell => matches!(
                    momentum,
                    MomentumState::StrongBearish | MomentumState::WeakBearish
                ),
            };
            if !confirmed {
                return orders;
            }
        }

        orders.push(match alignment {
            Side::Buy => OrderRequest::market_buy(ctx.symbol.clone(), 1.0),
            Side::Sell => OrderRequest::market_sell(ctx.symbol.clone(), 1.0),
        });
        orders
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let atr = current_atr_or_default(candles, self.config.atr_period, entry_price, 0.01);
        atr_stop_loss(entry_price, atr, self.config.stop_atr_multiple, side)
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let atr = current_atr_or_default(candles, self.config.atr_period, entry_price, 0.01);
        atr_take_profit(entry_price, atr, self.config.target_atr_multiple, side)
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let atr = current_atr_or_default(candles, self.config.atr_period, current_price, 0.01);

        let entry_price = position.average_entry_price.to_f64();
        let profit_atr = if atr > 0.0 {
            match position.side {
                Side::Buy => (current_price - entry_price) / atr,
                Side::Sell => (entry_price - current_price) / atr,
            }
        } else {
            0.0
        };

        if profit_atr >= self.config.trailing_activation {
            let new_stop = match position.side {
                Side::Buy => current_price - self.config.trailing_atr_multiple * atr,
                Side::Sell => current_price + self.config.trailing_atr_multiple * atr,
            };
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
        // Nothing needed - state tracking happens in on_bar and on_trade_closed
    }

    fn on_trade_closed(&mut self, trade: &Trade) {
        self.lifecycle
            .close_trade(&trade.symbol, self.config.cooldown_bars);

        tracing::info!(
            symbol = %trade.symbol,
            return_pct = format!("{:.2}%", trade.return_pct()),
            net_pnl = format!("{:.2}", trade.net_pnl),
            "Momentum Scalper trade closed"
        );
    }

    fn init(&mut self) {
        self.lifecycle.clear();
        tracing::info!("Momentum Scalper strategy initialized");
    }
}
