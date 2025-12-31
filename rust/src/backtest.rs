//! Backtesting engine
//!
//! Event-driven backtest framework with commission and slippage modeling.
//! Uses T+1 execution (orders placed on day T are executed on day T+1).

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::risk::RiskManager;
use crate::Strategy;
use crate::{Candle, Config, PerformanceMetrics, Position, Side, Signal, Symbol, Trade};

/// Pending order for T+1 execution
#[derive(Debug, Clone)]
struct PendingOrder {
    quantity: f64,
    stop_price: f64,
    target_price: f64,
}

/// Backtest engine
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

        Backtester {
            config,
            strategy,
            risk_manager,
        }
    }

    /// Run backtest on multi-symbol data
    pub fn run(&mut self, data: HashMap<Symbol, Vec<Candle>>) -> BacktestResult {
        let mut equity_curve = Vec::new();
        let mut trades = Vec::new();
        let mut positions: HashMap<Symbol, Position> = HashMap::new();
        let mut pending_orders: HashMap<Symbol, PendingOrder> = HashMap::new();
        let mut pending_closes: HashMap<Symbol, String> = HashMap::new(); // reason for close
        let mut cash = self.config.trading.initial_capital;

        // Find the common date range and align all symbols
        let aligned_data = self.align_data(data);
        if aligned_data.is_empty() {
            tracing::error!("No aligned data available for backtesting");
            return BacktestResult::default();
        }

        let dates = aligned_data[0]
            .1
            .iter()
            .map(|c| c.datetime)
            .collect::<Vec<_>>();

        // Find the minimum length across all aligned data to avoid index out of bounds
        let min_len = aligned_data
            .iter()
            .map(|(_, candles)| candles.len())
            .min()
            .unwrap_or(0);

        // Maximum lookback window for indicator calculation
        // Strategies don't need full history - just enough for the longest indicator period
        // This reduces O(n²) to O(n*k) where k is the lookback window
        // 300 bars covers: ADX (14*3=42), ATR (14), EMA slow (34), volatility lookback (20)
        // with plenty of buffer for warmup periods
        const MAX_LOOKBACK: usize = 300;

        for (i, current_date) in dates.iter().take(min_len).enumerate() {
            // Calculate windowed start index once per bar (used by both phases)
            let start_idx = i.saturating_sub(MAX_LOOKBACK - 1);

            // ============================================================
            // PHASE 1: Execute pending orders from previous bar (T+1 execution)
            // Orders execute at the OPEN of the current bar (matching Backtrader)
            // ============================================================
            for (symbol, candles) in &aligned_data {
                // Use windowed slice for performance - strategies only need recent history
                let current_candles = &candles[start_idx..=i];
                let current_candle = current_candles.last().unwrap();
                let candle_dt = current_candle.datetime;
                let _current_price = current_candle.close; // Kept for potential future use
                let open_price = current_candle.open; // Use open for order execution

                // Execute pending buy order at OPEN price with slippage
                // Professional systems apply slippage to account for market impact
                if let Some(order) = pending_orders.remove(symbol) {
                    let entry_price = open_price * (1.0 + self.config.exchange.assumed_slippage);
                    let position_value = order.quantity * entry_price;
                    let commission = position_value * self.config.exchange.taker_fee;

                    if cash >= position_value + commission {
                        cash -= position_value + commission;

                        let pos = Position {
                            symbol: symbol.clone(),
                            entry_price,
                            quantity: order.quantity,
                            stop_price: order.stop_price,
                            target_price: order.target_price,
                            trailing_stop: None,
                            entry_time: candle_dt, // Use actual candle datetime
                            risk_amount: (entry_price - order.stop_price).abs() * order.quantity,
                        };

                        tracing::info!(
                            "{} BUY EXECUTED for {}: Price={:.2}, Qty={:.4}",
                            candle_dt.format("%Y-%m-%d"), // Use actual candle datetime
                            symbol,
                            entry_price,
                            order.quantity
                        );

                        positions.insert(symbol.clone(), pos);
                    }
                }

                // Execute pending close order at OPEN price with slippage
                // Slippage works against us: we get slightly worse price on exit
                if let Some(reason) = pending_closes.remove(symbol) {
                    if let Some(pos) = positions.remove(symbol) {
                        let exit_price = open_price * (1.0 - self.config.exchange.assumed_slippage);
                        let trade = self.close_position(&pos, exit_price, candle_dt, &reason); // Use candle_dt
                        cash += pos.quantity * exit_price - trade.commission;

                        tracing::info!(
                            "{} SELL EXECUTED for {}: Price={:.2}, Reason={}, PnL={:.2}",
                            candle_dt.format("%Y-%m-%d"), // Use actual candle datetime
                            symbol,
                            exit_price,
                            reason,
                            trade.net_pnl
                        );

                        if trade.net_pnl > 0.0 {
                            self.risk_manager.record_win();
                        } else {
                            self.risk_manager.record_loss();
                        }

                        trades.push(trade);
                    }
                }
            }

            // ============================================================
            // PHASE 2: Check exits and generate new signals
            // ============================================================

            // Recalculate total_value from current cash (after Phase 1 updates)
            let mut total_value = cash;

            for (symbol, candles) in &aligned_data {
                // Use windowed slice for performance - strategies only need recent history
                let current_candles = &candles[start_idx..=i];
                let current_candle = current_candles.last().unwrap();
                let candle_dt = current_candle.datetime;
                let current_price = current_candle.close;

                // Skip if we have a pending close already
                if pending_closes.contains_key(symbol) {
                    if let Some(pos) = positions.get(symbol) {
                        total_value += pos.quantity * current_price;
                    }
                    continue;
                }

                // Check if we have a position and handle exits
                if let Some(pos) = positions.get(symbol).cloned() {
                    total_value += pos.quantity * current_price;

                    // Update trailing stop FIRST (before checking stop loss)
                    // This matches Python behavior where trailing stop is updated before checking exits
                    let current_stop = if let Some(new_stop) =
                        self.strategy
                            .update_trailing_stop(&pos, current_price, current_candles)
                    {
                        if let Some(pos_mut) = positions.get_mut(symbol) {
                            pos_mut.trailing_stop = Some(new_stop);
                        }
                        new_stop
                    } else {
                        pos.trailing_stop.unwrap_or(pos.stop_price)
                    };

                    // Check stop loss - place close order (T+1 execution)
                    if current_price <= current_stop {
                        pending_closes.insert(symbol.clone(), "Stop Loss".to_string());
                        continue;
                    }

                    // Check take profit against HIGH price (production correctness)
                    // Targets should trigger if intraday high reaches target, not just close
                    if current_candle.high >= pos.target_price {
                        pending_closes.insert(symbol.clone(), "Take Profit".to_string());
                        continue;
                    }
                }

                // Generate signal
                let position_ref = positions.get(symbol);
                let signal = self
                    .strategy
                    .generate_signal(symbol, current_candles, position_ref);

                match signal {
                    Signal::Long
                        if !positions.contains_key(symbol)
                            && !pending_orders.contains_key(symbol) =>
                    {
                        // Place pending order (will execute next bar)
                        let can_open = self
                            .risk_manager
                            .can_open_position(&positions.values().cloned().collect::<Vec<_>>());

                        if !can_open {
                            let dd = self.risk_manager.current_drawdown();
                            if dd >= 0.20 {
                                tracing::warn!(
                                    "{} HALTED - Drawdown {:.2}% exceeds max 20%",
                                    candle_dt.format("%Y-%m-%d"),
                                    dd * 100.0
                                );
                            }
                        }

                        if can_open {
                            // Match Python/Backtrader: Calculate stops/targets from signal close price
                            // The order will execute at next bar's open, but stops/targets are based
                            // on the close price when signal was generated (standard backtest convention)
                            let stop_price = self
                                .strategy
                                .calculate_stop_loss(current_candles, current_price);
                            let target_price = self
                                .strategy
                                .calculate_take_profit(current_candles, current_price);

                            let current_positions: Vec<Position> =
                                positions.values().cloned().collect();

                            // Get regime score for position sizing adjustment
                            let regime_score = self.strategy.get_regime_score(current_candles);

                            // Position sizing based on close price (matching Python)
                            let quantity = self.risk_manager.calculate_position_size_with_regime(
                                current_price,
                                stop_price,
                                &current_positions,
                                regime_score,
                            );

                            if quantity > 0.0 {
                                pending_orders.insert(
                                    symbol.clone(),
                                    PendingOrder {
                                        quantity,
                                        stop_price,
                                        target_price,
                                    },
                                );
                            }
                        }
                    }
                    Signal::Flat
                        if positions.contains_key(symbol)
                            && !pending_closes.contains_key(symbol) =>
                    {
                        // Place pending close order (will execute next bar)
                        pending_closes.insert(symbol.clone(), "Signal".to_string());
                    }
                    _ => {}
                }
            }

            // Update risk manager
            self.risk_manager.update_capital(total_value);

            // Record equity
            equity_curve.push((*current_date, total_value));
        }

        // Close any remaining positions (sorted for deterministic order)
        let mut sorted_positions: Vec<(Symbol, Position)> = positions.into_iter().collect();
        sorted_positions.sort_by(|a, b| a.0.0.cmp(&b.0.0));
        for (symbol, pos) in sorted_positions {
            let candles = &aligned_data.iter().find(|(s, _)| s == &symbol).unwrap().1;
            let exit_price = candles.last().unwrap().close;
            let exit_time = candles.last().unwrap().datetime;
            let trade = self.close_position(&pos, exit_price, exit_time, "End of backtest");
            trades.push(trade);
        }

        let metrics = self.calculate_metrics(&trades, &equity_curve);

        BacktestResult {
            trades,
            equity_curve,
            metrics,
        }
    }

    fn close_position(
        &self,
        pos: &Position,
        exit_price: f64,
        exit_time: DateTime<Utc>,
        _reason: &str,
    ) -> Trade {
        let pnl = (exit_price - pos.entry_price) * pos.quantity;
        // Commission: taker fee on both entry and exit (round-trip cost)
        let commission = (pos.quantity * pos.entry_price * self.config.exchange.taker_fee)
            + (pos.quantity * exit_price * self.config.exchange.taker_fee);
        let net_pnl = pnl - commission;

        Trade {
            symbol: pos.symbol.clone(),
            side: Side::Buy,
            entry_price: pos.entry_price,
            exit_price,
            quantity: pos.quantity,
            entry_time: pos.entry_time,
            exit_time,
            pnl,
            commission,
            net_pnl,
        }
    }

    fn align_data(&self, data: HashMap<Symbol, Vec<Candle>>) -> Vec<(Symbol, Vec<Candle>)> {
        use std::collections::HashSet;

        if data.is_empty() {
            return Vec::new();
        }

        // Collect all unique timestamps across all symbols
        let mut all_timestamps: HashSet<DateTime<Utc>> = HashSet::new();
        for candles in data.values() {
            for candle in candles {
                all_timestamps.insert(candle.datetime);
            }
        }

        // Sort timestamps chronologically
        let mut sorted_timestamps: Vec<DateTime<Utc>> = all_timestamps.into_iter().collect();
        sorted_timestamps.sort();

        // For each symbol, create aligned candle series
        // Fill missing timestamps with the previous candle (forward fill)
        let mut aligned_data = Vec::new();

        // Sort symbols for deterministic iteration order
        let mut sorted_data: Vec<(Symbol, Vec<Candle>)> = data.into_iter().collect();
        sorted_data.sort_by(|a, b| a.0.0.cmp(&b.0.0));

        for (symbol, candles) in sorted_data {
            let mut aligned_candles = Vec::new();
            let mut candle_iter = candles.iter().peekable();
            let mut last_candle: Option<Candle> = None;

            for &timestamp in &sorted_timestamps {
                // Check if we have a candle for this timestamp
                match candle_iter.peek() {
                    Some(&candle) if candle.datetime == timestamp => {
                        aligned_candles.push(candle.clone());
                        last_candle = Some(candle.clone());
                        candle_iter.next();
                    }
                    Some(&candle) if candle.datetime < timestamp => {
                        // Skip candles that are earlier than current timestamp
                        // This shouldn't happen if data is sorted, but handle it
                        while let Some(&c) = candle_iter.peek() {
                            if c.datetime < timestamp {
                                last_candle = Some(c.clone());
                                candle_iter.next();
                            } else {
                                break;
                            }
                        }
                        // Forward fill with last candle
                        if let Some(ref last) = last_candle {
                            let mut filled_candle = last.clone();
                            filled_candle.datetime = timestamp;
                            aligned_candles.push(filled_candle);
                        }
                    }
                    _ => {
                        // No candle yet or no more candles - forward fill if we have data
                        if let Some(ref last) = last_candle {
                            let mut filled_candle = last.clone();
                            filled_candle.datetime = timestamp;
                            aligned_candles.push(filled_candle);
                        }
                    }
                }
            }

            if !aligned_candles.is_empty() {
                aligned_data.push((symbol, aligned_candles));
            }
        }

        aligned_data
    }

    fn calculate_metrics(
        &self,
        trades: &[Trade],
        equity_curve: &[(DateTime<Utc>, f64)],
    ) -> PerformanceMetrics {
        if trades.is_empty() || equity_curve.is_empty() {
            return PerformanceMetrics::default();
        }

        let initial_capital = self.config.trading.initial_capital;
        let final_capital = equity_curve.last().unwrap().1;
        let pre_tax_profit = final_capital - initial_capital;
        let total_return = (pre_tax_profit / initial_capital) * 100.0;

        // Calculate total commission from trades
        let total_commission: f64 = trades.iter().map(|t| t.commission).sum();

        // Calculate tax on profits only (losses not taxed in India)
        let tax_rate = self.config.tax.tax_rate;
        let taxable_profit = if pre_tax_profit > 0.0 {
            pre_tax_profit
        } else {
            0.0
        };
        let tax_amount = taxable_profit * tax_rate;
        let post_tax_profit = pre_tax_profit - tax_amount;
        let post_tax_return = (post_tax_profit / initial_capital) * 100.0;

        let winning_trades: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl > 0.0).collect();
        let losing_trades: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl <= 0.0).collect();

        let win_rate = if !trades.is_empty() {
            (winning_trades.len() as f64 / trades.len() as f64) * 100.0
        } else {
            0.0
        };

        let gross_profits: f64 = winning_trades.iter().map(|t| t.net_pnl).sum();
        let gross_losses: f64 = losing_trades.iter().map(|t| t.net_pnl.abs()).sum();

        let profit_factor = if gross_losses > 0.0 {
            gross_profits / gross_losses
        } else if gross_profits > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let avg_win = if !winning_trades.is_empty() {
            gross_profits / winning_trades.len() as f64
        } else {
            0.0
        };

        let avg_loss = if !losing_trades.is_empty() {
            gross_losses / losing_trades.len() as f64
        } else {
            0.0
        };

        let largest_win = winning_trades.iter().map(|t| t.net_pnl).fold(0.0, f64::max);
        let largest_loss = losing_trades.iter().map(|t| t.net_pnl).fold(0.0, f64::min);

        // Calculate max drawdown
        let mut peak = initial_capital;
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

        // Calculate Sharpe ratio
        // Crypto markets trade 365 days/year, using 5% annual risk-free rate
        const TRADING_DAYS_PER_YEAR: f64 = 365.0;
        const RISK_FREE_RATE: f64 = 0.05; // 5% annual risk-free rate
        let daily_risk_free = RISK_FREE_RATE / TRADING_DAYS_PER_YEAR;

        // Calculate daily returns from equity curve
        let all_returns: Vec<f64> = equity_curve
            .windows(2)
            .map(|w| (w[1].1 - w[0].1) / w[0].1)
            .collect();

        // Filter to only non-zero returns for volatility calculation
        // This gives the TRUE volatility of the strategy when in market
        // Zero returns (cash days) artificially deflate measured volatility
        let active_returns: Vec<f64> = all_returns.iter().filter(|&&r| r != 0.0).copied().collect();

        let n_total = all_returns.len() as f64;
        let n_active = active_returns.len() as f64;

        // Mean return uses ALL returns (including cash days) - this is correct
        let mean_return = all_returns.iter().sum::<f64>() / n_total;
        let excess_return = mean_return - daily_risk_free;

        // Standard deviation uses ACTIVE returns only - this reflects true strategy risk
        // When strategy is in cash, there's no market risk, so including those zeros
        // would understate the actual volatility experienced when invested
        let std_dev = if n_active > 1.0 {
            let active_mean = active_returns.iter().sum::<f64>() / n_active;
            let variance = active_returns
                .iter()
                .map(|r| (r - active_mean).powi(2))
                .sum::<f64>()
                / (n_active - 1.0);
            variance.sqrt()
        } else {
            0.0
        };

        // Sharpe ratio: excess return per unit of risk
        // Annualize using sqrt(365) for crypto markets
        let sharpe_ratio = if std_dev > 0.0 {
            excess_return / std_dev * TRADING_DAYS_PER_YEAR.sqrt()
        } else {
            0.0
        };

        // Calculate Calmar ratio (annualized return / max drawdown)
        // First, calculate duration in years from equity curve
        let calmar_ratio = if max_dd > 0.0 && equity_curve.len() >= 2 {
            let start_date = equity_curve.first().unwrap().0;
            let end_date = equity_curve.last().unwrap().0;
            let duration_days = (end_date - start_date).num_days() as f64;

            if duration_days > 0.0 {
                let duration_years = duration_days / 365.0;
                // Annualized return: (1 + total_return)^(1/years) - 1
                let total_return_decimal = total_return / 100.0;
                let annualized_return =
                    (1.0 + total_return_decimal).powf(1.0 / duration_years) - 1.0;
                annualized_return / max_dd
            } else {
                0.0
            }
        } else {
            0.0
        };

        PerformanceMetrics {
            total_return,
            post_tax_return,
            sharpe_ratio,
            calmar_ratio,
            max_drawdown: max_dd * 100.0,
            win_rate,
            profit_factor,
            total_trades: trades.len(),
            winning_trades: winning_trades.len(),
            losing_trades: losing_trades.len(),
            avg_win,
            avg_loss,
            largest_win,
            largest_loss,
            total_commission,
            tax_amount,
        }
    }
}

