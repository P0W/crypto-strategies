//! Production-Grade Backtesting Engine
//!
//! Fully decoupled event-driven backtest framework.
//! - Strategy-agnostic: queries strategy for requirements
//! - Unified MTF support: single code path for all strategies
//! - T+1 execution model with realistic commission/slippage
//! - Memory efficient: uses slices, not copies
//!
//! # Currency Handling
//!
//! The backtesting engine is **currency-agnostic**. All calculations work with
//! dimensionless numbers, requiring only that `initial_capital` (from config) and
//! price data (from CSV files) are in the **same currency**.
//!
//! Key calculations:
//! - Position value = quantity × price
//! - PnL = (exit_price - entry_price) × quantity
//! - Return % = (final_equity - initial_capital) / initial_capital × 100
//!
//! Performance metrics (Sharpe, Calmar, returns) are percentage-based and
//! currency-independent. No currency conversion or exchange rate handling is performed.
//!
//! Example:
//! - Config: `initial_capital: 100000` (in USD)
//! - CSV data: prices in USD (e.g., BTC at $90,000)
//! - Result: All calculations consistent in USD
//!
//! Changing both capital and prices to INR (or any other currency) produces
//! identical percentage returns, as the system only cares about relative values.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::multi_timeframe::{MultiTimeframeCandles, MultiTimeframeData};
use crate::risk::RiskManager;
use crate::Strategy;
use crate::{Candle, Config, PerformanceMetrics, Position, Side, Signal, Symbol, Trade};

/// Pending order for T+1 execution
#[derive(Debug, Clone)]
struct PendingOrder {
    side: Side,
    quantity: f64,
    stop_price: f64,
    target_price: f64,
}

/// Backtest result container
#[derive(Debug, Default)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub metrics: PerformanceMetrics,
}

/// Production backtesting engine
pub struct Backtester {
    config: Config,
    strategy: Box<dyn Strategy>,
    risk_manager: RiskManager,
}

impl Backtester {
    pub fn new(config: Config, strategy: Box<dyn Strategy>) -> Self {
        let risk_manager = RiskManager::new(
            config.trading.initial_capital,
            config.trading.risk_per_trade,
            config.trading.max_positions,
            config.trading.max_portfolio_heat,
            config.trading.max_position_pct,
            config.trading.max_drawdown,
            config.trading.drawdown_warning,
            config.trading.drawdown_critical,
            config.trading.drawdown_warning_multiplier,
            config.trading.drawdown_critical_multiplier,
            config.trading.consecutive_loss_limit,
            config.trading.consecutive_loss_multiplier,
        );

        Self {
            config,
            strategy,
            risk_manager,
        }
    }

