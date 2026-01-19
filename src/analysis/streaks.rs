//! Win/Loss Streak Analysis
//!
//! Tracks consecutive winning and losing trades to understand
//! strategy behavior and psychological demands.

use crate::Trade;

/// Streak analysis for consecutive wins and losses
#[derive(Debug, Clone, Default)]
pub struct StreakAnalysis {
    /// Maximum consecutive winning trades
    pub max_win_streak: usize,
    /// Maximum consecutive losing trades
    pub max_loss_streak: usize,
    /// Current streak (positive = wins, negative = losses)
    pub current_streak: i32,
    /// Average length of winning streaks
    pub avg_win_streak: f64,
    /// Average length of losing streaks
    pub avg_loss_streak: f64,
    /// Total number of winning streaks
    pub win_streak_count: usize,
    /// Total number of losing streaks
    pub loss_streak_count: usize,
}

impl StreakAnalysis {
    /// Returns (max_win_streak, max_loss_streak) for PerformanceMetrics
    #[inline]
    pub fn max_streaks(&self) -> (usize, usize) {
        (self.max_win_streak, self.max_loss_streak)
    }

    /// Create streak analysis from a list of trades
    pub fn from_trades(trades: &[Trade]) -> Self {
        if trades.is_empty() {
            return Self::default();
        }

        let mut max_win_streak = 0usize;
        let mut max_loss_streak = 0usize;
        let mut current_win_streak = 0usize;
        let mut current_loss_streak = 0usize;

        // Track all streak lengths for averages
        let mut win_streaks: Vec<usize> = Vec::new();
        let mut loss_streaks: Vec<usize> = Vec::new();

        let mut last_was_win: Option<bool> = None;

        for trade in trades {
            let is_win = trade.net_pnl.is_positive();

            match (is_win, last_was_win) {
                (true, Some(true)) => {
                    // Continue winning streak
                    current_win_streak += 1;
                }
                (true, Some(false)) => {
                    // End losing streak, start winning streak
                    if current_loss_streak > 0 {
                        loss_streaks.push(current_loss_streak);
                        max_loss_streak = max_loss_streak.max(current_loss_streak);
                    }
                    current_loss_streak = 0;
                    current_win_streak = 1;
                }
                (true, None) => {
                    // First trade is a win
                    current_win_streak = 1;
                }
                (false, Some(false)) => {
                    // Continue losing streak
                    current_loss_streak += 1;
                }
                (false, Some(true)) => {
                    // End winning streak, start losing streak
                    if current_win_streak > 0 {
                        win_streaks.push(current_win_streak);
                        max_win_streak = max_win_streak.max(current_win_streak);
                    }
                    current_win_streak = 0;
                    current_loss_streak = 1;
                }
                (false, None) => {
                    // First trade is a loss
                    current_loss_streak = 1;
                }
            }

            last_was_win = Some(is_win);
        }

        // Don't forget the final streak
        if current_win_streak > 0 {
            win_streaks.push(current_win_streak);
            max_win_streak = max_win_streak.max(current_win_streak);
        }
        if current_loss_streak > 0 {
            loss_streaks.push(current_loss_streak);
            max_loss_streak = max_loss_streak.max(current_loss_streak);
        }

        // Calculate averages
        let avg_win_streak = if win_streaks.is_empty() {
            0.0
        } else {
            win_streaks.iter().sum::<usize>() as f64 / win_streaks.len() as f64
        };

        let avg_loss_streak = if loss_streaks.is_empty() {
            0.0
        } else {
            loss_streaks.iter().sum::<usize>() as f64 / loss_streaks.len() as f64
        };

        // Current streak (positive for wins, negative for losses)
        let current_streak = if current_win_streak > 0 {
            current_win_streak as i32
        } else {
            -(current_loss_streak as i32)
        };

        Self {
            max_win_streak,
            max_loss_streak,
            current_streak,
            avg_win_streak,
            avg_loss_streak,
            win_streak_count: win_streaks.len(),
            loss_streak_count: loss_streaks.len(),
        }
    }

    /// Render streak analysis as a formatted string
    pub fn render(&self) -> String {
        const GREEN: &str = "\x1b[32m";
        const RED: &str = "\x1b[31m";
        const RESET: &str = "\x1b[0m";
        const BOLD: &str = "\x1b[1m";
        const DIM: &str = "\x1b[2m";

        let mut output = String::new();

        output.push_str(&format!("\n{}STREAK ANALYSIS{}\n", BOLD, RESET));
        output.push_str(&format!("{}\n", "─".repeat(45)));
        output.push_str(&format!(
            "{}{:>20}  {:>10}  {:>10}{}\n",
            DIM, "Metric", "Wins", "Losses", RESET
        ));
        output.push_str(&format!("{}\n", "─".repeat(45)));

        // Max streaks
        output.push_str(&format!(
            "{:>20}  {}{:>10}{RESET}  {}{:>10}{RESET}\n",
            "Max Streak", GREEN, self.max_win_streak, RED, self.max_loss_streak
        ));

        // Average streaks
        output.push_str(&format!(
            "{:>20}  {}{:>10.1}{RESET}  {}{:>10.1}{RESET}\n",
            "Avg Streak", GREEN, self.avg_win_streak, RED, self.avg_loss_streak
        ));

        // Streak counts
        output.push_str(&format!(
            "{:>20}  {:>10}  {:>10}\n",
            "Streak Count", self.win_streak_count, self.loss_streak_count
        ));

        output.push_str(&format!("{}\n", "─".repeat(45)));

        // Current streak
        let (streak_color, streak_label) = if self.current_streak > 0 {
            (GREEN, "winning")
        } else if self.current_streak < 0 {
            (RED, "losing")
        } else {
            (RESET, "neutral")
        };

        output.push_str(&format!(
            "{}Current: {}{} {} trade(s){}\n",
            DIM,
            streak_color,
            self.current_streak.abs(),
            streak_label,
            RESET
        ));

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Money, Side, Symbol};
    use chrono::{TimeZone, Utc};

