//! Order Management System (OMS)
//!
//! Production-grade order management for backtesting with:
//! - Limit orders with price-time priority
//! - Intra-candle fill detection
//! - Multiple positions per symbol
//! - Partial fills and position netting
//!
//! ## Performance Targets
//! - OrderBook insert: < 100ns (target: 50ns)
//! - Fill detection: < 1μs per order per candle
//! - Memory overhead: < 50MB for 10,000 orders

pub mod execution;
pub mod orderbook;
pub mod position_manager;
pub mod strategy;
pub mod types;

// Re-export core types
pub use execution::ExecutionEngine;
pub use orderbook::OrderBook;
pub use position_manager::PositionManager;
pub use strategy::{OrderRequest, StrategyContext};
pub use types::{Fill, Order, OrderId, OrderState, OrderType, Position, TimeInForce};