    /// Unified backtest runner - handles both single-TF and MTF strategies
    ///
    /// For single-TF strategies: pass data with only primary timeframe
    /// For MTF strategies: pass data with all required timeframes
    ///
    /// Takes a reference to avoid cloning in the optimizer (memory optimization).
    pub fn run(&mut self, data: &crate::MultiSymbolMultiTimeframeData) -> BacktestResult {
        if data.is_empty() {
            tracing::error!("No data provided for backtesting");
            return BacktestResult::default();
        }

        // Get strategy requirements
        let required_tfs = self.strategy.required_timeframes();
        let is_mtf = !required_tfs.is_empty();

        if is_mtf {
            tracing::debug!("Running MTF backtest with timeframes: {:?}", required_tfs);
        }

        // Align data across symbols
        let aligned = crate::multi_timeframe::align_multi_timeframe_data(data);
        if aligned.is_empty() {
            tracing::error!("No aligned data after filtering");
            return BacktestResult::default();
        }

        // Get primary timeframe info
        let primary_tf = aligned[0].1.primary_timeframe().to_string();
        let primary_len = aligned
            .iter()
            .map(|(_, mtf)| mtf.primary().len())
            .min()
            .unwrap_or(0);

        if primary_len == 0 {
            return BacktestResult::default();
        }

        // Pre-collect dates for iteration
        let dates: Vec<DateTime<Utc>> = aligned[0]
            .1
            .primary()
            .iter()
            .take(primary_len)
            .map(|c| c.datetime)
            .collect();

        // State
        let mut equity_curve = Vec::with_capacity(dates.len());
        let mut trades = Vec::new();
        let mut positions: HashMap<Symbol, Position> = HashMap::new();
        let mut pending_orders: HashMap<Symbol, PendingOrder> = HashMap::new();
        let mut pending_closes: HashMap<Symbol, String> = HashMap::new();
        let mut cash = self.config.trading.initial_capital;

        // Lookback window for indicators (no need for full history)
        const LOOKBACK: usize = 300;

        // Main simulation loop
        for (bar_idx, current_date) in dates.iter().enumerate() {
            let start_idx = bar_idx.saturating_sub(LOOKBACK - 1);

            // ================================================================
            // PHASE 1: Execute pending orders at current bar's OPEN (T+1)
            // ================================================================
            for (symbol, mtf_data) in &aligned {
                let primary = mtf_data.primary();
                let candle = &primary[bar_idx];
                let open_price = candle.open;

                // Execute pending entry
                if let Some(order) = pending_orders.remove(symbol) {
                    let (entry_price, is_long) = match order.side {
                        Side::Buy => (
                            open_price * (1.0 + self.config.exchange.assumed_slippage),
                            true,
                        ),
                        Side::Sell => (
                            open_price * (1.0 - self.config.exchange.assumed_slippage),
                            false,
                        ),
                    };

                    let position_value = order.quantity * entry_price;
                    let commission = position_value * self.config.exchange.taker_fee;
                    let cash_needed = if is_long {
                        position_value + commission
                    } else {
                        commission
                    };

                    if cash >= cash_needed {
                        if is_long {
                            cash -= position_value + commission;
                        } else {
                            cash += position_value - commission;
                        }

                        positions.insert(
                            symbol.clone(),
                            Position {
                                symbol: symbol.clone(),
                                side: order.side,
                                entry_price,
                                quantity: order.quantity,
                                stop_price: order.stop_price,
                                target_price: order.target_price,
                                trailing_stop: None,
                                entry_time: candle.datetime,
                                risk_amount: (entry_price - order.stop_price).abs()
                                    * order.quantity,
                            },
                        );

                        tracing::debug!(
                            "{} {:?} {} @ {:.2}",
                            candle.datetime.format("%Y-%m-%d"),
                            order.side,
                            symbol,
                            entry_price
                        );
                    }
                }

                // Execute pending close
                if let Some(reason) = pending_closes.remove(symbol) {
                    if let Some(pos) = positions.remove(symbol) {
                        let exit_price = match pos.side {
                            Side::Buy => open_price * (1.0 - self.config.exchange.assumed_slippage),
                            Side::Sell => {
                                open_price * (1.0 + self.config.exchange.assumed_slippage)
                            }
                        };

                        let trade = self.close_position(&pos, exit_price, candle.datetime, &reason);

                        match pos.side {
                            Side::Buy => cash += pos.quantity * exit_price - trade.commission,
                            Side::Sell => cash -= pos.quantity * exit_price + trade.commission,
                        }

                        if trade.net_pnl > 0.0 {
                            self.risk_manager.record_win();
                        } else {
                            self.risk_manager.record_loss();
                        }

                        tracing::debug!(
                            "{} CLOSE {} @ {:.2} PnL={:.2} ({})",
                            candle.datetime.format("%Y-%m-%d"),
                            symbol,
                            exit_price,
                            trade.net_pnl,
                            reason
                        );

                        trades.push(trade);
                    }
                }
            }

            // ================================================================
            // PHASE 2: Check exits and generate signals at CLOSE
            // ================================================================
            let mut total_value = cash;

            for (symbol, mtf_data) in &aligned {
                let primary = mtf_data.primary();
                let current_slice = &primary[start_idx..=bar_idx];
                let candle = current_slice.last().unwrap();
                let price = candle.close;

                // Skip if pending close
                if pending_closes.contains_key(symbol) {
                    if let Some(pos) = positions.get(symbol) {
                        total_value += pos.quantity * price;
                    }
                    continue;
                }

                // Handle existing position
                if let Some(pos) = positions.get(symbol).cloned() {
                    total_value += pos.quantity * price;

                    // Update trailing stop
                    let active_stop = if let Some(new_stop) =
                        self.strategy
                            .update_trailing_stop(&pos, price, current_slice)
                    {
                        if let Some(p) = positions.get_mut(symbol) {
                            p.trailing_stop = Some(new_stop);
                        }
                        new_stop
                    } else {
                        pos.trailing_stop.unwrap_or(pos.stop_price)
                    };

                    // Check stop loss
                    let stopped = match pos.side {
                        Side::Buy => price <= active_stop,
                        Side::Sell => price >= active_stop,
                    };
                    if stopped {
                        pending_closes.insert(symbol.clone(), "Stop".to_string());
                        continue;
                    }

                    // Check take profit (use high/low for intraday)
                    let target_hit = match pos.side {
                        Side::Buy => candle.high >= pos.target_price,
                        Side::Sell => candle.low <= pos.target_price,
                    };
                    if target_hit {
                        pending_closes.insert(symbol.clone(), "Target".to_string());
                        continue;
                    }
                }

                // Build MTF view for signal generation
                let signal = if is_mtf {
                    let mut mtf_view = MultiTimeframeCandles::new(&primary_tf, candle.datetime);
                    mtf_view.add_timeframe(&primary_tf, current_slice);

                    // Add secondary timeframes aligned to current datetime
                    for tf in mtf_data.timeframes() {
                        if tf == primary_tf {
                            continue;
                        }
                        if let Some(tf_candles) = mtf_data.get(tf) {
                            // Find candles up to current datetime
                            let aligned_end = tf_candles
                                .iter()
                                .rposition(|c| c.datetime <= candle.datetime)
                                .map(|i| i + 1)
                                .unwrap_or(0);

                            if aligned_end > 0 {
                                let tf_start = aligned_end.saturating_sub(LOOKBACK);
                                mtf_view.add_timeframe(tf, &tf_candles[tf_start..aligned_end]);
                            }
                        }
                    }

                    self.strategy
                        .generate_signal_mtf(symbol, &mtf_view, positions.get(symbol))
                } else {
                    self.strategy
                        .generate_signal(symbol, current_slice, positions.get(symbol))
                };

                // Process signal
                match signal {
                    Signal::Long
                        if !positions.contains_key(symbol)
                            && !pending_orders.contains_key(symbol) =>
                    {
                        if let Some(order) =
                            self.create_order(symbol, Side::Buy, current_slice, price, &positions)
                        {
                            pending_orders.insert(symbol.clone(), order);
                        }
                    }
                    Signal::Short
                        if !positions.contains_key(symbol)
                            && !pending_orders.contains_key(symbol) =>
                    {
                        if let Some(order) =
                            self.create_order(symbol, Side::Sell, current_slice, price, &positions)
                        {
                            pending_orders.insert(symbol.clone(), order);
                        }
                    }
                    Signal::Flat
                        if positions.contains_key(symbol)
                            && !pending_closes.contains_key(symbol) =>
                    {
                        pending_closes.insert(symbol.clone(), "Signal".to_string());
                    }
                    _ => {}
                }
            }

            self.risk_manager.update_capital(total_value);
            equity_curve.push((*current_date, total_value));
        }

        // Close remaining positions
        self.close_remaining_positions(&aligned, &mut positions, &mut trades);

        let metrics = self.calculate_metrics(&trades, &equity_curve, &primary_tf);
        BacktestResult {
            trades,
            equity_curve,
            metrics,
        }
    }

