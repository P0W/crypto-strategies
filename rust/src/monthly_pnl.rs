//! Monthly P&L analysis and matrix rendering
//!
//! This module provides functionality to break down backtest results into
//! month-on-month profit/loss analysis with professional matrix display.

use chrono::{DateTime, Datelike, Utc};
use std::collections::BTreeMap;

use crate::Trade;

/// Monthly P&L data for a specific month
#[derive(Debug, Clone, Default)]
pub struct MonthlyPnL {
    /// Net profit/loss for the month (after commissions)
    pub net_pnl: f64,
    /// Number of trades executed in the month
    pub trade_count: usize,
    /// Number of winning trades
    pub winning_trades: usize,
    /// Number of losing trades
    pub losing_trades: usize,
    /// Win rate percentage
    pub win_rate: f64,
}

impl MonthlyPnL {
    fn new() -> Self {
        Self::default()
    }

    fn add_trade(&mut self, trade: &Trade) {
        self.net_pnl += trade.net_pnl;
        self.trade_count += 1;
        
        if trade.net_pnl > 0.0 {
            self.winning_trades += 1;
        } else {
            self.losing_trades += 1;
        }
        
        self.win_rate = if self.trade_count > 0 {
            (self.winning_trades as f64 / self.trade_count as f64) * 100.0
        } else {
            0.0
        };
    }
}

/// Year-Month key for organizing data
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct YearMonth {
    pub year: i32,
    pub month: u32,
}

impl YearMonth {
    fn new(year: i32, month: u32) -> Self {
        Self { year, month }
    }
    
    fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self {
            year: dt.year(),
            month: dt.month(),
        }
    }
}

/// Monthly P&L matrix organized by year and month
pub struct MonthlyPnLMatrix {
    /// Map of (year, month) -> MonthlyPnL data
    data: BTreeMap<YearMonth, MonthlyPnL>,
}

impl MonthlyPnLMatrix {
    /// Create a new monthly P&L matrix from trades
    pub fn from_trades(trades: &[Trade]) -> Self {
        let mut data = BTreeMap::new();
        
        for trade in trades {
            let ym = YearMonth::from_datetime(trade.exit_time);
            data.entry(ym).or_insert_with(MonthlyPnL::new).add_trade(trade);
        }
        
        Self { data }
    }
    
    /// Get unique years in the data
    fn years(&self) -> Vec<i32> {
        let mut years: Vec<i32> = self.data.keys().map(|ym| ym.year).collect();
        years.sort();
        years.dedup();
        years
    }
    
    /// Get P&L for a specific year and month
    fn get(&self, year: i32, month: u32) -> Option<&MonthlyPnL> {
        self.data.get(&YearMonth::new(year, month))
    }
    
    /// Calculate yearly total P&L
    fn yearly_total(&self, year: i32) -> f64 {
        self.data
            .iter()
            .filter(|(ym, _)| ym.year == year)
            .map(|(_, pnl)| pnl.net_pnl)
            .sum()
    }
    
    /// Calculate total P&L across all months
    pub fn total_pnl(&self) -> f64 {
        self.data.values().map(|pnl| pnl.net_pnl).sum()
    }
    
    /// Render the monthly P&L matrix as a formatted string
    pub fn render(&self) -> String {
        if self.data.is_empty() {
            return "No trades to display monthly P&L matrix.".to_string();
        }
        
        let years = self.years();
        let mut output = String::new();
        
        // Header
        output.push_str(&format!("\n{}\n", "=".repeat(120)));
        output.push_str("MONTHLY P&L MATRIX (₹)\n");
        output.push_str(&format!("{}\n", "=".repeat(120)));
        
        // Column headers
        output.push_str(&format!(
            "{:>6} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>12}\n",
            "Year", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec", "Total"
        ));
        output.push_str(&format!("{}\n", "-".repeat(120)));
        
        // Data rows (one per year)
        for year in years {
            output.push_str(&format!("{:>6} │", year));
            
            // Monthly P&L values
            for month in 1..=12 {
                let cell = if let Some(pnl) = self.get(year, month) {
                    self.format_pnl_cell(pnl.net_pnl)
                } else {
                    "          ".to_string() // Empty cell
                };
                output.push_str(&format!(" {} │", cell));
            }
            
            // Yearly total
            let year_total = self.yearly_total(year);
            output.push_str(&format!(" {:>12.2}\n", year_total));
        }
        
        output.push_str(&format!("{}\n", "=".repeat(120)));
        
        // Summary statistics
        output.push_str(&format!("Total P&L: ₹{:.2}\n", self.total_pnl()));
        
        // Count profitable vs losing months
        let profitable_months = self.data.values().filter(|pnl| pnl.net_pnl > 0.0).count();
        let total_months = self.data.len();
        let monthly_win_rate = if total_months > 0 {
            (profitable_months as f64 / total_months as f64) * 100.0
        } else {
            0.0
        };
        
        output.push_str(&format!(
            "Monthly Win Rate: {:.1}% ({} profitable months / {} total months)\n",
            monthly_win_rate, profitable_months, total_months
        ));
        
        output.push_str(&format!("{}\n", "=".repeat(120)));
        
        output
    }
    
