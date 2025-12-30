//! Backtesting engine
//!
//! Event-driven backtest framework with commission and slippage modeling.

use std::collections::HashMap;
use chrono::{DateTime, Utc};

use crate::{
    Candle, Config, PerformanceMetrics, Position, Signal, Side, Symbol, Trade,
};
use crate::risk::RiskManager;
use crate::Strategy;

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
        let mut cash = self.config.trading.initial_capital;

        // Find the common date range and align all symbols
        let aligned_data = self.align_data(data);
        if aligned_data.is_empty() {
            tracing::error!("No aligned data available for backtesting");
            return BacktestResult::default();
        }

        let dates = aligned_data[0].1.iter().map(|c| c.datetime).collect::<Vec<_>>();

        for (i, current_date) in dates.iter().enumerate() {
            let mut total_value = cash;

            // Process each symbol
            for (symbol, candles) in &aligned_data {
                let current_candles = &candles[..=i];
                let current_price = current_candles.last().unwrap().close;

                // Check if we have a position and handle exits first
                if let Some(pos) = positions.get(symbol).cloned() {
                    total_value += pos.quantity * current_price;

                    // Check stop loss
                    let stop_price = pos.trailing_stop.unwrap_or(pos.stop_price);
                    if current_price <= stop_price {
                        let trade = self.close_position(&pos, current_price, *current_date, "Stop Loss");
                        cash += pos.quantity * current_price - trade.commission;
                        
                        if trade.net_pnl > 0.0 {
                            self.risk_manager.record_win();
                        } else {
                            self.risk_manager.record_loss();
                        }
                        
                        trades.push(trade);
                        positions.remove(symbol);
                        continue;
                    }

                    // Check take profit
                    if current_price >= pos.target_price {
                        let trade = self.close_position(&pos, current_price, *current_date, "Take Profit");
                        cash += pos.quantity * current_price - trade.commission;
                        
                        self.risk_manager.record_win();
                        trades.push(trade);
                        positions.remove(symbol);
                        continue;
                    }

                    // Update trailing stop
                    if let Some(new_stop) = self.strategy.update_trailing_stop(&pos, current_price, current_candles) {
                        if let Some(pos_mut) = positions.get_mut(symbol) {
                            pos_mut.trailing_stop = Some(new_stop);
                        }
                    }
                }

                // Generate signal
                let position_ref = positions.get(symbol);
                let signal = self.strategy.generate_signal(symbol, current_candles, position_ref);

                match signal {
                    Signal::Long if !positions.contains_key(symbol) => {
                        // Try to open position
                        if self.risk_manager.can_open_position(&positions.values().cloned().collect::<Vec<_>>()) {
                            let entry_price = current_price * (1.0 + self.config.exchange.assumed_slippage);
                            let stop_price = self.strategy.calculate_stop_loss(current_candles, entry_price);
                            let target_price = self.strategy.calculate_take_profit(current_candles, entry_price);

                            let current_positions: Vec<Position> = positions.values().cloned().collect();
                            let quantity = self.risk_manager.calculate_position_size(
                                entry_price,
                                stop_price,
                                &current_positions,
                            );

                            if quantity > 0.0 {
                                let position_value = quantity * entry_price;
                                let commission = position_value * self.config.exchange.taker_fee;

                                if cash >= position_value + commission {
                                    cash -= position_value + commission;

                                    let pos = Position {
                                        symbol: symbol.clone(),
                                        entry_price,
                                        quantity,
                                        stop_price,
                                        target_price,
                                        trailing_stop: None,
                                        entry_time: *current_date,
                                        risk_amount: (entry_price - stop_price).abs() * quantity,
                                    };

                                    positions.insert(symbol.clone(), pos);
                                }
                            }
                        }
                    }
                    Signal::Flat if positions.contains_key(symbol) => {
                        // Close position
                        let pos = positions.get(symbol).unwrap();
                        let exit_price = current_price * (1.0 - self.config.exchange.assumed_slippage);
                        let trade = self.close_position(pos, exit_price, *current_date, "Signal");
                        cash += pos.quantity * exit_price - trade.commission;

                        if trade.net_pnl > 0.0 {
                            self.risk_manager.record_win();
                        } else {
                            self.risk_manager.record_loss();
                        }

                        trades.push(trade);
                        positions.remove(symbol);
                    }
                    _ => {}
                }
            }

            // Update risk manager
            self.risk_manager.update_capital(total_value);

            // Record equity
            equity_curve.push((*current_date, total_value));
        }

        // Close any remaining positions
        for (symbol, pos) in positions {
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

    fn close_position(&self, pos: &Position, exit_price: f64, exit_time: DateTime<Utc>, _reason: &str) -> Trade {
        let pnl = (exit_price - pos.entry_price) * pos.quantity;
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
        
        for (symbol, candles) in data {
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

    fn calculate_metrics(&self, trades: &[Trade], equity_curve: &[(DateTime<Utc>, f64)]) -> PerformanceMetrics {
        if trades.is_empty() || equity_curve.is_empty() {
            return PerformanceMetrics::default();
        }

        let initial_capital = self.config.trading.initial_capital;
        let final_capital = equity_curve.last().unwrap().1;
        let total_return = ((final_capital - initial_capital) / initial_capital) * 100.0;

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

        // Calculate Sharpe ratio (simplified)
        // Note: Annualization factor assumes daily data (252 trading days)
        // For other timeframes, this should be adjusted accordingly
        let returns: Vec<f64> = equity_curve
            .windows(2)
            .map(|w| (w[1].1 - w[0].1) / w[0].1)
            .collect();

        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / returns.len() as f64;
        let std_dev = variance.sqrt();

        let sharpe_ratio = if std_dev > 0.0 {
            mean_return / std_dev * (252.0_f64).sqrt() // Annualized for daily data
        } else {
            0.0
        };

        let calmar_ratio = if max_dd > 0.0 {
            (total_return / 100.0) / max_dd
        } else {
            0.0
        };

        PerformanceMetrics {
            total_return,
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
        }
    }
}

#[derive(Debug, Default)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub metrics: PerformanceMetrics,
}
