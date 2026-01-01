//! Core data types used across the trading system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Validation errors for candle data
#[derive(Debug, Error)]
pub enum CandleValidationError {
    #[error("high ({high}) must be >= low ({low})")]
    HighLessThanLow { high: f64, low: f64 },

    #[error("volume ({0}) must be >= 0")]
    NegativeVolume(f64),

    #[error("open ({open}) must be between low ({low}) and high ({high})")]
    OpenOutOfRange { open: f64, low: f64, high: f64 },

    #[error("close ({close}) must be between low ({low}) and high ({high})")]
    CloseOutOfRange { close: f64, low: f64, high: f64 },

    #[error("prices must be positive: open={open}, high={high}, low={low}, close={close}")]
    NonPositivePrice {
        open: f64,
        high: f64,
        low: f64,
        close: f64,
    },
}

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

impl Candle {
    /// Create a new candle with validation
    pub fn new(
        datetime: DateTime<Utc>,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> Result<Self, CandleValidationError> {
        let candle = Self {
            datetime,
            open,
            high,
            low,
            close,
            volume,
        };
        candle.validate()?;
        Ok(candle)
    }

    /// Create a candle without validation (for trusted sources or when validation is done separately)
    pub fn new_unchecked(
        datetime: DateTime<Utc>,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> Self {
        Self {
            datetime,
            open,
            high,
            low,
            close,
            volume,
        }
    }

    /// Validate the candle data
    pub fn validate(&self) -> Result<(), CandleValidationError> {
        // Check for non-positive prices
        if self.open <= 0.0 || self.high <= 0.0 || self.low <= 0.0 || self.close <= 0.0 {
            return Err(CandleValidationError::NonPositivePrice {
                open: self.open,
                high: self.high,
                low: self.low,
                close: self.close,
            });
        }

        // Check high >= low
        if self.high < self.low {
            return Err(CandleValidationError::HighLessThanLow {
                high: self.high,
                low: self.low,
            });
        }

        // Check volume >= 0
        if self.volume < 0.0 {
            return Err(CandleValidationError::NegativeVolume(self.volume));
        }

        // Check open is within [low, high] range
        if self.open < self.low || self.open > self.high {
            return Err(CandleValidationError::OpenOutOfRange {
                open: self.open,
                low: self.low,
                high: self.high,
            });
        }

        // Check close is within [low, high] range
        if self.close < self.low || self.close > self.high {
            return Err(CandleValidationError::CloseOutOfRange {
                close: self.close,
                low: self.low,
                high: self.high,
            });
        }

        Ok(())
    }

    /// Check if the candle is valid without returning detailed error
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
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
        match self.side {
            Side::Buy => ((self.exit_price - self.entry_price) / self.entry_price) * 100.0,
            Side::Sell => ((self.entry_price - self.exit_price) / self.entry_price) * 100.0,
        }
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
    /// Trading expectancy: average profit/loss per trade (in currency)
    /// Formula: (Win Rate × Avg Win) - (Loss Rate × Avg Loss)
    /// Matches Backtrader and standard trading platforms
    pub expectancy: f64,
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
