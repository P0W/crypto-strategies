//! Day of Week Performance Analysis
//!
//! Analyzes trade performance by day of the week to identify
//! patterns and optimal trading days.

use chrono::Datelike;

use crate::Trade;

/// Day of week performance statistics
#[derive(Default, Clone)]
struct DayStats {
    total_pnl: f64,
    trade_count: usize,
    wins: usize,
}

/// Day of week performance analysis
pub struct DayOfWeekAnalysis {
    /// P&L and stats per weekday (0=Mon, 6=Sun)
    data: [DayStats; 7],
}

impl DayOfWeekAnalysis {
    /// Create day of week analysis from trades
    pub fn from_trades(trades: &[Trade]) -> Self {
        let mut data = [
            DayStats::default(),
            DayStats::default(),
            DayStats::default(),
            DayStats::default(),
            DayStats::default(),
            DayStats::default(),
            DayStats::default(),
        ];

        for trade in trades {
            let day_idx = trade.exit_time.weekday().num_days_from_monday() as usize;
            data[day_idx].total_pnl += trade.net_pnl.to_f64();
            data[day_idx].trade_count += 1;
            if trade.net_pnl.is_positive() {
                data[day_idx].wins += 1;
            }
        }

        Self { data }
    }

    /// Get P&L for a specific day (0=Mon, 6=Sun)
    pub fn get_pnl(&self, day_idx: usize) -> f64 {
        self.data.get(day_idx).map(|d| d.total_pnl).unwrap_or(0.0)
    }

    /// Get trade count for a specific day
    pub fn get_trade_count(&self, day_idx: usize) -> usize {
        self.data.get(day_idx).map(|d| d.trade_count).unwrap_or(0)
    }

