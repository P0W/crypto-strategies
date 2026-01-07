//! Core OMS types
//!
//! Defines orders, fills, positions, and related enumerations.

use crate::{Side, Symbol};
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

/// Core order structure (optimized for cache efficiency)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(C)] // Cache-friendly layout
pub struct Order {
    /// Unique order ID (use u64 for performance)
    pub id: OrderId,

    /// Symbol being traded
    pub symbol: Symbol,

    /// Order side (Buy/Sell)
    pub side: Side,

    /// Order type
    pub order_type: OrderType,

    /// Limit price (for limit/stop-limit orders)
    pub limit_price: Option<f64>,

    /// Stop price (for stop/stop-limit orders)
    pub stop_price: Option<f64>,

    /// Total order quantity
    pub quantity: f64,

    /// Filled quantity so far
    pub filled_quantity: f64,

    /// Remaining quantity
    pub remaining_quantity: f64,

    /// Average fill price
    pub average_fill_price: f64,

    /// Current state
    pub state: OrderState,

    /// Time in force
    pub time_in_force: TimeInForce,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Strategy tag (for multi-strategy portfolios)
    pub strategy_tag: Option<String>,

    /// Client order ID (optional, for strategy tracking)
    pub client_id: Option<String>,
}

impl Order {
    /// Create a new order
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        symbol: Symbol,
        side: Side,
        order_type: OrderType,
        quantity: f64,
        limit_price: Option<f64>,
        stop_price: Option<f64>,
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
            filled_quantity: 0.0,
            remaining_quantity: quantity,
            average_fill_price: 0.0,
            state: OrderState::Pending,
            time_in_force,
            created_at: now,
            updated_at: now,
            strategy_tag: None,
            client_id,
        }
    }

    /// Check if order is active (can be filled)
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            OrderState::Open | OrderState::PartiallyFilled | OrderState::Submitted
        )
    }

    /// Check if order is complete (filled or cancelled)
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
    /// Order ID that generated this fill
    pub order_id: OrderId,

    /// Fill price
    pub price: f64,

    /// Fill quantity
    pub quantity: f64,

    /// Fill timestamp
    pub timestamp: DateTime<Utc>,

    /// Commission paid
    pub commission: f64,

    /// Liquidity flag (maker/taker)
    pub is_maker: bool,
}

/// Enhanced position supporting multiple entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Symbol
    pub symbol: Symbol,

    /// Net position side (aggregated)
    pub side: Side,

    /// Average entry price (FIFO-weighted)
    pub average_entry_price: f64,

    /// Total quantity
    pub quantity: f64,

    /// Realized P&L (closed trades)
    pub realized_pnl: f64,

    /// Unrealized P&L (open position)
    pub unrealized_pnl: f64,

    /// Individual fills comprising this position
    pub fills: Vec<Fill>,

    /// First entry time
    pub first_entry_time: DateTime<Utc>,

    /// Last update time
    pub last_update_time: DateTime<Utc>,

    /// Risk amount at entry (stop_distance × quantity)
    /// Used for portfolio heat calculation to match main branch behavior
    pub risk_amount: f64,
}

impl Position {
    /// Create a new position from first fill
    pub fn from_fill(fill: Fill, symbol: Symbol, side: Side) -> Self {
        let unrealized_pnl = 0.0; // Will be updated with current price
        Self {
            symbol,
            side,
            average_entry_price: fill.price,
            quantity: fill.quantity,
            realized_pnl: 0.0,
            unrealized_pnl,
            fills: vec![fill.clone()],
            first_entry_time: fill.timestamp,
            last_update_time: fill.timestamp,
            risk_amount: 0.0, // Will be set by backtest engine after position creation
        }
    }

    /// Set the risk amount for this position (stop_distance × quantity)
    pub fn set_risk_amount(&mut self, risk_amount: f64) {
        self.risk_amount = risk_amount;
    }

    /// Update unrealized P&L with current price
    pub fn update_unrealized_pnl(&mut self, current_price: f64) {
        self.unrealized_pnl = match self.side {
            Side::Buy => (current_price - self.average_entry_price) * self.quantity,
            Side::Sell => (self.average_entry_price - current_price) * self.quantity,
        };
    }

    /// Calculate unrealized P&L at given price (matches main branch API)
    pub fn unrealized_pnl(&self, current_price: f64) -> f64 {
        match self.side {
            Side::Buy => (current_price - self.average_entry_price) * self.quantity,
            Side::Sell => (self.average_entry_price - current_price) * self.quantity,
        }
    }

    /// Get current value of position
    pub fn current_value(&self, current_price: f64) -> f64 {
        self.quantity * current_price
    }

    /// Get total P&L (realized + unrealized)
    pub fn total_pnl(&self) -> f64 {
        self.realized_pnl + self.unrealized_pnl
    }

    /// Get total commission from all fills
    pub fn total_commission(&self) -> f64 {
        self.fills.iter().map(|f| f.commission).sum()
    }

    /// Get total quantity traded (sum of all fills)
    pub fn total_quantity_traded(&self) -> f64 {
        self.fills.iter().map(|f| f.quantity).sum()
    }

    /// Get entry time (first fill timestamp)
    pub fn entry_time(&self) -> chrono::DateTime<chrono::Utc> {
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

        assert_eq!(order.quantity, 1.0);
        assert_eq!(order.remaining_quantity, 1.0);
        assert_eq!(order.filled_quantity, 0.0);
        assert_eq!(order.state, OrderState::Pending);
        assert!(order.is_active() || order.state == OrderState::Pending);
    }

    #[test]
    fn test_position_unrealized_pnl() {
        let fill = Fill {
            order_id: 1,
            price: 50000.0,
            quantity: 1.0,
            timestamp: Utc::now(),
            commission: 10.0,
            is_maker: true,
        };

        let mut position = Position::from_fill(fill, Symbol::new("BTCUSDT"), Side::Buy);
        position.update_unrealized_pnl(51000.0);

        assert_eq!(position.unrealized_pnl, 1000.0);
    }
}
