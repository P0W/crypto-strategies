//! Order Sizer - Shared position sizing logic for backtest and live trading
//!
//! This module centralizes the position sizing logic to ensure backtest and live
//! trading use identical order processing. Strategies return "unit signals" with
//! quantity=1.0, and the OrderSizer calculates actual position size based on:
//!
//! - Available capital
//! - Risk per trade (stop distance)
//! - Portfolio heat (existing positions)
//! - Regime score (market conditions)
//! - Current drawdown
//!
//! ## Architecture
//!
//! ```text
//! Strategy → OrderRequest(qty=1.0) → size_order() → Order(qty=calculated)
//! ```
//!
//! The separation ensures:
//! - Strategy decides WHAT to trade (direction, symbol)
//! - OrderSizer/RiskManager decides HOW MUCH (position sizing)

use crate::oms::strategy::OrderRequest;
use crate::oms::types::{Order, Position};
use crate::risk::RiskManager;
use crate::strategies::Strategy;
use crate::{Candle, Money, Symbol};
use std::collections::HashMap;

/// Result of sizing an order
#[derive(Debug)]
pub enum SizedOrder {
    /// Order was sized successfully
    Entry {
        order: Order,
        stop_price: f64,
        target_price: f64,
    },
    /// Exit order (uses strategy's quantity)
    Exit(Order),
    /// Order was rejected (risk limits, zero quantity, etc.)
    Rejected(OrderRejection),
}

/// Reason for order rejection
#[derive(Debug, Clone)]
pub enum OrderRejection {
    TradingHalted,
    MaxPositionsReached { current: usize, max: usize },
    ZeroQuantity,
    NoCandles,
}

/// Size an order request, applying risk management rules
///
/// This is the single source of truth for position sizing logic,
/// used by both backtest and live trading to ensure consistency.
///
/// # Arguments
/// * `req` - The order request from strategy (typically with quantity=1.0)
/// * `candles` - Price history for indicator calculations
/// * `has_position` - Whether there's an existing position for this symbol
/// * `position_count` - Total number of open positions
/// * `all_positions` - All current positions for portfolio heat calculation
/// * `risk_manager` - Reference to risk manager for position sizing
/// * `strategy` - Reference to strategy for stop/target/regime calculations
///
/// # Returns
/// * `SizedOrder::Entry` - Sized entry order with cached stop/target
/// * `SizedOrder::Exit` - Exit order with strategy's quantity
/// * `SizedOrder::Rejected` - Order rejected with reason
pub fn size_order(
    req: &OrderRequest,
    candles: &[Candle],
    has_position: bool,
    position_count: usize,
    all_positions: &[&Position],
    risk_manager: &RiskManager,
    strategy: &dyn Strategy,
) -> SizedOrder {
    // Exit orders pass through with strategy's quantity
    if has_position {
        return SizedOrder::Exit(req.to_order());
    }

    // Entry order - apply full risk management

    // Check if trading is halted (exit orders still allowed above)
    if risk_manager.should_halt_trading() {
        return SizedOrder::Rejected(OrderRejection::TradingHalted);
    }

    // Check position count limit
    if !risk_manager.can_open_position_count(position_count) {
        return SizedOrder::Rejected(OrderRejection::MaxPositionsReached {
            current: position_count,
            max: risk_manager.max_positions(),
        });
    }

    // Get current price from candles
    let price = match candles.last() {
        Some(c) => c.close,
        None => return SizedOrder::Rejected(OrderRejection::NoCandles),
    };

    // Calculate stop loss for risk-based position sizing
    let stop_price = strategy.calculate_stop_loss(candles, price, req.side);

    // Calculate regime score for position size adjustment
    let regime_score = strategy.get_regime_score(candles);

    // Calculate quantity via risk manager
    let quantity = risk_manager.calculate_position_size_with_regime(
        price,
        stop_price,
        all_positions,
        regime_score,
    );

    if quantity <= 0.0 {
        return SizedOrder::Rejected(OrderRejection::ZeroQuantity);
    }

    // Calculate target price for entry level caching
    let target_price = strategy.calculate_take_profit(candles, price, req.side);

    // Create order with risk-calculated quantity
    let mut order = req.to_order();
    order.quantity = Money::from_f64(quantity);
    order.remaining_quantity = Money::from_f64(quantity);

    SizedOrder::Entry {
        order,
        stop_price,
        target_price,
    }
}

/// Order sizer struct for convenience (wraps the free function)
///
/// Use this when you want to size multiple orders with the same risk_manager/strategy
/// references and don't need mutable access between calls.
pub struct OrderSizer<'a> {
    risk_manager: &'a RiskManager,
    strategy: &'a dyn Strategy,
}

impl<'a> OrderSizer<'a> {
    /// Create a new order sizer
    pub fn new(risk_manager: &'a RiskManager, strategy: &'a dyn Strategy) -> Self {
        Self {
            risk_manager,
            strategy,
        }
    }

    /// Size an order request (delegates to free function)
    pub fn size_order(
        &self,
        req: &OrderRequest,
        candles: &[Candle],
        has_position: bool,
        position_count: usize,
        all_positions: &[&Position],
    ) -> SizedOrder {
        size_order(
            req,
            candles,
            has_position,
            position_count,
            all_positions,
            self.risk_manager,
            self.strategy,
        )
    }

