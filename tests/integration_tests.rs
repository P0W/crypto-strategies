//! Integration tests for the crypto-strategies system
//!
//! These tests verify that all components work together correctly.

use chrono::{Duration, Utc};

use crypto_strategies::indicators;
use crypto_strategies::risk::RiskManager;
use crypto_strategies::strategies::volatility_regime::{
    VolatilityRegimeConfig, VolatilityRegimeStrategy,
};
use crypto_strategies::strategies::Strategy;
use crypto_strategies::{Candle, Config, Position, Side, Signal, Symbol, Trade};

// =============================================================================
// Test Utilities
// =============================================================================

/// Generate mock candle data for testing
fn generate_mock_candles(count: usize, base_price: f64, volatility: f64) -> Vec<Candle> {
    let mut candles = Vec::with_capacity(count);
    let mut price = base_price;
    let start_time = Utc::now() - Duration::days(count as i64);

    for i in 0..count {
        // Simple random walk
        let change = if i % 3 == 0 {
            volatility
        } else if i % 3 == 1 {
            -volatility * 0.5
        } else {
            volatility * 0.3
        };

        price += change;
        let high = price + volatility * 0.5;
        let low = price - volatility * 0.5;
        let open = price - change * 0.3;
        let close = price;

        candles.push(Candle {
            datetime: start_time + Duration::days(i as i64),
            open,
            high,
            low,
            close,
            volume: 1000.0 + (i as f64 * 10.0),
        });
    }

    candles
}

/// Generate trending candle data (for testing trend-following strategies)
fn generate_trending_candles(count: usize, base_price: f64, trend_strength: f64) -> Vec<Candle> {
    let mut candles = Vec::with_capacity(count);
    let start_time = Utc::now() - Duration::days(count as i64);

    for i in 0..count {
        let price = base_price + (i as f64 * trend_strength);
        let volatility = base_price * 0.02;

        candles.push(Candle {
            datetime: start_time + Duration::days(i as i64),
            open: price - volatility * 0.5,
            high: price + volatility,
            low: price - volatility,
            close: price + volatility * 0.3,
            volume: 1000.0 + (i as f64 * 10.0),
        });
    }

    candles
}

/// Generate compression (low volatility) candle data
#[allow(dead_code)]
fn generate_compression_candles(count: usize, base_price: f64) -> Vec<Candle> {
    let mut candles = Vec::with_capacity(count);
    let start_time = Utc::now() - Duration::days(count as i64);

    for i in 0..count {
        let tiny_change = (i % 2) as f64 * 0.1 - 0.05;
        let price = base_price + tiny_change;

        candles.push(Candle {
            datetime: start_time + Duration::days(i as i64),
            open: price - 0.05,
            high: price + 0.1,
            low: price - 0.1,
            close: price + 0.05,
            volume: 500.0,
        });
    }

    candles
}

// =============================================================================
// Strategy Tests
// =============================================================================

#[test]
fn test_volatility_regime_strategy_creation() {
    let config = VolatilityRegimeConfig::default();
    let _strategy = VolatilityRegimeStrategy::new(config);

    // Strategy should be created without panic - if we reach here, test passed
}

#[test]
fn test_strategy_generates_flat_with_insufficient_data() {
    let config = VolatilityRegimeConfig::default();
    let strategy = VolatilityRegimeStrategy::new(config);

    let symbol = Symbol::new("BTCINR");
    let candles = generate_mock_candles(5, 100.0, 1.0); // Too few candles

    let signal = strategy.generate_signal(&symbol, &candles, None);
    assert_eq!(signal, Signal::Flat);
}

#[test]
fn test_strategy_generates_signal_with_sufficient_data() {
    let config = VolatilityRegimeConfig::default();
    let strategy = VolatilityRegimeStrategy::new(config);

    let symbol = Symbol::new("BTCINR");
    let candles = generate_trending_candles(50, 100.0, 0.5); // Trending up

    let signal = strategy.generate_signal(&symbol, &candles, None);
    // Signal could be Long or Flat depending on conditions
    assert!(matches!(signal, Signal::Long | Signal::Flat));
}

