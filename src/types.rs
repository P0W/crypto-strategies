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

/// Trading pair symbol using Arc<str> for cheap cloning
///
/// Symbols are frequently cloned when passed to strategies, orders, and positions.
/// Using Arc<str> instead of String reduces heap allocations from O(n) to O(1) per clone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Symbol(#[serde(with = "arc_str_serde")] std::sync::Arc<str>);

/// Custom serde for Arc<str>
mod arc_str_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Arc::from(s.as_str()))
    }
}

impl Symbol {
    pub fn new(s: impl AsRef<str>) -> Self {
        Symbol(std::sync::Arc::from(s.as_ref()))
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

/// Completed trade record with precise decimal arithmetic for monetary values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub symbol: Symbol,
    pub side: Side,
    pub entry_price: Money,
    pub exit_price: Money,
    pub quantity: Money,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub pnl: Money,
    pub commission: Money,
    pub net_pnl: Money,
}

impl Trade {
    /// Calculate return percentage
    pub fn return_pct(&self) -> f64 {
        if self.entry_price.is_zero() {
            return 0.0;
        }
        let pct = match self.side {
            Side::Buy => (self.exit_price - self.entry_price) / self.entry_price,
            Side::Sell => (self.entry_price - self.exit_price) / self.entry_price,
        };
        pct.to_f64() * 100.0
    }

    /// Create a Trade from f64 values (for migration compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn from_f64(
        symbol: Symbol,
        side: Side,
        entry_price: f64,
        exit_price: f64,
        quantity: f64,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        pnl: f64,
        commission: f64,
        net_pnl: f64,
    ) -> Self {
        Self {
            symbol,
            side,
            entry_price: Money::from_f64(entry_price),
            exit_price: Money::from_f64(exit_price),
            quantity: Money::from_f64(quantity),
            entry_time,
            exit_time,
            pnl: Money::from_f64(pnl),
            commission: Money::from_f64(commission),
            net_pnl: Money::from_f64(net_pnl),
        }
    }
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

// ============================================================================
// Money Type - Precise Decimal Arithmetic for Monetary Values
// ============================================================================

use rust_decimal::Decimal;
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Money type for precise decimal arithmetic in monetary calculations.
///
/// Wraps `rust_decimal::Decimal` to prevent floating-point drift in PnL tracking.
/// Use this type for all monetary values: prices, quantities, pnl, capital, commissions.
///
/// # Why Money instead of f64?
/// `0.1 + 0.2 != 0.3` in f64. Over thousands of trades, PnL tracking will drift
/// from exchange balances, causing reconciliation failures.
///
/// # Example
/// ```
/// use crypto_strategies::Money;
/// let price = Money::from_f64(100.50);
/// let qty = Money::from_f64(2.0);
/// let total = price * qty;
/// assert_eq!(total.to_f64(), 201.0);
/// ```
#[derive(Debug, Clone, Copy, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(#[serde(with = "rust_decimal::serde::str")] Decimal);

impl Money {
    /// Zero value
    pub const ZERO: Money = Money(Decimal::ZERO);

    /// One value
    pub const ONE: Money = Money(Decimal::ONE);

    /// Create from f64 (for migration compatibility)
    /// Note: This conversion may lose precision for values with many decimal places
    pub fn from_f64(value: f64) -> Self {
        Money(Decimal::try_from(value).unwrap_or_else(|_| {
            // Fallback for extreme values (NaN, Infinity)
            if value.is_nan() || value.is_infinite() {
                Decimal::ZERO
            } else {
                Decimal::from_f64_retain(value).unwrap_or(Decimal::ZERO)
            }
        }))
    }

    /// Convert to f64 (for indicator calculations that require f64)
    pub fn to_f64(self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.0.to_f64().unwrap_or(0.0)
    }

    /// Create from i64 (for whole number values)
    pub fn from_i64(value: i64) -> Self {
        Money(Decimal::from(value))
    }

    /// Get absolute value
    pub fn abs(self) -> Self {
        Money(self.0.abs())
    }

    /// Check if value is zero
    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    /// Check if value is positive
    pub fn is_positive(self) -> bool {
        self.0.is_sign_positive() && !self.0.is_zero()
    }

    /// Check if value is negative
    pub fn is_negative(self) -> bool {
        self.0.is_sign_negative()
    }

    /// Get maximum of two values
    pub fn max(self, other: Self) -> Self {
        Money(self.0.max(other.0))
    }

    /// Get minimum of two values
    pub fn min(self, other: Self) -> Self {
        Money(self.0.min(other.0))
    }

    /// Round to specified decimal places
    pub fn round_dp(self, dp: u32) -> Self {
        Money(self.0.round_dp(dp))
    }

    /// Get the underlying Decimal
    pub fn inner(self) -> Decimal {
        self.0
    }
}

impl Default for Money {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// PartialEq is derived via Eq, but we need PartialOrd manually
impl PartialEq for Money {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd for Money {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Money {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl std::hash::Hash for Money {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Add for Money {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Money(self.0 + rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Money {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Money(self.0 - rhs.0)
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul for Money {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        Money(self.0 * rhs.0)
    }
}

impl Div for Money {
    type Output = Self;
    fn div(self, rhs: Self) -> Self::Output {
        if rhs.0.is_zero() {
            Money::ZERO // Safe division by zero handling
        } else {
            Money(self.0 / rhs.0)
        }
    }
}

impl Neg for Money {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Money(-self.0)
    }
}

// Conversion traits for f64 interop (needed during migration)
impl From<f64> for Money {
    fn from(value: f64) -> Self {
        Money::from_f64(value)
    }
}

impl From<Money> for f64 {
    fn from(value: Money) -> Self {
        value.to_f64()
    }
}

impl From<i64> for Money {
    fn from(value: i64) -> Self {
        Money::from_i64(value)
    }
}

// Sum iterator support
impl std::iter::Sum for Money {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Money::ZERO, |acc, x| acc + x)
    }
}

impl<'a> std::iter::Sum<&'a Money> for Money {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.fold(Money::ZERO, |acc, x| acc + *x)
    }
}

#[cfg(test)]
mod money_tests {
    use super::*;

    #[test]
    fn test_money_precision() {
        // Classic floating point problem: 0.1 + 0.2 != 0.3 in f64
        let a = Money::from_f64(0.1);
        let b = Money::from_f64(0.2);
        let c = Money::from_f64(0.3);
        assert_eq!(a + b, c, "Money should handle 0.1 + 0.2 = 0.3 correctly");
    }

    #[test]
    fn test_money_arithmetic() {
        let price = Money::from_f64(100.0);
        let qty = Money::from_f64(2.5);
        let total = price * qty;
        assert_eq!(total.to_f64(), 250.0);
    }

    #[test]
    fn test_money_comparison() {
        let a = Money::from_f64(100.0);
        let b = Money::from_f64(200.0);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a.max(b), b);
        assert_eq!(a.min(b), a);
    }

    #[test]
    fn test_money_div_by_zero() {
        let a = Money::from_f64(100.0);
        let zero = Money::ZERO;
        assert_eq!(a / zero, Money::ZERO);
    }

    #[test]
    fn test_money_sum() {
        let values = vec![
            Money::from_f64(10.0),
            Money::from_f64(20.0),
            Money::from_f64(30.0),
        ];
        let total: Money = values.into_iter().sum();
        assert_eq!(total.to_f64(), 60.0);
    }

    #[test]
    fn test_money_serde() {
        let money = Money::from_f64(123.456);
        let json = serde_json::to_string(&money).unwrap();
        let parsed: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(money, parsed);
    }
}
