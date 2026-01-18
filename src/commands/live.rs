//! Live Trading Command - Production-Grade OMS Implementation
//!
//! Features:
//! - Ultra-low latency order processing with microsecond timing
//! - Detailed HFT-style logging (timestamps, latencies, fill ratios)
//! - Async event loop with graceful shutdown
//! - Multi-timeframe (MTF) support
//! - OMS-based order lifecycle management
//! - Full long/short position support
//! - Crash recovery from SQLite state
//! - Risk management integration
//! - Paper and live trading modes

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crypto_strategies::coindcx::{
    ClientConfig, CoinDCXClient, OrderRequest as CoinDCXOrderRequest, OrderSide as CoinDCXOrderSide,
};
use crypto_strategies::multi_timeframe::{MultiTimeframeCandles, MultiTimeframeData};
use crypto_strategies::oms::{ExecutionEngine, Fill, OrderBook, PositionManager, StrategyContext};
use crypto_strategies::risk::RiskManager;
use crypto_strategies::state_manager::{
    create_state_manager, Checkpoint, PendingOrder, Position as StatePosition, SqliteStateManager,
};
use crypto_strategies::strategies::{self, Strategy};
use crypto_strategies::{Config, Money, Side, Symbol, Trade};

/// Performance metrics for HFT monitoring
#[derive(Debug, Default)]
struct PerformanceMetrics {
    total_cycles: u64,
    total_orders_placed: u64,
    total_fills: u64,
    total_cancels: u64,
    avg_cycle_latency_us: u64,
    max_cycle_latency_us: u64,
    avg_order_latency_us: u64,
    max_order_latency_us: u64,
    fill_ratio: f64,
}

impl PerformanceMetrics {
    fn update_cycle_latency(&mut self, latency_us: u64) {
        self.total_cycles += 1;
        self.avg_cycle_latency_us =
            (self.avg_cycle_latency_us * (self.total_cycles - 1) + latency_us) / self.total_cycles;
        if latency_us > self.max_cycle_latency_us {
            self.max_cycle_latency_us = latency_us;
        }
    }

    fn record_order(&mut self, latency_us: u64) {
        self.total_orders_placed += 1;
        // Update running average using incremental formula
        self.avg_order_latency_us = (self.avg_order_latency_us * (self.total_orders_placed - 1)
            + latency_us)
            / self.total_orders_placed;
        if latency_us > self.max_order_latency_us {
            self.max_order_latency_us = latency_us;
        }
        self.update_fill_ratio();
    }

    fn record_fill(&mut self) {
        self.total_fills += 1;
        self.update_fill_ratio();
    }

    fn update_fill_ratio(&mut self) {
        if self.total_orders_placed > 0 {
            self.fill_ratio = self.total_fills as f64 / self.total_orders_placed as f64;
        }
    }

    fn log_summary(&self) {
        info!("════════════════════════════════════════════════════════");
        info!("📊 PERFORMANCE METRICS (HFT-Style)");
        info!("════════════════════════════════════════════════════════");
        info!("Cycles processed:      {}", self.total_cycles);
        info!("Orders placed:         {}", self.total_orders_placed);
        info!(
            "Orders filled:         {} ({:.2}% fill ratio)",
            self.total_fills,
            self.fill_ratio * 100.0
        );
        info!("Orders cancelled:      {}", self.total_cancels);
        info!("Avg cycle latency:     {} μs", self.avg_cycle_latency_us);
        info!("Max cycle latency:     {} μs", self.max_cycle_latency_us);
        info!("Avg order latency:     {} μs", self.avg_order_latency_us);
        info!("Max order latency:     {} μs", self.max_order_latency_us);
        if self.max_cycle_latency_us > 10_000 {
            warn!("⚠️  Max latency > 10ms - consider optimization");
        }
        if self.max_order_latency_us > 1_000 {
            warn!("⚠️  Max order latency > 1ms - check order processing");
        }
        info!("════════════════════════════════════════════════════════");
    }
}

/// Info about an order pending on the exchange
#[derive(Debug, Clone)]
struct PendingExchangeOrder {
    symbol: Symbol,
    side: Side,
    quantity: f64,
    limit_price: Option<f64>,
    submitted_at: Instant,
}

/// Live trader state with OMS integration
struct LiveTrader {
    config: Config,
    strategy: Box<dyn Strategy>,
    risk_manager: RiskManager,
    exchange: CoinDCXClient,
    state_manager: SqliteStateManager,

    // OMS components
    orderbooks: HashMap<Symbol, OrderBook>,
    position_manager: PositionManager,
    execution_engine: ExecutionEngine,

    // MTF candle cache
    candle_cache: HashMap<Symbol, MultiTimeframeData>,
    required_timeframes: Vec<String>,
    primary_timeframe: String,

    // Trading state
    paper_mode: bool,
    cycle_count: u32,
    paper_cash: f64,

    // Live trading state (exchange integration)
    /// Pending orders on exchange: exchange_order_id -> order info
    pending_exchange_orders: HashMap<String, PendingExchangeOrder>,
    /// Cached balances from exchange: currency -> available balance
    exchange_balances: HashMap<String, f64>,
    /// Last time balances were synced
    last_balance_sync: Option<Instant>,

    // Stop/Target tracking (matches backtest.rs pattern)
    // Format: (stop_price, target_price) - cached at entry time
    entry_levels: HashMap<Symbol, (f64, f64)>,
    trailing_stops: HashMap<Symbol, f64>,

    // Performance monitoring
    metrics: PerformanceMetrics,
    last_metrics_log: Instant,
}