    fn create_trade(net_pnl: f64) -> Trade {
        let dt = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
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
        let analysis = StreakAnalysis::from_trades(&[]);
        assert_eq!(analysis.max_win_streak, 0);
        assert_eq!(analysis.max_loss_streak, 0);
        assert_eq!(analysis.current_streak, 0);
    }

    #[test]
    fn test_all_wins() {
        let trades = vec![create_trade(100.0), create_trade(200.0), create_trade(50.0)];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 3);
        assert_eq!(analysis.max_loss_streak, 0);
        assert_eq!(analysis.current_streak, 3);
        assert_eq!(analysis.win_streak_count, 1);
        assert_eq!(analysis.loss_streak_count, 0);
    }

    #[test]
    fn test_all_losses() {
        let trades = vec![
            create_trade(-100.0),
            create_trade(-200.0),
            create_trade(-50.0),
        ];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 0);
        assert_eq!(analysis.max_loss_streak, 3);
        assert_eq!(analysis.current_streak, -3);
        assert_eq!(analysis.win_streak_count, 0);
        assert_eq!(analysis.loss_streak_count, 1);
    }

    #[test]
    fn test_alternating_trades() {
        let trades = vec![
            create_trade(100.0),
            create_trade(-50.0),
            create_trade(200.0),
            create_trade(-100.0),
        ];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 1);
        assert_eq!(analysis.max_loss_streak, 1);
        assert_eq!(analysis.current_streak, -1);
        assert_eq!(analysis.win_streak_count, 2);
        assert_eq!(analysis.loss_streak_count, 2);
        assert!((analysis.avg_win_streak - 1.0).abs() < 0.01);
        assert!((analysis.avg_loss_streak - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_mixed_streaks() {
        // W W W L L W W W W L
        let trades = vec![
            create_trade(100.0), // W
            create_trade(100.0), // W
            create_trade(100.0), // W (streak of 3)
            create_trade(-50.0), // L
            create_trade(-50.0), // L (streak of 2)
            create_trade(100.0), // W
            create_trade(100.0), // W
            create_trade(100.0), // W
            create_trade(100.0), // W (streak of 4)
            create_trade(-50.0), // L (streak of 1)
        ];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 4);
        assert_eq!(analysis.max_loss_streak, 2);
        assert_eq!(analysis.current_streak, -1);
        assert_eq!(analysis.win_streak_count, 2); // Two winning streaks: 3 and 4
        assert_eq!(analysis.loss_streak_count, 2); // Two losing streaks: 2 and 1

        // Average win streak: (3 + 4) / 2 = 3.5
        assert!((analysis.avg_win_streak - 3.5).abs() < 0.01);
        // Average loss streak: (2 + 1) / 2 = 1.5
        assert!((analysis.avg_loss_streak - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_ending_on_win_streak() {
        let trades = vec![
            create_trade(-50.0), // L
            create_trade(100.0), // W
            create_trade(100.0), // W
            create_trade(100.0), // W
        ];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 3);
        assert_eq!(analysis.max_loss_streak, 1);
        assert_eq!(analysis.current_streak, 3);
    }

    #[test]
    fn test_single_trade_win() {
        let trades = vec![create_trade(100.0)];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 1);
        assert_eq!(analysis.max_loss_streak, 0);
        assert_eq!(analysis.current_streak, 1);
    }

    #[test]
    fn test_single_trade_loss() {
        let trades = vec![create_trade(-100.0)];
        let analysis = StreakAnalysis::from_trades(&trades);

        assert_eq!(analysis.max_win_streak, 0);
        assert_eq!(analysis.max_loss_streak, 1);
        assert_eq!(analysis.current_streak, -1);
    }

    #[test]
    fn test_render_output() {
        let trades = vec![
            create_trade(100.0),
            create_trade(100.0),
            create_trade(-50.0),
        ];
        let analysis = StreakAnalysis::from_trades(&trades);
        let output = analysis.render();

        assert!(output.contains("STREAK ANALYSIS"));
        assert!(output.contains("Max Streak"));
        assert!(output.contains("Avg Streak"));
        assert!(output.contains("Current:"));
    }
}