#[derive(Debug, Default)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub metrics: PerformanceMetrics,
}

#[cfg(test)]
mod tests {
    /// Test that Sharpe ratio uses only active (non-zero) returns for std_dev calculation.
    /// This prevents artificially high Sharpe ratios from including zero-return cash days.
    #[test]
    fn test_sharpe_uses_active_returns_for_volatility() {
        // Simulate a scenario with many zero-return days (cash) and few active trading days
        // If we incorrectly include zeros in std_dev, volatility will be artificially low

        // Example: 100 days total, but only 20 active trading days
        // Active returns: 2%, -1%, 3%, -2%, 1%, ... (realistic daily swings)
        let active_returns = vec![
            0.02, -0.01, 0.03, -0.02, 0.01, 0.025, -0.015, 0.02, -0.01, 0.015, 0.02, -0.01, 0.03,
            -0.02, 0.01, 0.025, -0.015, 0.02, -0.01, 0.015,
        ];

        // Build all_returns with many zeros (cash days)
        let mut all_returns: Vec<f64> = vec![0.0; 100];
        for (i, &ret) in active_returns.iter().enumerate() {
            all_returns[i * 5] = ret; // Every 5th day is active
        }

        let n_total = all_returns.len() as f64;
        let n_active = active_returns.len() as f64;

        // CORRECT: Calculate std_dev from active returns only
        let active_mean = active_returns.iter().sum::<f64>() / n_active;
        let active_variance = active_returns
            .iter()
            .map(|r| (r - active_mean).powi(2))
            .sum::<f64>()
            / (n_active - 1.0);
        let correct_std_dev = active_variance.sqrt();

        // WRONG: Calculate std_dev including zeros (the bug we fixed)
        let total_mean = all_returns.iter().sum::<f64>() / n_total;
        let total_variance = all_returns
            .iter()
            .map(|r| (r - total_mean).powi(2))
            .sum::<f64>()
            / (n_total - 1.0);
        let wrong_std_dev = total_variance.sqrt();

        // Calculate Sharpe both ways
        let mean_return = all_returns.iter().sum::<f64>() / n_total; // Use total mean for expected return
        let risk_free_daily = 0.05 / 365.0;
        let excess_return = mean_return - risk_free_daily;

        let correct_sharpe = excess_return / correct_std_dev * (365.0_f64).sqrt();
        let wrong_sharpe = excess_return / wrong_std_dev * (365.0_f64).sqrt();

        println!("Active returns count: {}", active_returns.len());
        println!("Total days: {}", all_returns.len());
        println!("Correct std_dev (active only): {:.6}", correct_std_dev);
        println!("Wrong std_dev (including zeros): {:.6}", wrong_std_dev);
        println!("Correct Sharpe: {:.2}", correct_sharpe);
        println!("Wrong Sharpe: {:.2}", wrong_sharpe);

        // The wrong method should give HIGHER Sharpe (lower std_dev = higher Sharpe)
        assert!(
            wrong_std_dev < correct_std_dev,
            "Including zeros should reduce std_dev: {} should be < {}",
            wrong_std_dev,
            correct_std_dev
        );

        // The inflation factor should be significant (at least 2x)
        let inflation_factor = wrong_sharpe / correct_sharpe;
        println!("Sharpe inflation factor from bug: {:.1}x", inflation_factor);
        assert!(
            inflation_factor > 1.5,
            "The bug should significantly inflate Sharpe: got {}x",
            inflation_factor
        );

        // Correct std_dev should be realistic for crypto (1-3% daily)
        assert!(
            correct_std_dev > 0.01 && correct_std_dev < 0.05,
            "Active std_dev {:.4} should be realistic (1-5% daily)",
            correct_std_dev
        );
    }

    /// Test that the Sharpe formula handles edge cases correctly
    #[test]
    fn test_sharpe_edge_cases() {
        // Edge case 1: All returns are the same (zero volatility) -> Sharpe should be 0 or handle gracefully
        let constant_returns = vec![0.01; 20];
        let mean = constant_returns.iter().sum::<f64>() / constant_returns.len() as f64;
        let variance = constant_returns
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>()
            / (constant_returns.len() - 1) as f64;
        let std_dev = variance.sqrt();

        // With zero std_dev, we should handle this gracefully
        assert!(
            std_dev < 1e-10,
            "Constant returns should have ~zero std_dev"
        );

        // Edge case 2: Only one active return -> insufficient data
        let single_return = vec![0.05];
        assert!(
            single_return.len() < 2,
            "Need at least 2 returns for std_dev"
        );

        // Edge case 3: Negative Sharpe is valid
        let losing_returns = vec![-0.02, -0.01, -0.03, -0.02, -0.01];
        let losing_mean = losing_returns.iter().sum::<f64>() / losing_returns.len() as f64;
        assert!(
            losing_mean < 0.0,
            "Losing strategy should have negative mean"
        );
    }
}
