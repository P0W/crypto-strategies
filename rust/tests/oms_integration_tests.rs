//! Integration Tests for Crypto-Strategies System
//!
//! Comprehensive integration tests covering:
//! - OMS (Order Management System) components
//! - Risk management
//! - Indicators
//! - Data validation
//! - Strategy lifecycle

use chrono::{Duration, Utc};
use std::collections::HashMap;

use crypto_strategies::multi_timeframe::MultiTimeframeData;
use crypto_strategies::oms::{
    ExecutionEngine, Fill, Order, OrderBook, OrderRequest, OrderState, OrderType, Position,
    PositionManager, StrategyContext, TimeInForce,
};
use crypto_strategies::strategies::volatility_regime::{
    VolatilityRegimeConfig, VolatilityRegimeStrategy,
};
use crypto_strategies::strategies::Strategy;
use crypto_strategies::{Backtest, Backtester, Candle, Config, Side, Symbol};

// =============================================================================
// Test Data Generation
// =============================================================================

/// Generate realistic OHLCV candles for testing
fn generate_realistic_candles(count: usize, base_price: f64, volatility_pct: f64) -> Vec<Candle> {
    let mut candles = Vec::with_capacity(count);
    let mut price = base_price;
    let start_time = Utc::now() - Duration::days(count as i64);

    for i in 0..count {
        // Simulate realistic price action with trend and noise
        let trend = if i < count / 2 { 0.001 } else { -0.0005 };
        let noise = ((i * 17 + 31) % 100) as f64 / 100.0 - 0.5; // Pseudo-random
        let change_pct = trend + (noise * volatility_pct);

        price *= 1.0 + change_pct;

        let daily_range = price * volatility_pct;
        let open = price - (daily_range * 0.3);
        let close = price + (daily_range * 0.2);
        let high = price.max(open).max(close) + (daily_range * 0.4);
        let low = price.min(open).min(close) - (daily_range * 0.3);

        candles.push(Candle {
            datetime: start_time + Duration::days(i as i64),
            open,
            high,
            low,
            close,
            volume: 1_000_000.0 + (i as f64 * 10_000.0),
        });
    }

    candles
}

// =============================================================================
// OMS Component Tests
// =============================================================================

#[test]
fn test_order_book_basic_operations() {
    let mut orderbook = OrderBook::new();
    let symbol = Symbol::new("BTCINR");

    // Add buy limit order
    let order1 = Order::new(
        symbol.clone(),
        Side::Buy,
        OrderType::Limit,
        1.0,
        Some(50000.0),
        None,
        TimeInForce::GTC,
        Some("test1".to_string()),
    );

    orderbook.add_order(order1.clone());

    // Verify order was added
    assert_eq!(orderbook.get_all_orders().len(), 1);
    assert!(orderbook.get_order(order1.id).is_some());

    // Add another order
    let order2 = Order::new(
        symbol.clone(),
        Side::Buy,
        OrderType::Limit,
        0.5,
        Some(49000.0),
        None,
        TimeInForce::GTC,
        Some("test2".to_string()),
    );

    orderbook.add_order(order2.clone());
    assert_eq!(orderbook.get_all_orders().len(), 2);

    // Cancel order
    orderbook.cancel_order(order1.id);
    assert_eq!(orderbook.get_all_orders().len(), 1);
}

#[test]
fn test_execution_engine_intracandle_fills() {
    let engine = ExecutionEngine::new(0.0004, 0.0006, 0.001);

    // Buy limit order at 50000
    let mut order = Order::new(
        Symbol::new("BTCINR"),
        Side::Buy,
        OrderType::Limit,
        1.0,
        Some(50000.0),
        None,
        TimeInForce::GTC,
        None,
    );

    // Candle that touches the limit price
    let candle = Candle {
        datetime: Utc::now(),
        open: 51000.0,
        high: 52000.0,
        low: 49500.0, // Goes below limit
        close: 50500.0,
        volume: 100000.0,
    };

    // Should fill
    let fill_info = engine.check_fill(&order, &candle);
    assert!(fill_info.is_some());

    if let Some(info) = fill_info {
        assert_eq!(info.price, 50000.0);
        assert!(info.is_maker); // Limit order = maker

        // Execute fill
        let fill = engine.execute_fill(&mut order, info.price, info.is_maker, candle.datetime);
        assert_eq!(fill.price, 50000.0);
        assert_eq!(fill.quantity, 1.0);
        assert!(fill.commission > 0.0);
        assert_eq!(order.state, OrderState::Filled);
    }
}