#[test]
fn test_strategy_stop_loss_calculation() {
    let config = VolatilityRegimeConfig::default();
    let strategy = VolatilityRegimeStrategy::new(config);

    let candles = generate_mock_candles(50, 100.0, 2.0);
    let entry_price = 100.0;

    let stop_loss = strategy.calculate_stop_loss(&candles, entry_price);

    // Stop should be below entry
    assert!(stop_loss < entry_price);
    // Stop should be reasonable (not too far)
    assert!(stop_loss > entry_price * 0.8);
}

#[test]
fn test_strategy_take_profit_calculation() {
    let config = VolatilityRegimeConfig::default();
    let strategy = VolatilityRegimeStrategy::new(config);

    let candles = generate_mock_candles(50, 100.0, 2.0);
    let entry_price = 100.0;

    let take_profit = strategy.calculate_take_profit(&candles, entry_price);

    // Target should be above entry
    assert!(take_profit > entry_price);
}

#[test]
fn test_strategy_trailing_stop_not_active_initially() {
    let config = VolatilityRegimeConfig::default();
    let strategy = VolatilityRegimeStrategy::new(config);

    let candles = generate_mock_candles(50, 100.0, 2.0);
    let position = Position {
        symbol: Symbol::new("BTCINR"),
        entry_price: 100.0,
        quantity: 1.0,
        stop_price: 95.0,
        target_price: 110.0,
        trailing_stop: None,
        entry_time: Utc::now(),
        risk_amount: 5.0,
    };

    // Price at entry - trailing should not activate
    let new_stop = strategy.update_trailing_stop(&position, 100.0, &candles);
    assert!(new_stop.is_none());
}

#[test]
fn test_strategy_trailing_stop_activates_at_profit() {
    let config = VolatilityRegimeConfig {
        trailing_activation: 0.5, // Activate at 50% to target
        ..Default::default()
    };

    let strategy = VolatilityRegimeStrategy::new(config);

    let candles = generate_mock_candles(50, 100.0, 2.0);
    let position = Position {
        symbol: Symbol::new("BTCINR"),
        entry_price: 100.0,
        quantity: 1.0,
        stop_price: 95.0,
        target_price: 110.0,
        trailing_stop: None,
        entry_time: Utc::now(),
        risk_amount: 5.0,
    };

    // Price significantly above entry - trailing may activate
    let new_stop = strategy.update_trailing_stop(&position, 108.0, &candles);
    // May or may not activate depending on ATR
    assert!(new_stop.is_none() || new_stop.unwrap() > position.stop_price);
}

// =============================================================================
// Risk Manager Tests
// =============================================================================

#[test]
fn test_risk_manager_creation() {
    let rm = RiskManager::new(
        100_000.0, // initial capital
        0.02,      // risk per trade
        5,         // max positions
        0.30,      // max portfolio heat
        0.20,      // max position pct
        0.20,      // max drawdown
        0.10,      // drawdown warning
        0.15,      // drawdown critical
        0.50,      // drawdown warning multiplier
        0.25,      // drawdown critical multiplier
        3,         // consecutive loss limit
        0.75,      // consecutive loss multiplier
    );

    assert_eq!(rm.initial_capital, 100_000.0);
    assert_eq!(rm.current_capital, 100_000.0);
    assert!(!rm.should_halt_trading());
}

#[test]
fn test_risk_manager_drawdown_calculation() {
    let mut rm = RiskManager::new(
        100_000.0, 0.02, 5, 0.30, 0.20, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
    );

    // No drawdown initially
    assert_eq!(rm.current_drawdown(), 0.0);

    // 10% drawdown
    rm.update_capital(90_000.0);
    assert!((rm.current_drawdown() - 0.10).abs() < 0.001);

    // New high resets drawdown
    rm.update_capital(110_000.0);
    assert_eq!(rm.current_drawdown(), 0.0);
}

