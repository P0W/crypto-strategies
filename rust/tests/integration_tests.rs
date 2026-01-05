//! Integration tests for the crypto-strategies system
//!
//! These tests verify that all components work together correctly.

use chrono::{Duration, Utc};

use crypto_strategies::indicators;
use crypto_strategies::risk::RiskManager;
use crypto_strategies::strategies::regime_grid::{RegimeGridConfig, RegimeGridStrategy};
use crypto_strategies::strategies::volatility_regime::{
    VolatilityRegimeConfig, VolatilityRegimeStrategy,
};
use crypto_strategies::strategies::Strategy;
use crypto_strategies::{Candle, Position, Side, Signal, Symbol, Trade};

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
        side: Side::Buy,
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
        side: Side::Buy,
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
            side: Side::Buy,
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
            side: Side::Buy,
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
        side: Side::Buy,
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
fn test_short_position_unrealized_pnl() {
    let position = Position {
        symbol: Symbol::new("BTCINR"),
        side: Side::Sell,
        entry_price: 100.0,
        quantity: 10.0,
        stop_price: 105.0,  // Stop is higher for shorts
        target_price: 90.0, // Target is lower for shorts
        trailing_stop: None,
        entry_time: Utc::now(),
        risk_amount: 50.0,
    };

    // Price goes down - profit for short
    let current_price = 95.0;
    let pnl = position.unrealized_pnl(current_price);
    assert_eq!(pnl, 50.0); // (100 - 95) * 10 = 50

    // Price goes up - loss for short
    let current_price = 105.0;
    let pnl = position.unrealized_pnl(current_price);
    assert_eq!(pnl, -50.0); // (100 - 105) * 10 = -50
}

#[test]
fn test_position_side_field() {
    let long_position = Position {
        symbol: Symbol::new("BTCINR"),
        side: Side::Buy,
        entry_price: 100.0,
        quantity: 10.0,
        stop_price: 95.0,
        target_price: 110.0,
        trailing_stop: None,
        entry_time: Utc::now(),
        risk_amount: 50.0,
    };

    assert_eq!(long_position.side, Side::Buy);

    let short_position = Position {
        symbol: Symbol::new("BTCINR"),
        side: Side::Sell,
        entry_price: 100.0,
        quantity: 10.0,
        stop_price: 105.0,
        target_price: 90.0,
        trailing_stop: None,
        entry_time: Utc::now(),
        risk_amount: 50.0,
    };

    assert_eq!(short_position.side, Side::Sell);
}