    /// Legacy single-TF interface - converts to MTF format
    pub fn run_single_tf(&mut self, data: HashMap<Symbol, Vec<Candle>>) -> BacktestResult {
        let primary_tf = self.config.timeframe();
        let mtf_data: crate::MultiSymbolMultiTimeframeData = data
            .into_iter()
            .map(|(symbol, candles)| {
                let mut mtf = MultiTimeframeData::new(&primary_tf);
                mtf.add_timeframe(&primary_tf, candles);
                (symbol, mtf)
            })
            .collect();

        self.run(&mtf_data)
    }

    #[inline]
    fn create_order(
        &self,
        _symbol: &Symbol,
        side: Side,
        candles: &[Candle],
        price: f64,
        positions: &HashMap<Symbol, Position>,
    ) -> Option<PendingOrder> {
        // Avoid allocating a Vec by using the HashMap directly
        if !self.risk_manager.can_open_position_count(positions.len()) {
            return None;
        }

        let stop_price = self.strategy.calculate_stop_loss(candles, price);
        let target_price = self.strategy.calculate_take_profit(candles, price);
        let regime_score = self.strategy.get_regime_score(candles);

        let quantity = self.risk_manager.calculate_position_size_with_regime_iter(
            price,
            stop_price,
            positions.values(),
            regime_score,
        );

        if quantity <= 0.0 {
            return None;
        }

        Some(PendingOrder {
            side,
            quantity,
            stop_price,
            target_price,
        })
    }