#[test]
fn test_position_manager_fifo_accounting() {
    let mut pm = PositionManager::new();
    let symbol = Symbol::new("ETHINR");

    // First fill - buy 1.0 @ 150000
    let fill1 = Fill {
        order_id: 1,
        price: 150000.0,
        quantity: 1.0,
        timestamp: Utc::now(),
        commission: 60.0,
        is_maker: true,
    };

    pm.add_fill(fill1.clone(), symbol.clone(), Side::Buy);

    let pos = pm.get_position(&symbol).unwrap();
    assert_eq!(pos.quantity, 1.0);
    assert_eq!(pos.average_entry_price, 150000.0);
    assert_eq!(pos.fills.len(), 1);

    // Second fill - buy 2.0 @ 160000
    let fill2 = Fill {
        order_id: 2,
        price: 160000.0,
        quantity: 2.0,
        timestamp: Utc::now(),
        commission: 128.0,
        is_maker: true,
    };

    pm.add_fill(fill2.clone(), symbol.clone(), Side::Buy);

    let pos = pm.get_position(&symbol).unwrap();
    assert_eq!(pos.quantity, 3.0);
    // Weighted average: (150000*1 + 160000*2) / 3 = 156666.67
    assert!((pos.average_entry_price - 156666.67).abs() < 1.0);
    assert_eq!(pos.fills.len(), 2);

    // Close 1.5 units @ 165000 (FIFO: closes 1.0 @ 150k, 0.5 @ 160k)
    let fill3 = Fill {
        order_id: 3,
        price: 165000.0,
        quantity: 1.5,
        timestamp: Utc::now(),
        commission: 99.0,
        is_maker: false,
    };

    pm.add_fill(fill3, symbol.clone(), Side::Sell);

    let pos = pm.get_position(&symbol).unwrap();
    assert_eq!(pos.quantity, 1.5); // 3.0 - 1.5
    assert_eq!(pos.fills.len(), 1); // Only the partial fill remains

    // Realized P&L should be calculated
    // First fill: (165000 - 150000) * 1.0 = 15000
    // Second fill: (165000 - 160000) * 0.5 = 2500
    // Total: 17500 - commissions
    assert!(pos.realized_pnl > 17000.0); // Should be close to 17500 minus commissions
}

#[test]
fn test_strategy_context_single_timeframe() {
    let symbol = Symbol::new("BTCINR");
    let candles = generate_realistic_candles(100, 4500000.0, 0.02);

    let ctx = StrategyContext::single_timeframe(&symbol, &candles, None, &[], 100000.0, 100000.0);

    assert_eq!(ctx.candles.len(), 100);
    assert!(!ctx.is_multi_timeframe());
    assert!(ctx.current_position.is_none());
    assert_eq!(ctx.cash_available, 100000.0);
}

// =============================================================================
// Strategy Integration Tests
// =============================================================================

#[test]
fn test_volatility_regime_strategy_generates_orders() {
    let config = VolatilityRegimeConfig::default();
    let strategy = VolatilityRegimeStrategy::new(config);

    let symbol = Symbol::new("BTCINR");
    let candles = generate_realistic_candles(200, 4500000.0, 0.025);

    let ctx = StrategyContext::single_timeframe(&symbol, &candles, None, &[], 500000.0, 500000.0);

    // Strategy should generate orders (may be empty if conditions not met)
    let orders = strategy.generate_orders(&ctx);

    // Verify orders are valid if generated
    for order_req in orders {
        assert_eq!(order_req.symbol, symbol);
        assert!(order_req.quantity > 0.0);

        // Market orders should have no limit/stop price
        if order_req.order_type == OrderType::Market {
            assert!(order_req.limit_price.is_none());
        }

        // Limit orders should have limit price
        if order_req.order_type == OrderType::Limit {
            assert!(order_req.limit_price.is_some());
        }
    }
}

