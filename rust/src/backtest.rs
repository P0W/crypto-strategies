//! Production-Grade Backtesting Engine with OMS
//!
//! Fully decoupled event-driven backtest framework with Order Management System.
//! - Strategy-agnostic: queries strategy for order requests
//! - Unified MTF support: single code path for all strategies
//! - Order lifecycle management: place → execute → fill → update positions
//! - Intra-candle fill detection for realistic execution
//! - Memory efficient: uses slices, not copies
//!
//! # Currency Handling
//!
//! The backtesting engine is **currency-agnostic**. All calculations work with
//! dimensionless numbers, requiring only that `initial_capital` (from config) and
//! price data (from CSV files) are in the **same currency**.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::multi_timeframe::MultiTimeframeCandles;
use crate::oms::{
    ExecutionEngine, Order, OrderBook, OrderRequest, Position, PositionManager, StrategyContext,
};
use crate::risk::RiskManager;
use crate::Strategy;
use crate::{Config, PerformanceMetrics, Side, Symbol, Trade};

/// Backtest result container
#[derive(Debug, Default)]
pub struct BacktestResult {
    pub trades: Vec<Trade>,
    pub equity_curve: Vec<(DateTime<Utc>, f64)>,
    pub metrics: PerformanceMetrics,
}

/// Production backtesting engine with OMS
pub struct Backtester {
    config: Config,
    strategy: Box<dyn Strategy>,
    risk_manager: RiskManager,
    execution_engine: ExecutionEngine,
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

        let execution_engine = ExecutionEngine::new(
            config.exchange.maker_fee,
            config.exchange.taker_fee,
            config.exchange.assumed_slippage,
        );

