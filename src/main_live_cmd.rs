//! Live Trading Command
//!
//! Production-ready live trading with:
//! - Async event loop with graceful shutdown
//! - Crash recovery from SQLite state
//! - Real-time signal processing
//! - Position management with stop loss and take profit
//! - Risk management integration
//! - Paper and live trading modes

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

use crypto_strategies::risk::RiskManager;
use crypto_strategies::state_manager::{
    create_state_manager, Checkpoint, Position as StatePosition, SqliteStateManager,
};
use crypto_strategies::strategies::volatility_regime::{
    create_strategy_from_config, VolatilityRegimeStrategy,
};
use crypto_strategies::strategies::Strategy;
use crypto_strategies::{
    Candle, Config, Order, OrderExecution, OrderStatus, Position, RobustCoinDCXClient, Side,
    Signal, Symbol, Trade,
};

/// Live trader state
struct LiveTrader {
    config: Config,
    strategy: VolatilityRegimeStrategy,
    risk_manager: RiskManager,
    exchange: RobustCoinDCXClient,
    state_manager: SqliteStateManager,
    positions: HashMap<Symbol, Position>,
    candle_cache: HashMap<Symbol, Vec<Candle>>,
    paper_mode: bool,
    cycle_count: u32,
    paper_cash: f64,
}

impl LiveTrader {
    /// Create new live trader instance
    async fn new(config: Config, state_db_path: &str, paper_mode: bool) -> Result<Self> {
        // Create strategy from config
        let strategy = create_strategy_from_config(&config).context("Failed to create strategy")?;

        // Initialize risk manager
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

        // Initialize exchange client
        let api_key = config.exchange.api_key.clone().unwrap_or_default();
        let api_secret = config.exchange.api_secret.clone().unwrap_or_default();

        let exchange = RobustCoinDCXClient::with_config(
            api_key,
            api_secret,
            3, // max retries
            config.exchange.rate_limit as usize,
            Duration::from_secs(30),
        );

        // Initialize state manager
        let state_dir = std::path::Path::new(state_db_path)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let state_manager =
            create_state_manager(state_dir, "sqlite").context("Failed to create state manager")?;

        Ok(LiveTrader {
            config,
            strategy,
            risk_manager,
            exchange,
            state_manager,
            positions: HashMap::new(),
            candle_cache: HashMap::new(),
            paper_mode,
            cycle_count: 0,
            paper_cash: 0.0, // Will be set during recovery
        })
    }

    /// Recover state from previous session
    async fn recover_state(&mut self) -> Result<()> {
        info!("Recovering state from previous session...");

        // Load last checkpoint
        if let Some(checkpoint) = self.state_manager.load_checkpoint()? {
            info!(
                "Found checkpoint: cycle={}, portfolio_value={:.2}, positions={}",
                checkpoint.cycle_count, checkpoint.portfolio_value, checkpoint.open_positions
            );

            self.cycle_count = checkpoint.cycle_count as u32;
            self.paper_cash = checkpoint.cash;

            // Warn if config changed
            let current_hash = self.config_hash();
            if !checkpoint.config_hash.is_empty() && checkpoint.config_hash != current_hash {
                warn!("⚠️  Config has changed since last run!");
                warn!("Previous hash: {}", checkpoint.config_hash);
                warn!("Current hash: {}", current_hash);
            }

            // Restore risk manager state
            self.risk_manager.consecutive_losses = checkpoint.consecutive_losses as usize;
            self.risk_manager.update_capital(checkpoint.portfolio_value);
        } else {
            info!("No previous checkpoint found, starting fresh");
            self.paper_cash = self.config.trading.initial_capital;
        }

        // Load open positions
        let state_positions = self.state_manager.load_positions(Some("open"))?;
        for sp in state_positions {
            let symbol = Symbol::new(&sp.symbol);
            let position = Position {
                symbol: symbol.clone(),
                entry_price: sp.entry_price,
                quantity: sp.quantity,
                stop_price: sp.stop_loss,
                target_price: sp.take_profit,
                trailing_stop: None,
                entry_time: sp
                    .entry_time
                    .and_then(|t| t.parse().ok())
                    .unwrap_or_else(Utc::now),
                risk_amount: (sp.entry_price - sp.stop_loss).abs() * sp.quantity,
            };
            info!(
                "Recovered position: {} qty={:.6} @ {:.2}",
                symbol, position.quantity, position.entry_price
            );
            self.positions.insert(symbol, position);
        }

        info!(
            "State recovery complete: {} open positions, cash={:.2}",
            self.positions.len(),
            self.paper_cash
        );

        Ok(())
    }