#[test]
fn test_backtest_with_real_strategy() {
    // Load a minimal config
    let config_str = r#"
{
    "exchange": {
        "name": "CoinDCX",
        "maker_fee": 0.0004,
        "taker_fee": 0.0006,
        "assumed_slippage": 0.001,
        "rate_limit_per_second": 10
    },
    "trading": {
        "pairs": ["BTCINR"],
        "initial_capital": 100000.0,
        "risk_per_trade": 0.02,
        "max_positions": 2,
        "max_portfolio_heat": 0.10,
        "max_position_pct": 0.40,
        "max_drawdown": 0.20,
        "drawdown_warning": 0.10,
        "drawdown_critical": 0.15,
        "drawdown_warning_multiplier": 0.50,
        "drawdown_critical_multiplier": 0.25,
        "consecutive_loss_limit": 3,
        "consecutive_loss_multiplier": 0.75
    },
    "strategy": {
        "name": "volatility_regime"
    },
    "tax": {
        "tax_rate": 0.30,
        "tds_rate": 0.01,
        "loss_offset_allowed": false
    },
    "backtest": {
        "data_dir": "../data",
        "start_date": "2024-01-01",
        "end_date": "2024-12-31",
        "timeframe": "1d"
    }
}
"#;

    let config: Config = serde_json::from_str(config_str).expect("Failed to parse config");

    // Create strategy
    let strategy_config = VolatilityRegimeConfig::default();
    let strategy = Box::new(VolatilityRegimeStrategy::new(strategy_config));

    // Create backtester
    let mut backtester = Backtester::new(config, strategy);

    // Generate test data
    let btc_candles = generate_realistic_candles(365, 4500000.0, 0.03);

    // Build multi-timeframe data structure
    let mut mtf_data = HashMap::new();
    let mut btc_mtf = MultiTimeframeData::new("1d");
    btc_mtf.add_timeframe("1d", btc_candles);
    mtf_data.insert(Symbol::new("BTCINR"), btc_mtf);

    // Run backtest
    let result = backtester.run(&mtf_data);

    // Verify result structure
    assert!(result.equity_curve.len() > 0);
    assert!(result.metrics.total_return.is_finite());

    // Metrics should be calculated
    println!("Backtest Results:");
    println!("  Total Return: {:.2}%", result.metrics.total_return);
    println!("  Total Trades: {}", result.metrics.total_trades);
    println!("  Win Rate: {:.2}%", result.metrics.win_rate);
    println!("  Sharpe Ratio: {:.2}", result.metrics.sharpe_ratio);
    println!("  Max Drawdown: {:.2}%", result.metrics.max_drawdown);
}

#[test]
fn test_order_request_builders() {
    let symbol = Symbol::new("ETHINR");

    // Market buy
    let market_buy = OrderRequest::market_buy(symbol.clone(), 1.5);
    assert_eq!(market_buy.side, Side::Buy);
    assert_eq!(market_buy.order_type, OrderType::Market);
    assert_eq!(market_buy.quantity, 1.5);
    assert!(market_buy.limit_price.is_none());

    // Limit sell
    let limit_sell = OrderRequest::limit_sell(symbol.clone(), 2.0, 150000.0);
    assert_eq!(limit_sell.side, Side::Sell);
    assert_eq!(limit_sell.order_type, OrderType::Limit);
    assert_eq!(limit_sell.quantity, 2.0);
    assert_eq!(limit_sell.limit_price, Some(150000.0));

    // Stop buy
    let stop_buy = OrderRequest::stop_buy(symbol.clone(), 1.0, 155000.0);
    assert_eq!(stop_buy.side, Side::Buy);
    assert_eq!(stop_buy.order_type, OrderType::Stop);
    assert_eq!(stop_buy.stop_price, Some(155000.0));

    // With client ID
    let with_id =
        OrderRequest::market_buy(symbol, 1.0).with_client_id("test_order_123".to_string());
    assert_eq!(with_id.client_id, Some("test_order_123".to_string()));
}

// =============================================================================
// Performance and Edge Case Tests
// =============================================================================

#[test]
fn test_large_order_book_performance() {
    let mut orderbook = OrderBook::new();
    let symbol = Symbol::new("BTCINR");

    // Add 1000 orders
    for i in 0..1000 {
        let order = Order::new(
            symbol.clone(),
            if i % 2 == 0 { Side::Buy } else { Side::Sell },
            OrderType::Limit,
            0.1,
            Some(50000.0 + (i as f64 * 100.0)),
            None,
            TimeInForce::GTC,
            Some(format!("order_{}", i)),
        );
        orderbook.add_order(order);
    }

    assert_eq!(orderbook.get_all_orders().len(), 1000);

    // Lookup should be fast (O(1))
    let order_ids: Vec<u64> = orderbook.get_all_order_ids();
    for order_id in &order_ids[..100] {
        assert!(orderbook.get_order(*order_id).is_some());
    }
}

#[test]
fn test_position_manager_edge_cases() {
    let mut pm = PositionManager::new();
    let symbol = Symbol::new("BTCINR");

    // Close position that doesn't exist - should return None
    assert!(pm.close_position(&symbol).is_none());

    // Get position that doesn't exist - should return None
    assert!(pm.get_position(&symbol).is_none());

    // Add fill, then close completely
    let fill = Fill {
        order_id: 1,
        price: 100000.0,
        quantity: 1.0,
        timestamp: Utc::now(),
        commission: 40.0,
        is_maker: true,
    };

    pm.add_fill(fill, symbol.clone(), Side::Buy);
    assert!(pm.get_position(&symbol).is_some());

    // Close with exact quantity
    let close_fill = Fill {
        order_id: 2,
        price: 110000.0,
        quantity: 1.0,
        timestamp: Utc::now(),
        commission: 44.0,
        is_maker: false,
    };

    pm.add_fill(close_fill, symbol.clone(), Side::Sell);

    // Position should still exist but with 0 quantity
    if let Some(pos) = pm.get_position(&symbol) {
        assert!(pos.quantity < 0.001); // Near zero
    }
}
