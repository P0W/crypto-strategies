//! Regime-Aware Grid Trading Strategy Implementation
//!
//! This strategy implements a sophisticated grid trading system that adapts to
//! market conditions through regime classification. It focuses on capital preservation
//! by avoiding trading during unfavorable market conditions.
//!
//! Performance optimized: Indicators are calculated once per signal generation
//! and reused to avoid O(N²) complexity.

use crate::indicators::{adx, atr, ema, rsi};
use crate::oms::{OrderRequest, StrategyContext};
use crate::strategies::Strategy;
use crate::{Candle, Position, Side};
use chrono::{DateTime, Utc};

use super::config::RegimeGridConfig;
use super::MarketRegime;

/// Pre-calculated indicators to avoid redundant computation
struct Indicators {
    current_ema_short: Option<f64>,
    current_ema_long: Option<f64>,
    current_adx: Option<f64>,
    current_rsi: Option<f64>,
}

impl Indicators {
    /// Calculate all indicators once from candle data
    fn new(candles: &[Candle], config: &RegimeGridConfig) -> Self {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

        let ema_short = ema(&close, config.ema_short_period);
        let ema_long = ema(&close, config.ema_long_period);
        let adx_values = adx(&high, &low, &close, config.adx_period);
        let rsi_values = rsi(&close, config.rsi_period);

        Self {
            current_ema_short: ema_short.last().and_then(|&x| x),
            current_ema_long: ema_long.last().and_then(|&x| x),
            current_adx: adx_values.last().and_then(|&x| x),
            current_rsi: rsi_values.last().and_then(|&x| x),
        }
    }

    /// Calculate ATR only (for stop/target/trailing methods and volatility check)
    fn atr_only(candles: &[Candle], atr_period: usize) -> Option<f64> {
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        atr(&high, &low, &close, atr_period).last().and_then(|&x| x)
    }
}
use std::sync::RwLock;

/// Grid state tracking
#[derive(Debug, Clone, Default)]
struct GridState {
    /// When volatility kill switch was activated (None if not active)
    paused_until: Option<DateTime<Utc>>,
    /// When drawdown limit was breached (None if not active)
    /// Strategy will not trade until equity recovers to peak
    drawdown_breach_time: Option<DateTime<Utc>>,
    /// The peak equity when drawdown was breached (used for recovery check)
    drawdown_breach_peak: Option<f64>,
}

/// Regime-Aware Grid Trading Strategy
pub struct RegimeGridStrategy {
    config: RegimeGridConfig,
    state: RwLock<GridState>,
}

impl RegimeGridStrategy {
    pub fn new(config: RegimeGridConfig) -> Self {
        RegimeGridStrategy {
            config,
            state: RwLock::new(GridState::default()),
        }
    }

    /// Calculate position size for a single grid level based on available capital
    /// Divides the allocated capital across all grid levels
    fn calculate_grid_quantity(&self, ctx: &StrategyContext, price: f64, num_levels: usize) -> f64 {
        // Guard against invalid inputs
        if ctx.equity <= 0.0 || price <= 0.0 || num_levels == 0 {
            return 0.0;
        }

        // Use configured max_capital_usage_pct (default 40%) of equity
        let capital_for_grid = ctx.equity * self.config.max_capital_usage_pct;
        // Divide equally across grid levels
        let capital_per_level = capital_for_grid / num_levels as f64;
        // Convert to quantity at given price, ensure non-negative
        (capital_per_level / price).max(0.0)
    }

    /// Check if volatility kill switch is active
    fn is_volatility_paused(&self) -> bool {
        let state = self.state.read().unwrap();
        if let Some(paused_until) = state.paused_until {
            Utc::now() < paused_until
        } else {
            false
        }
    }