    /// Render with ANSI color codes for terminal display
    pub fn render_colored(&self) -> String {
        if self.data.is_empty() {
            return "No trades to display monthly P&L matrix.".to_string();
        }
        
        const GREEN: &str = "\x1b[32m";
        const RED: &str = "\x1b[31m";
        const RESET: &str = "\x1b[0m";
        const BOLD: &str = "\x1b[1m";
        
        let years = self.years();
        let mut output = String::new();
        
        // Header
        output.push_str(&format!("\n{}{}{}\n", BOLD, "=".repeat(120), RESET));
        output.push_str(&format!("{}MONTHLY P&L MATRIX (₹){}\n", BOLD, RESET));
        output.push_str(&format!("{}{}{}\n", BOLD, "=".repeat(120), RESET));
        
        // Column headers
        output.push_str(&format!(
            "{}{:>6} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>12}{}\n",
            BOLD, "Year", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec", "Total", RESET
        ));
        output.push_str(&format!("{}\n", "-".repeat(120)));
        
        // Data rows (one per year)
        for year in years {
            output.push_str(&format!("{:>6} │", year));
            
            // Monthly P&L values
            for month in 1..=12 {
                let cell = if let Some(pnl) = self.get(year, month) {
                    let color = if pnl.net_pnl > 0.0 { GREEN } else { RED };
                    format!("{}{:>10.2}{}", color, pnl.net_pnl, RESET)
                } else {
                    "          ".to_string() // Empty cell
                };
                output.push_str(&format!(" {} │", cell));
            }
            
            // Yearly total
            let year_total = self.yearly_total(year);
            let color = if year_total > 0.0 { GREEN } else { RED };
            output.push_str(&format!(" {}{:>12.2}{}\n", color, year_total, RESET));
        }
        
        output.push_str(&format!("{}\n", "=".repeat(120)));
        
        // Summary statistics
        let total = self.total_pnl();
        let color = if total > 0.0 { GREEN } else { RED };
        output.push_str(&format!("{}Total P&L: ₹{:.2}{}\n", BOLD, total, RESET));
        output.push_str(&format!("{color}         : ₹{total:.2}{RESET}\n"));
        
        // Count profitable vs losing months
        let profitable_months = self.data.values().filter(|pnl| pnl.net_pnl > 0.0).count();
        let losing_months = self.data.values().filter(|pnl| pnl.net_pnl <= 0.0).count();
        let total_months = self.data.len();
        let monthly_win_rate = if total_months > 0 {
            (profitable_months as f64 / total_months as f64) * 100.0
        } else {
            0.0
        };
        
        output.push_str(&format!(
            "{}Monthly Win Rate: {:.1}% ({} profitable / {} losing / {} total months){}\n",
            BOLD, monthly_win_rate, profitable_months, losing_months, total_months, RESET
        ));
        
        output.push_str(&format!("{}\n", "=".repeat(120)));
        
        output
    }
    
    /// Format a P&L value for a cell (right-aligned, 10 chars)
    fn format_pnl_cell(&self, value: f64) -> String {
        format!("{:>10.2}", value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use crate::{Side, Symbol};

    fn create_test_trade(year: i32, month: u32, day: u32, net_pnl: f64) -> Trade {
        let dt = chrono::Utc
            .with_ymd_and_hms(year, month, day, 12, 0, 0)
            .unwrap();
        
        Trade {
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            entry_price: 50000.0,
            exit_price: 50000.0 + net_pnl,
            quantity: 1.0,
            entry_time: dt,
            exit_time: dt,
            pnl: net_pnl,
            commission: 0.0,
            net_pnl,
        }
    }

    #[test]
    fn test_monthly_pnl_aggregation() {
        let trades = vec![
            create_test_trade(2024, 1, 15, 1000.0),
            create_test_trade(2024, 1, 20, -500.0),
            create_test_trade(2024, 2, 10, 2000.0),
        ];
        
        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        
        // Check January 2024
        let jan_pnl = matrix.get(2024, 1).unwrap();
        assert_eq!(jan_pnl.net_pnl, 500.0); // 1000 - 500
        assert_eq!(jan_pnl.trade_count, 2);
        assert_eq!(jan_pnl.winning_trades, 1);
        assert_eq!(jan_pnl.losing_trades, 1);
        
        // Check February 2024
        let feb_pnl = matrix.get(2024, 2).unwrap();
        assert_eq!(feb_pnl.net_pnl, 2000.0);
        assert_eq!(feb_pnl.trade_count, 1);
    }

    #[test]
    fn test_yearly_total() {
        let trades = vec![
            create_test_trade(2024, 1, 15, 1000.0),
            create_test_trade(2024, 6, 20, 2000.0),
            create_test_trade(2024, 12, 10, -500.0),
        ];
        
        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        assert_eq!(matrix.yearly_total(2024), 2500.0);
    }

    #[test]
    fn test_multi_year_matrix() {
        let trades = vec![
            create_test_trade(2023, 12, 15, 1000.0),
            create_test_trade(2024, 1, 15, 2000.0),
            create_test_trade(2024, 6, 20, -500.0),
            create_test_trade(2025, 1, 10, 1500.0),
        ];
        
        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        
        let years = matrix.years();
        assert_eq!(years, vec![2023, 2024, 2025]);
        
        assert_eq!(matrix.yearly_total(2023), 1000.0);
        assert_eq!(matrix.yearly_total(2024), 1500.0); // 2000 - 500
        assert_eq!(matrix.yearly_total(2025), 1500.0);
        assert_eq!(matrix.total_pnl(), 4000.0);
    }

    #[test]
    fn test_empty_trades() {
        let trades: Vec<Trade> = vec![];
        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        
        assert_eq!(matrix.total_pnl(), 0.0);
        assert!(matrix.years().is_empty());
    }
}
