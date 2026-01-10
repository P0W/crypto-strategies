//! Core OMS types
//!
//! Defines orders, fills, positions, and related enumerations.
//! Uses Money type for all monetary values to prevent floating-point drift.

use crate::{Money, Side, Symbol};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Order ID type - u64 for performance
pub type OrderId = u64;

/// Atomic counter for fast order ID generation
static ORDER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate next order ID (thread-safe, lock-free)
pub fn next_order_id() -> OrderId {
    ORDER_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Order type - determines execution logic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Execute immediately at market price (next candle open)
    Market,

    /// Execute when price reaches limit price
    /// Buy limit: executes when price ≤ limit_price
    /// Sell limit: executes when price ≥ limit_price
    Limit,

    /// Stop-loss: converts to market when stop triggered
    /// Buy stop: triggers when price ≥ stop_price
    /// Sell stop: triggers when price ≤ stop_price
    Stop,

    /// Stop-limit: converts to limit order when stop triggered
    StopLimit,
}

/// Time-in-force specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good till cancelled
    GTC,

    /// Good till date
    GTD(DateTime<Utc>),

    /// Immediate or cancel (fill immediately or cancel)
    IOC,

    /// Fill or kill (fill completely or cancel)
    FOK,
}

/// Order state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderState {
    /// Order created, not yet submitted
    Pending,

    /// Order submitted to exchange/orderbook
    Submitted,

    /// Order accepted and active
    Open,

    /// Order partially filled
    PartiallyFilled,

    /// Order completely filled
    Filled,

    /// Order cancelled by user
    Cancelled,

    /// Order rejected (insufficient margin, invalid price, etc.)
    Rejected,

    /// Order expired (GTD timeout)
    Expired,
}

/// Core order structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub limit_price: Option<Money>,
    pub stop_price: Option<Money>,
    pub quantity: Money,
    pub filled_quantity: Money,
    pub remaining_quantity: Money,
    pub average_fill_price: Money,
    pub state: OrderState,
    pub time_in_force: TimeInForce,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub strategy_tag: Option<String>,
    pub client_id: Option<String>,
    pub created_bar_idx: Option<usize>,
}

impl Order {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        symbol: Symbol,
        side: Side,
        order_type: OrderType,
        quantity: Money,
        limit_price: Option<Money>,
        stop_price: Option<Money>,
        time_in_force: TimeInForce,
        client_id: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: next_order_id(),
            symbol,
            side,
            order_type,
            limit_price,
            stop_price,
            quantity,
            filled_quantity: Money::ZERO,
            remaining_quantity: quantity,
            average_fill_price: Money::ZERO,
            state: OrderState::Pending,
            time_in_force,
            created_at: now,
            updated_at: now,
            strategy_tag: None,
            client_id,
            created_bar_idx: None,
        }
    }

    /// Create from f64 values (for migration compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn from_f64(
        symbol: Symbol,
        side: Side,
        order_type: OrderType,
        quantity: f64,
        limit_price: Option<f64>,
        stop_price: Option<f64>,
        time_in_force: TimeInForce,
        client_id: Option<String>,
    ) -> Self {
        Self::new(
            symbol,
            side,
            order_type,
            Money::from_f64(quantity),
            limit_price.map(Money::from_f64),
            stop_price.map(Money::from_f64),
            time_in_force,
            client_id,
        )
    }

    pub fn with_created_bar_idx(mut self, bar_idx: usize) -> Self {
        self.created_bar_idx = Some(bar_idx);
        self
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            OrderState::Open | OrderState::PartiallyFilled | OrderState::Submitted
        )
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.state,
            OrderState::Filled | OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
        )
    }
}

/// Individual fill record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: OrderId,
    pub price: Money,
    pub quantity: Money,
    pub timestamp: DateTime<Utc>,
    pub commission: Money,
    pub is_maker: bool,
}

impl Fill {
    pub fn from_f64(
        order_id: OrderId,
        price: f64,
        quantity: f64,
        timestamp: DateTime<Utc>,
        commission: f64,
        is_maker: bool,
    ) -> Self {
        Self {
            order_id,
            price: Money::from_f64(price),
            quantity: Money::from_f64(quantity),
            timestamp,
            commission: Money::from_f64(commission),
            is_maker,
        }
    }
}

/// Position with FIFO accounting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: Symbol,
    pub side: Side,
    pub average_entry_price: Money,
    pub quantity: Money,
    pub realized_pnl: Money,
    pub unrealized_pnl: Money,
    pub fills: Vec<Fill>,
    pub first_entry_time: DateTime<Utc>,
    pub last_update_time: DateTime<Utc>,
    pub risk_amount: Money,
}

impl Position {
    pub fn from_fill(fill: Fill, symbol: Symbol, side: Side) -> Self {
        Self {
            symbol,
            side,
            average_entry_price: fill.price,
            quantity: fill.quantity,
            realized_pnl: Money::ZERO,
            unrealized_pnl: Money::ZERO,
            fills: vec![fill.clone()],
            first_entry_time: fill.timestamp,
            last_update_time: fill.timestamp,
            risk_amount: Money::ZERO,
        }
    }

    pub fn set_risk_amount(&mut self, risk_amount: f64) {
        self.risk_amount = Money::from_f64(risk_amount);
    }

    pub fn update_unrealized_pnl(&mut self, current_price: f64) {
        let price = Money::from_f64(current_price);
        self.unrealized_pnl = match self.side {
            Side::Buy => (price - self.average_entry_price) * self.quantity,
            Side::Sell => (self.average_entry_price - price) * self.quantity,
        };
    }

    pub fn unrealized_pnl(&self, current_price: f64) -> f64 {
        let price = Money::from_f64(current_price);
        let pnl = match self.side {
            Side::Buy => (price - self.average_entry_price) * self.quantity,
            Side::Sell => (self.average_entry_price - price) * self.quantity,
        };
        pnl.to_f64()
    }

    pub fn current_value(&self, current_price: f64) -> f64 {
        (self.quantity * Money::from_f64(current_price)).to_f64()
    }

    pub fn total_pnl(&self) -> f64 {
        (self.realized_pnl + self.unrealized_pnl).to_f64()
    }

    pub fn total_commission(&self) -> f64 {
        self.fills
            .iter()
            .map(|f| f.commission)
            .sum::<Money>()
            .to_f64()
    }

    pub fn total_quantity_traded(&self) -> f64 {
        self.fills
            .iter()
            .map(|f| f.quantity)
            .sum::<Money>()
            .to_f64()
    }

    pub fn entry_time(&self) -> DateTime<Utc> {
        self.first_entry_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_id_generation() {
        let id1 = next_order_id();
        let id2 = next_order_id();
        assert!(id2 > id1);
    }

    #[test]
    fn test_order_creation() {
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

        assert_eq!(order.quantity.to_f64(), 1.0);
        assert_eq!(order.remaining_quantity.to_f64(), 1.0);
        assert_eq!(order.filled_quantity.to_f64(), 0.0);
        assert_eq!(order.state, OrderState::Pending);
        assert!(order.is_active() || order.state == OrderState::Pending);
    }

    #[test]
    fn test_position_unrealized_pnl() {
        let fill = Fill::from_f64(1, 50000.0, 1.0, Utc::now(), 10.0, true);
        let mut position = Position::from_fill(fill, Symbol::new("BTCUSDT"), Side::Buy);
        position.update_unrealized_pnl(51000.0);
        assert_eq!(position.unrealized_pnl.to_f64(), 1000.0);
    }
}