    /// Classify market regime based on indicators
    fn classify_regime(&self, candles: &[Candle], ind: &Indicators) -> Option<MarketRegime> {
        if candles.is_empty() {
            return None;
        }

        let current_candle = candles.last()?;
        let current_price = current_candle.close;

        // Check for high volatility single candle
        let candle_change =
            ((current_candle.close - current_candle.open) / current_candle.open).abs();
        if candle_change > self.config.high_volatility_candle_pct {
            return Some(MarketRegime::HighVolatility);
        }

        let adx = ind.current_adx?;
        let ema_short = ind.current_ema_short?;
        let ema_long = ind.current_ema_long?;
        let rsi = ind.current_rsi?;

        // Regime 1: Sideways (IDEAL)
        // ADX < threshold AND price within ±band% of short EMA
        if adx < self.config.adx_sideways_threshold {
            let distance_from_ema = (current_price - ema_short).abs() / ema_short;
            if distance_from_ema <= self.config.ema_band_pct {
                return Some(MarketRegime::Sideways);
            }
        }

        // Regime 3: Bear Market (NO TRADING)
        // Price < long EMA AND RSI < bear threshold
        if current_price < ema_long && rsi < self.config.rsi_bear_threshold {
            return Some(MarketRegime::Bearish);
        }

        // Regime 2: Bull Market (MODIFIED GRID)
        // Price > long EMA AND RSI between bull_min and bull_max
        if current_price > ema_long
            && rsi >= self.config.rsi_bull_min
            && rsi <= self.config.rsi_bull_max
        {
            return Some(MarketRegime::Bullish);
        }

        // Default to high volatility if no clear regime
        Some(MarketRegime::HighVolatility)
    }

    /// Place grid of limit orders in sideways market
    fn place_grid_orders(
        &self,
        ctx: &StrategyContext,
        current_price: f64,
        orders: &mut Vec<OrderRequest>,
    ) {
        // Use config parameters for grid spacing
        let grid_spacing = current_price * self.config.grid_spacing_pct;
        let num_grids = self.config.max_grids.min(10); // Cap at 10 for safety

        // Calculate proper position size per grid level
        // Total grid levels = buy grids + potential sell grids
        let total_levels = num_grids * 2;
        let quantity_per_level = self.calculate_grid_quantity(ctx, current_price, total_levels);

        // Check existing orders to avoid duplicates
        let existing_buy_prices: Vec<f64> = ctx
            .open_orders
            .iter()
            .filter(|o| o.side == Side::Buy && o.limit_price.is_some())
            .map(|o| o.limit_price.unwrap().to_f64())
            .collect();

        let existing_sell_prices: Vec<f64> = ctx
            .open_orders
            .iter()
            .filter(|o| o.side == Side::Sell && o.limit_price.is_some())
            .map(|o| o.limit_price.unwrap().to_f64())
            .collect();

        // Place buy limit orders below current price
        for i in 1..=num_grids {
            let buy_price = current_price - (grid_spacing * i as f64);

            // Check if we already have an order near this price
            let has_nearby_order = existing_buy_prices
                .iter()
                .any(|&p| (p - buy_price).abs() < grid_spacing * 0.2);

            if !has_nearby_order && quantity_per_level > 0.0 {
                orders.push(
                    OrderRequest::limit_buy(ctx.symbol.clone(), quantity_per_level, buy_price)
                        .with_client_id(format!("grid_buy_{}", i)),
                );
            }
        }

        // Place sell limit orders above current price (if we have position)
        if let Some(pos) = ctx.current_position {
            if !pos.quantity.is_positive() {
                return;
            }

            let pos_qty = pos.quantity.to_f64();
            for i in 1..=num_grids {
                let sell_price = current_price + (grid_spacing * i as f64);

                let has_nearby_order = existing_sell_prices
                    .iter()
                    .any(|&p| (p - sell_price).abs() < grid_spacing * 0.2);

                if !has_nearby_order {
                    let sell_qty = quantity_per_level.min(pos_qty / num_grids as f64);
                    if sell_qty > 0.0 {
                        orders.push(
                            OrderRequest::limit_sell(ctx.symbol.clone(), sell_qty, sell_price)
                                .with_client_id(format!("grid_sell_{}", i)),
                        );
                    }
                }
            }
        }
    }

