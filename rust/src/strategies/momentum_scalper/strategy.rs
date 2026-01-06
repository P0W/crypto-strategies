//! Momentum Scalper Strategy Implementation
//!
//! Performance optimized: Indicators are calculated incrementally in `on_bar`
//! using the `ta` crate and custom incremental ADX, avoiding O(N²) complexity.

use crate::indicators::IncrementalAdx;
use crate::oms::{Fill, OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side, Trade};
use chrono::{DateTime, Utc};
use ta::indicators::{ExponentialMovingAverage, MovingAverageConvergenceDivergence};
use ta::Next;

use super::config::MomentumScalperConfig;
use super::MomentumState;

/// Momentum Scalper Strategy
pub struct MomentumScalperStrategy {
    config: MomentumScalperConfig,
    
    // Stateful Indicators
    ema_fast: ExponentialMovingAverage,
    ema_slow: ExponentialMovingAverage,
    macd: MovingAverageConvergenceDivergence,
    adx: IncrementalAdx,
    
    // State Tracking
    last_processed_time: Option<DateTime<Utc>>,
    bars_in_position: usize,
    cooldown_counter: usize,
    
    // Cached Values
    current_ema_fast: f64,
    current_ema_slow: f64,
    current_adx: f64,
    current_macd: f64,
    current_signal: f64,
    current_hist: f64,
    prev_hist: f64,
}

impl MomentumScalperStrategy {
    pub fn new(config: MomentumScalperConfig) -> Self {
        let ema_fast = ExponentialMovingAverage::new(config.ema_fast).unwrap();
        let ema_slow = ExponentialMovingAverage::new(config.ema_slow).unwrap();
        let macd = MovingAverageConvergenceDivergence::new(config.macd_fast, config.macd_slow, config.macd_signal).unwrap();
        let adx = IncrementalAdx::new(config.adx_period);

        MomentumScalperStrategy {
            config,
            ema_fast,
            ema_slow,
            macd,
            adx,
            last_processed_time: None,
            bars_in_position: 0,
            cooldown_counter: 0,
            current_ema_fast: 0.0,
            current_ema_slow: 0.0,
            current_adx: 0.0,
            current_macd: 0.0,
            current_signal: 0.0,
            current_hist: 0.0,
            prev_hist: 0.0,
        }
    }

    /// Get EMA alignment signal from cached values
    fn get_ema_alignment(&self) -> Option<Side> {
        if self.current_ema_fast == 0.0 || self.current_ema_slow == 0.0 {
            return None;
        }
        
        if self.current_ema_fast > self.current_ema_slow {
            Some(Side::Buy)
        } else if self.current_ema_fast < self.current_ema_slow {
            Some(Side::Sell)
        } else {
            None
        }
    }

    /// Get MACD momentum state from cached values
    fn get_momentum_state(&self) -> MomentumState {
        if !self.config.use_macd {
            return MomentumState::Neutral;
        }

        if self.current_hist > 0.0 && self.current_hist > self.prev_hist && self.current_macd > self.current_signal {
            MomentumState::StrongBullish
        } else if self.current_hist > 0.0 {
            MomentumState::WeakBullish
        } else if self.current_hist < 0.0
            && self.current_hist < self.prev_hist
            && self.current_macd < self.current_signal
        {
            MomentumState::StrongBearish
        } else if self.current_hist < 0.0 {
            MomentumState::WeakBearish
        } else {
            MomentumState::Neutral
        }
    }

    /// Check if should exit on EMA cross
    fn should_exit_on_cross(&self, is_long: bool) -> bool {
        if !self.config.exit_on_cross {
            return false;
        }

        if let Some(alignment) = self.get_ema_alignment() {
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

    fn on_bar(&mut self, ctx: &StrategyContext) {
        // Incremental update
        if ctx.candles.is_empty() {
            return;
        }

        let start_idx = if let Some(last_time) = self.last_processed_time {
            if ctx.candles.last().unwrap().datetime > last_time {
                ctx.candles.iter()
                    .position(|c| c.datetime > last_time)
                    .unwrap_or(ctx.candles.len())
            } else {
                return; // Already processed
            }
        } else {
            0
        };

        for candle in &ctx.candles[start_idx..] {
            // Save previous history
            self.prev_hist = self.current_hist;

            // Update indicators
            self.current_ema_fast = self.ema_fast.next(candle.close);
            self.current_ema_slow = self.ema_slow.next(candle.close);
            
            let macd_out = self.macd.next(candle.close);
            self.current_macd = macd_out.macd;
            self.current_signal = macd_out.signal;
            self.current_hist = macd_out.histogram;

            self.current_adx = self.adx.next(candle.high, candle.low, candle.close);

            self.last_processed_time = Some(candle.datetime);
        }

        // Cooldown logic
        if ctx.current_position.is_none() && self.cooldown_counter > 0 {
            self.cooldown_counter -= 1;
        }
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        // Check warmup
        if self.current_ema_slow == 0.0 || self.current_ema_fast == 0.0 {
            return orders;
        }

        if self.cooldown_counter > 0 && ctx.current_position.is_none() {
            return orders;
        }

        // If in position, check exit conditions
        if let Some(pos) = ctx.current_position {
            if self.should_exit_on_cross(true) {
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            if self.bars_in_position >= self.config.max_hold_bars {
                orders.push(OrderRequest::market_sell(ctx.symbol.clone(), pos.quantity));
                return orders;
            }

            let momentum = self.get_momentum_state();
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

        // Entry logic
        let should_buy = match self.get_ema_alignment() {
            Some(side) => side == Side::Buy,
            None => false,
        };

        if !should_buy {
            return orders;
        }

        if self.config.adx_threshold > 0.0 && self.current_adx < self.config.adx_threshold {
            return orders;
        }

        if self.config.use_macd {
            let momentum = self.get_momentum_state();
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
        // BATCH calculation (rarely called)
        use crate::indicators::atr;
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        
        let current_atr = atr(&high, &low, &close, self.config.atr_period)
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.01);
            
        entry_price - self.config.stop_atr_multiple * current_atr
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        use crate::indicators::atr;
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        
        let current_atr = atr(&high, &low, &close, self.config.atr_period)
            .last()
            .and_then(|&x| x)
            .unwrap_or(entry_price * 0.01);
            
        entry_price + self.config.target_atr_multiple * current_atr
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        use crate::indicators::atr;
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let current_atr = atr(&high, &low, &close, self.config.atr_period)
            .last()
            .and_then(|&x| x)
            .unwrap_or(current_price * 0.01);

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

    fn get_regime_score(&self, _candles: &[Candle]) -> f64 {
        match self.get_momentum_state() {
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

    fn init(&mut self) {
        self.bars_in_position = 0;
        self.cooldown_counter = 0;
        tracing::info!("Momentum Scalper strategy initialized");
    }
}
