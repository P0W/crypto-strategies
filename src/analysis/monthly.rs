//! Monthly P&L Matrix and Yearly Summary
//!
//! Provides monthly breakdown of P&L with yearly aggregations,
//! best/worst month tracking, and CAGR calculations.

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
        self.net_pnl += trade.net_pnl.to_f64();
        self.trade_count += 1;

        if trade.net_pnl.is_positive() {
            self.winning_trades += 1;
        } else {
            self.losing_trades += 1;
        }

        self.win_rate = super::win_rate(self.winning_trades, self.trade_count);
    }
}

/// Year-Month key for organizing data
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct YearMonth {
    pub year: i32,
    pub month: u32,
}

impl YearMonth {
    pub fn new(year: i32, month: u32) -> Self {
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
            data.entry(ym)
                .or_insert_with(MonthlyPnL::new)
                .add_trade(trade);
        }

        Self { data }
    }

    /// Get unique years in the data
    pub fn years(&self) -> Vec<i32> {
        let mut years: Vec<i32> = self.data.keys().map(|ym| ym.year).collect();
        years.sort();
        years.dedup();
        years
    }

    /// Get P&L for a specific year and month
    pub fn get(&self, year: i32, month: u32) -> Option<&MonthlyPnL> {
        self.data.get(&YearMonth::new(year, month))
    }

    /// Calculate yearly total P&L
    pub fn yearly_total(&self, year: i32) -> f64 {
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

    /// Get count of profitable months
    pub fn profitable_months(&self) -> usize {
        self.data.values().filter(|pnl| pnl.net_pnl > 0.0).count()
    }

    /// Get count of losing months
    pub fn losing_months(&self) -> usize {
        self.data.values().filter(|pnl| pnl.net_pnl <= 0.0).count()
    }

    /// Render the monthly P&L matrix as a formatted string (no colors)
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
                    format!("{:>10.2}", pnl.net_pnl)
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

        let profitable = self.profitable_months();
        let total = self.data.len();
        let monthly_win_rate = if total > 0 {
            (profitable as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        output.push_str(&format!(
            "Monthly Win Rate: {:.1}% ({} profitable months / {} total months)\n",
            monthly_win_rate, profitable, total
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

        // Table width: 6(year) + 2(│ ) + 12*(10+3)(months) + 12(total) + 1 = 177
        const TABLE_WIDTH: usize = 177;

        let years = self.years();
        let mut output = String::new();

        // Header
        output.push_str(&format!("\n{}{}{}\n", BOLD, "═".repeat(TABLE_WIDTH), RESET));
        output.push_str(&format!("{}MONTHLY P&L MATRIX (₹){}\n", BOLD, RESET));
        output.push_str(&format!("{}{}{}\n", BOLD, "═".repeat(TABLE_WIDTH), RESET));

        // Column headers
        output.push_str(&format!(
            "{}{:>6} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>10} │ {:>12}{}\n",
            BOLD, "Year", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec", "Total", RESET
        ));
        output.push_str(&format!("{}\n", "─".repeat(TABLE_WIDTH)));

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

        output.push_str(&format!("{}\n", "═".repeat(TABLE_WIDTH)));

        // Summary statistics
        let total = self.total_pnl();
        let color = if total > 0.0 { GREEN } else { RED };
        output.push_str(&format!(
            "{}Total P&L:{} {}₹{:.2}{}\n",
            BOLD, RESET, color, total, RESET
        ));

        // Count profitable vs losing months
        let profitable_months = self.profitable_months();
        let losing_months = self.losing_months();
        let total_months = self.data.len();
        let monthly_win_rate = if total_months > 0 {
            (profitable_months as f64 / total_months as f64) * 100.0
        } else {
            0.0
        };

        output.push_str(&format!(
            "{}Monthly Win Rate:{} {:.1}% ({} profitable / {} losing)\n",
            BOLD, RESET, monthly_win_rate, profitable_months, losing_months
        ));

        output
    }

    /// Render a compact yearly summary (more professional)
    pub fn render_yearly_summary(&self, initial_capital: f64) -> String {
        if self.data.is_empty() {
            return "No trades to display.\n".to_string();
        }

        const GREEN: &str = "\x1b[32m";
        const RED: &str = "\x1b[31m";
        const RESET: &str = "\x1b[0m";
        const BOLD: &str = "\x1b[1m";
        const DIM: &str = "\x1b[2m";

        let years = self.years();
        let mut output = String::new();
        let mut cumulative_pnl = 0.0;

        // Yearly Performance Table
        output.push_str(&format!("\n{}YEARLY PERFORMANCE{}\n", BOLD, RESET));
        output.push_str(&format!("{}\n", "─".repeat(90)));
        output.push_str(&format!(
            "{}{:>6}  {:>12}  {:>8}  {:>8}  {:>8}  {:>12}  {:>12}{}\n",
            DIM, "Year", "P&L", "Return", "Trades", "Win %", "Best Mo", "Worst Mo", RESET
        ));
        output.push_str(&format!("{}\n", "─".repeat(90)));

        let months = [
            "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];

        for year in &years {
            let year_pnl = self.yearly_total(*year);
            cumulative_pnl += year_pnl;
            let year_capital = initial_capital + cumulative_pnl - year_pnl;
            let year_return = if year_capital > 0.0 {
                (year_pnl / year_capital) * 100.0
            } else {
                0.0
            };

            // Get trades and win rate for year
            let year_data: Vec<_> = self
                .data
                .iter()
                .filter(|(ym, _)| ym.year == *year)
                .collect();
            let trades: usize = year_data.iter().map(|(_, pnl)| pnl.trade_count).sum();
            let wins: usize = year_data.iter().map(|(_, pnl)| pnl.winning_trades).sum();
            let win_rate = if trades > 0 {
                (wins as f64 / trades as f64) * 100.0
            } else {
                0.0
            };

            // Best and worst months
            let best_month = year_data
                .iter()
                .max_by(|a, b| a.1.net_pnl.partial_cmp(&b.1.net_pnl).unwrap())
                .map(|(ym, pnl)| (ym.month, pnl.net_pnl));
            let worst_month = year_data
                .iter()
                .min_by(|a, b| a.1.net_pnl.partial_cmp(&b.1.net_pnl).unwrap())
                .map(|(ym, pnl)| (ym.month, pnl.net_pnl));

            let color = if year_pnl >= 0.0 { GREEN } else { RED };

            // Format best/worst with fixed width (no color in width calc)
            let best_str = best_month
                .map(|(m, v)| {
                    format!(
                        "{}{:>12}{}",
                        GREEN,
                        format!("{} {:+.0}", months[m as usize], v),
                        RESET
                    )
                })
                .unwrap_or_else(|| format!("{:>12}", "-"));
            let worst_str = worst_month
                .map(|(m, v)| {
                    format!(
                        "{}{:>12}{}",
                        RED,
                        format!("{} {:+.0}", months[m as usize], v),
                        RESET
                    )
                })
                .unwrap_or_else(|| format!("{:>12}", "-"));

            output.push_str(&format!(
                "{:>6}  {}{:>12.0}{}  {}{:>7.1}%{}  {:>8}  {:>7.1}%  {}  {}\n",
                year,
                color,
                year_pnl,
                RESET,
                color,
                year_return,
                RESET,
                trades,
                win_rate,
                best_str,
                worst_str
            ));
        }

        output.push_str(&format!("{}\n", "─".repeat(90)));

        // Summary row
        let total_pnl = self.total_pnl();
        let total_return = (total_pnl / initial_capital) * 100.0;
        let total_trades: usize = self.data.values().map(|p| p.trade_count).sum();
        let total_wins: usize = self.data.values().map(|p| p.winning_trades).sum();
        let overall_win_rate = if total_trades > 0 {
            (total_wins as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let color = if total_pnl >= 0.0 { GREEN } else { RED };
        output.push_str(&format!(
            "{}{:>6}  {}{:>12.0}{}  {}{:>7.1}%{}  {:>8}  {:>7.1}%{}\n",
            BOLD,
            "TOTAL",
            color,
            total_pnl,
            RESET,
            color,
            total_return,
            RESET,
            total_trades,
            overall_win_rate,
            RESET
        ));

        // CAGR calculation
        let num_years = years.len() as f64;
        if num_years > 0.0 && total_return > -100.0 {
            let cagr = ((1.0 + total_return / 100.0).powf(1.0 / num_years) - 1.0) * 100.0;
            output.push_str(&format!("{}CAGR: {:+.1}%{}\n", DIM, cagr, RESET));
        }

        output.push_str(&format!("{}\n", "─".repeat(90)));

        // Monthly consistency
        let profitable_months = self.profitable_months();
        let losing_months = self.losing_months();
        let monthly_wr = if !self.data.is_empty() {
            (profitable_months as f64 / self.data.len() as f64) * 100.0
        } else {
            0.0
        };

        output.push_str(&format!(
            "{}Monthly: {:.0}% consistency ({} green / {} red){}\n",
            DIM, monthly_wr, profitable_months, losing_months, RESET
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Money, Side, Symbol};
    use chrono::TimeZone;

    fn create_test_trade(year: i32, month: u32, day: u32, net_pnl: f64) -> Trade {
        let dt = Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).unwrap();

        Trade {
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            entry_price: Money::from_f64(50000.0),
            exit_price: Money::from_f64(50000.0 + net_pnl),
            quantity: Money::from_f64(1.0),
            entry_time: dt,
            exit_time: dt,
            pnl: Money::from_f64(net_pnl),
            commission: Money::ZERO,
            net_pnl: Money::from_f64(net_pnl),
        }
    }

    #[test]
    fn test_empty_trades() {
        let trades: Vec<Trade> = vec![];
        let matrix = MonthlyPnLMatrix::from_trades(&trades);

        assert_eq!(matrix.total_pnl(), 0.0);
        assert!(matrix.years().is_empty());
        assert_eq!(matrix.profitable_months(), 0);
        assert_eq!(matrix.losing_months(), 0);
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
    fn test_profitable_vs_losing_months() {
        let trades = vec![
            create_test_trade(2024, 1, 15, 1000.0), // Jan: profitable
            create_test_trade(2024, 2, 15, -500.0), // Feb: losing
            create_test_trade(2024, 3, 15, 200.0),  // Mar: profitable
            create_test_trade(2024, 4, 15, -100.0), // Apr: losing
            create_test_trade(2024, 5, 15, 0.0),    // May: losing (zero is not profitable)
        ];

        let matrix = MonthlyPnLMatrix::from_trades(&trades);

        assert_eq!(matrix.profitable_months(), 2);
        assert_eq!(matrix.losing_months(), 3);
    }

    #[test]
    fn test_year_month_ordering() {
        let ym1 = YearMonth::new(2023, 12);
        let ym2 = YearMonth::new(2024, 1);
        let ym3 = YearMonth::new(2024, 6);

        assert!(ym1 < ym2);
        assert!(ym2 < ym3);
        assert!(ym1 < ym3);
    }

    #[test]
    fn test_render_colored_output() {
        let trades = vec![
            create_test_trade(2024, 1, 15, 1000.0),
            create_test_trade(2024, 2, 15, -500.0),
        ];

        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        let output = matrix.render_colored();

        assert!(output.contains("MONTHLY P&L MATRIX"));
        assert!(output.contains("2024"));
        assert!(output.contains("Total P&L:"));
    }

    #[test]
    fn test_render_yearly_summary() {
        let trades = vec![
            create_test_trade(2024, 1, 15, 1000.0),
            create_test_trade(2024, 6, 15, 2000.0),
        ];

        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        let output = matrix.render_yearly_summary(10000.0);

        assert!(output.contains("YEARLY PERFORMANCE"));
        assert!(output.contains("2024"));
        assert!(output.contains("CAGR:"));
    }

    #[test]
    fn test_monthly_win_rate() {
        let trades = vec![
            create_test_trade(2024, 1, 10, 100.0),
            create_test_trade(2024, 1, 15, 200.0),
            create_test_trade(2024, 1, 20, -50.0),
        ];

        let matrix = MonthlyPnLMatrix::from_trades(&trades);
        let jan = matrix.get(2024, 1).unwrap();

        // 2 wins out of 3 = 66.67%
        assert!((jan.win_rate - 66.67).abs() < 0.1);
    }
}