    /// Get win rate for a specific day
    pub fn get_win_rate(&self, day_idx: usize) -> f64 {
        self.data
            .get(day_idx)
            .map(|d| {
                if d.trade_count > 0 {
                    (d.wins as f64 / d.trade_count as f64) * 100.0
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0)
    }

    /// Get the best performing day index
    pub fn best_day(&self) -> Option<usize> {
        self.data
            .iter()
            .enumerate()
            .filter(|(_, d)| d.trade_count > 0)
            .max_by(|a, b| a.1.total_pnl.partial_cmp(&b.1.total_pnl).unwrap())
            .map(|(i, _)| i)
    }

    /// Get the worst performing day index
    pub fn worst_day(&self) -> Option<usize> {
        self.data
            .iter()
            .enumerate()
            .filter(|(_, d)| d.trade_count > 0)
            .min_by(|a, b| a.1.total_pnl.partial_cmp(&b.1.total_pnl).unwrap())
            .map(|(i, _)| i)
    }

    /// Render as formatted string with ANSI colors
    pub fn render(&self) -> String {
        const GREEN: &str = "\x1b[32m";
        const RED: &str = "\x1b[31m";
        const RESET: &str = "\x1b[0m";
        const BOLD: &str = "\x1b[1m";
        const DIM: &str = "\x1b[2m";

        let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let mut output = String::new();

        output.push_str(&format!("\n{}DAY OF WEEK PERFORMANCE{}\n", BOLD, RESET));
        output.push_str(&format!("{}\n", "─".repeat(55)));
        output.push_str(&format!(
            "{}{:>5}  {:>12}  {:>8}  {:>8}  {:>10}{}\n",
            DIM, "Day", "P&L", "Trades", "Win %", "Avg P&L", RESET
        ));
        output.push_str(&format!("{}\n", "─".repeat(55)));

        let best_idx = self.best_day();
        let worst_idx = self.worst_day();

        for (i, stats) in self.data.iter().enumerate() {
            if stats.trade_count == 0 {
                output.push_str(&format!(
                    "{}{:>5}  {:>12}  {:>8}  {:>8}  {:>10}{}\n",
                    DIM, days[i], "-", "-", "-", "-", RESET
                ));
                continue;
            }

            let win_rate = (stats.wins as f64 / stats.trade_count as f64) * 100.0;
            let avg_pnl = stats.total_pnl / stats.trade_count as f64;
            let color = if stats.total_pnl >= 0.0 { GREEN } else { RED };

            // Mark best/worst
            let marker = if Some(i) == best_idx {
                " ★"
            } else if Some(i) == worst_idx {
                " ✗"
            } else {
                ""
            };

            output.push_str(&format!(
                "{:>5}  {}{:>12.0}{}  {:>8}  {:>7.1}%  {}{:>10.0}{}{}\n",
                days[i],
                color,
                stats.total_pnl,
                RESET,
                stats.trade_count,
                win_rate,
                color,
                avg_pnl,
                RESET,
                marker
            ));
        }

        output.push_str(&format!("{}\n", "─".repeat(55)));
        output.push_str(&format!("{}★ = Best day  ✗ = Worst day{}\n", DIM, RESET));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Money, Side, Symbol};
    use chrono::{TimeZone, Utc};

    fn create_trade_on_day(year: i32, month: u32, day: u32, net_pnl: f64) -> Trade {
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
        let analysis = DayOfWeekAnalysis::from_trades(&[]);
        for i in 0..7 {
            assert_eq!(analysis.get_pnl(i), 0.0);
            assert_eq!(analysis.get_trade_count(i), 0);
        }
        assert!(analysis.best_day().is_none());
        assert!(analysis.worst_day().is_none());
    }

    #[test]
    fn test_single_day_trades() {
        // 2024-01-15 is a Monday
        let trades = vec![
            create_trade_on_day(2024, 1, 15, 100.0),
            create_trade_on_day(2024, 1, 15, 200.0),
            create_trade_on_day(2024, 1, 15, -50.0),
        ];

        let analysis = DayOfWeekAnalysis::from_trades(&trades);

        // Monday (index 0)
        assert_eq!(analysis.get_pnl(0), 250.0); // 100 + 200 - 50
        assert_eq!(analysis.get_trade_count(0), 3);
        assert!((analysis.get_win_rate(0) - 66.67).abs() < 0.1); // 2/3 wins

        // Other days should be empty
        for i in 1..7 {
            assert_eq!(analysis.get_trade_count(i), 0);
        }
    }

    #[test]
    fn test_multiple_days() {
        // 2024-01-15 = Monday, 2024-01-16 = Tuesday, 2024-01-17 = Wednesday
        let trades = vec![
            create_trade_on_day(2024, 1, 15, 100.0),  // Monday
            create_trade_on_day(2024, 1, 16, -200.0), // Tuesday
            create_trade_on_day(2024, 1, 17, 500.0),  // Wednesday
        ];

        let analysis = DayOfWeekAnalysis::from_trades(&trades);

        assert_eq!(analysis.get_pnl(0), 100.0); // Monday
        assert_eq!(analysis.get_pnl(1), -200.0); // Tuesday
        assert_eq!(analysis.get_pnl(2), 500.0); // Wednesday

        assert_eq!(analysis.best_day(), Some(2)); // Wednesday
        assert_eq!(analysis.worst_day(), Some(1)); // Tuesday
    }

    #[test]
    fn test_weekend_trades() {
        // 2024-01-20 = Saturday, 2024-01-21 = Sunday
        let trades = vec![
            create_trade_on_day(2024, 1, 20, 300.0), // Saturday
            create_trade_on_day(2024, 1, 21, 400.0), // Sunday
        ];

        let analysis = DayOfWeekAnalysis::from_trades(&trades);

        assert_eq!(analysis.get_pnl(5), 300.0); // Saturday (index 5)
        assert_eq!(analysis.get_pnl(6), 400.0); // Sunday (index 6)
        assert_eq!(analysis.best_day(), Some(6)); // Sunday is best
    }

    #[test]
    fn test_win_rate_calculation() {
        // All on Monday
        let trades = vec![
            create_trade_on_day(2024, 1, 15, 100.0),
            create_trade_on_day(2024, 1, 22, 200.0),
            create_trade_on_day(2024, 1, 29, -50.0),
            create_trade_on_day(2024, 2, 5, 150.0),
        ];

        let analysis = DayOfWeekAnalysis::from_trades(&trades);

        // 3 wins out of 4 trades = 75%
        assert!((analysis.get_win_rate(0) - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_render_output() {
        let trades = vec![
            create_trade_on_day(2024, 1, 15, 100.0), // Monday
            create_trade_on_day(2024, 1, 16, -50.0), // Tuesday
        ];

        let analysis = DayOfWeekAnalysis::from_trades(&trades);
        let output = analysis.render();

        assert!(output.contains("DAY OF WEEK PERFORMANCE"));
        assert!(output.contains("Mon"));
        assert!(output.contains("Tue"));
        assert!(output.contains("★")); // Best day marker
        assert!(output.contains("✗")); // Worst day marker
    }

    #[test]
    fn test_all_same_pnl() {
        // Edge case: all days have same P&L
        let trades = vec![
            create_trade_on_day(2024, 1, 15, 100.0), // Monday
            create_trade_on_day(2024, 1, 16, 100.0), // Tuesday
        ];

        let analysis = DayOfWeekAnalysis::from_trades(&trades);

        // Both should be valid, one should be best and one worst
        assert!(analysis.best_day().is_some());
        assert!(analysis.worst_day().is_some());
    }
}
