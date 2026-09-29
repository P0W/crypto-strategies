//! Historical-data adapter over the shared incremental trading engine.

use crate::analysis::{win_rate, StreakAnalysis};
use crate::oms::TradingEngine;
use crate::{Config, PerformanceMetrics, Strategy, Trade, PNL_EPSILON};
use anyhow::{ensure, Result};
use chrono::{DateTime, Utc};

#[derive(Debug, Default)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub metrics: PerformanceMetrics,
}

pub struct Backtester {
    config: Config,
    strategy: Box<dyn Strategy>,
    evaluation_start: Option<DateTime<Utc>>,
}

fn daily_equity_returns(equity_curve: &[(DateTime<Utc>, f64)]) -> Vec<f64> {
    let mut daily_closes: Vec<(chrono::NaiveDate, f64)> = Vec::new();
    for (timestamp, equity) in equity_curve {
        let date = timestamp.date_naive();
        if let Some((last_date, last_equity)) = daily_closes.last_mut() {
            if *last_date == date {
                *last_equity = *equity;
                continue;
            }
        }
        daily_closes.push((date, *equity));
    }
    daily_closes
        .windows(2)
        .filter_map(|window| {
            let previous = window[0].1;
            (previous > 0.0).then(|| (window[1].1 - previous) / previous)
        })
        .collect()
}

impl Backtester {
    pub fn new(config: Config, strategy: Box<dyn Strategy>) -> Self {
        Self {
            config,
            strategy,
            evaluation_start: None,
        }
    }

    pub fn with_evaluation_start(mut self, start: Option<DateTime<Utc>>) -> Self {
        self.evaluation_start = start;
        self
    }

    pub fn run(&mut self, data: &crate::MultiSymbolMultiTimeframeData) -> Result<BacktestResult> {
        let aligned = crate::multi_timeframe::align_multi_timeframe_data(data);
        ensure!(!aligned.is_empty(), "No synchronized data for backtesting");
        let primary_tf = aligned[0].1.primary_timeframe().to_string();
        let dates: Vec<_> = aligned[0]
            .1
            .primary()
            .iter()
            .map(|c| c.datetime)
            .filter(|date| self.evaluation_start.is_none_or(|start| *date >= start))
            .collect();
        ensure!(!dates.is_empty(), "No candles in the evaluation period");
        let data = aligned.into_iter().collect();
        let mut engine = TradingEngine::new(self.config.clone(), self.strategy.clone_boxed())?;
        let mut equity_curve = Vec::with_capacity(dates.len());
        for timestamp in dates {
            engine.on_bar(&data, timestamp)?;
            equity_curve.push((timestamp, engine.equity()));
        }
        if let Some((timestamp, equity)) = equity_curve.last_mut() {
            engine.finish(*timestamp)?;
            *equity = engine.equity();
        }
        let trades = engine.trades().to_vec();
        let metrics = self.calculate_metrics(&trades, &equity_curve, &primary_tf);
        Ok(BacktestResult {
            trades,
            equity_curve,
            metrics,
        })
    }