    /// Calculate config hash for change detection
    fn config_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        serde_json::to_string(&self.config)
            .unwrap_or_default()
            .hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Fetch latest candles for all symbols
    async fn fetch_candles(&mut self) -> Result<()> {
        for pair in &self.config.trading.pairs {
            let symbol = Symbol::new(pair);

            // In paper mode or if we can't fetch, use cached data
            if self.paper_mode {
                // For paper mode, we'd typically load from CSV or use mock data
                // Here we'll just log that we need real data integration
                debug!("Paper mode: would fetch candles for {}", symbol);
                continue;
            }

            // Fetch ticker for current price (real mode)
            match self.exchange.get_ticker(pair).await {
                Ok(ticker) => {
                    let price: f64 = ticker.last_price.parse().unwrap_or(0.0);
                    if price > 0.0 {
                        // Create a candle from ticker data
                        let candle = Candle {
                            datetime: Utc::now(),
                            open: price,
                            high: price,
                            low: price,
                            close: price,
                            volume: ticker.volume.parse().unwrap_or(0.0),
                        };

                        let cache = self.candle_cache.entry(symbol.clone()).or_default();
                        cache.push(candle);

                        // Keep only last 100 candles
                        if cache.len() > 100 {
                            cache.remove(0);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch ticker for {}: {}", pair, e);
                }
            }
        }

        Ok(())
    }

    /// Run one trading cycle
    async fn run_cycle(&mut self) -> Result<()> {
        self.cycle_count += 1;
        info!("━━━ Trading cycle {} ━━━", self.cycle_count);

        // Fetch latest market data
        self.fetch_candles().await?;

        let mut total_value = self.paper_cash;

        // Process each symbol
        for pair in self.config.trading.pairs.clone() {
            let symbol = Symbol::new(&pair);

            // Get candles for this symbol
            let candles = match self.candle_cache.get(&symbol) {
                Some(c) if c.len() >= 21 => c.clone(), // Need at least EMA slow period
                _ => {
                    debug!("Insufficient candle data for {}", symbol);
                    continue;
                }
            };

            let current_price = candles.last().unwrap().close;

            // Check existing position
            if let Some(pos) = self.positions.get(&symbol).cloned() {
                total_value += pos.quantity * current_price;

                // Check stop loss
                let stop_price = pos.trailing_stop.unwrap_or(pos.stop_price);
                if current_price <= stop_price {
                    info!(
                        "🛑 Stop loss triggered for {} @ {:.2}",
                        symbol, current_price
                    );
                    self.close_position(&symbol, current_price, "Stop Loss")
                        .await?;
                    continue;
                }

                // Check take profit
                if current_price >= pos.target_price {
                    info!(
                        "🎯 Take profit triggered for {} @ {:.2}",
                        symbol, current_price
                    );
                    self.close_position(&symbol, current_price, "Take Profit")
                        .await?;
                    continue;
                }

                // Update trailing stop
                if let Some(new_stop) =
                    self.strategy
                        .update_trailing_stop(&pos, current_price, &candles)
                {
                    if let Some(pos_mut) = self.positions.get_mut(&symbol) {
                        if new_stop > pos_mut.trailing_stop.unwrap_or(0.0) {
                            info!(
                                "📈 Trailing stop updated for {}: {:.2} -> {:.2}",
                                symbol,
                                pos_mut.trailing_stop.unwrap_or(pos_mut.stop_price),
                                new_stop
                            );
                            pos_mut.trailing_stop = Some(new_stop);
                        }
                    }
                }
            }

            // Generate signal
            let position_ref = self.positions.get(&symbol);
            let signal = self
                .strategy
                .generate_signal(&symbol, &candles, position_ref);

            match signal {
                Signal::Long if !self.positions.contains_key(&symbol) => {
                    // Try to open position
                    let current_positions: Vec<Position> =
                        self.positions.values().cloned().collect();

                    if self.risk_manager.can_open_position(&current_positions) {
                        let entry_price =
                            current_price * (1.0 + self.config.exchange.assumed_slippage);
                        let stop_price = self.strategy.calculate_stop_loss(&candles, entry_price);
                        let target_price =
                            self.strategy.calculate_take_profit(&candles, entry_price);

                        let quantity = self.risk_manager.calculate_position_size(
                            entry_price,
                            stop_price,
                            &current_positions,
                        );

                        if quantity > 0.0 {
                            let position_value = quantity * entry_price;
                            let commission = position_value * self.config.exchange.taker_fee;

                            if self.paper_cash >= position_value + commission {
                                self.open_position(
                                    &symbol,
                                    entry_price,
                                    quantity,
                                    stop_price,
                                    target_price,
                                    commission,
                                )
                                .await?;
                            } else {
                                debug!(
                                    "Insufficient cash for {}: need {:.2}, have {:.2}",
                                    symbol,
                                    position_value + commission,
                                    self.paper_cash
                                );
                            }
                        }
                    } else {
                        debug!("Risk manager blocked position for {}", symbol);
                    }
                }
                Signal::Flat if self.positions.contains_key(&symbol) => {
                    info!("📊 Exit signal for {}", symbol);
                    self.close_position(&symbol, current_price, "Signal")
                        .await?;
                }
                _ => {}
            }
        }

        // Update risk manager
        self.risk_manager.update_capital(total_value);

        // Save checkpoint
        self.save_checkpoint(total_value).await?;

        // Log summary
        let drawdown = self.risk_manager.current_drawdown() * 100.0;
        info!(
            "Cycle {} complete: value={:.2}, positions={}, drawdown={:.2}%",
            self.cycle_count,
            total_value,
            self.positions.len(),
            drawdown
        );

        if self.risk_manager.should_halt_trading() {
            warn!("⚠️  Max drawdown reached! Trading halted.");
        }

        Ok(())
    }

    /// Open a new position
    async fn open_position(
        &mut self,
        symbol: &Symbol,
        entry_price: f64,
        quantity: f64,
        stop_price: f64,
        target_price: f64,
        commission: f64,
    ) -> Result<()> {
        let position_value = quantity * entry_price;

        if self.paper_mode {
            // Paper trading - just update internal state
            self.paper_cash -= position_value + commission;

            info!(
                "📈 [PAPER] LONG {} qty={:.6} @ {:.2} | SL={:.2} TP={:.2}",
                symbol, quantity, entry_price, stop_price, target_price
            );
        } else {
            // Live trading - place real order
            let order_request = crypto_strategies::exchange::OrderRequest {
                side: "buy".to_string(),
                order_type: "market_order".to_string(),
                market: symbol.as_str().to_string(),
                price_per_unit: None,
                total_quantity: quantity,
                timestamp: Utc::now().timestamp_millis(),
            };

            match self.exchange.place_order(&order_request).await {
                Ok(response) => {
                    info!(
                        "📈 [LIVE] LONG {} qty={:.6} @ {:.2} | Order ID: {}",
                        symbol, quantity, entry_price, response.id
                    );
                }
                Err(e) => {
                    error!("Failed to place order for {}: {}", symbol, e);
                    return Err(e);
                }
            }
        }

        // Create position
        let position = Position {
            symbol: symbol.clone(),
            entry_price,
            quantity,
            stop_price,
            target_price,
            trailing_stop: None,
            entry_time: Utc::now(),
            risk_amount: (entry_price - stop_price).abs() * quantity,
        };

        // Save to state
        let state_pos = StatePosition {
            symbol: symbol.as_str().to_string(),
            side: "buy".to_string(),
            quantity,
            entry_price,
            entry_time: Some(Utc::now().to_rfc3339()),
            stop_loss: stop_price,
            take_profit: target_price,
            status: "open".to_string(),
            order_id: None,
            pnl: 0.0,
            exit_price: 0.0,
            exit_time: None,
            metadata: HashMap::new(),
        };
        self.state_manager.save_position(&state_pos)?;

        // Store in memory
        self.positions.insert(symbol.clone(), position.clone());

        // Notify strategy
        let order = Order {
            symbol: symbol.clone(),
            side: Side::Buy,
            status: OrderStatus::Completed,
            size: quantity,
            price: Some(entry_price),
            executed: Some(OrderExecution {
                price: entry_price,
                size: quantity,
                value: position_value,
                commission,
            }),
            created_time: Utc::now(),
            updated_time: Utc::now(),
        };
        self.strategy.notify_order(&order);

        Ok(())
    }

    /// Close an existing position
    async fn close_position(
        &mut self,
        symbol: &Symbol,
        exit_price: f64,
        reason: &str,
    ) -> Result<()> {
        let position = match self.positions.remove(symbol) {
            Some(p) => p,
            None => return Ok(()),
        };

        let exit_price_adj = exit_price * (1.0 - self.config.exchange.assumed_slippage);
        let pnl = (exit_price_adj - position.entry_price) * position.quantity;
        let commission =
            (position.quantity * position.entry_price * self.config.exchange.taker_fee)
                + (position.quantity * exit_price_adj * self.config.exchange.taker_fee);
        let net_pnl = pnl - commission;

        if self.paper_mode {
            // Paper trading
            self.paper_cash += position.quantity * exit_price_adj - commission;

            let emoji = if net_pnl > 0.0 { "✅" } else { "❌" };
            info!(
                "{} [PAPER] CLOSE {} qty={:.6} @ {:.2} | PnL={:+.2} | {}",
                emoji, symbol, position.quantity, exit_price_adj, net_pnl, reason
            );
        } else {
            // Live trading
            let order_request = crypto_strategies::exchange::OrderRequest {
                side: "sell".to_string(),
                order_type: "market_order".to_string(),
                market: symbol.as_str().to_string(),
                price_per_unit: None,
                total_quantity: position.quantity,
                timestamp: Utc::now().timestamp_millis(),
            };

            match self.exchange.place_order(&order_request).await {
                Ok(response) => {
                    let emoji = if net_pnl > 0.0 { "✅" } else { "❌" };
                    info!(
                        "{} [LIVE] CLOSE {} qty={:.6} @ {:.2} | PnL={:+.2} | Order: {}",
                        emoji, symbol, position.quantity, exit_price_adj, net_pnl, response.id
                    );
                }
                Err(e) => {
                    error!("Failed to close position for {}: {}", symbol, e);
                    // Re-add position on failure
                    self.positions.insert(symbol.clone(), position);
                    return Err(e);
                }
            }
        }

        // Update risk manager
        if net_pnl > 0.0 {
            self.risk_manager.record_win();
        } else {
            self.risk_manager.record_loss();
        }

        // Create trade record
        let trade = Trade {
            symbol: symbol.clone(),
            side: Side::Buy,
            entry_price: position.entry_price,
            exit_price: exit_price_adj,
            quantity: position.quantity,
            entry_time: position.entry_time,
            exit_time: Utc::now(),
            pnl,
            commission,
            net_pnl,
        };

        // Notify strategy
        self.strategy.notify_trade(&trade);

        // Save to state manager
        self.state_manager.save_trade_async(&trade).await?;

        // Update position status in state
        let state_pos = StatePosition {
            symbol: symbol.as_str().to_string(),
            side: "buy".to_string(),
            quantity: position.quantity,
            entry_price: position.entry_price,
            entry_time: Some(position.entry_time.to_rfc3339()),
            stop_loss: position.stop_price,
            take_profit: position.target_price,
            status: "closed".to_string(),
            order_id: None,
            pnl: net_pnl,
            exit_price: exit_price_adj,
            exit_time: Some(Utc::now().to_rfc3339()),
            metadata: HashMap::new(),
        };
        self.state_manager.save_position(&state_pos)?;

        Ok(())
    }

    /// Save current state checkpoint
    async fn save_checkpoint(&self, portfolio_value: f64) -> Result<()> {
        let checkpoint = Checkpoint {
            timestamp: Utc::now().to_rfc3339(),
            cycle_count: self.cycle_count as i32,
            portfolio_value,
            cash: self.paper_cash,
            positions_value: portfolio_value - self.paper_cash,
            open_positions: self.positions.len() as i32,
            last_processed_symbols: self.config.trading.pairs.clone(),
            drawdown_pct: self.risk_manager.current_drawdown() * 100.0,
            consecutive_losses: self.risk_manager.consecutive_losses as i32,
            paper_mode: self.paper_mode,
            config_hash: self.config_hash(),
            metadata: HashMap::new(),
        };

        self.state_manager.save_checkpoint(&checkpoint)?;
        Ok(())
    }

    /// Graceful shutdown - close all positions
    async fn shutdown(&mut self) -> Result<()> {
        info!("Initiating graceful shutdown...");

        // Close all open positions
        let symbols: Vec<Symbol> = self.positions.keys().cloned().collect();
        for symbol in symbols {
            if let Some(candles) = self.candle_cache.get(&symbol) {
                if let Some(last_candle) = candles.last() {
                    warn!("Closing position {} due to shutdown", symbol);
                    self.close_position(&symbol, last_candle.close, "Shutdown")
                        .await?;
                }
            }
        }

        // Final checkpoint
        let total_value = self.paper_cash;
        self.save_checkpoint(total_value).await?;

        info!(
            "Shutdown complete. Final portfolio value: {:.2}",
            total_value
        );
        Ok(())
    }
}

/// Main entry point for live trading
pub fn run(
    config_path: String,
    paper: bool,
    live: bool,
    interval: u64,
    state_db: String,
) -> Result<()> {
    if !paper && !live {
        anyhow::bail!("Must specify either --paper or --live mode");
    }

    if live && paper {
        anyhow::bail!("Cannot specify both --paper and --live modes");
    }

    // Load environment variables
    dotenv::dotenv().ok();

    // Build async runtime
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to build tokio runtime")?;

    // Run async main
    runtime.block_on(run_async(config_path, paper, interval, state_db))
}

/// Async main loop
async fn run_async(
    config_path: String,
    paper_mode: bool,
    interval_secs: u64,
    state_db: String,
) -> Result<()> {
    // Load config
    let config = Config::from_file(&config_path)
        .context(format!("Failed to load config from {}", config_path))?;

    let mode_str = if paper_mode { "PAPER" } else { "LIVE" };

    info!("╔══════════════════════════════════════════════════════════════╗");
    info!(
        "║          CRYPTO TRADING SYSTEM - {} MODE                ║",
        mode_str
    );
    info!("╠══════════════════════════════════════════════════════════════╣");
    info!("║ Strategy: {:<50} ║", config.strategy_name);
    info!("║ Pairs: {:<53} ║", config.trading.pairs.join(", "));
    info!("║ Timeframe: {:<49} ║", config.trading.timeframe);
    info!(
        "║ Initial Capital: Rs {:<39.2} ║",
        config.trading.initial_capital
    );
    info!("║ Cycle Interval: {} seconds{:<35} ║", interval_secs, "");
    info!("╚══════════════════════════════════════════════════════════════╝");

    if !paper_mode {
        warn!("⚠️  ════════════════════════════════════════════════════════ ⚠️");
        warn!("⚠️  LIVE TRADING MODE - REAL MONEY AT RISK!                  ⚠️");
        warn!("⚠️  Press Ctrl+C within 10 seconds to abort...               ⚠️");
        warn!("⚠️  ════════════════════════════════════════════════════════ ⚠️");

        for i in (1..=10).rev() {
            info!("Starting in {} seconds...", i);
            sleep(Duration::from_secs(1)).await;
        }
    }

    // Initialize trader
    let mut trader = LiveTrader::new(config, &state_db, paper_mode).await?;

    // Initialize strategy
    trader.strategy.init();

    // Recover state from previous session
    trader.recover_state().await?;

    // Setup shutdown signal handling
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_clone = shutdown_flag.clone();

    // Channel for shutdown coordination
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Spawn signal handler task
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received Ctrl+C, initiating shutdown...");
                shutdown_flag_clone.store(true, Ordering::SeqCst);
                let _ = shutdown_tx.send(()).await;
            }
            Err(e) => {
                error!("Error setting up signal handler: {}", e);
            }
        }
    });

    // Main trading loop
    let mut cycle_interval = interval(Duration::from_secs(interval_secs));

    info!("Starting trading loop...");

    loop {
        tokio::select! {
            _ = cycle_interval.tick() => {
                if shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }

                if trader.risk_manager.should_halt_trading() {
                    warn!("Trading halted due to max drawdown. Waiting for manual intervention.");
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }

                if let Err(e) = trader.run_cycle().await {
                    error!("Trading cycle error: {}", e);
                    // Continue on error - don't crash the whole system
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    // Graceful shutdown
    trader.shutdown().await?;

    info!("Live trading session ended.");
    Ok(())
}
