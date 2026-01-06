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
use crate::oms::{ExecutionEngine, Order, OrderBook, Position, PositionManager, StrategyContext};
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

        // T+1 execution: queue of (symbol, order_id) to execute at next bar's open
        let mut t1_pending: Vec<(Symbol, u64)> = Vec::new();

        // CRITICAL: Store stop/target at entry time to prevent drift
        // Key insight: Main branch stores these in PendingOrder at entry; OMS recalculates every bar
        // This cache fixes the stop/target at entry time like the main branch does
        // Format: (stop_price, target_price)
        let mut entry_levels: HashMap<Symbol, (f64, f64)> = HashMap::new();

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
            // PHASE 0 (T+1 only): Execute orders queued from previous day
            // ================================================================
            if self.config.backtest.use_t1_execution && !t1_pending.is_empty() {
                for (symbol, order_id) in t1_pending.drain(..) {
                    if let Some((_, mtf_data)) = aligned.iter().find(|(s, _)| s == &symbol) {
                        let primary = mtf_data.primary();
                        let candle = &primary[bar_idx];

                        if let Some(orderbook) = orderbooks.get_mut(&symbol) {
                            if let Some(order) = orderbook.get_order_mut(order_id) {
                                if order.is_active() {
                                    // Execute at open price with slippage
                                    let fill_price = candle.open
                                        * (1.0
                                            + self.config.exchange.assumed_slippage
                                                * if order.side == Side::Buy { 1.0 } else { -1.0 });

                                    // Check if we have enough cash for buy orders (matches main branch)
                                    if order.side == Side::Buy {
                                        let position_value = fill_price * order.quantity;
                                        let commission =
                                            position_value * self.config.exchange.taker_fee;
                                        let cash_needed = position_value + commission;
                                        if cash < cash_needed {
                                            tracing::debug!(
                                                "T+1: Insufficient cash: have {:.2}, need {:.2} - skipping order",
                                                cash,
                                                cash_needed
                                            );
                                            continue;
                                        }
                                    }

                                    let fill = self.execution_engine.execute_fill(
                                        order,
                                        fill_price,
                                        false, // taker (market execution)
                                        candle.datetime,
                                    );

                                    tracing::info!(
                                        "{} T+1 execution: {:?} {} @ {:.2} (queued from previous day)",
                                        candle.datetime.format("%Y-%m-%d"),
                                        order.side,
                                        symbol,
                                        fill.price
                                    );

                                    // Update cash
                                    match order.side {
                                        Side::Buy => {
                                            let cost = fill.price * fill.quantity + fill.commission;
                                            cash -= cost;
                                        }
                                        Side::Sell => {
                                            let proceeds =
                                                fill.price * fill.quantity - fill.commission;
                                            cash += proceeds;
                                        }
                                    }

                                    // Check position before fill
                                    let had_position_before =
                                        position_manager.get_position(&symbol).is_some();
                                    let prev_pos = if had_position_before {
                                        position_manager.get_position_raw(&symbol).cloned()
                                    } else {
                                        None
                                    };

                                    // Update position
                                    position_manager.add_fill(
                                        fill.clone(),
                                        symbol.clone(),
                                        order.side,
                                    );

                                    // Check if position closed
                                    let has_position_after =
                                        position_manager.get_position(&symbol).is_some();

                                    if had_position_before && !has_position_after {
                                        if let Some(prev) = prev_pos {
                                            // CRITICAL: Clear closed position from manager to prevent P&L accumulation
                                            position_manager.close_position(&symbol);

                                            // Use proper trade creation method
                                            let trade = self.create_trade_from_position(
                                                &prev,
                                                fill.price,
                                                candle.datetime,
                                            );

                                            if trade.net_pnl > 0.0 {
                                                self.risk_manager.record_win();
                                            } else {
                                                self.risk_manager.record_loss();
                                            }

                                            // Clear cached entry levels for closed position
                                            entry_levels.remove(&symbol);

                                            trades.push(trade.clone());
                                            self.strategy.on_trade_closed(&trade);
                                        }
                                    }

                                    // Notify strategy
                                    if let Some(pos) = position_manager.get_position(&symbol) {
                                        self.strategy.on_order_filled(&fill, pos);
                                    }

                                    // Mark order as filled in orderbook
                                    orderbook.mark_filled(order_id);
                                }
                            }
                        }
                    }
                }
            }

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
                                // T+1 mode: Only queue stop/target orders for next day
                                // Entry market orders should execute same day
                                let is_stop_or_target = order
                                    .client_id
                                    .as_ref()
                                    .map(|n| n.contains("Stop") || n.contains("Target"))
                                    .unwrap_or(false);

                                if self.config.backtest.use_t1_execution && is_stop_or_target {
                                    tracing::debug!(
                                        "{} T+1 trigger: {:?} {} @ {:.2} ({}) - queuing for next day",
                                        candle.datetime.format("%Y-%m-%d"),
                                        order.side,
                                        symbol,
                                        fill_price_info.price,
                                        order.client_id.as_ref().unwrap_or(&"".to_string())
                                    );
                                    t1_pending.push((symbol.clone(), order_id));
                                    continue; // Don't execute now, wait for next bar
                                }

                                // Execute immediately (intra-candle mode OR entry orders in T+1 mode)
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

                                // Check if position exists BEFORE fill (to detect closes)
                                let had_position_before =
                                    position_manager.get_position(symbol).is_some();
                                let prev_pos = if had_position_before {
                                    // Get raw position (even if qty=0) for trade creation
                                    position_manager.get_position_raw(symbol).cloned()
                                } else {
                                    None
                                };

                                // Update position
                                position_manager.add_fill(fill.clone(), symbol.clone(), order.side);

                                // Check if position closed (had position before, none after)
                                let has_position_after =
                                    position_manager.get_position(symbol).is_some();

                                tracing::trace!(
                                    "{} Fill check: had_before={} has_after={} prev_pos_qty={}",
                                    candle.datetime.format("%Y-%m-%d"),
                                    had_position_before,
                                    has_position_after,
                                    prev_pos.as_ref().map(|p| p.quantity).unwrap_or(0.0)
                                );

                                if had_position_before && !has_position_after {
                                    // Position just closed - create trade
                                    if let Some(closed_pos) = prev_pos {
                                        let trade = self.create_trade_from_position(
                                            &closed_pos,
                                            fill.price,
                                            candle.datetime,
                                        );

                                        // Record win/loss for risk manager
                                        if trade.net_pnl > 0.0 {
                                            self.risk_manager.record_win();
                                        } else {
                                            self.risk_manager.record_loss();
                                        }

                                        tracing::debug!(
                                            "{} TRADE CLOSED {} PnL={:.2}",
                                            candle.datetime.format("%Y-%m-%d"),
                                            symbol,
                                            trade.net_pnl
                                        );

                                        // Clear cached entry levels for closed position
                                        entry_levels.remove(symbol);

                                        // Notify strategy
                                        self.strategy.on_trade_closed(&trade);

                                        trades.push(trade);
                                    }
                                }

                                // Notify strategy of fill
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

                // Get current position AFTER update (clone to allow mutation of manager later)
                let position_data = position_manager.get_position(symbol).cloned();

                // Calculate total value
                if let Some(pos) = &position_data {
                    total_value += pos.quantity * price;

                    // CRITICAL FIX: Use cached stop/target levels from entry time
                    // Main branch stores these at entry in PendingOrder; we cache them here
                    let (stop_price, target_price) =
                        *entry_levels.entry(symbol.clone()).or_insert_with(|| {
                            // First time seeing this position - calculate and cache stop/target
                            // Use entry slice for correct ATR calculation
                            let entry_slice = match primary
                                .binary_search_by_key(&pos.first_entry_time, |c| c.datetime)
                            {
                                Ok(idx) => {
                                    let start = idx.saturating_sub(LOOKBACK - 1);
                                    &primary[start..=idx]
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        "Could not find entry candle for {}, using current slice",
                                        symbol
                                    );
                                    current_slice
                                }
                            };
                            let stop = self
                                .strategy
                                .calculate_stop_loss(entry_slice, pos.average_entry_price);
                            let target = self
                                .strategy
                                .calculate_take_profit(entry_slice, pos.average_entry_price);
                            tracing::debug!(
                                "{} {} ENTRY LEVELS CACHED: stop={:.4} target={:.4}",
                                pos.first_entry_time.format("%Y-%m-%d"),
                                symbol,
                                stop,
                                target
                            );
                            (stop, target)
                        });

                    tracing::trace!(
                        "{} {} position check: entry={:.2} current={:.2} stop={:.2} target={:.2} low={:.2} high={:.2}",
                        candle.datetime.format("%Y-%m-%d"),
                        symbol,
                        pos.average_entry_price,
                        price,
                        stop_price,
                        target_price,
                        candle.low,
                        candle.high
                    );

                    let stopped = match pos.side {
                        Side::Buy => price <= stop_price || candle.low <= stop_price,
                        Side::Sell => price >= stop_price || candle.high >= stop_price,
                    };

                    let target_hit = match pos.side {
                        Side::Buy => candle.high >= target_price,
                        Side::Sell => candle.low <= target_price,
                    };

                    if stopped || target_hit {
                        let reason = if stopped { "Stop" } else { "Target" };
                        let trigger_price = if stopped { stop_price } else { target_price };

                        // Create synthetic order for stop/target execution
                        let mut close_order = Order::new(
                            symbol.clone(),
                            match pos.side {
                                Side::Buy => Side::Sell,
                                Side::Sell => Side::Buy,
                            },
                            crate::oms::types::OrderType::Market,
                            pos.quantity,
                            None,
                            None,
                            crate::oms::types::TimeInForce::GTC,
                            Some(reason.to_string()),
                        );

                        // T+1 mode: Queue for next day execution
                        if self.config.backtest.use_t1_execution {
                            // Store stop/target order for next candle
                            let order_id = close_order.id;
                            orderbooks
                                .entry(symbol.clone())
                                .or_default()
                                .add_order(close_order);

                            t1_pending.push((symbol.clone(), order_id));

                            tracing::info!(
                                "{} {} TRIGGERED (T+1): {} {:?} pos, entry={:.4}, trigger={:.4}, queued for next day",
                                candle.datetime.format("%Y-%m-%d"),
                                symbol,
                                reason,
                                pos.side,
                                pos.average_entry_price,
                                trigger_price
                            );

                            continue; // Don't execute now, wait for next bar
                        }

                        // Intra-candle mode: Execute immediately
                        // Determine execution price (handle gaps)
                        let exec_price = match pos.side {
                            Side::Buy => {
                                if candle.open < trigger_price {
                                    candle.open
                                } else {
                                    trigger_price
                                }
                            }
                            Side::Sell => {
                                if candle.open > trigger_price {
                                    candle.open
                                } else {
                                    trigger_price
                                }
                            }
                        };

                        tracing::info!(
                            "{} {} TRIGGERED: {} {:?} pos, entry={:.4}, trigger={:.4}, exec_before_slip={:.4}, OHLC=[{:.4},{:.4},{:.4},{:.4}]",
                            candle.datetime.format("%Y-%m-%d"),
                            symbol,
                            reason,
                            pos.side,
                            pos.average_entry_price,
                            trigger_price,
                            exec_price,
                            candle.open,
                            candle.high,
                            candle.low,
                            candle.close
                        );

                        // Execute immediate fill with slippage
                        let slippage_factor = if pos.side == Side::Buy {
                            1.0 - self.config.exchange.assumed_slippage
                        } else {
                            1.0 + self.config.exchange.assumed_slippage
                        };

                        let fill = self.execution_engine.execute_fill(
                            &mut close_order,
                            exec_price * slippage_factor,
                            false, // Taker
                            candle.datetime,
                        );

                        // Update cash
                        match close_order.side {
                            Side::Buy => cash -= fill.price * fill.quantity + fill.commission,
                            Side::Sell => cash += fill.price * fill.quantity - fill.commission,
                        }

                        // Update position manager
                        position_manager.add_fill(fill.clone(), symbol.clone(), close_order.side);

                        // Record trade
                        if position_manager.get_position(symbol).is_none() {
                            // CRITICAL: Clear closed position from manager to prevent P&L accumulation
                            position_manager.close_position(symbol);

                            // Clear cached entry levels for closed position
                            entry_levels.remove(symbol);

                            let trade = self.create_trade_from_position(
                                pos, // Use the cloned position data
                                fill.price,
                                candle.datetime,
                            );

                            if trade.net_pnl > 0.0 {
                                self.risk_manager.record_win();
                            } else {
                                self.risk_manager.record_loss();
                            }

                            self.strategy.on_trade_closed(&trade);
                            trades.push(trade);
                        }

                        tracing::debug!(
                            "{} {} closed @ {:.2} ({}) PnL={:.2}",
                            candle.datetime.format("%Y-%m-%d"),
                            symbol,
                            fill.price,
                            reason,
                            trades.last().map(|t| t.net_pnl).unwrap_or(0.0)
                        );

                        // Notify strategy
                        self.strategy.on_order_filled(&fill, pos);

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
                            // Determine execution price (handle gaps)
                            let exec_price = match pos.side {
                                Side::Buy => {
                                    if candle.open < new_stop {
                                        candle.open
                                    } else {
                                        new_stop
                                    }
                                }
                                Side::Sell => {
                                    if candle.open > new_stop {
                                        candle.open
                                    } else {
                                        new_stop
                                    }
                                }
                            };

                            let mut close_order = Order::new(
                                symbol.clone(),
                                match pos.side {
                                    Side::Buy => Side::Sell,
                                    Side::Sell => Side::Buy,
                                },
                                crate::oms::types::OrderType::Market,
                                pos.quantity,
                                None,
                                None,
                                crate::oms::types::TimeInForce::GTC,
                                Some("TrailingStop".to_string()),
                            );

                            let slippage_factor = if pos.side == Side::Buy {
                                1.0 - self.config.exchange.assumed_slippage
                            } else {
                                1.0 + self.config.exchange.assumed_slippage
                            };

                            let fill = self.execution_engine.execute_fill(
                                &mut close_order,
                                exec_price * slippage_factor,
                                false,
                                candle.datetime,
                            );

                            match close_order.side {
                                Side::Buy => cash -= fill.price * fill.quantity + fill.commission,
                                Side::Sell => cash += fill.price * fill.quantity - fill.commission,
                            }

                            position_manager.add_fill(
                                fill.clone(),
                                symbol.clone(),
                                close_order.side,
                            );

                            if position_manager.get_position(symbol).is_none() {
                                // CRITICAL: Clear closed position from manager to prevent P&L accumulation
                                position_manager.close_position(symbol);

                                // Clear cached entry levels for closed position
                                entry_levels.remove(symbol);

                                let trade = self.create_trade_from_position(
                                    pos,
                                    fill.price,
                                    candle.datetime,
                                );

                                if trade.net_pnl > 0.0 {
                                    self.risk_manager.record_win();
                                } else {
                                    self.risk_manager.record_loss();
                                }

                                self.strategy.on_trade_closed(&trade);
                                trades.push(trade);
                            }

                            tracing::debug!(
                                "{} {} trailing stop @ {:.2} PnL={:.2}",
                                candle.datetime.format("%Y-%m-%d"),
                                symbol,
                                fill.price,
                                trades.last().map(|t| t.net_pnl).unwrap_or(0.0)
                            );

                            self.strategy.on_order_filled(&fill, pos);
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
                        position_data.as_ref(),
                        &open_orders,
                        cash,
                        total_value,
                    )
                } else {
                    StrategyContext::single_timeframe(
                        symbol,
                        current_slice,
                        position_data.as_ref(),
                        &open_orders,
                        cash,
                        total_value,
                    )
                };

                // Notify strategy of new bar (to update counters etc)
                self.strategy.on_bar(&ctx);

                // Get orders from strategy
                let order_requests = self.strategy.generate_orders(&ctx);

                if !order_requests.is_empty() {
                    tracing::debug!(
                        "{} {} generated {} orders",
                        candle.datetime.format("%Y-%m-%d"),
                        symbol,
                        order_requests.len()
                    );
                }

                // Process each order request
                for order_req in order_requests {
                    let order = order_req.into_order();
                    let is_entry_order = position_data.is_none();

                    // CRITICAL FIX: Exit orders must be allowed even when trading is halted
                    // Otherwise positions can't close and drawdown stays above threshold
                    if is_entry_order && self.risk_manager.should_halt_trading() {
                        tracing::debug!("Risk manager halted trading - skipping ENTRY order");
                        continue;
                    }

                    // For entry orders: calculate quantity via risk manager
                    // For exit/grid orders: use strategy's specified quantity
                    let mut final_order = if is_entry_order {
                        // Validate with risk manager
                        let position_count = position_manager.open_position_count();

                        if !self.risk_manager.can_open_position_count(position_count) {
                            tracing::debug!(
                                "Max positions reached ({}) - skipping order",
                                position_count
                            );
                            continue;
                        }

                        // Calculate position size based on risk
                        let regime_score = self.strategy.get_regime_score(current_slice);

                        // Get all current positions for portfolio heat calculation
                        let all_positions: Vec<&crate::oms::types::Position> = position_manager
                            .get_all_positions()
                            .map(|(_, p)| p)
                            .collect();

                        let quantity = self.risk_manager.calculate_position_size_with_regime(
                            price,
                            self.strategy.calculate_stop_loss(current_slice, price),
                            &all_positions,
                            regime_score,
                        );

                        if quantity <= 0.0 {
                            tracing::debug!("Risk manager returned zero quantity - skipping order");
                            continue;
                        }

                        // Create order with risk-calculated quantity
                        let mut entry_order = order;
                        entry_order.quantity = quantity;
                        entry_order.remaining_quantity = quantity;

                        // CRITICAL: Cache stop/target at SIGNAL time (now), not at ENTRY time (T+1)
                        // Main branch stores these in PendingOrder at signal time
                        // Using current_slice here matches main branch behavior
                        // NOTE: Only pre-cache if T+1 execution is enabled, otherwise let the
                        // lazy calculation handle it at position creation time
                        if self.config.backtest.use_t1_execution {
                            let stop = self.strategy.calculate_stop_loss(current_slice, price);
                            let target = self.strategy.calculate_take_profit(current_slice, price);
                            entry_levels.insert(symbol.clone(), (stop, target));
                            tracing::debug!(
                                "{} {} ENTRY LEVELS PRE-CACHED at signal: stop={:.4} target={:.4}",
                                candle.datetime.format("%Y-%m-%d"),
                                symbol,
                                stop,
                                target
                            );
                        }

                        entry_order
                    } else {
                        // Exit or grid order - use strategy's quantity as-is
                        order
                    };

                    // GENERIC FIX: Execute Market orders immediately at Close (Market-On-Close)
                    // This matches the behavior of signal-based backtesters and prevents gap slippage
                    if final_order.order_type == crate::oms::types::OrderType::Market {
                        let slippage_factor = match final_order.side {
                            Side::Buy => 1.0 + self.config.exchange.assumed_slippage,
                            Side::Sell => 1.0 - self.config.exchange.assumed_slippage,
                        };
                        let fill_price = price * slippage_factor;

                        // Check if we have enough cash for buy orders (matches main branch)
                        if final_order.side == Side::Buy {
                            let position_value = fill_price * final_order.quantity;
                            let commission = position_value * self.config.exchange.taker_fee;
                            let cash_needed = position_value + commission;
                            if cash < cash_needed {
                                tracing::debug!(
                                    "Insufficient cash: have {:.2}, need {:.2} - skipping order",
                                    cash,
                                    cash_needed
                                );
                                continue;
                            }
                        }

                        let fill = self.execution_engine.execute_fill(
                            &mut final_order,
                            fill_price,
                            false, // Taker
                            candle.datetime,
                        );

                        // Update cash
                        match final_order.side {
                            Side::Buy => cash -= fill.price * fill.quantity + fill.commission,
                            Side::Sell => cash += fill.price * fill.quantity - fill.commission,
                        }

                        // Check if we had a position before this fill
                        let had_position_before = position_manager.get_position(symbol).is_some();
                        let prev_pos = if had_position_before {
                            position_manager.get_position_raw(symbol).cloned()
                        } else {
                            None
                        };

                        // Update position manager
                        position_manager.add_fill(fill.clone(), symbol.clone(), final_order.side);

                        // Check if position closed
                        let has_position_after = position_manager.get_position(symbol).is_some();

                        if had_position_before && !has_position_after {
                            // Position just closed - create trade
                            if let Some(closed_pos) = prev_pos {
                                let trade = self.create_trade_from_position(
                                    &closed_pos,
                                    fill.price,
                                    candle.datetime,
                                );

                                // Record win/loss
                                if trade.net_pnl > 0.0 {
                                    self.risk_manager.record_win();
                                } else {
                                    self.risk_manager.record_loss();
                                }

                                // Clear cached entry levels for closed position
                                entry_levels.remove(symbol);

                                tracing::debug!(
                                    "{} TRADE CLOSED {} PnL={:.2} (Strategy Exit)",
                                    candle.datetime.format("%Y-%m-%d"),
                                    symbol,
                                    trade.net_pnl
                                );

                                self.strategy.on_trade_closed(&trade);
                                trades.push(trade);
                            }
                        }

                        tracing::debug!(
                            "{} FILL {:?} {} @ {:.2} qty={:.4} (MOC)",
                            candle.datetime.format("%Y-%m-%d"),
                            final_order.side,
                            symbol,
                            fill.price,
                            fill.quantity
                        );

                        // Notify strategy
                        if let Some(pos) = position_manager.get_position(symbol) {
                            self.strategy.on_order_filled(&fill, pos);
                        }
                    } else {
                        // Limit/Stop orders go to book for next execution
                        if let Some(orderbook) = orderbooks.get_mut(symbol) {
                            orderbook.add_order(final_order.clone());

                            tracing::debug!(
                                "{} ORDER {:?} {} @ {:.2} qty={:.4} {}",
                                candle.datetime.format("%Y-%m-%d"),
                                final_order.side,
                                symbol,
                                final_order.limit_price.unwrap_or(price),
                                final_order.quantity,
                                if is_entry_order {
                                    "(ENTRY)"
                                } else {
                                    "(EXIT/GRID)"
                                }
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

                // Clear cached entry levels for closed position
                entry_levels.remove(symbol);

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
        // Calculate P&L for the remaining position only (don't double-count realized_pnl from partial fills)
        let pnl = match pos.side {
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

        // Tax calculation (Net Profit model)
        let tax_rate = self.config.tax.tax_rate;
        // Use net profit for tax base (simplified)
        let net_profit = total_wins - total_losses;
        let taxable_gains = if net_profit > 0.0 { net_profit } else { 0.0 };
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
