//! Order Management System (OMS)
//!
//! Production-grade order management for backtesting and live trading with:
//! - Limit orders with price-time priority
//! - Intra-candle fill detection
//! - Multiple positions per symbol
//! - Partial fills and position netting
//! - **Shared position sizing logic** (OrderSizer) for backtest/live consistency
//!
//! ## Performance Targets
//! - OrderBook insert: < 100ns (target: 50ns)
//! - Fill detection: < 1μs per order per candle
//! - Memory overhead: < 50MB for 10,000 orders
//!
//! ## Architecture
//!
//! ```text
//! Strategy → OrderRequest(qty=1.0) → OrderSizer → Order(qty=calculated) → OrderBook
//! ```

pub mod execution;
pub mod order_sizer;
pub mod orderbook;
pub mod position_manager;
pub mod strategy;
pub mod types;

// Re-export core types
pub use execution::ExecutionEngine;
pub use order_sizer::{size_order, OrderRejection, OrderSizer, SizedOrder};
pub use orderbook::OrderBook;
pub use position_manager::PositionManager;
pub use strategy::{OrderRequest, StrategyContext};
pub use types::{Fill, Order, OrderId, OrderState, OrderType, Position, TimeInForce};