    /// Place grid biased for bull market
    fn place_bull_grid_orders(
        &self,
        ctx: &StrategyContext,
        current_price: f64,
        orders: &mut Vec<OrderRequest>,
    ) {
        let grid_spacing = current_price * self.config.bull_grid_spacing_pct;

        let buy_levels = self.config.bull_max_grids;
        let sell_levels = (self.config.bull_max_grids / 2).max(1);
        let total_levels = buy_levels + sell_levels;
        let quantity_per_level = self.calculate_grid_quantity(ctx, current_price, total_levels);

        let existing_buy_prices: Vec<f64> = ctx
            .open_orders
            .iter()
            .filter(|o| o.side == Side::Buy && o.limit_price.is_some())
            .map(|o| o.limit_price.unwrap().to_f64())
            .collect();

        for i in 1..=buy_levels {
            let buy_price = current_price - (grid_spacing * i as f64);

            let has_nearby_order = existing_buy_prices
                .iter()
                .any(|&p| (p - buy_price).abs() < grid_spacing * 0.2);

            if !has_nearby_order && quantity_per_level > 0.0 {
                orders.push(
                    OrderRequest::limit_buy(ctx.symbol.clone(), quantity_per_level, buy_price)
                        .with_client_id(format!("bull_grid_buy_{}", i)),
                );
            }
        }

        if let Some(pos) = ctx.current_position {
            let existing_sell_prices: Vec<f64> = ctx
                .open_orders
                .iter()
                .filter(|o| o.side == Side::Sell && o.limit_price.is_some())
                .map(|o| o.limit_price.unwrap().to_f64())
                .collect();

            let bull_sell_spacing = current_price * self.config.bull_sell_target_pct;

            for i in 1..=sell_levels {
                let sell_price = current_price + (bull_sell_spacing * i as f64);

                let has_nearby_order = existing_sell_prices
                    .iter()
                    .any(|&p| (p - sell_price).abs() < bull_sell_spacing * 0.2);

                if !has_nearby_order {
                    let sell_qty =
                        quantity_per_level.min(pos.quantity.to_f64() / sell_levels as f64);
                    if sell_qty > 0.0 {
                        orders.push(
                            OrderRequest::limit_sell(ctx.symbol.clone(), sell_qty, sell_price)
                                .with_client_id(format!("bull_grid_sell_{}", i)),
                        );
                    }
                }
            }
        }
    }

    /// Place sell-only orders when at max exposure (to take profits)
    fn place_sell_only_orders(
        &self,
        ctx: &StrategyContext,
        current_price: f64,
        pos: &Position,
        orders: &mut Vec<OrderRequest>,
    ) {
        if !pos.quantity.is_positive() {
            return;
        }

        let pos_qty = pos.quantity.to_f64();
        let grid_spacing = current_price * self.config.grid_spacing_pct;
        let num_sell_levels = 5.min(self.config.max_grids);

        let existing_sell_prices: Vec<f64> = ctx
            .open_orders
            .iter()
            .filter(|o| o.side == Side::Sell && o.limit_price.is_some())
            .map(|o| o.limit_price.unwrap().to_f64())
            .collect();

        for i in 1..=num_sell_levels {
            let sell_price = current_price + (grid_spacing * i as f64);

            let has_nearby_order = existing_sell_prices
                .iter()
                .any(|&p| (p - sell_price).abs() < grid_spacing * 0.2);

            if !has_nearby_order {
                let sell_qty = pos_qty / num_sell_levels as f64;
                if sell_qty > 0.0 {
                    orders.push(
                        OrderRequest::limit_sell(ctx.symbol.clone(), sell_qty, sell_price)
                            .with_client_id(format!("max_exp_sell_{}", i)),
                    );
                }
            }
        }
    }
}