impl LiveTrader {
    async fn new(config: Config, state_db_path: &str, paper_mode: bool) -> Result<Self> {
        let start = Instant::now();
        info!("⚙️  Initializing trading engine...");

        let strategy = strategies::create_strategy(&config)?;
        info!(
            "✓ Strategy loaded: {} ({} μs)",
            strategy.name(),
            start.elapsed().as_micros()
        );

        let primary_timeframe = config.timeframe();
        let strategy_tfs = strategy.required_timeframes();
        let mut required_timeframes: Vec<String> =
            strategy_tfs.iter().map(|s| s.to_string()).collect();
        if !required_timeframes.contains(&primary_timeframe) {
            required_timeframes.push(primary_timeframe.clone());
        }

        info!(
            "✓ Timeframes: {:?} (primary: {})",
            required_timeframes, primary_timeframe
        );

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
        info!(
            "✓ Risk manager initialized (capital: {:.2})",
            config.trading.initial_capital
        );

        let api_key = config.exchange.api_key.clone().unwrap_or_default();
        let api_secret = config.exchange.api_secret.clone().unwrap_or_default();

        let client_config = ClientConfig::default()
            .with_max_retries(3)
            .with_rate_limit(config.exchange.rate_limit as usize)
            .with_timeout(Duration::from_secs(30));

        let exchange = CoinDCXClient::with_config(api_key, api_secret, client_config);
        info!(
            "✓ Exchange client connected (rate limit: {} req/s)",
            config.exchange.rate_limit
        );

        let state_dir = std::path::Path::new(state_db_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let state_manager = create_state_manager(state_dir, "sqlite")?;
        info!("✓ State manager ready (path: {})", state_db_path);

        let execution_engine = ExecutionEngine::new(
            config.exchange.maker_fee,
            config.exchange.taker_fee,
            config.exchange.assumed_slippage,
        );
        info!(
            "✓ Execution engine configured (maker: {:.4}%, taker: {:.4}%, slippage: {:.4}%)",
            config.exchange.maker_fee * 100.0,
            config.exchange.taker_fee * 100.0,
            config.exchange.assumed_slippage * 100.0
        );

        info!(
            "⚡ Initialization complete ({} μs)",
            start.elapsed().as_micros()
        );

        Ok(LiveTrader {
            config,
            strategy,
            risk_manager,
            exchange,
            state_manager,
            orderbooks: HashMap::new(),
            position_manager: PositionManager::new(),
            execution_engine,
            candle_cache: HashMap::new(),
            required_timeframes,
            primary_timeframe,
            paper_mode,
            cycle_count: 0,
            paper_cash: 0.0,
            pending_exchange_orders: HashMap::new(),
            exchange_balances: HashMap::new(),
            last_balance_sync: None,
            entry_levels: HashMap::new(),
            trailing_stops: HashMap::new(),
            metrics: PerformanceMetrics::default(),
            last_metrics_log: Instant::now(),
        })
    }

    async fn recover_state(&mut self) -> Result<()> {
        let start = Instant::now();
        info!("🔄 Recovering state from previous session...");

        if let Some(checkpoint) = self.state_manager.load_checkpoint()? {
            info!("✓ Checkpoint found:");
            info!("  └─ Cycle: {}", checkpoint.cycle_count);
            info!("  └─ Portfolio Value: {:.2}", checkpoint.portfolio_value);
            info!("  └─ Open Positions: {}", checkpoint.open_positions);
            info!("  └─ Consecutive Losses: {}", checkpoint.consecutive_losses);
            info!("  └─ Cash: {:.2}", checkpoint.cash);

            self.cycle_count = checkpoint.cycle_count as u32;
            self.paper_cash = checkpoint.cash;
            self.risk_manager.consecutive_losses = checkpoint.consecutive_losses as usize;
            self.risk_manager.update_capital(checkpoint.portfolio_value);

            let current_hash = self.config_hash();
            if !checkpoint.config_hash.is_empty() && checkpoint.config_hash != current_hash {
                warn!("⚠️  Config hash mismatch - parameters may have changed!");
                warn!("  └─ Old hash: {}", checkpoint.config_hash);
                warn!("  └─ New hash: {}", current_hash);
            }
        } else {
            info!("ℹ️  No checkpoint found - starting fresh");
            self.paper_cash = self.config.trading.initial_capital;
        }

        let state_positions = self.state_manager.load_positions(Some("open"))?;
        info!("📦 Loading {} open position(s)...", state_positions.len());

        for sp in state_positions {
            let symbol = Symbol::new(&sp.symbol);
            let side = if sp.side == "sell" {
                Side::Sell
            } else {
                Side::Buy
            };

            let fill = Fill::from_f64(
                0,
                sp.entry_price,
                sp.quantity,
                sp.entry_time
                    .and_then(|t| t.parse().ok())
                    .unwrap_or_else(Utc::now),
                0.0,
                true,
            );

            self.position_manager.add_fill(fill, symbol.clone(), side);

            // Restore stop/target levels if saved
            if sp.stop_loss > 0.0 || sp.take_profit > 0.0 {
                self.entry_levels
                    .insert(symbol.clone(), (sp.stop_loss, sp.take_profit));
                info!(
                    "  └─ Restored levels: stop={:.2}, target={:.2}",
                    sp.stop_loss, sp.take_profit
                );
            }

            // Restore trailing stop from metadata if present
            if let Some(trailing) = sp.metadata.get("trailing_stop") {
                if let Some(trailing_val) = trailing.as_f64() {
                    self.trailing_stops.insert(symbol.clone(), trailing_val);
                    info!("  └─ Restored trailing stop: {:.2}", trailing_val);
                }
            }

            info!(
                "  ✓ {} {} {:.6} @ {:.2} (P&L: {:.2})",
                symbol,
                if side == Side::Buy { "LONG " } else { "SHORT" },
                sp.quantity,
                sp.entry_price,
                sp.pnl
            );
        }

        // Load pending orders and restore to orderbooks
        let pending_orders = self.state_manager.load_pending_orders()?;
        if !pending_orders.is_empty() {
            info!("📋 Restoring {} pending order(s)...", pending_orders.len());
            for po in pending_orders {
                let symbol = Symbol::new(&po.symbol);
                let side = if po.side == "sell" {
                    Side::Sell
                } else {
                    Side::Buy
                };
                let order_type = match po.order_type.as_str() {
                    "limit" => crypto_strategies::oms::OrderType::Limit,
                    "stop" => crypto_strategies::oms::OrderType::Stop,
                    "stop_limit" => crypto_strategies::oms::OrderType::StopLimit,
                    _ => crypto_strategies::oms::OrderType::Market,
                };

                let order = crypto_strategies::oms::Order {
                    id: po.order_id.parse().unwrap_or(0),
                    symbol: symbol.clone(),
                    side,
                    order_type,
                    quantity: Money::from_f64(po.quantity),
                    limit_price: po.limit_price.map(Money::from_f64),
                    stop_price: po.stop_price.map(Money::from_f64),
                    filled_quantity: Money::ZERO,
                    remaining_quantity: Money::from_f64(po.quantity),
                    average_fill_price: Money::ZERO,
                    state: crypto_strategies::oms::OrderState::Open,
                    time_in_force: crypto_strategies::oms::TimeInForce::GTC,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    strategy_tag: None,
                    client_id: po.client_id,
                    created_bar_idx: None,
                };

                let orderbook = self.orderbooks.entry(symbol.clone()).or_default();
                orderbook.add_order(order);

                info!(
                    "  ✓ Restored {} {} @ {:?}",
                    po.side.to_uppercase(),
                    symbol,
                    po.limit_price.or(po.stop_price)
                );
            }
            // Clear from DB since they're now in memory
            self.state_manager.clear_pending_orders()?;
        }

        info!(
            "⚡ State recovery complete ({} μs)",
            start.elapsed().as_micros()
        );
        Ok(())
    }

    async fn bootstrap_candles(&mut self, symbol: &Symbol) -> Result<()> {
        use crypto_strategies::Candle;

        let start = Instant::now();
        info!("📥 Bootstrapping historical data for {}...", symbol);

        let mut mtf_data = MultiTimeframeData::new(self.primary_timeframe.clone());

        for tf in &self.required_timeframes {
            let tf_start = Instant::now();
            let raw_candles = self
                .exchange
                .get_candles(symbol.as_str(), tf, Some(500))
                .await?;

            if raw_candles.is_empty() {
                warn!("  ⚠️  No {} candles received for {}", tf, symbol);
                continue;
            }

            // Convert coindcx::Candle to crypto_strategies::Candle
            let candles: Vec<Candle> = raw_candles
                .into_iter()
                .filter_map(|c| c.try_into().ok())
                .collect();

            if candles.is_empty() {
                warn!("  ⚠️  Failed to convert {} candles for {}", tf, symbol);
                continue;
            }

            // Safe access - we checked is_empty above
            let first_ts = candles.first().map(|c| c.datetime);
            let last_ts = candles.last().map(|c| c.datetime);

            if let (Some(first), Some(last)) = (first_ts, last_ts) {
                info!(
                    "  ✓ {} candles: {} bars ({} to {}) [{} μs]",
                    tf,
                    candles.len(),
                    first.format("%Y-%m-%d %H:%M"),
                    last.format("%Y-%m-%d %H:%M"),
                    tf_start.elapsed().as_micros()
                );
            }

            mtf_data.add_timeframe(tf.clone(), candles);
        }

        self.candle_cache.insert(symbol.clone(), mtf_data);
        info!(
            "⚡ Bootstrap complete for {} ({} μs)",
            symbol,
            start.elapsed().as_micros()
        );
        Ok(())
    }

    async fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<()> {
        info!("════════════════════════════════════════════════════════");
        info!("🚀 LIVE TRADING ENGINE STARTED");
        info!("════════════════════════════════════════════════════════");
        info!(
            "Mode:     {}",
            if self.paper_mode {
                "PAPER TRADING"
            } else {
                "LIVE TRADING ⚠️"
            }
        );
        info!("Strategy: {}", self.strategy.name());
        info!("Symbols:  {:?}", self.config.trading.symbols);
        info!("Capital:  {:.2}", self.paper_cash);
        info!("════════════════════════════════════════════════════════");

        // Bootstrap all symbols
        let bootstrap_start = Instant::now();
        for sym in &self.config.trading.symbols.clone() {
            let symbol = Symbol::new(sym);
            self.bootstrap_candles(&symbol).await?;
            self.orderbooks.insert(symbol.clone(), OrderBook::new());
        }
        info!(
            "⚡ All symbols bootstrapped ({} ms)",
            bootstrap_start.elapsed().as_millis()
        );

        // Live mode: sync balances and reconcile positions on startup
        if !self.paper_mode {
            info!("════════════════════════════════════════════════════════");
            info!("🔗 Connecting to exchange for live trading...");
            self.sync_balances()
                .await
                .context("Failed to sync balances on startup")?;
            info!("│  ✓ INR Balance: ₹{:.2}", self.paper_cash);

            self.reconcile_positions()
                .await
                .context("Failed to reconcile positions on startup")?;
            info!("════════════════════════════════════════════════════════");
        }

        // Main event loop
        let poll_secs = self.parse_tf_seconds(&self.primary_timeframe);
        info!("⏱️  Polling interval: {} seconds", poll_secs);
        let mut ticker = interval(Duration::from_secs(poll_secs));

        while !shutdown.load(Ordering::Relaxed) {
            ticker.tick().await;
            let cycle_start = Instant::now();

            self.cycle_count += 1;
            debug!(
                "┌─ Cycle {} started at {}",
                self.cycle_count,
                Utc::now().format("%H:%M:%S%.3f")
            );

            if let Err(e) = self.process_cycle().await {
                error!("│  ❌ Cycle error: {}", e);
            }

            let cycle_latency_us = cycle_start.elapsed().as_micros() as u64;
            self.metrics.update_cycle_latency(cycle_latency_us);

            debug!(
                "└─ Cycle {} complete ({} μs)",
                self.cycle_count, cycle_latency_us
            );

            // Warn if cycle latency is high
            if cycle_latency_us > 5_000_000 {
                // > 5ms
                warn!("⚠️  High cycle latency: {} ms", cycle_latency_us / 1000);
            }

            // Periodic checkpoint
            if self.cycle_count.is_multiple_of(10) {
                let checkpoint_start = Instant::now();
                if let Err(e) = self.save_checkpoint() {
                    error!("Failed to save checkpoint: {}", e);
                } else {
                    debug!(
                        "💾 Checkpoint saved ({} μs)",
                        checkpoint_start.elapsed().as_micros()
                    );
                }
            }

            // Log performance metrics every 5 minutes
            if self.last_metrics_log.elapsed() > Duration::from_secs(300) {
                self.metrics.log_summary();
                self.log_portfolio_status();
                self.last_metrics_log = Instant::now();
            }
        }

        info!("════════════════════════════════════════════════════════");
        info!("🛑 SHUTDOWN SIGNAL RECEIVED");
        info!("════════════════════════════════════════════════════════");
        self.save_checkpoint()?;
        self.metrics.log_summary();
        info!("✓ Live trading stopped gracefully");
        Ok(())
    }

    async fn process_cycle(&mut self) -> Result<()> {
        // Live mode: poll for fills from exchange
        if !self.paper_mode {
            if let Err(e) = self.poll_pending_orders().await {
                warn!("│  ⚠️  Order polling failed: {}", e);
            }

            // Periodic balance sync (every 5 minutes)
            let should_sync = self
                .last_balance_sync
                .is_none_or(|t| t.elapsed() > Duration::from_secs(300));
            if should_sync {
                if let Err(e) = self.sync_balances().await {
                    warn!("│  ⚠️  Balance sync failed: {}", e);
                }
            }
        }

        for sym in &self.config.trading.symbols.clone() {
            let symbol = Symbol::new(sym);

            let update_start = Instant::now();
            if let Err(e) = self.update_candles(&symbol).await {
                warn!("│  ⚠️  Candle update failed for {}: {}", symbol, e);
                continue;
            }
            debug!(
                "│  ✓ Candles updated for {} ({} μs)",
                symbol,
                update_start.elapsed().as_micros()
            );

            let process_start = Instant::now();
            if let Err(e) = self.process_symbol(&symbol).await {
                error!("│  ❌ Symbol processing failed for {}: {}", symbol, e);
            } else {
                debug!(
                    "│  ✓ Processed {} ({} μs)",
                    symbol,
                    process_start.elapsed().as_micros()
                );
            }
        }
        Ok(())
    }

    async fn update_candles(&mut self, symbol: &Symbol) -> Result<()> {
        use crypto_strategies::Candle;

        for tf in &self.required_timeframes.clone() {
            if let Ok(raw_candles) = self
                .exchange
                .get_candles(symbol.as_str(), tf, Some(2))
                .await
            {
                if let Some(latest_raw) = raw_candles.last() {
                    if let Ok(latest) = Candle::try_from(latest_raw.clone()) {
                        if let Some(mtf_data) = self.candle_cache.get_mut(symbol) {
                            if let Some(candles) = mtf_data.get_mut(tf) {
                                // Update last candle or append if new
                                if let Some(last) = candles.last() {
                                    if last.datetime == latest.datetime {
                                        // Update existing candle
                                        if let Some(last_mut) = candles.last_mut() {
                                            *last_mut = latest;
                                        }
                                    } else {
                                        // New candle
                                        candles.push(latest);
                                    }
                                } else {
                                    candles.push(latest);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn process_symbol(&mut self, symbol: &Symbol) -> Result<()> {
        let mtf_data = self.candle_cache.get(symbol).context("MTF missing")?;
        let candles = mtf_data
            .get(&self.primary_timeframe)
            .context("Primary TF missing")?;

        if candles.is_empty() {
            return Ok(());
        }

        let current_candle = match candles.last() {
            Some(c) => c,
            None => return Ok(()), // Safety: already checked is_empty, but be defensive
        };

        // Calculate portfolio value before getting mutable orderbook reference
        // to avoid borrow checker conflicts
        let equity = self.calculate_portfolio_value();
        let cash_available = self.paper_cash;

        let orderbook = match self.orderbooks.get_mut(symbol) {
            Some(ob) => ob,
            None => {
                warn!("│  ⚠️  No orderbook for {} - skipping", symbol);
                return Ok(());
            }
        };

        // Step 1: Check fills (microsecond precision)
        let fill_check_start = Instant::now();
        let mut orders: Vec<_> = orderbook.get_all_orders().into_iter().cloned().collect();
        let initial_order_count = orders.len();

        for order in &mut orders {
            // Live trading passes None for bar_idx - no look-ahead bias concern in real-time
            if let Some(fill_price) = self
                .execution_engine
                .check_fill(order, current_candle, None)
            {
                let fill_latency = fill_check_start.elapsed().as_micros();
                let is_maker = fill_price.is_maker;
                let price = fill_price.price;
                let fill = self.execution_engine.execute_fill(
                    order,
                    price,
                    is_maker,
                    current_candle.datetime,
                );

                self.position_manager
                    .add_fill(fill.clone(), order.symbol.clone(), order.side);
                self.metrics.record_fill();

                if let Some(pos) = self.position_manager.get_position(&order.symbol) {
                    self.strategy.on_order_filled(&fill, pos);
                }

                orderbook.mark_filled(order.id);

                info!(
                    "│  💰 FILL #{} [{}μs latency]",
                    self.metrics.total_fills, fill_latency
                );
                info!("│    └─ Symbol:    {}", order.symbol);
                info!(
                    "│    └─ Side:      {}",
                    if order.side == Side::Buy {
                        "BUY "
                    } else {
                        "SELL"
                    }
                );
                info!("│    └─ Quantity:  {:.6}", fill.quantity);
                info!("│    └─ Price:     {:.2}", fill.price);
                info!(
                    "│    └─ Type:      {}",
                    if is_maker { "MAKER" } else { "TAKER" }
                );
                info!("│    └─ Commission: {:.4}", fill.commission);
                info!(
                    "│    └─ Timestamp:  {}",
                    fill.timestamp.format("%H:%M:%S%.3f")
                );
            }
        }

        let fills_detected =
            self.metrics.total_fills - (self.metrics.total_fills - orders.len() as u64);
        if fills_detected > 0 {
            debug!(
                "│  ✓ Fill detection: {} orders checked, {} filled ({} μs)",
                initial_order_count,
                fills_detected,
                fill_check_start.elapsed().as_micros()
            );
        }

        // Step 2: Check stop loss / take profit / trailing stops
        // This mirrors the backtest.rs logic for production parity
        if let Some(pos) = self.position_manager.get_position(symbol).cloned() {
            let price = current_candle.close;

            // Get or calculate stop/target levels (cached at entry time)
            let (stop_price, target_price) =
                self.entry_levels.entry(symbol.clone()).or_insert_with(|| {
                    let entry = pos.average_entry_price.to_f64();
                    let stop = self.strategy.calculate_stop_loss(candles, entry, pos.side);
                    let target = self
                        .strategy
                        .calculate_take_profit(candles, entry, pos.side);
                    info!(
                        "│  📍 Entry levels cached for {}: stop={:.2}, target={:.2}",
                        symbol, stop, target
                    );
                    (stop, target)
                });
            let stop_price = *stop_price;
            let target_price = *target_price;

            // Update trailing stop if strategy provides one
            if let Some(new_trailing) = self.strategy.update_trailing_stop(&pos, price, candles) {
                let current_stored = self.trailing_stops.get(symbol).copied();
                let best_stop = match current_stored {
                    Some(stored) => new_trailing.max(stored), // Never lower the trailing stop
                    None => new_trailing,
                };
                self.trailing_stops.insert(symbol.clone(), best_stop);
            }

            // Use trailing stop if set, otherwise initial stop
            let active_stop = self
                .trailing_stops
                .get(symbol)
                .copied()
                .unwrap_or(stop_price);

            // Check stop/target hit
            let stopped = match pos.side {
                Side::Buy => price <= active_stop,
                Side::Sell => price >= active_stop,
            };

            let target_hit = match pos.side {
                Side::Buy => current_candle.high >= target_price,
                Side::Sell => current_candle.low <= target_price,
            };

            if stopped || target_hit {
                let reason = if target_hit { "TARGET" } else { "STOP" };
                let trigger_price = if target_hit {
                    target_price
                } else {
                    active_stop
                };

                info!(
                    "│  🎯 {} HIT for {} {:?} @ {:.2} (entry: {:.2})",
                    reason, symbol, pos.side, trigger_price, pos.average_entry_price
                );

                // Create exit order - opposite side to close position
                let exit_order = match pos.side {
                    Side::Buy => crypto_strategies::oms::OrderRequest::market_sell(
                        symbol.clone(),
                        pos.quantity.to_f64(),
                    ),
                    Side::Sell => crypto_strategies::oms::OrderRequest::market_buy(
                        symbol.clone(),
                        pos.quantity.to_f64(),
                    ),
                };

                // Add to orderbook for execution
                let order = exit_order.to_order();
                let exit_side = order.side;
                orderbook.add_order(order.clone());

                info!(
                    "│  📋 EXIT ORDER placed: {} {} @ market",
                    if exit_side == Side::Buy {
                        "BUY"
                    } else {
                        "SELL"
                    },
                    symbol
                );

                // Clear cached levels for this position
                self.entry_levels.remove(symbol);
                self.trailing_stops.remove(symbol);
            }
        }

        // Step 3: Check closed positions
        if let Some(pos) = self.position_manager.get_position(symbol) {
            if pos.quantity.is_zero() && pos.fills.len() > 1 {
                let trade = Trade {
                    symbol: symbol.clone(),
                    side: pos.side,
                    entry_price: pos.average_entry_price,
                    exit_price: Money::from_f64(current_candle.close),
                    quantity: Money::from_f64(pos.total_quantity_traded()),
                    entry_time: pos.entry_time(),
                    exit_time: Utc::now(),
                    pnl: pos.realized_pnl,
                    commission: Money::from_f64(pos.total_commission()),
                    net_pnl: pos.realized_pnl - Money::from_f64(pos.total_commission()),
                };

                self.strategy.on_trade_closed(&trade);

                if trade.net_pnl.is_positive() {
                    self.risk_manager.record_win();
                } else {
                    self.risk_manager.record_loss();
                }

                let return_pct = trade.return_pct();
                info!("│  ✅ TRADE CLOSED");
                info!("│    └─ Symbol:      {}", symbol);
                info!(
                    "│    └─ Side:        {}",
                    if trade.side == Side::Buy {
                        "LONG "
                    } else {
                        "SHORT"
                    }
                );
                info!("│    └─ Entry:       {:.2}", trade.entry_price);
                info!("│    └─ Exit:        {:.2}", trade.exit_price);
                info!("│    └─ Quantity:    {:.6}", trade.quantity);
                info!("│    └─ Gross P&L:   {:.2}", trade.pnl);
                info!("│    └─ Commission:  {:.2}", trade.commission);
                info!(
                    "│    └─ Net P&L:     {:.2} ({:+.2}%)",
                    trade.net_pnl, return_pct
                );
                info!(
                    "│    └─ Duration:    {}",
                    (trade.exit_time - trade.entry_time).num_seconds() / 3600
                );
            }
        }

        // Step 3: Generate orders (strategy logic)
        let strategy_start = Instant::now();
        let mtf_ref = MultiTimeframeCandles::from_data(mtf_data);
        // Collect orders into a Vec<Order> for the slice reference
        let open_orders_vec: Vec<_> = orderbook.get_all_orders().into_iter().cloned().collect();
        let ctx = StrategyContext {
            symbol,
            candles,
            mtf_candles: Some(&mtf_ref),
            current_position: self.position_manager.get_position(symbol),
            open_orders: &open_orders_vec,
            cash_available,
            equity,
            peak_equity: self.risk_manager.peak_capital(),
        };

        let requests = self.strategy.generate_orders(&ctx);
        let strategy_latency = strategy_start.elapsed().as_micros();

        if !requests.is_empty() {
            debug!(
                "│  ⚡ Strategy generated {} order(s) ({} μs)",
                requests.len(),
                strategy_latency
            );
        }

        // Step 4: Validate and place orders
        // Collect live orders separately to avoid borrow checker issues
        let mut placed_count = 0;
        let mut live_orders: Vec<crypto_strategies::oms::Order> = Vec::new();

        for req in requests {
            if self.risk_manager.should_halt_trading() {
                warn!("│  ⛔ Trading halted by risk manager - skipping order");
                break;
            }

            let pos_count = self.position_manager.open_position_count();
            if !self.risk_manager.can_open_position_count(pos_count) {
                warn!(
                    "│  ⛔ Max positions reached ({}) - skipping order",
                    pos_count
                );
                continue;
            }

            let order = req.to_order();

            if self.paper_mode {
                let order_start = Instant::now();
                orderbook.add_order(order.clone());
                let order_latency_us = order_start.elapsed().as_micros() as u64;
                self.metrics.record_order(order_latency_us);
                placed_count += 1;

                info!(
                    "│  📋 ORDER PLACED #{} [{}μs latency]",
                    self.metrics.total_orders_placed, order_latency_us
                );
                info!("│    └─ Symbol:   {}", order.symbol);
                info!(
                    "│    └─ Side:     {}",
                    if order.side == Side::Buy {
                        "BUY "
                    } else {
                        "SELL"
                    }
                );
                info!("│    └─ Type:     {:?}", order.order_type);
                info!("│    └─ Quantity: {:.6}", order.quantity);
                if let Some(price) = order.limit_price {
                    info!("│    └─ Price:    {:.2}", price);
                }
                info!("│    └─ Order ID: {}", order.id);
            } else {
                // Collect for deferred exchange submission
                live_orders.push(order);
            }
        }

        if placed_count > 0 {
            debug!("│  ✓ Placed {} paper order(s)", placed_count);
        }

        // Send live orders to exchange (after orderbook borrow ends)
        let mut live_placed = 0;
        for order in live_orders {
            let order_start = Instant::now();
            match self.send_order_to_exchange(&order).await {
                Ok(exchange_id) => {
                    let order_latency_us = order_start.elapsed().as_micros() as u64;
                    self.metrics.record_order(order_latency_us);
                    live_placed += 1;
                    info!(
                        "│  📋 LIVE ORDER #{} [{}μs latency] exchange_id={}",
                        self.metrics.total_orders_placed, order_latency_us, exchange_id
                    );
                }
                Err(e) => {
                    error!("│  ❌ Failed to place order on exchange: {}", e);
                }
            }
        }

        if live_placed > 0 {
            debug!("│  ✓ Placed {} live order(s) on exchange", live_placed);
        }

        Ok(())
    }

    fn calculate_portfolio_value(&self) -> f64 {
        let mut total = self.paper_cash;
        for (_sym, pos) in self.position_manager.get_all_positions() {
            total += pos.unrealized_pnl.to_f64();
        }
        total
    }

    /// Send order to CoinDCX exchange
    /// Converts internal Order to CoinDCX format and places it
    /// Tracks order in pending_exchange_orders for fill detection
    async fn send_order_to_exchange(
        &mut self,
        order: &crypto_strategies::oms::Order,
    ) -> Result<String> {
        let market = order.symbol.as_str();
        let quantity = order.quantity.to_f64();
        let side = match order.side {
            Side::Buy => CoinDCXOrderSide::Buy,
            Side::Sell => CoinDCXOrderSide::Sell,
        };

        // Create CoinDCX order request
        let order_req = match order.limit_price {
            Some(price) => CoinDCXOrderRequest::limit(side, market, quantity, price.to_f64()),
            None => CoinDCXOrderRequest::market(side, market, quantity),
        }
        .with_client_order_id(format!("strat_{}", order.id));

        info!(
            "│  📤 Sending to exchange: {} {} {:.8} {}",
            if order.side == Side::Buy {
                "BUY"
            } else {
                "SELL"
            },
            market,
            quantity,
            order
                .limit_price
                .map_or("@ MARKET".to_string(), |p| format!("@ {:.2}", p))
        );

        // Place order on exchange
        let response = self
            .exchange
            .place_order(&order_req)
            .await
            .context("Failed to place order on CoinDCX")?;

        // Extract exchange order ID
        let exchange_order_id = response
            .orders
            .first()
            .map(|o| o.id.clone())
            .ok_or_else(|| anyhow::anyhow!("No order ID in exchange response"))?;

        info!("│  ✅ Exchange accepted: order_id={}", exchange_order_id);

        // Track pending order for fill detection
        self.pending_exchange_orders.insert(
            exchange_order_id.clone(),
            PendingExchangeOrder {
                symbol: order.symbol.clone(),
                side: order.side,
                quantity,
                limit_price: order.limit_price.map(|p| p.to_f64()),
                submitted_at: Instant::now(),
            },
        );

        Ok(exchange_order_id)
    }

    /// Sync balances from exchange
    /// Updates paper_cash with actual INR balance in live mode
    async fn sync_balances(&mut self) -> Result<()> {
        let start = Instant::now();
        debug!("💰 Syncing balances from exchange...");

        let balances = self
            .exchange
            .get_balances()
            .await
            .context("Failed to fetch balances from exchange")?;

        self.exchange_balances.clear();
        let mut inr_balance = 0.0;

        for balance in &balances {
            if balance.balance > 0.0 || balance.locked_balance > 0.0 {
                self.exchange_balances
                    .insert(balance.currency.clone(), balance.balance);

                if balance.currency == "INR" {
                    inr_balance = balance.balance;
                }

                debug!(
                    "│  {}: available={:.8}, locked={:.8}",
                    balance.currency, balance.balance, balance.locked_balance
                );
            }
        }

        // In live mode, use actual INR balance
        if !self.paper_mode {
            self.paper_cash = inr_balance;
        }

        self.last_balance_sync = Some(Instant::now());
        debug!(
            "│  ✓ Balance sync complete ({} μs), INR={:.2}",
            start.elapsed().as_micros(),
            inr_balance
        );

        Ok(())
    }

    /// Poll pending orders on exchange and detect fills
    /// Returns number of fills detected
    async fn poll_pending_orders(&mut self) -> Result<usize> {
        if self.pending_exchange_orders.is_empty() {
            return Ok(0);
        }

        let start = Instant::now();
        let mut fills_detected = 0;
        let mut completed_orders: Vec<String> = Vec::new();

        // Check each pending order
        for (exchange_id, pending) in &self.pending_exchange_orders {
            match self.exchange.get_order_status(exchange_id).await {
                Ok(status) => {
                    let status_str = status.status.to_lowercase();

                    if status_str == "filled" {
                        // Order fully filled
                        let fill_price = status.avg_price.unwrap_or(0.0);
                        let filled_qty = status.total_quantity.unwrap_or(pending.quantity);

                        info!(
                            "│  💰 FILL detected: {} {} {:.8} @ {:.2}",
                            if pending.side == Side::Buy {
                                "BUY"
                            } else {
                                "SELL"
                            },
                            pending.symbol,
                            filled_qty,
                            fill_price
                        );

                        // Create fill and add to position manager
                        let fill = Fill::from_f64(
                            0, // Order ID (internal)
                            fill_price,
                            filled_qty,
                            Utc::now(),
                            self.config.exchange.taker_fee * fill_price * filled_qty,
                            false, // taker
                        );

                        self.position_manager
                            .add_fill(fill, pending.symbol.clone(), pending.side);
                        self.metrics.record_fill();
                        fills_detected += 1;
                        completed_orders.push(exchange_id.clone());

                        // Update risk manager based on P&L
                        if let Some(pos) = self.position_manager.get_position(&pending.symbol) {
                            if pos.quantity.is_zero() {
                                // Position closed - record win/loss
                                let pnl = pos.realized_pnl.to_f64();
                                if pnl > 0.0 {
                                    self.risk_manager.record_win();
                                } else {
                                    self.risk_manager.record_loss();
                                }
                            }
                        }
                    } else if status_str == "partially_filled" {
                        // Partial fill - log but keep tracking
                        let remaining = status.remaining_quantity.unwrap_or(0.0);
                        let age_secs = pending.submitted_at.elapsed().as_secs();
                        debug!(
                            "│  ⏳ Partial fill: {} remaining={:.8} (age: {}s)",
                            exchange_id, remaining, age_secs
                        );
                    } else if status_str == "open" || status_str == "init" {
                        // Still pending - check for timeout (warn if > 5 min)
                        let age_secs = pending.submitted_at.elapsed().as_secs();
                        if age_secs > 300 {
                            warn!(
                                "│  ⚠️  Order {} pending for {}s: {} {} {:.8} @ {}",
                                exchange_id,
                                age_secs,
                                if pending.side == Side::Buy {
                                    "BUY"
                                } else {
                                    "SELL"
                                },
                                pending.symbol,
                                pending.quantity,
                                pending
                                    .limit_price
                                    .map_or("MARKET".to_string(), |p| format!("{:.2}", p))
                            );
                        }
                    } else if status_str == "cancelled" || status_str == "rejected" {
                        // Order cancelled/rejected
                        warn!("│  ⚠️  Order {} was {}", exchange_id, status_str);
                        completed_orders.push(exchange_id.clone());
                    }
                    // "open" or "init" - still pending, do nothing
                }
                Err(e) => {
                    warn!("│  ⚠️  Failed to get status for {}: {}", exchange_id, e);
                }
            }
        }

        // Remove completed orders
        for id in completed_orders {
            self.pending_exchange_orders.remove(&id);
        }

        if fills_detected > 0 {
            debug!(
                "│  ✓ Order polling: {} fills detected ({} μs)",
                fills_detected,
                start.elapsed().as_micros()
            );
            // Refresh balances after fills
            let _ = self.sync_balances().await;
        }

        Ok(fills_detected)
    }

    /// Reconcile local positions with exchange on startup
    /// Warns about discrepancies but doesn't auto-fix (safety first)
    async fn reconcile_positions(&mut self) -> Result<()> {
        if self.paper_mode {
            return Ok(()); // Skip in paper mode
        }

        info!("🔍 Reconciling positions with exchange...");

        // Sync balances first
        self.sync_balances().await?;

        // Check each symbol we trade
        for sym in &self.config.trading.symbols.clone() {
            let symbol = Symbol::new(sym);

            // Extract base currency (e.g., "BTC" from "BTCINR")
            let base_currency = if sym.ends_with("INR") {
                &sym[..sym.len() - 3]
            } else if sym.ends_with("USDT") {
                &sym[..sym.len() - 4]
            } else {
                sym.as_str()
            };

            // Check exchange balance
            let exchange_qty = self
                .exchange_balances
                .get(base_currency)
                .copied()
                .unwrap_or(0.0);

            // Check local position
            let local_qty = self
                .position_manager
                .get_position(&symbol)
                .map(|p| p.quantity.to_f64())
                .unwrap_or(0.0);

            // Compare
            let diff = (exchange_qty - local_qty).abs();
            if diff > 0.00000001 {
                // Tolerance for float comparison
                warn!(
                    "│  ⚠️  Position mismatch for {}: exchange={:.8}, local={:.8}",
                    symbol, exchange_qty, local_qty
                );

                if exchange_qty > 0.0 && local_qty == 0.0 {
                    warn!("│     → Exchange has position, local doesn't - may need manual sync");
                } else if local_qty > 0.0 && exchange_qty == 0.0 {
                    warn!("│     → Local has position, exchange doesn't - clearing local state");
                    self.position_manager.close_position(&symbol);
                    self.entry_levels.remove(&symbol);
                    self.trailing_stops.remove(&symbol);
                }
            } else if exchange_qty > 0.0 {
                info!("│  ✓ {} position matches: {:.8}", symbol, exchange_qty);
            }
        }

        // Check for active orders on exchange
        for sym in &self.config.trading.symbols.clone() {
            match self.exchange.get_active_orders(sym).await {
                Ok(orders) if !orders.is_empty() => {
                    info!(
                        "│  📋 {} active orders on exchange for {}",
                        orders.len(),
                        sym
                    );
                    for order in orders {
                        info!(
                            "│     └─ {} {} qty={:?} @ {:?}",
                            order.side.as_deref().unwrap_or("?"),
                            order.id,
                            order.total_quantity,
                            order.price_per_unit
                        );
                    }
                }
                Ok(_) => {} // No active orders
                Err(e) => {
                    warn!("│  ⚠️  Failed to check active orders for {}: {}", sym, e);
                }
            }
        }

        info!("│  ✓ Position reconciliation complete");
        Ok(())
    }

    fn log_portfolio_status(&self) {
        let portfolio_value = self.calculate_portfolio_value();
        let drawdown = self.risk_manager.current_drawdown();
        let consecutive_losses = self.risk_manager.consecutive_losses;

        info!("════════════════════════════════════════════════════════");
        info!("📊 PORTFOLIO STATUS");
        info!("════════════════════════════════════════════════════════");
        info!("Cash:                  {:.2}", self.paper_cash);
        info!("Portfolio Value:       {:.2}", portfolio_value);
        info!("Drawdown:              {:.2}%", drawdown * 100.0);
        info!("Consecutive Losses:    {}", consecutive_losses);
        info!(
            "Open Positions:        {}",
            self.position_manager.open_position_count()
        );
        info!(
            "Trading Status:        {}",
            if self.risk_manager.should_halt_trading() {
                "HALTED ⛔"
            } else {
                "ACTIVE ✓"
            }
        );

        for (symbol, pos) in self.position_manager.get_all_positions() {
            info!(
                "  ├─ {} {} {:.6} @ {:.2} (U-PnL: {:.2})",
                symbol,
                if pos.side == Side::Buy {
                    "LONG "
                } else {
                    "SHORT"
                },
                pos.quantity,
                pos.average_entry_price,
                pos.unrealized_pnl
            );
        }
        info!("════════════════════════════════════════════════════════");
    }

    fn save_checkpoint(&mut self) -> Result<()> {
        use std::collections::HashMap as MetadataMap;

        let value = self.calculate_portfolio_value();
        let positions_value = value - self.paper_cash;

        let checkpoint = Checkpoint {
            timestamp: Utc::now().to_rfc3339(),
            cycle_count: self.cycle_count as i32,
            portfolio_value: value,
            cash: self.paper_cash,
            positions_value,
            open_positions: self.position_manager.open_position_count() as i32,
            last_processed_symbols: self.config.trading.symbols.clone(),
            drawdown_pct: self.risk_manager.current_drawdown(),
            consecutive_losses: self.risk_manager.consecutive_losses as i32,
            paper_mode: self.paper_mode,
            config_hash: self.config_hash(),
            metadata: MetadataMap::new(),
        };

        self.state_manager.save_checkpoint(&checkpoint)?;

        for (symbol, pos) in self.position_manager.get_all_positions() {
            // Get cached stop/target levels if available
            let (stop_loss, take_profit) =
                self.entry_levels.get(symbol).copied().unwrap_or((0.0, 0.0));

            // Use trailing stop if set, otherwise initial stop
            let active_stop = self
                .trailing_stops
                .get(symbol)
                .copied()
                .unwrap_or(stop_loss);

            let mut metadata = MetadataMap::new();
            // Persist trailing stop in metadata for recovery
            if let Some(&trailing) = self.trailing_stops.get(symbol) {
                metadata.insert("trailing_stop".to_string(), serde_json::json!(trailing));
            }

            let sp = StatePosition {
                symbol: symbol.to_string(),
                side: if pos.side == Side::Buy { "buy" } else { "sell" }.to_string(),
                entry_price: pos.average_entry_price.to_f64(),
                quantity: pos.quantity.to_f64(),
                stop_loss: active_stop,
                take_profit,
                status: "open".to_string(),
                order_id: None,
                pnl: pos.unrealized_pnl.to_f64(),
                exit_price: 0.0,
                entry_time: Some(pos.entry_time().to_rfc3339()),
                exit_time: None,
                metadata,
            };
            self.state_manager.save_position(&sp)?;
        }

        // Save pending orders from all orderbooks
        self.state_manager.clear_pending_orders()?;
        for (symbol, orderbook) in &self.orderbooks {
            for order in orderbook.get_all_orders() {
                if order.state == crypto_strategies::oms::OrderState::Open
                    || order.state == crypto_strategies::oms::OrderState::PartiallyFilled
                {
                    let po = PendingOrder {
                        order_id: order.id.to_string(),
                        symbol: symbol.to_string(),
                        side: if order.side == Side::Buy {
                            "buy"
                        } else {
                            "sell"
                        }
                        .to_string(),
                        order_type: match order.order_type {
                            crypto_strategies::oms::OrderType::Limit => "limit",
                            crypto_strategies::oms::OrderType::Stop => "stop",
                            crypto_strategies::oms::OrderType::StopLimit => "stop_limit",
                            crypto_strategies::oms::OrderType::Market => "market",
                        }
                        .to_string(),
                        quantity: order.remaining_quantity.to_f64(),
                        limit_price: order.limit_price.map(|p| p.to_f64()),
                        stop_price: order.stop_price.map(|p| p.to_f64()),
                        client_id: order.client_id.clone(),
                    };
                    self.state_manager.save_pending_order(&po)?;
                }
            }
        }

        Ok(())
    }

    fn config_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        serde_json::to_string(&self.config)
            .unwrap_or_default()
            .hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn parse_tf_seconds(&self, tf: &str) -> u64 {
        match tf {
            "1m" => 60,
            "5m" => 300,
            "15m" => 900,
            "1h" => 3600,
            "4h" => 14400,
            "1d" => 86400,
            _ => 3600,
        }
    }
}

pub async fn run(config: Config, state_db_path: String, paper_mode: bool) -> Result<()> {
    let mut trader = LiveTrader::new(config, &state_db_path, paper_mode).await?;
    trader.recover_state().await?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("🛑 Ctrl+C detected - initiating graceful shutdown...");
        shutdown_clone.store(true, Ordering::Relaxed);
    });

    trader.run(shutdown).await
}
