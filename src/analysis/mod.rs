//! Trade and Performance Analysis Module
//!
//! This module provides comprehensive analysis tools for backtesting results:
//! - Streak analysis (consecutive wins/losses)
//! - Day of week performance
//! - Monthly P&L matrix with yearly summaries
//!
//! # Example
//! ```ignore
//! use crypto_strategies::analysis::{StreakAnalysis, DayOfWeekAnalysis, MonthlyPnLMatrix};
//!
//! let streaks = StreakAnalysis::from_trades(&trades);
//! let dow = DayOfWeekAnalysis::from_trades(&trades);
//! let monthly = MonthlyPnLMatrix::from_trades(&trades);
//!
//! println!("{}", streaks.render());
//! println!("{}", dow.render());
//! println!("{}", monthly.render_colored());
//! ```

mod day_of_week;
mod monthly;
mod streaks;

pub use day_of_week::DayOfWeekAnalysis;
pub use monthly::{MonthlyPnL, MonthlyPnLMatrix, YearMonth};
pub use streaks::StreakAnalysis;

/// Calculate win rate as percentage (0-100)
#[inline]
pub fn win_rate(winners: usize, total: usize) -> f64 {
    if total > 0 {
        (winners as f64 / total as f64) * 100.0
    } else {
        0.0
    }
}
