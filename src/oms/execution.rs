//! Execution engine with intra-candle fill detection

use crate::config::ExchangeConfig;
use crate::oms::costs::TransactionCostCalculator;
use crate::oms::types::{Fill, Order, OrderId, OrderState, OrderType};
use crate::{Candle, Money, Side};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Stop,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExitTrigger {
    pub reason: ExitReason,
    pub trigger_price: f64,
    pub execution_price: f64,
}

pub fn tighten_trailing_stop(side: Side, stored: Option<f64>, candidate: f64) -> f64 {
    match (side, stored) {
        (Side::Buy, Some(current)) => candidate.max(current),
        (Side::Sell, Some(current)) => candidate.min(current),
        (_, None) => candidate,
    }
}

/// Resolve stop/target hits conservatively when both occur in the same candle.
pub fn evaluate_exit(
    side: Side,
    candle: &Candle,
    stop_price: f64,
    target_price: f64,
) -> Option<ExitTrigger> {
    let stopped = match side {
        Side::Buy => candle.low <= stop_price,
        Side::Sell => candle.high >= stop_price,
    };
    let target_hit = match side {
        Side::Buy => candle.high >= target_price,
        Side::Sell => candle.low <= target_price,
    };

    let opened_through_stop = match side {
        Side::Buy => candle.open <= stop_price,
        Side::Sell => candle.open >= stop_price,
    };
    let opened_through_target = match side {
        Side::Buy => candle.open >= target_price,
        Side::Sell => candle.open <= target_price,
    };

    let (reason, trigger_price) = if opened_through_stop {
        (ExitReason::Stop, stop_price)
    } else if opened_through_target {
        (ExitReason::Target, target_price)
    } else if stopped {
        (ExitReason::Stop, stop_price)
    } else if target_hit {
        (ExitReason::Target, target_price)
    } else {
        return None;
    };

    let execution_price = match (side, reason) {
        (Side::Buy, ExitReason::Stop) => candle.open.min(trigger_price),
        (Side::Sell, ExitReason::Stop) => candle.open.max(trigger_price),
        (Side::Buy, ExitReason::Target) => candle.open.max(trigger_price),
        (Side::Sell, ExitReason::Target) => candle.open.min(trigger_price),
    };

    Some(ExitTrigger {
        reason,
        trigger_price,
        execution_price,
    })
}

/// Fill price with maker/taker flag
#[derive(Debug, Clone)]
pub struct FillPrice {
    pub price: f64,
    pub is_maker: bool,
}

/// Execution engine for processing orders against candles
pub struct ExecutionEngine {
    cost_calculator: TransactionCostCalculator,
    slippage: f64,
}

impl ExecutionEngine {
    /// Create new execution engine
    pub fn new(maker_commission_rate: f64, taker_commission_rate: f64, slippage: f64) -> Self {
        Self {
            cost_calculator: TransactionCostCalculator::percentage(
                maker_commission_rate,
                taker_commission_rate,
            ),
            slippage,
        }
    }

    pub fn from_exchange_config(config: &ExchangeConfig) -> Self {
        Self {
            cost_calculator: TransactionCostCalculator::from_exchange_config(config),
            slippage: config.assumed_slippage,
        }
    }

    pub fn estimate_commission(&self, side: Side, turnover: f64, is_maker: bool) -> Money {
        self.cost_calculator.estimate(side, turnover, is_maker)
    }

    pub fn calculate_commission(
        &self,
        order_id: OrderId,
        symbol: &crate::Symbol,
        side: Side,
        turnover: f64,
        is_maker: bool,
        timestamp: DateTime<Utc>,
    ) -> Money {
        self.cost_calculator
            .calculate(order_id, symbol, side, turnover, is_maker, timestamp)
    }

