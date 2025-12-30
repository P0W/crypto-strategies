//! Live trading command implementation
//!
//! Production-ready live trading framework with:
//! - SQLite state management for crash recovery  
//! - Position tracking and persistence
//! - Risk management integration
//! - Robust exchange client with retries and circuit breaker
//! - Strategy reuse from backtesting
//! - Graceful shutdown handling

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::time::{sleep, interval};
use tracing::{error, info, warn};

use crypto_strategies::config::Config;
use crypto_strategies::exchange::RobustCoinDCXClient;
use crypto_strategies::risk::RiskManager;
use crypto_strategies::state_manager::{SqliteStateManager, Checkpoint};
use crypto_strategies::strategies::Strategy;
use crypto_strategies::types::{Symbol, Signal, Position, Candle, Trade, Side};

pub fn run(
    config_path: String,
    paper: bool,
    live: bool,
    interval_secs: u64,
    state_db: String,
) -> Result<()> {
    if !paper && !live {
        anyhow::bail!("Must specify either --paper or --live mode");
    }

    if live {
        warn!("⚠️  LIVE TRADING MODE - REAL MONEY AT RISK!");
        warn!("Press Ctrl+C within 5 seconds to abort...");
        std::thread::sleep(Duration::from_secs(5));
    }

    // Run async runtime
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        run_async(config_path, paper, live, interval_secs, state_db).await
    })
}

async fn run_async(
    config_path: String,
    paper: bool,
    live: bool,
    interval_secs: u64,
    state_db: String,
) -> Result<()> {
    info!("🚀 Starting live trading engine");
    
    // 1. Load configuration
    let config = Config::from_file(&config_path)
        .context("Failed to load configuration")?;
    
    let mode = if paper { "PAPER" } else { "LIVE" };
    info!("Mode: {} trading", mode);
    info!("Symbols: {:?}", config.symbols);
    info!("Check interval: {}s", interval_secs);
    
    // 2. Initialize state manager with SQLite
    let state_manager = Arc::new(SqliteStateManager::new(&state_db)?);
    info!("✅ SQLite state manager initialized: {}", state_db);
    
    // 3. Create strategy dynamically based on config
    let strategy: Box<dyn Strategy> = match config.strategy_name.as_str() {
        "volatility_regime" => {
            let strategy_config = crypto_strategies::strategies::volatility_regime::create_strategy(&config)?;
            Box::new(strategy_config)
        }
        name => anyhow::bail!("Unknown strategy: {}", name),
    };
    info!("✅ Strategy loaded: {}", config.strategy_name);
    
    // Initialize strategy
    strategy.init();
    
    // 4. Initialize risk manager
    let risk_manager = Arc::new(RiskManager::new(
        config.risk.initial_capital,
        config.risk.max_positions,
        config.risk.position_size,
        config.risk.max_portfolio_heat,
        config.risk.max_drawdown,
    ));
    info!("✅ Risk manager initialized");
    info!("   Initial capital: ${:.2}", config.risk.initial_capital);
    info!("   Max positions: {}", config.risk.max_positions);
    info!("   Position size: {:.1}%", config.risk.position_size * 100.0);
    
    // 5. Connect to exchange with robust client
    let api_key = std::env::var("COINDCX_API_KEY")
        .context("COINDCX_API_KEY not found in environment")?;
    let api_secret = std::env::var("COINDCX_API_SECRET")
        .context("COINDCX_API_SECRET not found in environment")?;
    
    let exchange = Arc::new(RobustCoinDCXClient::new(
        api_key,
        api_secret,
        paper,
    ));
    info!("✅ Exchange client connected");
    
    // 6. Recover positions from state
    let mut positions: HashMap<Symbol, Position> = HashMap::new();
    let recovered_positions = state_manager.get_open_positions()?;
    for pos in recovered_positions {
        positions.insert(pos.symbol.clone(), pos);
    }
    info!("✅ Recovered {} open positions", positions.len());
    
    // 7. Save initial checkpoint
    let checkpoint = Checkpoint {
        timestamp: chrono::Utc::now(),
        cash: config.risk.initial_capital,
        portfolio_value: config.risk.initial_capital,
        open_positions: positions.len() as i32,
    };
    state_manager.save_checkpoint(&checkpoint)?;
    
    // 8. Set up graceful shutdown
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let shutdown_clone = shutdown.clone();
    
    tokio::spawn(async move {
        match signal::ctrl_c().await {
            Ok(()) => {
                warn!("🛑 Ctrl+C received - initiating graceful shutdown");
                shutdown_clone.notify_one();
            }
            Err(err) => {
                error!("Failed to listen for shutdown signal: {}", err);
            }
        }
    });
    
    // 9. Main trading loop
    info!("✅ Entering main trading loop...");
    let mut tick_interval = interval(Duration::from_secs(interval_secs));
    
    loop {
        tokio::select! {
            _ = tick_interval.tick() => {
                // Process trading logic
                if let Err(e) = process_trading_cycle(
                    &config,
                    &*strategy,
                    &exchange,
                    &risk_manager,
                    &state_manager,
                    &mut positions,
                    paper,
                ).await {
                    error!("Error in trading cycle: {}", e);
                }
            }
            _ = shutdown.notified() => {
                info!("Shutting down gracefully...");
                
                // Close all positions in paper mode
                if paper {
                    for (symbol, position) in positions.iter() {
                        info!("Closing position: {:?}", symbol);
                        state_manager.close_position(
                            &position.symbol,
                            position.entry_price,
                            chrono::Utc::now(),
                        )?;
                    }
                }
                
                info!("✅ Shutdown complete");
                break;
            }
        }
    }
    
    Ok(())
}