#[test]
fn test_risk_manager_halts_at_max_drawdown() {
    let mut rm = RiskManager::new(
        100_000.0, 0.02, 5, 0.30, 0.20, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
    );

    assert!(!rm.should_halt_trading());

    // Exceed max drawdown (20%)
    rm.update_capital(79_000.0);
    assert!(rm.should_halt_trading());
}

#[test]
fn test_risk_manager_position_sizing() {
    // Use higher max_position_pct (0.50) so the risk-based sizing isn't capped
    let rm = RiskManager::new(
        100_000.0, 0.02, 5, 0.30, 0.50, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
    );

    let entry = 100.0;
    let stop = 95.0; // 5 point stop
    let positions = vec![];

    let size = rm.calculate_position_size(entry, stop, &positions);

    // Risk = 100,000 * 0.02 = 2,000
    // Stop distance = 5
    // Size = 2,000 / 5 = 400
    // Max position = 100,000 * 0.50 = 50,000 / 100 = 500 (not capping)
    assert_eq!(size, 400.0);
}

#[test]
fn test_risk_manager_reduces_size_in_drawdown() {
    let mut rm = RiskManager::new(
        100_000.0, 0.02, 5, 0.30, 0.50, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
    );

    let entry = 100.0;
    let stop = 95.0;
    let positions = vec![];

    let size_normal = rm.calculate_position_size(entry, stop, &positions);

    // Enter warning drawdown zone
    rm.update_capital(89_000.0);
    let size_warning = rm.calculate_position_size(entry, stop, &positions);

    // Size should be reduced in drawdown
    assert!(size_warning < size_normal);
}

#[test]
fn test_risk_manager_consecutive_losses() {
    // Use higher max_position_pct so risk-based sizing isn't capped by max position
    let mut rm = RiskManager::new(
        100_000.0, 0.02, 5, 0.30, 0.50, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
    );

    let entry = 100.0;
    let stop = 95.0;
    let positions = vec![];

    let size_initial = rm.calculate_position_size(entry, stop, &positions);

    // Record 3 consecutive losses
    rm.record_loss();
    rm.record_loss();
    rm.record_loss();

    let size_after_losses = rm.calculate_position_size(entry, stop, &positions);

    // Size should be reduced after consecutive losses (multiplier = 0.75)
    // Initial: 400, After: 400 * 0.75 = 300
    assert!(
        size_after_losses < size_initial,
        "Expected size after losses ({}) < initial size ({})",
        size_after_losses,
        size_initial
    );
    assert_eq!(size_after_losses, 300.0);
}

#[test]
fn test_risk_manager_can_open_position() {
    let rm = RiskManager::new(
        100_000.0, 0.02, 2, 0.30, 0.20, 0.20, 0.10, 0.15, 0.50, 0.25, 3, 0.75,
    );

    let empty_positions: Vec<Position> = vec![];
    assert!(rm.can_open_position(&empty_positions));

    // With max positions reached
    let full_positions = vec![
        Position {
            symbol: Symbol::new("BTC"),
            entry_price: 100.0,
            quantity: 1.0,
            stop_price: 95.0,
            target_price: 110.0,
            trailing_stop: None,
            entry_time: Utc::now(),
            risk_amount: 5.0,
        },
        Position {
            symbol: Symbol::new("ETH"),
            entry_price: 100.0,
            quantity: 1.0,
            stop_price: 95.0,
            target_price: 110.0,
            trailing_stop: None,
            entry_time: Utc::now(),
            risk_amount: 5.0,
        },
    ];

    assert!(!rm.can_open_position(&full_positions));
}

// =============================================================================
// Indicator Tests
// =============================================================================

#[test]
fn test_sma_calculation() {
    let values = vec![10.0, 11.0, 12.0, 13.0, 14.0];
    let sma = indicators::sma(&values, 3);

    assert_eq!(sma[0], None);
    assert_eq!(sma[1], None);
    assert_eq!(sma[2], Some(11.0)); // (10+11+12)/3
    assert_eq!(sma[3], Some(12.0)); // (11+12+13)/3
    assert_eq!(sma[4], Some(13.0)); // (12+13+14)/3
}