    pub fn check_fill(
        &self,
        order: &Order,
        candle: &Candle,
        current_bar_idx: Option<usize>,
    ) -> Option<FillPrice> {
        if order.is_complete() {
            return None;
        }

        if let (Some(created_idx), Some(current_idx)) = (order.created_bar_idx, current_bar_idx) {
            if matches!(order.order_type, OrderType::Limit) && created_idx >= current_idx {
                return None;
            }
        }

        match (order.side, order.order_type) {
            (Side::Buy, OrderType::Limit) => {
                let limit_price = order.limit_price?.to_f64();
                if candle.low <= limit_price {
                    Some(FillPrice {
                        price: limit_price,
                        is_maker: true,
                    })
                } else {
                    None
                }
            }
            (Side::Sell, OrderType::Limit) => {
                let limit_price = order.limit_price?.to_f64();
                if candle.high >= limit_price {
                    Some(FillPrice {
                        price: limit_price,
                        is_maker: true,
                    })
                } else {
                    None
                }
            }
            (Side::Buy, OrderType::Stop) => {
                let stop_price = order.stop_price?.to_f64();
                if candle.high >= stop_price {
                    let execution_price = candle.open.max(stop_price);
                    Some(FillPrice {
                        price: execution_price * (1.0 + self.slippage),
                        is_maker: false,
                    })
                } else {
                    None
                }
            }
            (Side::Sell, OrderType::Stop) => {
                let stop_price = order.stop_price?.to_f64();
                if candle.low <= stop_price {
                    let execution_price = candle.open.min(stop_price);
                    Some(FillPrice {
                        price: execution_price * (1.0 - self.slippage),
                        is_maker: false,
                    })
                } else {
                    None
                }
            }
            (_, OrderType::Market) => Some(FillPrice {
                price: candle.open,
                is_maker: false,
            }),
            (_, OrderType::StopLimit) => None,
        }
    }

    pub fn execute_partial_fill(
        &self,
        order: &mut Order,
        fill_price: f64,
        max_fill_qty: f64,
        is_maker: bool,
        timestamp: DateTime<Utc>,
    ) -> Fill {
        let fill_qty = Money::from_f64(f64::min(order.remaining_quantity.to_f64(), max_fill_qty));
        let fill_price_m = Money::from_f64(fill_price);

        let turnover = fill_price * fill_qty.to_f64();
        let commission = self.calculate_commission(
            order.id,
            &order.symbol,
            order.side,
            turnover,
            is_maker,
            timestamp,
        );

        // Update weighted average fill price
        let prev_total_value = order.average_fill_price * order.filled_quantity;
        let new_value = fill_price_m * fill_qty;
        let new_total_qty = order.filled_quantity + fill_qty;

        order.average_fill_price = if new_total_qty.is_positive() {
            (prev_total_value + new_value) / new_total_qty
        } else {
            fill_price_m
        };

        order.filled_quantity += fill_qty;
        order.remaining_quantity -= fill_qty;

        order.state = if order.remaining_quantity.to_f64() <= 1e-8 {
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled
        };

        order.updated_at = timestamp;

        Fill {
            order_id: order.id,
            price: fill_price_m,
            quantity: fill_qty,
            timestamp,
            commission,
            is_maker,
        }
    }