async fn process_trading_cycle(
    config: &Config,
    strategy: &dyn Strategy,
    exchange: &RobustCoinDCXClient,
    risk_manager: &RiskManager,
    state_manager: &SqliteStateManager,
    positions: &mut HashMap<Symbol, Position>,
    paper: bool,
) -> Result<()> {
    // Fetch latest data for all symbols
    for symbol in &config.symbols {
        // Fetch recent candles (last 100 for indicators)
        let candles = exchange.fetch_ohlcv(symbol, "1d", Some(100)).await?;
        
        if candles.is_empty() {
            warn!("No data for symbol: {:?}", symbol);
            continue;
        }
        
        let current_position = positions.get(symbol);
        
        // Generate signal from strategy
        let signal = strategy.generate_signal(symbol, &candles, current_position);
        
        match signal {
            Signal::Long => {
                // Check if we can open a new position
                if current_position.is_none() {
                    let can_trade = risk_manager.can_open_position();
                    
                    if can_trade {
                        let current_price = candles.last().unwrap().close;
                        let stop_loss = strategy.calculate_stop_loss(&candles, current_price);
                        let take_profit = strategy.calculate_take_profit(&candles, current_price);
                        
                        // Calculate position size
                        let risk_per_trade = config.risk.position_size * risk_manager.get_current_equity();
                        let stop_distance = (current_price - stop_loss).abs();
                        let position_size = if stop_distance > 0.0 {
                            risk_per_trade / stop_distance
                        } else {
                            risk_per_trade / current_price * 0.01 // 1% default
                        };
                        
                        info!("📈 LONG signal for {:?} @ {:.2}", symbol, current_price);
                        info!("   Stop: {:.2}, Target: {:.2}", stop_loss, take_profit);
                        
                        if !paper {
                            // Place real order
                            match exchange.place_order(symbol, "buy", position_size, Some(current_price)).await {
                                Ok(order) => {
                                    info!("✅ Order placed: {:?}", order);
                                    
                                    // Create and save position
                                    let position = Position {
                                        symbol: symbol.clone(),
                                        side: "long".to_string(),
                                        entry_price: current_price,
                                        size: position_size,
                                        stop_loss: Some(stop_loss),
                                        take_profit: Some(take_profit),
                                        entry_time: chrono::Utc::now(),
                                        highest_price: current_price,
                                        lowest_price: current_price,
                                    };
                                    
                                    state_manager.save_position(&position)?;
                                    positions.insert(symbol.clone(), position);
                                    
                                    risk_manager.open_position(symbol.clone(), position_size, current_price);
                                }
                                Err(e) => {
                                    error!("Failed to place order: {}", e);
                                }
                            }
                        } else {
                            // Paper trading
                            info!("📝 [PAPER] Would enter LONG @ {:.2}", current_price);
                            
                            let position = Position {
                                symbol: symbol.clone(),
                                side: "long".to_string(),
                                entry_price: current_price,
                                size: position_size,
                                stop_loss: Some(stop_loss),
                                take_profit: Some(take_profit),
                                entry_time: chrono::Utc::now(),
                                highest_price: current_price,
                                lowest_price: current_price,
                            };
                            
                            state_manager.save_position(&position)?;
                            positions.insert(symbol.clone(), position);
                            risk_manager.open_position(symbol.clone(), position_size, current_price);
                        }
                    }
                }
            }
            Signal::Short => {
                // Similar logic for short positions (if supported)
                info!("SHORT signal received for {:?} (not implemented)", symbol);
            }
            Signal::Exit => {
                // Close existing position
                if let Some(position) = positions.get(symbol) {
                    let current_price = candles.last().unwrap().close;
                    
                    info!("📉 EXIT signal for {:?} @ {:.2}", symbol, current_price);
                    
                    if !paper {
                        // Place real exit order
                        match exchange.place_order(symbol, "sell", position.size, Some(current_price)).await {
                            Ok(_) => {
                                info!("✅ Exit order placed");
                                close_position_internal(symbol, position, current_price, state_manager, risk_manager, positions, strategy)?;
                            }
                            Err(e) => {
                                error!("Failed to place exit order: {}", e);
                            }
                        }
                    } else {
                        // Paper trading exit
                        info!("📝 [PAPER] Would exit @ {:.2}", current_price);
                        close_position_internal(symbol, position, current_price, state_manager, risk_manager, positions, strategy)?;
                    }
                }
            }
            Signal::Hold => {
                // Check for stop loss / take profit
                if let Some(position) = positions.get(symbol) {
                    let current_price = candles.last().unwrap().close;
                    
                    // Update trailing stop
                    if let Some(new_stop) = strategy.update_trailing_stop(position, current_price, &candles) {
                        let mut updated_pos = position.clone();
                        updated_pos.stop_loss = Some(new_stop);
                        positions.insert(symbol.clone(), updated_pos);
                    }
                    
                    // Check stop loss
                    if let Some(stop) = position.stop_loss {
                        if current_price <= stop {
                            info!("🛑 Stop loss hit for {:?} @ {:.2}", symbol, current_price);
                            close_position_internal(symbol, position, current_price, state_manager, risk_manager, positions, strategy)?;
                            continue;
                        }
                    }
                    
                    // Check take profit
                    if let Some(target) = position.take_profit {
                        if current_price >= target {
                            info!("🎯 Take profit hit for {:?} @ {:.2}", symbol, current_price);
                            close_position_internal(symbol, position, current_price, state_manager, risk_manager, positions, strategy)?;
                            continue;
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}

fn close_position_internal(
    symbol: &Symbol,
    position: &Position,
    exit_price: f64,
    state_manager: &SqliteStateManager,
    risk_manager: &RiskManager,
    positions: &mut HashMap<Symbol, Position>,
    strategy: &dyn Strategy,
) -> Result<()> {
    let pnl = (exit_price - position.entry_price) * position.size;
    let pnl_pct = ((exit_price - position.entry_price) / position.entry_price) * 100.0;
    
    info!("Position closed: P&L = ${:.2} ({:.2}%)", pnl, pnl_pct);
    
    // Save to state
    state_manager.close_position(symbol, exit_price, chrono::Utc::now())?;
    
    // Update risk manager
    risk_manager.close_position(symbol);
    
    // Notify strategy
    let trade = Trade {
        symbol: symbol.clone(),
        entry_time: position.entry_time,
        entry_price: position.entry_price,
        exit_time: chrono::Utc::now(),
        exit_price,
        size: position.size,
        side: position.side.clone(),
        pnl,
        pnl_pct,
        commission: 0.0,
        executions: vec![
            OrderExecution {
                price: position.entry_price,
                size: position.size,
                commission: 0.0,
                timestamp: position.entry_time,
            }
        ],
    };
    strategy.notify_trade(&trade);
    
    // Remove from active positions
    positions.remove(symbol);
    
    Ok(())
}