#[test]
fn test_trade_return_pct() {
    // Test Long trade
    let long_trade = Trade {
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

    assert_eq!(long_trade.return_pct(), 10.0); // (110-100)/100 * 100

    // Test Short trade (profit when price goes down)
    let short_trade = Trade {
        symbol: Symbol::new("BTCINR"),
        side: Side::Sell,
        entry_price: 100.0,
        exit_price: 90.0,
        quantity: 1.0,
        entry_time: Utc::now(),
        exit_time: Utc::now(),
        pnl: 10.0,
        commission: 0.2,
        net_pnl: 9.8,
    };

    assert_eq!(short_trade.return_pct(), 10.0); // (100-90)/100 * 100
}

// =============================================================================
// Config Tests
// =============================================================================

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

// =============================================================================
// Data Download & Indicator Processing Tests
// =============================================================================

/// RAII guard to ensure temp directories are cleaned up even on panic
struct TempDirGuard(std::path::PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Test downloading real data from CoinDCX API
/// This test makes actual HTTP requests - it's marked ignore for CI
#[tokio::test]
async fn test_download_real_data() {
    use crypto_strategies::data::CoinDCXDataFetcher;
    use std::env::temp_dir;

    let temp_data_dir = temp_dir().join("crypto_test_data");
    let _guard = TempDirGuard(temp_data_dir.clone()); // Cleanup on drop
    let fetcher = CoinDCXDataFetcher::new(&temp_data_dir);

    // Download a small amount of data (7 days of 1d candles)
    let result = fetcher.download_pair("BTCINR", "1d", 7).await;

    assert!(
        result.is_ok(),
        "Failed to download data: {:?}",
        result.err()
    );

    let filepath = result.unwrap();
    assert!(filepath.exists(), "Downloaded file does not exist");

    // Verify the data can be loaded
    let candles = crypto_strategies::data::load_csv(&filepath);
    assert!(candles.is_ok(), "Failed to load downloaded CSV");

    let candles = candles.unwrap();
    assert!(!candles.is_empty(), "Downloaded data is empty");
    println!("Downloaded {} candles for BTCINR 1d", candles.len());
    // Cleanup handled by TempDirGuard
}

/// Test fetching candles directly (smaller request)
#[tokio::test]
async fn test_fetch_candles() {
    use crypto_strategies::data::CoinDCXDataFetcher;
    use std::env::temp_dir;

    let fetcher = CoinDCXDataFetcher::new(temp_dir());

    // Fetch candles (limit is handled by API, request Some(10) to get limited)
    let result = fetcher.fetch_candles("I-BTC_INR", "1d", Some(10)).await;

    assert!(
        result.is_ok(),
        "Failed to fetch candles: {:?}",
        result.err()
    );

    let candles = result.unwrap();
    assert!(!candles.is_empty(), "No candles returned");

    println!("Fetched {} candles:", candles.len());
    for candle in candles.iter().take(10) {
        println!(
            "  {} O:{:.2} H:{:.2} L:{:.2} C:{:.2} V:{:.0}",
            candle.datetime.format("%Y-%m-%d"),
            candle.open,
            candle.high,
            candle.low,
            candle.close,
            candle.volume
        );
    }
}

/// Test listing available INR pairs from CoinDCX
#[tokio::test]
async fn test_list_inr_pairs() {
    use crypto_strategies::data::CoinDCXDataFetcher;
    use std::env::temp_dir;

    let fetcher = CoinDCXDataFetcher::new(temp_dir());

    let result = fetcher.list_inr_pairs().await;
    assert!(result.is_ok(), "Failed to list pairs: {:?}", result.err());

    let pairs = result.unwrap();
    println!("Available INR pairs ({}):", pairs.len());
    for pair in pairs.iter().take(20) {
        println!("  {}", pair);
    }

    // Should have at least BTC_INR
    assert!(
        pairs.iter().any(|p: &String| p.contains("BTC")),
        "BTC_INR not found in pairs"
    );
}

// =============================================================================
// Indicator Processing Tests with Real Data Structure
// =============================================================================

/// OHLCV data structure for indicator testing
type OhlcvData = (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>);

/// Generate realistic price data for indicator testing
fn generate_realistic_ohlcv(count: usize, start_price: f64) -> OhlcvData {
    let mut opens = Vec::with_capacity(count);
    let mut highs = Vec::with_capacity(count);
    let mut lows = Vec::with_capacity(count);
    let mut closes = Vec::with_capacity(count);
    let mut volumes = Vec::with_capacity(count);

    let mut price = start_price;

    for i in 0..count {
        // Simulate realistic price movement
        let volatility = price * 0.02; // 2% volatility
        let trend = (i as f64 / count as f64 - 0.5) * price * 0.001; // Slight trend

        let change = if i % 5 == 0 {
            volatility * 1.5 // Larger move every 5 bars
        } else if i % 2 == 0 {
            volatility * 0.5
        } else {
            -volatility * 0.3
        } + trend;

        let open = price;
        price += change;
        let close = price;
        let high = open.max(close) + volatility * 0.3;
        let low = open.min(close) - volatility * 0.3;
        let volume = 100000.0 + (i as f64 * 1000.0) + (change.abs() / volatility * 50000.0);

        opens.push(open);
        highs.push(high);
        lows.push(low);
        closes.push(close);
        volumes.push(volume);
    }

    (opens, highs, lows, closes, volumes)
}

#[test]
fn test_moving_averages_on_price_data() {
    let (_, _, _, closes, _) = generate_realistic_ohlcv(100, 50000.0);

    // Test SMA
    let sma_20 = indicators::sma(&closes, 20);
    assert_eq!(sma_20.len(), closes.len());
    assert!(sma_20[19].is_some(), "SMA should have value at period-1");
    assert!(sma_20[18].is_none(), "SMA should be None before period-1");

    // Test EMA
    let ema_12 = indicators::ema(&closes, 12);
    assert_eq!(ema_12.len(), closes.len());
    assert!(ema_12[11].is_some(), "EMA should have value at period-1");

    // EMA should react faster than SMA to recent prices
    let last_sma = sma_20.last().unwrap().unwrap();
    let last_ema = ema_12.last().unwrap().unwrap();
    let last_close = *closes.last().unwrap();

    // Both should be reasonably close to current price
    assert!(
        (last_sma - last_close).abs() / last_close < 0.1,
        "SMA too far from price"
    );
    assert!(
        (last_ema - last_close).abs() / last_close < 0.1,
        "EMA too far from price"
    );

    println!("Last close: {:.2}", last_close);
    println!("SMA(20): {:.2}", last_sma);
    println!("EMA(12): {:.2}", last_ema);
}

#[test]
fn test_volatility_indicators() {
    let (_, highs, lows, closes, _) = generate_realistic_ohlcv(100, 50000.0);

    // Test ATR
    let atr_14 = indicators::atr(&highs, &lows, &closes, 14);
    assert_eq!(atr_14.len(), closes.len());

    // ATR should be positive where defined
    for (i, val) in atr_14.iter().enumerate() {
        if let Some(atr) = val {
            assert!(*atr > 0.0, "ATR should be positive at index {}", i);
        }
    }

    // Test Bollinger Bands
    let (upper, middle, lower) = indicators::bollinger_bands(&closes, 20, 2.0);
    assert_eq!(upper.len(), closes.len());

    // Where defined, upper > middle > lower
    for i in 19..closes.len() {
        if let (Some(u), Some(m), Some(l)) = (upper[i], middle[i], lower[i]) {
            assert!(u > m, "Upper band should be > middle at {}", i);
            assert!(m > l, "Middle should be > lower at {}", i);
        }
    }

    let last_atr = atr_14.last().unwrap().unwrap();
    let last_upper = upper.last().unwrap().unwrap();
    let last_lower = lower.last().unwrap().unwrap();
    let band_width = last_upper - last_lower;

    println!("ATR(14): {:.2}", last_atr);
    println!("Bollinger Band Width: {:.2}", band_width);
    println!("Upper: {:.2}, Lower: {:.2}", last_upper, last_lower);
}

#[test]
fn test_momentum_indicators() {
    let (_, highs, lows, closes, volumes) = generate_realistic_ohlcv(100, 50000.0);

    // Test RSI
    let rsi_14 = indicators::rsi(&closes, 14);
    assert_eq!(rsi_14.len(), closes.len());

    // RSI should be between 0 and 100
    for (i, val) in rsi_14.iter().enumerate() {
        if let Some(rsi) = val {
            assert!(
                *rsi >= 0.0 && *rsi <= 100.0,
                "RSI out of range at {}: {}",
                i,
                rsi
            );
        }
    }

    // Test ADX
    let adx_14 = indicators::adx(&highs, &lows, &closes, 14);
    assert_eq!(adx_14.len(), closes.len());

    // ADX should be between 0 and 100
    for (i, val) in adx_14.iter().enumerate() {
        if let Some(adx) = val {
            assert!(
                *adx >= 0.0 && *adx <= 100.0,
                "ADX out of range at {}: {}",
                i,
                adx
            );
        }
    }

    // Test MACD (returns macd, signal, histogram)
    let (macd_line, signal_line, _histogram) = indicators::macd(&closes, 12, 26, 9);
    assert_eq!(macd_line.len(), closes.len());
    assert_eq!(signal_line.len(), closes.len());

    // Test MFI (Money Flow Index)
    let mfi_14 = indicators::mfi(&highs, &lows, &closes, &volumes, 14);
    assert_eq!(mfi_14.len(), closes.len());

    // MFI should be between 0 and 100
    for (i, val) in mfi_14.iter().enumerate() {
        if let Some(mfi) = val {
            assert!(
                *mfi >= 0.0 && *mfi <= 100.0,
                "MFI out of range at {}: {}",
                i,
                mfi
            );
        }
    }

    let last_rsi = rsi_14.last().unwrap().unwrap();
    let last_adx = adx_14.last().unwrap().unwrap_or(0.0);
    let last_macd = macd_line.last().unwrap().unwrap_or(0.0);
    let last_mfi = mfi_14.last().unwrap().unwrap();

    println!("RSI(14): {:.2}", last_rsi);
    println!("ADX(14): {:.2}", last_adx);
    println!("MACD: {:.2}", last_macd);
    println!("MFI(14): {:.2}", last_mfi);
}

#[test]
fn test_volume_indicators() {
    let (_, highs, lows, closes, volumes) = generate_realistic_ohlcv(100, 50000.0);

    // Test OBV (On Balance Volume)
    let obv = indicators::obv(&closes, &volumes);
    assert_eq!(obv.len(), closes.len());

    // OBV should change with price direction
    let mut increasing = 0;
    let mut decreasing = 0;
    for i in 1..closes.len() {
        if closes[i] > closes[i - 1] && obv[i] > obv[i - 1] {
            increasing += 1;
        } else if closes[i] < closes[i - 1] && obv[i] < obv[i - 1] {
            decreasing += 1;
        }
    }
    assert!(
        increasing + decreasing > closes.len() / 2,
        "OBV should correlate with price direction"
    );

    // Test VWAP
    let vwap = indicators::vwap(&highs, &lows, &closes, &volumes);
    assert_eq!(vwap.len(), closes.len());

    // VWAP should be within high-low range generally
    let last_vwap = vwap.last().unwrap();
    let last_close = *closes.last().unwrap();

    println!("Last OBV: {:.0}", obv.last().unwrap());
    println!("Last VWAP: {:.2}", last_vwap);
    println!("Last Close: {:.2}", last_close);
    println!(
        "OBV correlation: {}/{} price moves matched",
        increasing + decreasing,
        closes.len() - 1
    );
}

#[test]
fn test_combined_strategy_indicators() {
    // Test the indicators used by the Volatility Regime strategy together
    let (_, highs, lows, closes, _) = generate_realistic_ohlcv(100, 50000.0);

    // Volatility Regime uses: ATR, EMA(8), EMA(21), ADX
    let atr = indicators::atr(&highs, &lows, &closes, 14);
    let ema_fast = indicators::ema(&closes, 8);
    let ema_slow = indicators::ema(&closes, 21);
    let adx = indicators::adx(&highs, &lows, &closes, 14);

    // Calculate ATR ratio (current ATR / median ATR over lookback)
    let lookback = 20;
    let atr_values: Vec<f64> = atr.iter().filter_map(|x| *x).collect();
    if atr_values.len() >= lookback {
        let recent_atr = atr_values[atr_values.len() - lookback..].to_vec();
        let mut sorted = recent_atr.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_atr = sorted[sorted.len() / 2];
        let current_atr = *atr_values.last().unwrap();
        let atr_ratio = current_atr / median_atr;

        println!("\n=== Volatility Regime Analysis ===");
        println!("Current ATR: {:.2}", current_atr);
        println!("Median ATR ({}): {:.2}", lookback, median_atr);
        println!("ATR Ratio: {:.2}", atr_ratio);

        // Classify regime
        let regime = if atr_ratio < 0.6 {
            "COMPRESSION"
        } else if atr_ratio < 1.5 {
            "NORMAL"
        } else if atr_ratio < 2.5 {
            "EXPANSION"
        } else {
            "EXTREME"
        };
        println!("Regime: {}", regime);
    }

    // Check trend
    let last_ema_fast = ema_fast.last().unwrap().unwrap();
    let last_ema_slow = ema_slow.last().unwrap().unwrap();
    let trend = if last_ema_fast > last_ema_slow {
        "BULLISH"
    } else {
        "BEARISH"
    };

    // Check trend strength
    let last_adx = adx.last().unwrap().unwrap_or(0.0);
    let strength = if last_adx > 30.0 {
        "STRONG"
    } else if last_adx > 20.0 {
        "MODERATE"
    } else {
        "WEAK"
    };

    println!("EMA Fast(8): {:.2}", last_ema_fast);
    println!("EMA Slow(21): {:.2}", last_ema_slow);
    println!("Trend: {} ({})", trend, strength);
    println!("ADX: {:.2}", last_adx);

    // Strategy would generate signal if:
    // - Regime is COMPRESSION or NORMAL
    // - Trend is BULLISH
    // - ADX > 30
    let would_signal = last_ema_fast > last_ema_slow && last_adx > 30.0;
    println!("Would generate LONG signal: {}", would_signal);
}

/// Integration test: Download data and process with indicators
#[tokio::test]
async fn test_download_and_analyze() {
    use crypto_strategies::data::CoinDCXDataFetcher;
    use std::env::temp_dir;

    let temp_data_dir = temp_dir().join("crypto_test_analysis");
    let _guard = TempDirGuard(temp_data_dir.clone()); // Cleanup on drop
    let fetcher = CoinDCXDataFetcher::new(&temp_data_dir);

    // Download 30 days of BTC data
    println!("Downloading BTCINR 1d data...");
    let result = fetcher.download_pair("BTCINR", "1d", 30).await;

    if let Err(e) = &result {
        println!("Skipping test - download failed: {}", e);
        return;
    }

    let filepath = result.unwrap();
    let candles = crypto_strategies::data::load_csv(&filepath).unwrap();

    println!("\n=== Downloaded {} candles ===", candles.len());
    println!(
        "Date range: {} to {}",
        candles.first().unwrap().datetime.format("%Y-%m-%d"),
        candles.last().unwrap().datetime.format("%Y-%m-%d")
    );

    // Extract OHLCV arrays
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let highs: Vec<f64> = candles.iter().map(|c| c.high).collect();
    let lows: Vec<f64> = candles.iter().map(|c| c.low).collect();
    let volumes: Vec<f64> = candles.iter().map(|c| c.volume).collect();

    // Calculate all major indicators
    let sma_20 = indicators::sma(&closes, 20);
    let ema_12 = indicators::ema(&closes, 12);
    let rsi_14 = indicators::rsi(&closes, 14);
    let atr_14 = indicators::atr(&highs, &lows, &closes, 14);
    let adx_14 = indicators::adx(&highs, &lows, &closes, 14);
    let (bb_upper, bb_middle, bb_lower) = indicators::bollinger_bands(&closes, 20, 2.0);
    let (macd, signal, histogram) = indicators::macd(&closes, 12, 26, 9);
    let obv = indicators::obv(&closes, &volumes);

    // Print analysis for last candle
    let last = candles.last().unwrap();
    println!(
        "\n=== Latest Analysis ({}) ===",
        last.datetime.format("%Y-%m-%d")
    );
    println!(
        "Price: O:{:.2} H:{:.2} L:{:.2} C:{:.2}",
        last.open, last.high, last.low, last.close
    );
    println!("Volume: {:.0}", last.volume);

    println!("\n--- Moving Averages ---");
    if let Some(sma) = sma_20.last().unwrap() {
        println!(
            "SMA(20): {:.2} ({:+.2}%)",
            sma,
            (last.close - sma) / sma * 100.0
        );
    }
    if let Some(ema) = ema_12.last().unwrap() {
        println!(
            "EMA(12): {:.2} ({:+.2}%)",
            ema,
            (last.close - ema) / ema * 100.0
        );
    }

    println!("\n--- Momentum ---");
    if let Some(rsi) = rsi_14.last().unwrap() {
        let rsi_signal = if *rsi > 70.0 {
            "OVERBOUGHT"
        } else if *rsi < 30.0 {
            "OVERSOLD"
        } else {
            "NEUTRAL"
        };
        println!("RSI(14): {:.2} ({})", rsi, rsi_signal);
    }
    if let Some(adx) = adx_14.last().unwrap() {
        let trend_strength = if *adx > 40.0 {
            "VERY STRONG"
        } else if *adx > 25.0 {
            "STRONG"
        } else if *adx > 20.0 {
            "MODERATE"
        } else {
            "WEAK"
        };
        println!("ADX(14): {:.2} ({})", adx, trend_strength);
    }

    println!("\n--- Volatility ---");
    if let Some(atr) = atr_14.last().unwrap() {
        println!(
            "ATR(14): {:.2} ({:.2}% of price)",
            atr,
            atr / last.close * 100.0
        );
    }
    if let (Some(upper), Some(middle), Some(lower)) = (
        bb_upper.last().unwrap(),
        bb_middle.last().unwrap(),
        bb_lower.last().unwrap(),
    ) {
        let bb_position = (last.close - lower) / (upper - lower) * 100.0;
        println!(
            "Bollinger Bands: {:.2} / {:.2} / {:.2}",
            upper, middle, lower
        );
        println!("Price position in bands: {:.1}%", bb_position);
    }

    println!("\n--- MACD ---");
    if let (Some(m), Some(s), Some(h)) = (
        macd.last().unwrap(),
        signal.last().unwrap(),
        histogram.last().unwrap(),
    ) {
        let macd_signal = if *h > 0.0 { "BULLISH" } else { "BEARISH" };
        println!(
            "MACD: {:.2}, Signal: {:.2}, Histogram: {:.2} ({})",
            m, s, h, macd_signal
        );
    }

    println!("\n--- Volume ---");
    println!("OBV: {:.0}", obv.last().unwrap());
    // Cleanup handled by TempDirGuard
}

/// Integration test: Verify Sharpe ratio calculation uses active returns only
/// This test ensures we don't regress to including zero-return cash days in volatility,
/// which would artificially inflate Sharpe by deflating std_dev.
#[tokio::test]
async fn test_sharpe_ratio_excludes_zero_return_days() {
    use crypto_strategies::backtest::Backtester;
    use crypto_strategies::config::{
        BacktestConfig, Config, ExchangeConfig, TaxConfig, TradingConfig,
    };
    use crypto_strategies::data::CoinDCXDataFetcher;
    use crypto_strategies::multi_timeframe::MultiTimeframeData;
    use crypto_strategies::strategies::volatility_regime::{
        VolatilityRegimeConfig, VolatilityRegimeStrategy,
    };
    use std::collections::HashMap;
    use std::env::temp_dir;

    let temp_data_dir = temp_dir().join("crypto_test_sharpe");
    let _guard = TempDirGuard(temp_data_dir.clone());
    let fetcher = CoinDCXDataFetcher::new(&temp_data_dir);

    // Download enough data for meaningful backtest
    println!("Downloading BTCINR 1d data for Sharpe test...");
    let result = fetcher.download_pair("BTCINR", "1d", 1000).await;

    if let Err(e) = &result {
        println!("Skipping test - download failed: {}", e);
        return;
    }

    let filepath = result.unwrap();
    let candles = crypto_strategies::data::load_csv(&filepath).unwrap();
    println!("Downloaded {} candles", candles.len());

    if candles.len() < 100 {
        println!(
            "Skipping test - insufficient data ({} candles)",
            candles.len()
        );
        return;
    }

    // Create config and strategy with less restrictive params to get more trades
    let mut config = Config {
        exchange: ExchangeConfig::default(),
        trading: TradingConfig::default(),
        strategy: serde_json::json!({"name": "volatility_regime", "timeframe": "1d"}),
        tax: TaxConfig::default(),
        backtest: BacktestConfig::default(),
        grid: None,
    };
    config.trading.initial_capital = 100_000.0;
    config.trading.risk_per_trade = 0.02;

    let strategy_config = VolatilityRegimeConfig {
        adx_threshold: 15.0,      // Lower threshold = more signals
        stop_atr_multiple: 1.5,   // Tighter stop
        target_atr_multiple: 3.0, // Smaller target = faster exits
        ..Default::default()
    };
    let strategy = VolatilityRegimeStrategy::new(strategy_config);

    // Run backtest using MultiTimeframeData
    let mut backtester = Backtester::new(config, Box::new(strategy));
    let symbol = Symbol::new("BTCINR");
    let mut data = HashMap::new();

    // Create MultiTimeframeData for the symbol
    let mut mtf_data = MultiTimeframeData::new("1d");
    mtf_data.add_timeframe("1d", candles.clone());
    data.insert(symbol.clone(), mtf_data);

    let result = backtester.run(&data);

    println!("\n=== Sharpe Ratio Validation ===");
    println!("Total trades: {}", result.metrics.total_trades);
    println!("Total return: {:.2}%", result.metrics.total_return);
    println!("Sharpe ratio: {:.2}", result.metrics.sharpe_ratio);
    println!("Max drawdown: {:.2}%", result.metrics.max_drawdown);

    // If we have trades, validate Sharpe is realistic
    if result.metrics.total_trades >= 3 {
        // For crypto, Sharpe > 3.0 is extremely suspicious
        // A properly calculated Sharpe using active returns should be realistic
        assert!(
            result.metrics.sharpe_ratio < 3.0,
            "Sharpe ratio {} is suspiciously high! \
             This may indicate zero-return days are being included in std_dev calculation, \
             artificially deflating volatility.",
            result.metrics.sharpe_ratio
        );

        // Sharpe shouldn't be NaN or infinite
        assert!(
            result.metrics.sharpe_ratio.is_finite(),
            "Sharpe ratio should be finite, got: {}",
            result.metrics.sharpe_ratio
        );

        // Calculate equity curve duration in years
        let duration_days = candles.len() as f64;
        let duration_years = duration_days / 365.0;

        // Verify we're using a realistic calculation
        // If Sharpe = (return - rf) / vol, then vol = (return - rf) / sharpe
        let total_return_decimal = result.metrics.total_return / 100.0;
        let annualized_return = (1.0 + total_return_decimal).powf(1.0 / duration_years) - 1.0;
        let risk_free = 0.05; // 5% risk-free rate
        let excess_return = annualized_return - risk_free;

        println!("Duration: {:.2} years", duration_years);
        println!("Annualized return: {:.2}%", annualized_return * 100.0);
        println!("Excess return: {:.2}%", excess_return * 100.0);

        if result.metrics.sharpe_ratio.abs() > 0.1 {
            let implied_vol = excess_return.abs() / result.metrics.sharpe_ratio.abs();
            println!("Implied annualized volatility: {:.1}%", implied_vol * 100.0);

            // Crypto volatility should be at least 15% annualized
            // If implied vol is < 10%, something is wrong with the calculation
            // (Before the fix, it was showing ~12% which is unrealistic for crypto)
            assert!(
                implied_vol > 0.10 || result.metrics.sharpe_ratio.abs() < 0.5,
                "Implied volatility {:.1}% is too low for crypto. \
                 This suggests std_dev is being calculated incorrectly (possibly including zero-return days).",
                implied_vol * 100.0
            );
        }

        println!("✓ Sharpe ratio calculation appears correct");
    } else {
        println!(
            "Not enough trades ({}) to validate Sharpe ratio",
            result.metrics.total_trades
        );
    }
}

// =============================================================================
// Regime Grid Strategy Tests
// =============================================================================

#[test]
fn test_regime_grid_strategy_creation() {
    let config = RegimeGridConfig::default();
    let _strategy = RegimeGridStrategy::new(config);
    // Strategy should be created without panic
}

#[test]
fn test_regime_grid_generates_flat_with_insufficient_data() {
    let config = RegimeGridConfig::default();
    let strategy = RegimeGridStrategy::new(config);

    let symbol = Symbol::new("BTCINR");
    let candles = generate_mock_candles(50, 50000.0, 100.0); // Too few for 200 EMA

    let signal = strategy.generate_signal(&symbol, &candles, None);
    assert_eq!(signal, Signal::Flat);
}

#[test]
fn test_regime_grid_with_sufficient_data() {
    let config = RegimeGridConfig::default();
    let strategy = RegimeGridStrategy::new(config);

    let symbol = Symbol::new("BTCINR");
    // Generate enough candles for all indicators (200 EMA * 2 = 400)
    let candles = generate_mock_candles(500, 50000.0, 500.0);

    let signal = strategy.generate_signal(&symbol, &candles, None);
    // Signal should not panic - may be Flat, Long, or Short depending on regime
    assert!(matches!(signal, Signal::Flat | Signal::Long | Signal::Short));
}