    #[inline]
    fn close_position(
        &self,
        pos: &Position,
        exit_price: f64,
        exit_time: DateTime<Utc>,
        _reason: &str,
    ) -> Trade {
        let pnl = match pos.side {
            Side::Buy => (exit_price - pos.entry_price) * pos.quantity,
            Side::Sell => (pos.entry_price - exit_price) * pos.quantity,
        };

        let commission = (pos.quantity * pos.entry_price + pos.quantity * exit_price)
            * self.config.exchange.taker_fee;

        Trade {
            symbol: pos.symbol.clone(),
            side: pos.side,
            entry_price: pos.entry_price,
            exit_price,
            quantity: pos.quantity,
            entry_time: pos.entry_time,
            exit_time,
            pnl,
            commission,
            net_pnl: pnl - commission,
        }
    }

    fn close_remaining_positions(
        &self,
        aligned: &[(Symbol, MultiTimeframeData)],
        positions: &mut HashMap<Symbol, Position>,
        trades: &mut Vec<Trade>,
    ) {
        let mut sorted: Vec<_> = positions.drain().collect();
        sorted.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));

        for (symbol, pos) in sorted {
            if let Some((_, mtf)) = aligned.iter().find(|(s, _)| s == &symbol) {
                let primary = mtf.primary();
                if let Some(last) = primary.last() {
                    let trade = self.close_position(&pos, last.close, last.datetime, "EOD");
                    trades.push(trade);
                }
            }
        }
    }

    fn calculate_metrics(
        &self,
        trades: &[Trade],
        equity_curve: &[(DateTime<Utc>, f64)],
        timeframe: &str,
    ) -> PerformanceMetrics {
        if trades.is_empty() || equity_curve.is_empty() {
            return PerformanceMetrics::default();
        }

        let initial = self.config.trading.initial_capital;
        let final_val = equity_curve.last().unwrap().1;
        let pre_tax_profit = final_val - initial;
        let total_return = (pre_tax_profit / initial) * 100.0;

        let total_commission: f64 = trades.iter().map(|t| t.commission).sum();

        // Tax calculation
        let taxable = pre_tax_profit.max(0.0);
        let tax = taxable * self.config.tax.tax_rate;
        let post_tax_profit = pre_tax_profit - tax;
        let post_tax_return = (post_tax_profit / initial) * 100.0;

        // Win/Loss stats
        let winners: Vec<_> = trades.iter().filter(|t| t.net_pnl > 0.0).collect();
        let losers: Vec<_> = trades.iter().filter(|t| t.net_pnl <= 0.0).collect();

        let win_rate = if !trades.is_empty() {
            (winners.len() as f64 / trades.len() as f64) * 100.0
        } else {
            0.0
        };

        let gross_profit: f64 = winners.iter().map(|t| t.net_pnl).sum();
        let gross_loss: f64 = losers.iter().map(|t| t.net_pnl.abs()).sum();

        let profit_factor = if gross_loss > 0.0 {
            gross_profit / gross_loss
        } else if gross_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let avg_win = if !winners.is_empty() {
            gross_profit / winners.len() as f64
        } else {
            0.0
        };
        let avg_loss = if !losers.is_empty() {
            gross_loss / losers.len() as f64
        } else {
            0.0
        };

        let expectancy = if !trades.is_empty() {
            let wr = win_rate / 100.0;
            (wr * avg_win) - ((1.0 - wr) * avg_loss)
        } else {
            0.0
        };

        let largest_win = winners.iter().map(|t| t.net_pnl).fold(0.0, f64::max);
        let largest_loss = losers.iter().map(|t| t.net_pnl).fold(0.0, f64::min);

        // Max drawdown
        let mut peak = initial;
        let mut max_dd = 0.0;
        for (_, equity) in equity_curve {
            if *equity > peak {
                peak = *equity;
            }
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }

        // Sharpe ratio - annualized based on timeframe
        // For crypto: 365 days/year, 24 hours/day
        let periods_per_year: f64 = match timeframe {
            "1m" => 365.0 * 24.0 * 60.0, // 525,600
            "5m" => 365.0 * 24.0 * 12.0, // 105,120
            "15m" => 365.0 * 24.0 * 4.0, // 35,040
            "1h" => 365.0 * 24.0,        // 8,760
            "4h" => 365.0 * 6.0,         // 2,190
            _ => 365.0,                  // 365 (default for 1d and unknown)
        };
        const RISK_FREE: f64 = 0.05;
        let period_rf = RISK_FREE / periods_per_year;

        let returns: Vec<f64> = equity_curve
            .windows(2)
            .map(|w| (w[1].1 - w[0].1) / w[0].1)
            .collect();

        let n = returns.len() as f64;
        let mean_ret = if n > 0.0 {
            returns.iter().sum::<f64>() / n
        } else {
            0.0
        };
        let excess = mean_ret - period_rf;

        let std_dev = if n > 1.0 {
            let var = returns.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>() / (n - 1.0);
            var.sqrt()
        } else {
            0.0
        };

        let sharpe = if std_dev > 0.0 {
            excess / std_dev * periods_per_year.sqrt()
        } else {
            0.0
        };

        // Calmar ratio
        let calmar = if max_dd > 0.0 && equity_curve.len() >= 2 {
            let start = equity_curve.first().unwrap().0;
            let end = equity_curve.last().unwrap().0;
            let days = (end - start).num_days() as f64;
            if days > 0.0 {
                let years = days / 365.0;
                let ann_ret = (1.0 + total_return / 100.0).powf(1.0 / years) - 1.0;
                ann_ret / max_dd
            } else {
                0.0
            }
        } else {
            0.0
        };

        PerformanceMetrics {
            total_return,
            post_tax_return,
            sharpe_ratio: sharpe,
            calmar_ratio: calmar,
            max_drawdown: max_dd * 100.0,
            win_rate,
            profit_factor,
            expectancy,
            total_trades: trades.len(),
            winning_trades: winners.len(),
            losing_trades: losers.len(),
            avg_win,
            avg_loss,
            largest_win,
            largest_loss,
            total_commission,
            tax_amount: tax,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_sharpe_calculation() {
        let returns = vec![0.02, -0.01, 0.03, -0.02, 0.01, 0.0, 0.025, -0.015, 0.02];
        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        let var = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let std = var.sqrt();

        assert!(std > 0.0);
        assert!((mean - 0.00833).abs() < 0.01); // Approximate mean
    }
}