        Self {
            config,
            strategy,
            risk_manager,
            execution_engine,
        }
    }

    /// Unified backtest runner - handles both single-TF and MTF strategies
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

        // State - using OMS components
        let mut equity_curve = Vec::with_capacity(dates.len());
        let mut trades = Vec::new();
        let mut position_manager = PositionManager::new();
        let mut orderbooks: HashMap<Symbol, OrderBook> = HashMap::new();
        let mut cash = self.config.trading.initial_capital;

        // Initialize orderbooks for each symbol
        for (symbol, _) in &aligned {
            orderbooks.insert(symbol.clone(), OrderBook::new());
        }

        // Lookback window for indicators
        const LOOKBACK: usize = 300;

        // Main simulation loop
        for (bar_idx, current_date) in dates.iter().enumerate() {
            let start_idx = bar_idx.saturating_sub(LOOKBACK - 1);

            // ================================================================
            // PHASE 1: Process fills - check all orders against current candle
            // ================================================================
            for (symbol, mtf_data) in &aligned {
                let primary = mtf_data.primary();
                let candle = &primary[bar_idx];

                if let Some(orderbook) = orderbooks.get_mut(symbol) {
                    let order_ids: Vec<u64> = orderbook.get_all_order_ids();

                    for order_id in order_ids {
                        if let Some(order) = orderbook.get_order_mut(order_id) {
                            if !order.is_active() {
                                continue;
                            }

                            // Check if order fills during this candle
                            if let Some(fill_price_info) =
                                self.execution_engine.check_fill(order, candle)
                            {
                                // Execute the fill with HISTORICAL timestamp from candle
                                let fill = self.execution_engine.execute_fill(
                                    order,
                                    fill_price_info.price,
                                    fill_price_info.is_maker,
                                    candle.datetime,
                                );

                                // Update cash based on fill
                                match order.side {
                                    Side::Buy => {
                                        let cost = fill.price * fill.quantity + fill.commission;
                                        cash -= cost;
                                    }
                                    Side::Sell => {
                                        let proceeds = fill.price * fill.quantity - fill.commission;
                                        cash += proceeds;
                                    }
                                }

                                // Update position
                                position_manager.add_fill(fill.clone(), symbol.clone(), order.side);

                                // Notify strategy
                                if let Some(pos) = position_manager.get_position(symbol) {
                                    self.strategy.on_order_filled(&fill, pos);
                                }

                                tracing::debug!(
                                    "{} FILL {:?} {} @ {:.2} qty={:.4}",
                                    candle.datetime.format("%Y-%m-%d %H:%M"),
                                    order.side,
                                    symbol,
                                    fill.price,
                                    fill.quantity
                                );
                            }
                        }
                    }

                    // Remove filled/cancelled orders
                    let to_remove: Vec<u64> = orderbook
                        .get_all_orders()
                        .iter()
                        .filter(|o| o.is_complete())
                        .map(|o| o.id)
                        .collect();

                    for order_id in to_remove {
                        orderbook.cancel_order(order_id);
                    }
                }
            }

            // ================================================================
            // PHASE 2: Check stops and generate new orders
            // ================================================================
            let mut total_value = cash;

            for (symbol, mtf_data) in &aligned {
                let primary = mtf_data.primary();
                let current_slice = &primary[start_idx..=bar_idx];
                let candle = current_slice.last().unwrap();
                let price = candle.close;

                // Update position unrealized P&L first (before borrowing position)
                let mut prices = HashMap::new();
                prices.insert(symbol.clone(), price);
                position_manager.update_unrealized_pnl(&prices);

                // Get current position AFTER update
                let position = position_manager.get_position(symbol);

                // Calculate total value
                if let Some(pos) = position {
                    total_value += pos.quantity * price;

                    // Check stop loss and take profit
                    let stop_price = self
                        .strategy
                        .calculate_stop_loss(current_slice, pos.average_entry_price);
                    let target_price = self
                        .strategy
                        .calculate_take_profit(current_slice, pos.average_entry_price);

                    let stopped = match pos.side {
                        Side::Buy => price <= stop_price || candle.low <= stop_price,
                        Side::Sell => price >= stop_price || candle.high >= stop_price,
                    };

                    let target_hit = match pos.side {
                        Side::Buy => candle.high >= target_price,
                        Side::Sell => candle.low <= target_price,
                    };

                    if stopped || target_hit {
                        // Place market order to close position
                        let close_order = match pos.side {
                            Side::Buy => OrderRequest::market_sell(symbol.clone(), pos.quantity),
                            Side::Sell => OrderRequest::market_buy(symbol.clone(), pos.quantity),
                        };

                        if let Some(orderbook) = orderbooks.get_mut(symbol) {
                            orderbook.add_order(close_order.into_order());
                        }

                        let reason = if stopped { "Stop" } else { "Target" };
                        tracing::debug!(
                            "{} {} close order @ {:.2} ({})",
                            candle.datetime.format("%Y-%m-%d"),
                            symbol,
                            price,
                            reason
                        );
                        continue;
                    }

                    // Check trailing stop
                    if let Some(new_stop) =
                        self.strategy
                            .update_trailing_stop(pos, price, current_slice)
                    {
                        let trailing_stopped = match pos.side {
                            Side::Buy => price <= new_stop,
                            Side::Sell => price >= new_stop,
                        };

                        if trailing_stopped {
                            let close_order = match pos.side {
                                Side::Buy => {
                                    OrderRequest::market_sell(symbol.clone(), pos.quantity)
                                }
                                Side::Sell => {
                                    OrderRequest::market_buy(symbol.clone(), pos.quantity)
                                }
                            };

                            if let Some(orderbook) = orderbooks.get_mut(symbol) {
                                orderbook.add_order(close_order.into_order());
                            }

                            tracing::debug!(
                                "{} {} trailing stop @ {:.2}",
                                candle.datetime.format("%Y-%m-%d"),
                                symbol,
                                price
                            );
                            continue;
                        }
                    }
                }

                // Build strategy context
                let open_orders: Vec<Order> = orderbooks
                    .get(symbol)
                    .map(|ob| ob.get_all_orders().into_iter().cloned().collect())
                    .unwrap_or_default();

                // Build strategy context
                let mut mtf_view_storage;
                let ctx = if is_mtf {
                    // Build MTF view - store in outer scope for lifetime
                    mtf_view_storage = MultiTimeframeCandles::new(&primary_tf, candle.datetime);
                    mtf_view_storage.add_timeframe(&primary_tf, current_slice);

                    for tf in mtf_data.timeframes() {
                        if tf == primary_tf {
                            continue;
                        }
                        if let Some(tf_candles) = mtf_data.get(tf) {
                            let aligned_end = tf_candles
                                .iter()
                                .rposition(|c| c.datetime <= candle.datetime)
                                .map(|i| i + 1)
                                .unwrap_or(0);

                            if aligned_end > 0 {
                                let tf_start = aligned_end.saturating_sub(LOOKBACK);
                                mtf_view_storage
                                    .add_timeframe(tf, &tf_candles[tf_start..aligned_end]);
                            }
                        }
                    }

                    StrategyContext::multi_timeframe(
                        symbol,
                        &mtf_view_storage,
                        position,
                        &open_orders,
                        cash,
                        total_value,
                    )
                } else {
                    StrategyContext::single_timeframe(
                        symbol,
                        current_slice,
                        position,
                        &open_orders,
                        cash,
                        total_value,
                    )
                };

                // Get orders from strategy
                let order_requests = self.strategy.generate_orders(&ctx);

                // Process each order request
                for order_req in order_requests {
                    // Validate with risk manager
                    let position_count = if position.is_some() { 1 } else { 0 };

                    if !self.risk_manager.should_halt_trading()
                        && self.risk_manager.can_open_position_count(position_count)
                    {
                        // Calculate position size based on risk
                        let regime_score = self.strategy.get_regime_score(current_slice);

                        // Get all current positions for portfolio heat calculation
                        let all_positions = position_manager.get_all_positions();

                        let quantity = self.risk_manager.calculate_position_size_with_regime(
                            price,
                            self.strategy.calculate_stop_loss(current_slice, price),
                            &all_positions,
                            regime_score,
                        );

                        // Create order with calculated quantity
                        let mut order = order_req.into_order();
                        order.quantity = quantity;
                        order.remaining_quantity = quantity;

                        // Add to orderbook
                        if let Some(orderbook) = orderbooks.get_mut(symbol) {
                            orderbook.add_order(order.clone());

                            tracing::debug!(
                                "{} ORDER {:?} {} @ {:.2} qty={:.4}",
                                candle.datetime.format("%Y-%m-%d"),
                                order.side,
                                symbol,
                                order.limit_price.unwrap_or(price),
                                order.quantity
                            );
                        }
                    }
                }
            }

            self.risk_manager.update_capital(total_value);
            equity_curve.push((*current_date, total_value));
        }

        // Close remaining positions and convert to trades
        for (symbol, mtf_data) in &aligned {
            if let Some(pos) = position_manager.close_position(symbol) {
                let primary = mtf_data.primary();
                let last_candle = primary.last().unwrap();
                let exit_price = last_candle.close;

                let trade = self.create_trade_from_position(&pos, exit_price, last_candle.datetime);
                
                // Record win/loss for risk manager
                if trade.net_pnl > 0.0 {
                    self.risk_manager.record_win();
                } else {
                    self.risk_manager.record_loss();
                }
                
                // Notify strategy
                self.strategy.on_trade_closed(&trade);
                
                trades.push(trade);
            }
        }

        let metrics = self.calculate_metrics(&trades, &equity_curve, &primary_tf);
        BacktestResult {
            trades,
            equity_curve,
            metrics,
        }
    }

    fn create_trade_from_position(
        &self,
        pos: &Position,
        exit_price: f64,
        exit_time: DateTime<Utc>,
    ) -> Trade {
        let pnl = pos.realized_pnl
            + match pos.side {
                Side::Buy => (exit_price - pos.average_entry_price) * pos.quantity,
                Side::Sell => (pos.average_entry_price - exit_price) * pos.quantity,
            };

        let commission = pos.fills.iter().map(|f| f.commission).sum::<f64>()
            + exit_price * pos.quantity * self.config.exchange.taker_fee;

        let net_pnl = pnl - commission;

        Trade {
            symbol: pos.symbol.clone(),
            side: pos.side,
            entry_price: pos.average_entry_price,
            exit_price,
            quantity: pos.quantity,
            entry_time: pos.first_entry_time,
            exit_time,
            pnl,
            commission,
            net_pnl,
        }
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

        let winners: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl > 0.0).collect();
        let losers: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl <= 0.0).collect();

        let win_rate = if !trades.is_empty() {
            (winners.len() as f64 / trades.len() as f64) * 100.0
        } else {
            0.0
        };

        let total_wins: f64 = winners.iter().map(|t| t.net_pnl).sum();
        let total_losses: f64 = losers.iter().map(|t| t.net_pnl.abs()).sum();

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

        let expectancy = (win_rate / 100.0) * avg_win - ((100.0 - win_rate) / 100.0) * avg_loss;

        let largest_win = winners.iter().map(|t| t.net_pnl).fold(0.0, f64::max);
        let largest_loss = losers.iter().map(|t| t.net_pnl).fold(0.0, f64::min);

        let total_commission: f64 = trades.iter().map(|t| t.commission).sum();

        // Sharpe ratio
        let returns: Vec<f64> = equity_curve
            .windows(2)
            .map(|w| (w[1].1 - w[0].1) / w[0].1)
            .collect();

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

        // Max drawdown
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

        // Calmar ratio
        let calmar = if max_dd > 0.0 {
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

        // Tax calculation (India: 30% flat)
        let tax_rate = self.config.tax.tax_rate;
        let taxable_gains = total_wins;
        let tax = taxable_gains * tax_rate;
        let post_tax_return = ((final_equity - initial_capital - tax) / initial_capital) * 100.0;

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