#[test]
fn test_ema_calculation() {
    let values = vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
    let ema = indicators::ema(&values, 3);

    assert_eq!(ema[0], None);
    assert_eq!(ema[1], None);
    assert!(ema[2].is_some());

    // EMA should be between SMA and latest value
    let ema_val = ema[5].unwrap();
    assert!(ema_val > 12.0 && ema_val < 15.0);
}

#[test]
fn test_rsi_calculation() {
    // Prices going up should give RSI > 50
    let up_values: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
    let rsi_up = indicators::rsi(&up_values, 14);

    if let Some(rsi_val) = rsi_up.last().unwrap() {
        assert!(*rsi_val > 50.0, "RSI should be > 50 in uptrend");
    }

    // Prices going down should give RSI < 50
    let down_values: Vec<f64> = (0..20).map(|i| 100.0 - i as f64).collect();
    let rsi_down = indicators::rsi(&down_values, 14);

    if let Some(rsi_val) = rsi_down.last().unwrap() {
        assert!(*rsi_val < 50.0, "RSI should be < 50 in downtrend");
    }
}

#[test]
fn test_atr_calculation() {
    let high = vec![12.0, 13.0, 14.0, 15.0, 16.0, 15.5, 16.5, 17.0, 16.0, 17.5];
    let low = vec![10.0, 11.0, 12.0, 13.0, 14.0, 14.0, 15.0, 15.5, 14.5, 16.0];
    let close = vec![11.0, 12.0, 13.0, 14.0, 15.0, 15.0, 16.0, 16.5, 15.5, 17.0];

    let atr = indicators::atr(&high, &low, &close, 5);

    // ATR should have values after period warmup
    assert!(atr[4].is_some());

    // ATR should be positive
    assert!(atr[4].unwrap() > 0.0);
}

#[test]
fn test_bollinger_bands() {
    let values = vec![20.0, 21.0, 22.0, 21.5, 20.5, 21.0, 22.5, 23.0, 22.0, 21.5];
    let (upper, middle, lower) = indicators::bollinger_bands(&values, 5, 2.0);

    // After warmup, should have values
    assert!(upper[4].is_some());
    assert!(middle[4].is_some());
    assert!(lower[4].is_some());

    // Upper > Middle > Lower
    assert!(upper[4].unwrap() > middle[4].unwrap());
    assert!(middle[4].unwrap() > lower[4].unwrap());
}

#[test]
fn test_macd_calculation() {
    let values: Vec<f64> = (0..50)
        .map(|i| 100.0 + (i as f64 * 0.5) + (i % 3) as f64)
        .collect();
    let (macd_line, _signal, _histogram) = indicators::macd(&values, 12, 26, 9);

    // MACD should have values after slow period warmup
    assert!(macd_line[40].is_some());
}

#[test]
fn test_stochastic_calculation() {
    let high = vec![10.0, 11.0, 12.0, 13.0, 14.0, 13.5, 14.5, 15.0, 14.0, 15.5];
    let low = vec![9.0, 10.0, 11.0, 12.0, 13.0, 12.5, 13.5, 14.0, 13.0, 14.5];
    let close = vec![9.5, 10.5, 11.5, 12.5, 13.5, 13.0, 14.0, 14.5, 13.5, 15.0];

    let (k, _d) = indicators::stochastic(&high, &low, &close, 5, 3);

    // %K should be between 0 and 100
    if let Some(k_val) = k[5] {
        assert!((0.0..=100.0).contains(&k_val));
    }
}

#[test]
fn test_vwap_calculation() {
    let high = vec![10.0, 11.0, 12.0, 11.0, 10.0];
    let low = vec![9.0, 10.0, 11.0, 10.0, 9.0];
    let close = vec![9.5, 10.5, 11.5, 10.5, 9.5];
    let volume = vec![100.0, 150.0, 200.0, 150.0, 100.0];

    let vwap = indicators::vwap(&high, &low, &close, &volume);

    assert_eq!(vwap.len(), 5);
    // VWAP should be volume-weighted average
    assert!(vwap[4] > 9.0 && vwap[4] < 12.0);
}