impl Strategy for RegimeGridStrategy {
    fn name(&self) -> &'static str {
        "regime_grid"
    }

    fn generate_orders(&self, ctx: &StrategyContext) -> Vec<OrderRequest> {
        let mut orders = Vec::new();

        // Need minimum data for indicators
        let min_period = self
            .config
            .ema_long_period
            .max(self.config.adx_period)
            .max(self.config.rsi_period);

        if ctx.candles.len() < min_period {
            return orders;
        }

        // 1. Check volatility kill switch
        if self.is_volatility_paused() {
            return orders;
        }

        // 1b. Check ATR/price ratio for volatility kill - activate if too volatile
        let current_price = match ctx.candles.last() {
            Some(c) => c.close,
            None => return orders,
        };
        if let Some(current_atr) = Indicators::atr_only(ctx.candles, self.config.atr_period_1h) {
            let volatility_ratio = current_atr / current_price;
            if volatility_ratio > self.config.volatility_kill_threshold {
                tracing::warn!(
                    "{} Volatility kill switch activated: ATR/Price={:.2}% > threshold={:.2}%",
                    ctx.symbol,
                    volatility_ratio * 100.0,
                    self.config.volatility_kill_threshold * 100.0
                );
                // Set pause until
                {
                    let mut state = self.state.write().unwrap();
                    state.paused_until = Some(
                        Utc::now()
                            + chrono::Duration::hours(self.config.volatility_pause_hours as i64),
                    );
                }
                // Close any open position
                if let Some(pos) = ctx.current_position {
                    match pos.side {
                        Side::Buy => orders.push(OrderRequest::market_sell(
                            ctx.symbol.clone(),
                            pos.quantity.to_f64(),
                        )),
                        Side::Sell => orders.push(OrderRequest::market_buy(
                            ctx.symbol.clone(),
                            pos.quantity.to_f64(),
                        )),
                    }
                }
                return orders;
            }
        }

        // 2. CRITICAL: Check portfolio drawdown with cooldown logic
        // Ensure peak_equity is never less than current equity (shouldn't happen, but guard against it)
        let effective_peak = ctx.peak_equity.max(ctx.equity);
        let current_drawdown = if effective_peak > 0.0 {
            ((effective_peak - ctx.equity) / effective_peak).max(0.0)
        } else {
            0.0
        };

        // Check if we're in drawdown recovery mode
        {
            let state = self.state.read().unwrap();
            if let Some(breach_peak) = state.drawdown_breach_peak {
                // Only resume trading when equity recovers to 95% of the breach peak
                // This prevents whipsawing in/out during volatile recovery periods
                let recovery_threshold = breach_peak * 0.95;
                if ctx.equity < recovery_threshold {
                    // Still in cooldown - only close positions, don't open new ones
                    if let Some(pos) = ctx.current_position {
                        match pos.side {
                            Side::Buy => orders.push(OrderRequest::market_sell(
                                ctx.symbol.clone(),
                                pos.quantity.to_f64(),
                            )),
                            Side::Sell => orders.push(OrderRequest::market_buy(
                                ctx.symbol.clone(),
                                pos.quantity.to_f64(),
                            )),
                        }
                    }
                    return orders;
                } else {
                    // Will recover below - need to drop lock first
                    drop(state);
                    // Recovered! Clear the breach state
                    tracing::info!(
                        "{} Drawdown recovery complete: equity {:.0} >= recovery threshold {:.0}",
                        ctx.symbol,
                        ctx.equity,
                        recovery_threshold
                    );
                    let mut state = self.state.write().unwrap();
                    state.drawdown_breach_time = None;
                    state.drawdown_breach_peak = None;
                }
            }
        }

        // Check for new drawdown breach
        if current_drawdown > self.config.max_drawdown_pct {
            // Only log once when first breaching
            let should_log = self.state.read().unwrap().drawdown_breach_peak.is_none();
            if should_log {
                tracing::warn!(
                    "{} Drawdown limit hit: {:.1}% > {:.1}% - entering cooldown mode",
                    ctx.symbol,
                    current_drawdown * 100.0,
                    self.config.max_drawdown_pct * 100.0
                );
                let mut state = self.state.write().unwrap();
                state.drawdown_breach_time = Some(Utc::now());
                state.drawdown_breach_peak = Some(ctx.peak_equity);
            }

            // Close any open position when drawdown exceeded
            if let Some(pos) = ctx.current_position {
                match pos.side {
                    Side::Buy => orders.push(OrderRequest::market_sell(
                        ctx.symbol.clone(),
                        pos.quantity.to_f64(),
                    )),
                    Side::Sell => orders.push(OrderRequest::market_buy(
                        ctx.symbol.clone(),
                        pos.quantity.to_f64(),
                    )),
                }
            }
            return orders;
        }

        // 3. Check position exposure limit - don't add more if already at max
        let current_price = match ctx.candles.last() {
            Some(c) => c.close,
            None => return orders,
        };
        let position_value = ctx
            .current_position
            .map(|p| p.quantity.to_f64() * current_price)
            .unwrap_or(0.0);
        let max_position_value = ctx.equity * self.config.max_capital_usage_pct;
        let at_max_exposure = position_value >= max_position_value * 0.95;

        // 4. Calculate all indicators once
        let ind = Indicators::new(ctx.candles, &self.config);

        // 5. Classify market regime
        let regime = match self.classify_regime(ctx.candles, &ind) {
            Some(r) => r,
            None => return orders,
        };

        // 6. Apply regime-specific logic with actual grid orders
        match regime {
            MarketRegime::Bearish | MarketRegime::HighVolatility => {
                // Close positions in unfavorable regimes
                if let Some(pos) = ctx.current_position {
                    // Use opposite side to close the position
                    match pos.side {
                        Side::Buy => orders.push(OrderRequest::market_sell(
                            ctx.symbol.clone(),
                            pos.quantity.to_f64(),
                        )),
                        Side::Sell => orders.push(OrderRequest::market_buy(
                            ctx.symbol.clone(),
                            pos.quantity.to_f64(),
                        )),
                    }
                }
                orders
            }
            MarketRegime::Sideways => {
                // Only place new buy orders if not at max exposure
                if !at_max_exposure {
                    self.place_grid_orders(ctx, current_price, &mut orders);
                } else if let Some(pos) = ctx.current_position {
                    // At max exposure - only place sell orders to take profit
                    self.place_sell_only_orders(ctx, current_price, pos, &mut orders);
                }
                orders
            }
            MarketRegime::Bullish => {
                // Only place new buy orders if not at max exposure
                if !at_max_exposure {
                    self.place_bull_grid_orders(ctx, current_price, &mut orders);
                } else if let Some(pos) = ctx.current_position {
                    // At max exposure - only place sell orders to take profit
                    self.place_sell_only_orders(ctx, current_price, pos, &mut orders);
                }
                orders
            }
        }
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let atr =
            Indicators::atr_only(candles, self.config.adx_period).unwrap_or(entry_price * 0.02);
        let stop_distance = atr * self.config.stop_atr_multiple;

        match side {
            Side::Buy => entry_price - stop_distance,
            Side::Sell => entry_price + stop_distance,
        }
    }

    fn calculate_take_profit(&self, _candles: &[Candle], entry_price: f64, side: Side) -> f64 {
        let target_distance = entry_price * self.config.sell_target_pct;

        match side {
            Side::Buy => entry_price + target_distance,
            Side::Sell => entry_price - target_distance,
        }
    }

    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64> {
        let entry_price = position.average_entry_price.to_f64();
        let unrealized_pnl_pct = match position.side {
            Side::Buy => (current_price - entry_price) / entry_price,
            Side::Sell => (entry_price - current_price) / entry_price,
        };

        if unrealized_pnl_pct < self.config.trailing_activation_pct {
            return None;
        }

        let atr =
            Indicators::atr_only(candles, self.config.adx_period).unwrap_or(current_price * 0.02);

        let trailing_stop = match position.side {
            Side::Buy => current_price - (atr * self.config.trailing_atr_multiple),
            Side::Sell => current_price + (atr * self.config.trailing_atr_multiple),
        };

        Some(trailing_stop)
    }

    fn get_regime_score(&self, candles: &[Candle]) -> f64 {
        let ind = Indicators::new(candles, &self.config);

        match self.classify_regime(candles, &ind) {
            Some(MarketRegime::Sideways) => 1.5, // Ideal conditions
            Some(MarketRegime::Bullish) => 1.0,  // Modified grid
            _ => 0.0,                            // No trading
        }
    }

    fn clone_boxed(&self) -> Box<dyn Strategy> {
        Box::new(RegimeGridStrategy::new(self.config.clone()))
    }

    fn init(&mut self) {
        *self.state.write().unwrap() = GridState::default();
    }
}
