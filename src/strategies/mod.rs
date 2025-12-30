//! Trading Strategies Module
//!
//! Contains all available trading strategies and common abstractions.

pub mod volatility_regime;

use crate::{Candle, Order, Position, Signal, Symbol, Trade};

/// Trading strategy trait
///
/// This trait defines the interface for trading strategies, matching the backtrader interface
/// for compatibility and providing hooks for order and trade notifications.
pub trait Strategy: Send + Sync {
    /// Generate trading signal for the given candle data
    fn generate_signal(
        &self,
        symbol: &Symbol,
        candles: &[Candle],
        position: Option<&Position>,
    ) -> Signal;

    /// Calculate stop loss price for entry
    fn calculate_stop_loss(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Calculate take profit price for entry
    fn calculate_take_profit(&self, candles: &[Candle], entry_price: f64) -> f64;

    /// Update trailing stop if applicable
    fn update_trailing_stop(
        &self,
        position: &Position,
        current_price: f64,
        candles: &[Candle],
    ) -> Option<f64>;

    /// Notification when an order state changes
    ///
    /// Called when:
    /// - Order is submitted
    /// - Order is accepted by exchange
    /// - Order is partially filled
    /// - Order is completed
    /// - Order is canceled, rejected, or failed
    fn notify_order(&mut self, order: &Order) {
        // Default implementation: log order status
        match order.status {
            crate::OrderStatus::Submitted | crate::OrderStatus::Accepted => {
                // Order in flight, no action needed by default
            }
            crate::OrderStatus::Completed => {
                if let Some(ref exec) = order.executed {
                    tracing::info!(
                        symbol = %order.symbol,
                        side = ?order.side,
                        price = exec.price,
                        size = exec.size,
                        commission = exec.commission,
                        "Order executed"
                    );
                }
            }
            crate::OrderStatus::Canceled | crate::OrderStatus::Margin | crate::OrderStatus::Rejected => {
                tracing::warn!(
                    symbol = %order.symbol,
                    status = ?order.status,
                    "Order failed"
                );
            }
            _ => {}
        }
    }

    /// Notification when a trade (position) is closed
    ///
    /// Called with the completed trade details including P&L, commission, etc.
    /// Strategies can use this to track performance, adjust parameters, or log results.
    fn notify_trade(&mut self, trade: &Trade) {
        // Default implementation: log trade results
        let return_pct = trade.return_pct();
        let net_pnl_post_tax = if trade.net_pnl > 0.0 {
            trade.net_pnl * 0.7  // 30% tax on profits
        } else {
            trade.net_pnl
        };

        tracing::info!(
            symbol = %trade.symbol,
            entry_price = trade.entry_price,
            exit_price = trade.exit_price,
            quantity = trade.quantity,
            return_pct = format!("{:.2}%", return_pct),
            gross_pnl = trade.pnl,
            commission = trade.commission,
            net_pnl = trade.net_pnl,
            net_pnl_post_tax = format!("{:.2}", net_pnl_post_tax),
            "Trade closed"
        );
    }
    
    /// Initialize strategy (called once before trading starts)
    ///
    /// Use this to set up any internal state, load models, etc.
    fn init(&mut self) {
        // Default: no initialization needed
    }
}
