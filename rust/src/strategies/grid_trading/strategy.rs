//! Grid Trading Strategy
//!
//! A market-neutral strategy that profits from price oscillations within a range.
//!
//! ## How it works:
//! 1. Divide price range into N grid levels
//! 2. When price crosses DOWN through a grid level → BUY at that level
//! 3. When price crosses UP through a grid level → SELL if holding from that level
//! 4. Profits from the spread between buy and sell at each level
//!
//! ## Best suited for:
//! - Ranging/sideways markets
//! - High volatility within a bounded range
//! - Crypto pairs that oscillate frequently

use crate::indicators::atr;
use crate::strategies::Strategy;
use crate::{Candle, Order, Position, Signal, Symbol, Trade};
use std::collections::HashMap;
use std::sync::Mutex;

use super::config::GridTradingConfig;

/// State of a single grid level
#[derive(Debug, Clone, Copy, PartialEq)]
enum GridState {
    /// Waiting to buy at this level
    WaitingToBuy,
    /// Holding position bought at this level
    Holding { buy_price: f64 },
}

/// A single grid level
#[derive(Debug, Clone)]
struct GridLevel {
    price: f64,
    state: GridState,
}

/// Internal mutable state for grid tracking
struct GridInternalState {
    /// Grid levels by symbol
    grids: HashMap<String, Vec<GridLevel>>,
    /// Last known price by symbol
    last_price: HashMap<String, f64>,
    /// Bar counter for grid recalculation
    bar_counter: usize,
    /// Grid boundaries
    lower_price: HashMap<String, f64>,
    upper_price: HashMap<String, f64>,
    /// Total profit tracking
    total_profit: f64,
}

impl Default for GridInternalState {
    fn default() -> Self {
        Self {
            grids: HashMap::new(),
            last_price: HashMap::new(),
            bar_counter: 0,
            lower_price: HashMap::new(),
            upper_price: HashMap::new(),
            total_profit: 0.0,
        }
    }
}

pub struct GridTradingStrategy {
    config: GridTradingConfig,
    state: Mutex<GridInternalState>,
}

impl GridTradingStrategy {
    pub fn new(config: GridTradingConfig) -> Self {
        Self {
            config,
            state: Mutex::new(GridInternalState::default()),
        }
    }

    /// Initialize grid levels for a symbol based on current market conditions
    fn initialize_grids(&self, symbol: &str, candles: &[Candle]) -> Option<Vec<GridLevel>> {
        if candles.len() < self.config.min_bars {
            return None;
        }

        let current_price = candles.last()?.close;

        // Calculate grid range
        let (lower_price, upper_price) = if self.config.use_atr_grids {
            // Use ATR to determine range
            let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
            let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
            let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

            let atr_vals = atr(&high, &low, &close, self.config.atr_period);
            let current_atr = atr_vals.last().and_then(|&x| x).unwrap_or(current_price * 0.02);

            let range = current_atr * self.config.atr_multiplier;
            (current_price - range / 2.0, current_price + range / 2.0)
        } else if let Some(spacing) = self.config.grid_spacing_pct {
            // Use fixed percentage spacing
            let range = current_price * spacing * self.config.num_grids as f64;
            (current_price - range / 2.0, current_price + range / 2.0)
        } else {
            // Use recent high/low as range
            let lookback = self.config.min_bars.min(candles.len());
            let recent = &candles[candles.len() - lookback..];
            let high = recent.iter().map(|c| c.high).fold(f64::MIN, f64::max);
            let low = recent.iter().map(|c| c.low).fold(f64::MAX, f64::min);
            (low, high)
        };

        // Store grid boundaries
        {
            let mut state = self.state.lock().unwrap();
            state.lower_price.insert(symbol.to_string(), lower_price);
            state.upper_price.insert(symbol.to_string(), upper_price);
        }

        // Create grid levels
        let grid_size = (upper_price - lower_price) / self.config.num_grids as f64;
        let mut grids = Vec::with_capacity(self.config.num_grids + 1);

        for i in 0..=self.config.num_grids {
            let price = lower_price + (i as f64 * grid_size);
            // Initialize state based on current price position
            let state = if price < current_price {
                // Below current price = we should have bought here (simulate as holding)
                GridState::Holding { buy_price: price }
            } else {
                // Above current price = waiting to buy
                GridState::WaitingToBuy
            };
            grids.push(GridLevel { price, state });
        }

        tracing::debug!(
            symbol = symbol,
            lower = format!("{:.2}", lower_price),
            upper = format!("{:.2}", upper_price),
            num_grids = self.config.num_grids,
            "Initialized grid"
        );

        Some(grids)
    }

    /// Find which grid index a price falls into
    #[allow(dead_code)]
    fn find_grid_index(&self, symbol: &str, price: f64) -> Option<usize> {
        let state = self.state.lock().unwrap();
        let lower = *state.lower_price.get(symbol)?;
        let upper = *state.upper_price.get(symbol)?;

        if price <= lower {
            return Some(0);
        }
        if price >= upper {
            return Some(self.config.num_grids);
        }

        let grid_size = (upper - lower) / self.config.num_grids as f64;
        Some(((price - lower) / grid_size) as usize)
    }