    /// Process multiple order requests and return sized orders with entry level updates
    ///
    /// This is a convenience method that processes all orders and returns both the
    /// sized orders and the entry levels that should be cached.
    pub fn process_orders(
        &self,
        requests: &[OrderRequest],
        candles: &[Candle],
        has_position_fn: impl Fn(&Symbol) -> bool,
        position_count: usize,
        all_positions: &[&Position],
    ) -> (Vec<Order>, HashMap<Symbol, (f64, f64)>) {
        let mut orders = Vec::new();
        let mut entry_levels = HashMap::new();

        for req in requests {
            let has_position = has_position_fn(&req.symbol);

            match self.size_order(req, candles, has_position, position_count, all_positions) {
                SizedOrder::Entry {
                    order,
                    stop_price,
                    target_price,
                } => {
                    entry_levels.insert(req.symbol.clone(), (stop_price, target_price));
                    orders.push(order);
                }
                SizedOrder::Exit(order) => {
                    orders.push(order);
                }
                SizedOrder::Rejected(reason) => {
                    tracing::debug!("Order for {} rejected: {:?}", req.symbol, reason);
                }
            }
        }

        (orders, entry_levels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oms::strategy::OrderRequest;
    use crate::risk::RiskManager;
    use crate::strategies::volatility_regime::{VolatilityRegimeConfig, VolatilityRegimeStrategy};
    use crate::{Candle, Symbol};
    use chrono::Utc;

    fn create_candles(prices: &[f64]) -> Vec<Candle> {
        prices
            .iter()
            .map(|&p| Candle::new_unchecked(Utc::now(), p, p * 1.01, p * 0.99, p, 1000.0))
            .collect()
    }

    fn create_risk_manager() -> RiskManager {
        RiskManager::new(
            10000.0, // initial_capital
            0.02,    // risk_per_trade (2%)
            5,       // max_positions
            0.30,    // max_portfolio_heat (30%)
            0.25,    // max_position_pct (25%)
            0.20,    // max_drawdown (20%)
            0.10,    // drawdown_warning (10%)
            0.15,    // drawdown_critical (15%)
            0.75,    // drawdown_warning_multiplier
            0.5,     // drawdown_critical_multiplier
            3,       // consecutive_loss_limit
            0.5,     // consecutive_loss_multiplier
        )
    }

    fn create_strategy() -> Box<dyn Strategy> {
        Box::new(VolatilityRegimeStrategy::new(
            VolatilityRegimeConfig::default(),
        ))
    }

    #[test]
    fn test_entry_order_sizing() {
        let rm = create_risk_manager();
        let strategy = create_strategy();
        let sizer = OrderSizer::new(&rm, strategy.as_ref());

        let candles = create_candles(&[100.0; 50]);
        let req = OrderRequest::market_buy(Symbol::new("BTCUSDT"), 1.0);

        let result = sizer.size_order(&req, &candles, false, 0, &[]);

        match result {
            SizedOrder::Entry {
                order,
                stop_price,
                target_price,
            } => {
                // Quantity should be calculated, not 1.0
                assert!(order.quantity.to_f64() != 1.0);
                assert!(order.quantity.to_f64() > 0.0);
                // Stop should be below entry for long
                assert!(stop_price < 100.0);
                // Target should be above entry for long
                assert!(target_price > 100.0);
            }
            _ => panic!("Expected entry order"),
        }
    }

    #[test]
    fn test_exit_order_passes_through() {
        let rm = create_risk_manager();
        let strategy = create_strategy();
        let sizer = OrderSizer::new(&rm, strategy.as_ref());

        let candles = create_candles(&[100.0; 50]);
        let req = OrderRequest::market_sell(Symbol::new("BTCUSDT"), 5.5);

        // has_position=true means this is an exit order
        let result = sizer.size_order(&req, &candles, true, 1, &[]);

        match result {
            SizedOrder::Exit(order) => {
                // Exit order should preserve strategy's quantity
                assert_eq!(order.quantity.to_f64(), 5.5);
            }
            _ => panic!("Expected exit order"),
        }
    }

    #[test]
    fn test_max_positions_rejection() {
        let rm = create_risk_manager(); // max_positions = 5
        let strategy = create_strategy();
        let sizer = OrderSizer::new(&rm, strategy.as_ref());

        let candles = create_candles(&[100.0; 50]);
        let req = OrderRequest::market_buy(Symbol::new("BTCUSDT"), 1.0);

        // Already at max positions
        let result = sizer.size_order(&req, &candles, false, 5, &[]);

        match result {
            SizedOrder::Rejected(OrderRejection::MaxPositionsReached { current, max }) => {
                assert_eq!(current, 5);
                assert_eq!(max, 5);
            }
            _ => panic!("Expected rejection due to max positions"),
        }
    }

    #[test]
    fn test_no_candles_rejection() {
        let rm = create_risk_manager();
        let strategy = create_strategy();
        let sizer = OrderSizer::new(&rm, strategy.as_ref());

        let candles: Vec<Candle> = vec![];
        let req = OrderRequest::market_buy(Symbol::new("BTCUSDT"), 1.0);

        let result = sizer.size_order(&req, &candles, false, 0, &[]);

        match result {
            SizedOrder::Rejected(OrderRejection::NoCandles) => {}
            _ => panic!("Expected rejection due to no candles"),
        }
    }
}