#[test]
fn test_obv_calculation() {
    let close = vec![10.0, 11.0, 10.5, 11.5, 11.0];
    let volume = vec![100.0, 150.0, 120.0, 180.0, 90.0];

    let obv = indicators::obv(&close, &volume);

    assert_eq!(obv.len(), 5);
    assert_eq!(obv[0], 100.0); // First OBV = first volume
}

// =============================================================================
// Data Validation Tests
// =============================================================================

#[test]
fn test_candle_validation() {
    use crypto_strategies::data::validate_candles;

    // Valid candles
    let valid_candles = vec![Candle {
        datetime: Utc::now(),
        open: 100.0,
        high: 105.0,
        low: 95.0,
        close: 102.0,
        volume: 1000.0,
    }];

    let result = validate_candles(&valid_candles);
    assert!(result.is_valid());
}

#[test]
fn test_candle_validation_invalid_high_low() {
    use crypto_strategies::data::validate_candles;

    // Invalid: high < low
    let invalid_candles = vec![Candle {
        datetime: Utc::now(),
        open: 100.0,
        high: 90.0, // Invalid
        low: 95.0,
        close: 102.0,
        volume: 1000.0,
    }];

    let result = validate_candles(&invalid_candles);
    assert!(!result.is_valid());
}

// =============================================================================
// Type Tests
// =============================================================================

#[test]
fn test_symbol_creation() {
    let symbol = Symbol::new("BTCINR");
    assert_eq!(symbol.as_str(), "BTCINR");
}

#[test]
fn test_position_unrealized_pnl() {
    let position = Position {
        symbol: Symbol::new("BTCINR"),
        entry_price: 100.0,
        quantity: 10.0,
        stop_price: 95.0,
        target_price: 110.0,
        trailing_stop: None,
        entry_time: Utc::now(),
        risk_amount: 50.0,
    };

    // Price increased 5%
    let pnl = position.unrealized_pnl(105.0);
    assert_eq!(pnl, 50.0); // (105 - 100) * 10
}

#[test]
fn test_trade_return_pct() {
    let trade = Trade {
        symbol: Symbol::new("BTCINR"),
        side: Side::Buy,
        entry_price: 100.0,
        exit_price: 110.0,
        quantity: 1.0,
        entry_time: Utc::now(),
        exit_time: Utc::now(),
        pnl: 10.0,
        commission: 0.2,
        net_pnl: 9.8,
    };

    assert_eq!(trade.return_pct(), 10.0); // (110-100)/100 * 100
}

// =============================================================================
// Config Tests
// =============================================================================

#[test]
fn test_default_config() {
    let config = Config::default();

    assert_eq!(config.strategy_name, "volatility_regime");
    assert!(config.trading.initial_capital > 0.0);
    assert!(config.trading.risk_per_trade > 0.0);
}

#[test]
fn test_trading_config_symbols() {
    let config = Config::default();
    let symbols = config.trading.symbols();

    assert!(!symbols.is_empty());
}

// =============================================================================
// Cache Tests
// =============================================================================

#[test]
fn test_candle_cache() {
    use crypto_strategies::data::CandleCache;

    let mut cache = CandleCache::new(100, 60);
    let symbol = Symbol::new("BTCINR");

    // Initially empty
    assert!(cache.get(&symbol).is_none());

    // Add candle
    let candle = Candle {
        datetime: Utc::now(),
        open: 100.0,
        high: 105.0,
        low: 95.0,
        close: 102.0,
        volume: 1000.0,
    };

    cache.append(&symbol, candle);

    // Now has data
    assert!(cache.get(&symbol).is_some());
    assert_eq!(cache.get(&symbol).unwrap().len(), 1);
}

#[test]
fn test_indicator_cache() {
    let mut cache = indicators::IndicatorCache::new();
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

    // First call calculates
    let sma1 = cache.get_sma(&values, 3).clone();
    // Second call should use cache (same result)
    let sma2 = cache.get_sma(&values, 3).clone();

    assert_eq!(sma1, sma2);
}