    pub fn execute_fill(
        &self,
        order: &mut Order,
        fill_price: f64,
        is_maker: bool,
        timestamp: DateTime<Utc>,
    ) -> Fill {
        self.execute_partial_fill(
            order,
            fill_price,
            order.remaining_quantity.to_f64(),
            is_maker,
            timestamp,
        )
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
        let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);
        let order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        let candle = create_candle(51000.0, 52000.0, 49500.0, 50500.0);
        let fill = engine.check_fill(&order, &candle, None);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert_eq!(fill.price, 50000.0);
        assert!(fill.is_maker);
    }

    #[test]
    fn test_buy_limit_no_fill() {
        let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);
        let order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        let candle = create_candle(51000.0, 52000.0, 50100.0, 51500.0);
        let fill = engine.check_fill(&order, &candle, None);
        assert!(fill.is_none());
    }

    #[test]
    fn test_sell_limit_fill() {
        let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);
        let order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Sell,
            OrderType::Limit,
            1.0,
            Some(52000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        let candle = create_candle(51000.0, 52500.0, 50500.0, 51500.0);
        let fill = engine.check_fill(&order, &candle, None);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert_eq!(fill.price, 52000.0);
        assert!(fill.is_maker);
    }

    #[test]
    fn test_buy_stop_fill() {
        let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);
        let order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Stop,
            1.0,
            None,
            Some(51000.0),
            TimeInForce::GTC,
            None,
        );

        let candle = create_candle(50000.0, 51500.0, 49500.0, 50500.0);
        let fill = engine.check_fill(&order, &candle, None);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert!(fill.price > 51000.0);
        assert!(!fill.is_maker);
    }

    #[test]
    fn test_market_order_fill() {
        let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);
        let order = Order::from_f64(
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
        let fill = engine.check_fill(&order, &candle, None);

        assert!(fill.is_some());
        let fill = fill.unwrap();
        assert_eq!(fill.price, 50000.0);
        assert!(!fill.is_maker);
    }

    #[test]
    fn test_partial_fill() {
        let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);
        let mut order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            10.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        let timestamp = Utc::now();

        let fill1 = engine.execute_partial_fill(&mut order, 50000.0, 3.0, true, timestamp);
        assert_eq!(fill1.quantity.to_f64(), 3.0);
        assert_eq!(order.filled_quantity.to_f64(), 3.0);
        assert_eq!(order.remaining_quantity.to_f64(), 7.0);
        assert_eq!(order.state, OrderState::PartiallyFilled);

        let fill2 = engine.execute_partial_fill(&mut order, 50100.0, 7.0, true, timestamp);
        assert_eq!(fill2.quantity.to_f64(), 7.0);
        assert_eq!(order.filled_quantity.to_f64(), 10.0);
        assert!(order.remaining_quantity.to_f64() < 1e-8);
        assert_eq!(order.state, OrderState::Filled);

        let expected_avg = (50000.0 * 3.0 + 50100.0 * 7.0) / 10.0;
        assert!((order.average_fill_price.to_f64() - expected_avg).abs() < 0.01);
    }

    #[test]
    fn test_stop_order_uses_gap_open() {
        let engine = ExecutionEngine::new(0.001, 0.002, 0.001);
        let mut order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Sell,
            OrderType::Stop,
            1.0,
            None,
            Some(95.0),
            TimeInForce::GTC,
            None,
        );
        order.state = OrderState::Open;

        let candle = create_candle(90.0, 92.0, 85.0, 88.0);
        let fill = engine.check_fill(&order, &candle, None).unwrap();
        assert!((fill.price - 89.91).abs() < 1e-9);
    }

    #[test]
    fn test_complete_order_cannot_fill_again() {
        let engine = ExecutionEngine::new(0.001, 0.002, 0.001);
        let mut order = Order::from_f64(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Market,
            1.0,
            None,
            None,
            TimeInForce::GTC,
            None,
        );
        order.state = OrderState::Filled;

        let candle = create_candle(100.0, 101.0, 99.0, 100.0);
        assert!(engine.check_fill(&order, &candle, None).is_none());
    }

    #[test]
    fn test_short_trailing_stop_tightens_downward() {
        assert_eq!(tighten_trailing_stop(Side::Sell, Some(110.0), 105.0), 105.0);
        assert_eq!(tighten_trailing_stop(Side::Sell, Some(105.0), 108.0), 105.0);
    }

    #[test]
    fn test_exit_prefers_stop_when_both_levels_trade() {
        let candle = create_candle(100.0, 112.0, 88.0, 105.0);
        let trigger = evaluate_exit(Side::Buy, &candle, 90.0, 110.0).unwrap();

        assert_eq!(trigger.reason, ExitReason::Stop);
        assert_eq!(trigger.trigger_price, 90.0);
        assert_eq!(trigger.execution_price, 90.0);
    }

    #[test]
    fn test_gap_open_target_precedes_later_intrabar_stop() {
        let candle = create_candle(115.0, 120.0, 88.0, 100.0);
        let trigger = evaluate_exit(Side::Buy, &candle, 90.0, 110.0).unwrap();

        assert_eq!(trigger.reason, ExitReason::Target);
        assert_eq!(trigger.execution_price, 115.0);
    }
}
