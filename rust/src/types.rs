//! Core data types used across the trading system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// OHLCV candlestick data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub datetime: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Trading pair symbol
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub String);

impl Symbol {
    pub fn new(s: impl Into<String>) -> Self {
        Symbol(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Trade direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

/// Position state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: Symbol,
    pub side: Side,
    pub entry_price: f64,
    pub quantity: f64,
    pub stop_price: f64,
    pub target_price: f64,
    pub trailing_stop: Option<f64>,
    pub entry_time: DateTime<Utc>,
    pub risk_amount: f64,
}

impl Position {
    pub fn current_value(&self) -> f64 {
        self.quantity * self.entry_price
    }

    pub fn unrealized_pnl(&self, current_price: f64) -> f64 {
        match self.side {
            Side::Buy => (current_price - self.entry_price) * self.quantity,
            Side::Sell => (self.entry_price - current_price) * self.quantity,
        }
    }
}

/// Completed trade record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: Symbol,
    pub side: Side,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub pnl: f64,
    pub commission: f64,
    pub net_pnl: f64,
}

impl Trade {
    pub fn return_pct(&self) -> f64 {
        ((self.exit_price - self.entry_price) / self.entry_price) * 100.0
    }
}

/// Trading signal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Long,
    Short,
    Flat,
}

/// Order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Submitted,
    Accepted,
    Partial,
    Completed,
    Canceled,
    Margin,
    Rejected,
    Expired,
}

/// Order execution details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderExecution {
    pub price: f64,
    pub size: f64,
    pub value: f64,
    pub commission: f64,
}

/// Order information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub symbol: Symbol,
    pub side: Side,
    pub status: OrderStatus,
    pub size: f64,
    pub price: Option<f64>, // None for market orders
    pub executed: Option<OrderExecution>,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
}

/// Portfolio statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_return: f64,
    pub post_tax_return: f64,
    pub sharpe_ratio: f64,
    pub calmar_ratio: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub total_commission: f64,
    pub tax_amount: f64,
}