    /// Process price update and generate signals
    fn process_price_update(&self, symbol: &str, current_price: f64) -> Signal {
        let mut state = self.state.lock().unwrap();

        // Get last price
        let last_price = match state.last_price.get(symbol) {
            Some(&p) => p,
            None => {
                state.last_price.insert(symbol.to_string(), current_price);
                return Signal::Flat;
            }
        };

        // Update last price
        state.last_price.insert(symbol.to_string(), current_price);

        // Get grid levels - clone to avoid borrow issues
        let grids = match state.grids.get(symbol) {
            Some(g) => g.clone(),
            None => return Signal::Flat,
        };

        // Determine price direction
        let price_going_up = current_price > last_price;
        let price_going_down = current_price < last_price;

        // Track signals and profits
        let mut signal = Signal::Flat;
        let mut profit_earned = 0.0;
        let mut updated_grids = grids;

        // Check each grid level for crossings
        for grid in updated_grids.iter_mut() {
            match grid.state {
                GridState::WaitingToBuy => {
                    // BUY condition: price crosses DOWN through this level
                    if price_going_down && last_price >= grid.price && current_price < grid.price {
                        grid.state = GridState::Holding {
                            buy_price: current_price,
                        };
                        signal = Signal::Long;
                        tracing::debug!(
                            symbol = symbol,
                            grid_price = format!("{:.2}", grid.price),
                            buy_price = format!("{:.2}", current_price),
                            "Grid BUY triggered"
                        );
                    }
                }
                GridState::Holding { buy_price } => {
                    // SELL condition: price crosses UP through this level (above buy price)
                    if price_going_up && last_price <= grid.price && current_price > grid.price {
                        let profit = current_price - buy_price;
                        profit_earned += profit;
                        grid.state = GridState::WaitingToBuy;
                        signal = Signal::Flat; // Close position
                        tracing::debug!(
                            symbol = symbol,
                            grid_price = format!("{:.2}", grid.price),
                            sell_price = format!("{:.2}", current_price),
                            profit = format!("{:.2}", profit),
                            "Grid SELL triggered"
                        );
                    }
                }
            }
        }

        // Update state with new grids and profit
        state.grids.insert(symbol.to_string(), updated_grids.clone());
        state.total_profit += profit_earned;

        // Check if any grids are still holding
        let any_holding = updated_grids
            .iter()
            .any(|g| matches!(g.state, GridState::Holding { .. }));

        if any_holding && signal == Signal::Flat {
            // We have positions, maintain Long signal
            Signal::Long
        } else {
            signal
        }
    }
}

impl Strategy for GridTradingStrategy {
    fn name(&self) -> &'static str {
        "grid_trading"
    }

    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        _position: Option<&Position>,
    ) -> Signal {
        if candles.len() < self.config.min_bars {
            return Signal::Flat;
        }

        let symbol_str = symbol.as_str();
        let current = candles.last().unwrap();

        // Initialize or recalculate grids if needed
        {
            let mut state = self.state.lock().unwrap();
            let needs_init = !state.grids.contains_key(symbol_str);
            let needs_recalc = self.config.recalc_interval > 0
                && state.bar_counter >= self.config.recalc_interval;

            if needs_init || needs_recalc {
                drop(state); // Release lock before calling initialize_grids
                if let Some(grids) = self.initialize_grids(symbol_str, candles) {
                    let mut state = self.state.lock().unwrap();
                    state.grids.insert(symbol_str.to_string(), grids);
                    if needs_recalc {
                        state.bar_counter = 0;
                    }
                }
            } else {
                state.bar_counter += 1;
            }
        }

        // Process the current price
        self.process_price_update(symbol_str, current.close)
    }

    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64 {
        // Use lower grid boundary as stop loss
        let symbol = "default"; // We don't have symbol here, use a wide stop
        let state = self.state.lock().unwrap();

        if let Some(&lower) = state.lower_price.get(symbol) {
            lower * 0.98 // 2% below lower boundary
        } else {
            // Fallback: ATR-based stop
            let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
            let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
            let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

            let atr_vals = atr(&high, &low, &close, self.config.atr_period);
            let current_atr = atr_vals
                .last()
                .and_then(|&x| x)
                .unwrap_or(entry_price * 0.02);

            entry_price - current_atr * 2.0
        }
    }

    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64 {
        // Use upper grid boundary as take profit
        let symbol = "default";
        let state = self.state.lock().unwrap();

        if let Some(&upper) = state.upper_price.get(symbol) {
            upper * 1.02 // 2% above upper boundary
        } else {
            // Fallback: ATR-based target
            let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
            let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
            let close: Vec<f64> = candles.iter().map(|c| c.close).collect();

            let atr_vals = atr(&high, &low, &close, self.config.atr_period);
            let current_atr = atr_vals
                .last()
                .and_then(|&x| x)
                .unwrap_or(entry_price * 0.02);

            entry_price + current_atr * 2.0
        }
    }

    fn update_trailing_stop(
        &self,
        _position: &Position,
        _current_price: f64,
        _candles: &[Candle],
    ) -> Option<f64> {
        // Grid trading doesn't use trailing stops
        None
    }

    fn notify_order(&mut self, order: &Order) {
        if let Some(ref exec) = order.executed {
            tracing::debug!(
                symbol = %order.symbol,
                side = ?order.side,
                price = format!("{:.2}", exec.price),
                "Grid order executed"
            );
        }
    }

    fn notify_trade(&mut self, trade: &Trade) {
        tracing::debug!(
            symbol = %trade.symbol,
            pnl = format!("{:.2}", trade.net_pnl),
            "Grid trade closed"
        );
    }

    fn init(&mut self) {
        let mut state = self.state.lock().unwrap();
        *state = GridInternalState::default();
    }
}