    fn calculate_metrics(
        &self,
        trades: &[Trade],
        equity_curve: &[(DateTime<Utc>, f64)],
        _timeframe: &str,
    ) -> PerformanceMetrics {
        if trades.is_empty() || equity_curve.is_empty() {
            return PerformanceMetrics::default();
        }

        let initial_capital = self.config.trading.initial_capital;
        let final_equity = equity_curve.last().unwrap().1;
        let total_return = ((final_equity - initial_capital) / initial_capital) * 100.0;

        let winners: Vec<&Trade> = trades
            .iter()
            .filter(|trade| trade.net_pnl.to_f64() > PNL_EPSILON)
            .collect();
        let losers: Vec<&Trade> = trades
            .iter()
            .filter(|trade| trade.net_pnl.to_f64() < -PNL_EPSILON)
            .collect();

        let decisive_trades = winners.len() + losers.len();
        let win_rate = win_rate(winners.len(), decisive_trades);

        let total_wins: f64 = winners.iter().map(|t| t.net_pnl.to_f64()).sum();
        let total_losses: f64 = losers.iter().map(|t| t.net_pnl.abs().to_f64()).sum();

        let profit_factor = if total_losses > 0.0 {
            total_wins / total_losses
        } else if total_wins > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let avg_win = if !winners.is_empty() {
            total_wins / winners.len() as f64
        } else {
            0.0
        };

        let avg_loss = if !losers.is_empty() {
            total_losses / losers.len() as f64
        } else {
            0.0
        };

        let expectancy = trades
            .iter()
            .map(|trade| trade.net_pnl.to_f64())
            .sum::<f64>()
            / trades.len() as f64;

        let largest_win = winners
            .iter()
            .map(|t| t.net_pnl.to_f64())
            .fold(0.0, f64::max);
        let largest_loss = losers
            .iter()
            .map(|t| t.net_pnl.to_f64())
            .fold(0.0, f64::min);

        let total_commission: f64 = trades.iter().map(|t| t.commission.to_f64()).sum();

        // Sharpe ratio
        let returns = daily_equity_returns(equity_curve);

        let sharpe = if returns.len() > 1 {
            let mean = returns.iter().sum::<f64>() / returns.len() as f64;
            let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                / (returns.len() - 1) as f64;
            let std = variance.sqrt();

            if std > 0.0 {
                let risk_free_rate = 0.05 / 365.0;
                let excess_return = mean - risk_free_rate;
                (excess_return / std) * (365.0_f64).sqrt()
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Max drawdown and underwater metrics
        let mut peak = initial_capital;
        let mut max_dd = 0.0;
        let mut underwater_bars = 0usize;
        let mut current_underwater_streak = 0usize;
        let mut max_underwater_streak = 0usize;
        let mut drawdown_sum = 0.0;
        let mut drawdown_count = 0usize;

        for (_, equity) in equity_curve {
            if *equity > peak {
                peak = *equity;
                // Reset streak when new peak reached
                if current_underwater_streak > max_underwater_streak {
                    max_underwater_streak = current_underwater_streak;
                }
                current_underwater_streak = 0;
            }
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }

            // Track underwater time
            if dd > 0.0 {
                underwater_bars += 1;
                current_underwater_streak += 1;
                drawdown_sum += dd * 100.0;
                drawdown_count += 1;
            }
        }

        // Check final streak
        if current_underwater_streak > max_underwater_streak {
            max_underwater_streak = current_underwater_streak;
        }

        // Calculate underwater metrics
        let underwater_time_pct = if !equity_curve.is_empty() {
            (underwater_bars as f64 / equity_curve.len() as f64) * 100.0
        } else {
            0.0
        };

        let avg_drawdown = if drawdown_count > 0 {
            drawdown_sum / drawdown_count as f64
        } else {
            0.0
        };

        // Recovery factor: net profit / max drawdown (in currency terms)
        let net_profit = final_equity - initial_capital;
        let max_dd_currency = max_dd * peak;
        let recovery_factor = if max_dd_currency > 0.0 {
            net_profit / max_dd_currency
        } else if net_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        // Calmar ratio
        let calmar = if final_equity <= 0.0 {
            tracing::warn!(
                final_equity,
                "Calmar is undefined for nonpositive equity; assigning the worst ranking score"
            );
            f64::NEG_INFINITY
        } else if max_dd > 0.0 {
            let start = equity_curve.first().unwrap().0;
            let end = equity_curve.last().unwrap().0;
            let days = (end - start).num_days() as f64;
            if days > 0.0 {
                let years = days / 365.0;
                let ann_ret = (final_equity / initial_capital).powf(1.0 / years) - 1.0;
                ann_ret / max_dd
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Tax calculation. TDS is withholding/credit, not an additional final tax cost.
        let tax_rate = self.config.tax.tax_rate;
        let net_profit = total_wins - total_losses;
        let taxable_gains = if self.config.tax.loss_offset_allowed {
            net_profit.max(0.0)
        } else {
            total_wins.max(0.0)
        };
        let tax = taxable_gains * tax_rate;
        let post_tax_return = ((final_equity - initial_capital - tax) / initial_capital) * 100.0;

        // Calculate win/loss streaks
        let (max_win_streak, max_loss_streak) = StreakAnalysis::from_trades(trades).max_streaks();

        PerformanceMetrics::new(
            total_return,
            post_tax_return,
            sharpe,
            calmar,
            max_dd * 100.0,
            win_rate,
            profit_factor,
            expectancy,
            trades.len(),
            winners.len(),
            losers.len(),
            avg_win,
            avg_loss,
            largest_win,
            largest_loss,
            total_commission,
            tax,
            underwater_time_pct,
            avg_drawdown,
            max_underwater_streak,
            recovery_factor,
            max_win_streak,
            max_loss_streak,
        )
    }
}
