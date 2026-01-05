//! Execution engine with intra-candle fill detection

use crate::oms::types::{Fill, Order, OrderId, OrderState, OrderType};
use crate::{Candle, Side};
use chrono::Utc;

/// Fill price with maker/taker flag
#[derive(Debug, Clone)]
pub struct FillPrice {
    pub price: f64,
    pub is_maker: bool,
}

/// Execution engine for processing orders against candles
pub struct ExecutionEngine {
    maker_commission_rate: f64,
    taker_commission_rate: f64,
    slippage: f64,
}

impl ExecutionEngine {
    /// Create new execution engine
    pub fn new(maker_commission_rate: f64, taker_commission_rate: f64, slippage: f64) -> Self {
        Self {
            maker_commission_rate,
            taker_commission_rate,
            slippage,
        }
    }

    /// Check if order fills during this candle
    pub fn check_fill(&self, order: &Order, candle: &Candle) -> Option<FillPrice> {
        match (order.side, order.order_type) {
            // Buy limit: fills if candle low ≤ limit price
            (Side::Buy, OrderType::Limit) => {
                let limit_price = order.limit_price?;
                if candle.low <= limit_price {
                    Some(FillPrice {
                        price: limit_price,
                        is_maker: true,
                    })
                } else {
                    None
                }
            }

            // Sell limit: fills if candle high ≥ limit price
            (Side::Sell, OrderType::Limit) => {
                let limit_price = order.limit_price?;
                if candle.high >= limit_price {
                    Some(FillPrice {
                        price: limit_price,
                        is_maker: true,
                    })
                } else {
                    None
                }
            }

            // Buy stop: triggers if candle high ≥ stop price
            (Side::Buy, OrderType::Stop) => {
                let stop_price = order.stop_price?;
                if candle.high >= stop_price {
                    // Becomes market order, fills at stop price + slippage
                    Some(FillPrice {
                        price: stop_price * (1.0 + self.slippage),
                        is_maker: false,
                    })
                } else {
                    None
                }
            }

            // Sell stop: triggers if candle low ≤ stop price
            (Side::Sell, OrderType::Stop) => {
                let stop_price = order.stop_price?;
                if candle.low <= stop_price {
                    Some(FillPrice {
                        price: stop_price * (1.0 - self.slippage),
                        is_maker: false,
                    })
                } else {
                    None
                }
            }

            // Market orders: fill at candle open
            (_, OrderType::Market) => Some(FillPrice {
                price: candle.open,
                is_maker: false,
            }),

            // StopLimit not yet implemented
            (_, OrderType::StopLimit) => None,
        }
    }

    /// Execute a partial fill
    pub fn execute_partial_fill(
        &self,
        order: &mut Order,
        fill_price: f64,
        max_fill_qty: f64,
        is_maker: bool,
    ) -> Fill {
        let fill_qty = f64::min(order.remaining_quantity, max_fill_qty);

        // Calculate commission
        let commission_rate = if is_maker {
            self.maker_commission_rate
        } else {
            self.taker_commission_rate
        };
        let commission = fill_price * fill_qty * commission_rate;

        // Update average fill price (weighted average)
        let prev_total_value = order.average_fill_price * order.filled_quantity;
        let new_value = fill_price * fill_qty;
        let new_total_qty = order.filled_quantity + fill_qty;

        order.average_fill_price = if new_total_qty > 0.0 {
            (prev_total_value + new_value) / new_total_qty
        } else {
            fill_price
        };

        // Update quantities
        order.filled_quantity += fill_qty;
        order.remaining_quantity -= fill_qty;

        // Update state
        order.state = if order.remaining_quantity <= 1e-8 {
            // Use epsilon for floating point comparison
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled
        };

        order.updated_at = Utc::now();

        Fill {
            order_id: order.id,
            price: fill_price,
            quantity: fill_qty,
            timestamp: candle.datetime,
            commission,
            is_maker,
        }
    }

    /// Execute a complete fill (convenience wrapper)
    pub fn execute_fill(&self, order: &mut Order, fill_price: f64, is_maker: bool) -> Fill {
        self.execute_partial_fill(order, fill_price, order.remaining_quantity, is_maker)
    }
}

impl Default for ExecutionEngine {
    fn default() -> Self {
        Self::new(0.0004, 0.0006, 0.001) // Default: 0.04% maker, 0.06% taker, 0.1% slippage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oms::types::TimeInForce;
    use crate::Symbol;

    fn create_candle(open: f64, high: f64, low: f64, close: f64) -> Candle {
        Candle::new_unchecked(Utc::now(), open, high, low, close, 1000.0)
    }

    #[test]
    fn test_buy_limit_fill() {
        let engine = ExecutionEngine::default();
        let order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        // Candle touches limit price
        let candle = create_candle(51000.0, 52000.0, 49500.0, 50500.0);
        let fill = engine.check_fill(&order, &candle);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert_eq!(fill.price, 50000.0);
        assert!(fill.is_maker);
    }

    #[test]
    fn test_buy_limit_no_fill() {
        let engine = ExecutionEngine::default();
        let order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        // Candle doesn't reach limit price
        let candle = create_candle(51000.0, 52000.0, 50100.0, 51500.0);
        let fill = engine.check_fill(&order, &candle);

        assert!(fill.is_none());
    }

    #[test]
    fn test_sell_limit_fill() {
        let engine = ExecutionEngine::default();
        let order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Sell,
            OrderType::Limit,
            1.0,
            Some(52000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        // Candle reaches limit price
        let candle = create_candle(51000.0, 52500.0, 50500.0, 51500.0);
        let fill = engine.check_fill(&order, &candle);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert_eq!(fill.price, 52000.0);
        assert!(fill.is_maker);
    }

    #[test]
    fn test_buy_stop_fill() {
        let engine = ExecutionEngine::default();
        let order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Stop,
            1.0,
            None,
            Some(51000.0),
            TimeInForce::GTC,
            None,
        );

        // Candle triggers stop
        let candle = create_candle(50000.0, 51500.0, 49500.0, 50500.0);
        let fill = engine.check_fill(&order, &candle);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert!(fill.price > 51000.0); // Has slippage
        assert!(!fill.is_maker);
    }

    #[test]
    fn test_market_order_fill() {
        let engine = ExecutionEngine::default();
        let order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Market,
            1.0,
            None,
            None,
            TimeInForce::GTC,
            None,
        );

        let candle = create_candle(50000.0, 52000.0, 49500.0, 51000.0);
        let fill = engine.check_fill(&order, &candle);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert_eq!(fill.price, 50000.0); // Fills at open
        assert!(!fill.is_maker);
    }

    #[test]
    fn test_partial_fill() {
        let engine = ExecutionEngine::default();
        let mut order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            10.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        // Fill 3 out of 10
        let fill1 = engine.execute_partial_fill(&mut order, 50000.0, 3.0, true);
        assert_eq!(fill1.quantity, 3.0);
        assert_eq!(order.filled_quantity, 3.0);
        assert_eq!(order.remaining_quantity, 7.0);
        assert_eq!(order.state, OrderState::PartiallyFilled);

        // Fill remaining 7
        let fill2 = engine.execute_partial_fill(&mut order, 50100.0, 7.0, true);
        assert_eq!(fill2.quantity, 7.0);
        assert_eq!(order.filled_quantity, 10.0);
        assert!(order.remaining_quantity < 1e-8);
        assert_eq!(order.state, OrderState::Filled);

        // Check weighted average price
        let expected_avg = (50000.0 * 3.0 + 50100.0 * 7.0) / 10.0;
        assert!((order.average_fill_price - expected_avg).abs() < 0.01);
    }
}
