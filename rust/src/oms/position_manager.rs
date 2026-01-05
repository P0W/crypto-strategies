//! Position management with FIFO P&L calculation

use crate::oms::types::{Fill, Position};
use crate::{Side, Symbol};
use std::collections::HashMap;

/// Position manager for tracking multiple positions per symbol
pub struct PositionManager {
    positions: HashMap<Symbol, Position>,
}

impl PositionManager {
    /// Create new position manager
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
        }
    }

    /// Add a fill to positions (FIFO accounting)
    pub fn add_fill(&mut self, fill: Fill, symbol: Symbol, side: Side) {
        let needs_new_position = if let Some(position) = self.positions.get(&symbol) {
            // Position exists - check if same side or opposite
            position.side != side && position.quantity <= fill.quantity
        } else {
            true
        };

        if needs_new_position && !self.positions.contains_key(&symbol) {
            // New position
            let new_position = Position::from_fill(fill, symbol.clone(), side);
            self.positions.insert(symbol, new_position);
        } else if let Some(position) = self.positions.get_mut(&symbol) {
            if position.side == side {
                // Same side - add to position
                // FIFO weighted average entry price
                let prev_total_value = position.average_entry_price * position.quantity;
                let new_value = fill.price * fill.quantity;
                let new_total_qty = position.quantity + fill.quantity;

                position.average_entry_price = (prev_total_value + new_value) / new_total_qty;
                position.quantity += fill.quantity;
                position.fills.push(fill.clone());
                position.last_update_time = fill.timestamp;
            } else {
                // Opposite side - reduce or reverse position
                let mut remaining_qty = fill.quantity;

                // Close fills using FIFO
                while remaining_qty > 0.0 && !position.fills.is_empty() {
                    let first_fill = &mut position.fills[0];

                    if first_fill.quantity <= remaining_qty {
                        // Close entire first fill
                        let pnl = match position.side {
                            Side::Buy => (fill.price - first_fill.price) * first_fill.quantity,
                            Side::Sell => (first_fill.price - fill.price) * first_fill.quantity,
                        };
                        position.realized_pnl += pnl - fill.commission;
                        remaining_qty -= first_fill.quantity;
                        position.quantity -= first_fill.quantity;
                        position.fills.remove(0);
                    } else {
                        // Partially close first fill
                        let pnl = match position.side {
                            Side::Buy => (fill.price - first_fill.price) * remaining_qty,
                            Side::Sell => (first_fill.price - fill.price) * remaining_qty,
                        };
                        position.realized_pnl += pnl - fill.commission;
                        first_fill.quantity -= remaining_qty;
                        position.quantity -= remaining_qty;
                        remaining_qty = 0.0;
                    }
                }

                // If we have remaining quantity, we've reversed the position
                if remaining_qty > 0.0 {
                    // Create new position in opposite direction
                    position.side = match position.side {
                        Side::Buy => Side::Sell,
                        Side::Sell => Side::Buy,
                    };
                    position.quantity = remaining_qty;
                    position.average_entry_price = fill.price;
                    position.fills = vec![Fill {
                        order_id: fill.order_id,
                        price: fill.price,
                        quantity: remaining_qty,
                        timestamp: fill.timestamp,
                        commission: fill.commission,
                        is_maker: fill.is_maker,
                    }];
                }

                position.last_update_time = fill.timestamp;
            }
        }
    }

    /// Get position for symbol
    pub fn get_position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(symbol)
    }

    /// Get mutable position for symbol
    pub fn get_position_mut(&mut self, symbol: &Symbol) -> Option<&mut Position> {
        self.positions.get_mut(symbol)
    }

    /// Get all positions as an iterator over (Symbol, &Position)
    pub fn get_all_positions(&self) -> impl Iterator<Item = (&Symbol, &Position)> {
        self.positions.iter()
    }

    /// Update unrealized P&L for all positions
    pub fn update_unrealized_pnl(&mut self, prices: &HashMap<Symbol, f64>) {
        for (symbol, position) in &mut self.positions {
            if let Some(&current_price) = prices.get(symbol) {
                position.update_unrealized_pnl(current_price);
            }
        }
    }

    /// Close position for symbol
    pub fn close_position(&mut self, symbol: &Symbol) -> Option<Position> {
        self.positions.remove(symbol)
    }

    /// Get total unrealized P&L across all positions
    pub fn total_unrealized_pnl(&self) -> f64 {
        self.positions.values().map(|p| p.unrealized_pnl).sum()
    }

    /// Get total realized P&L across all positions
    pub fn total_realized_pnl(&self) -> f64 {
        self.positions.values().map(|p| p.realized_pnl).sum()
    }

    /// Clear all positions
    pub fn clear(&mut self) {
        self.positions.clear();
    }

    /// Get count of open positions
    pub fn open_position_count(&self) -> usize {
        self.positions.len()
    }

    /// Get number of positions on specific symbol
    pub fn position_count_for_symbol(&self, symbol: &Symbol) -> usize {
        if self.positions.contains_key(symbol) {
            1
        } else {
            0
        }
    }
}

impl Default for PositionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oms::types::OrderId;
    use chrono::Utc;

    fn create_fill(order_id: OrderId, price: f64, quantity: f64) -> Fill {
        Fill {
            order_id,
            price,
            quantity,
            timestamp: Utc::now(),
            commission: 0.0,
            is_maker: true,
        }
    }

    #[test]
    fn test_new_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        let fill = create_fill(1, 50000.0, 1.0);
        pm.add_fill(fill, symbol.clone(), Side::Buy);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity, 1.0);
        assert_eq!(pos.average_entry_price, 50000.0);
        assert_eq!(pos.side, Side::Buy);
    }

    #[test]
    fn test_add_to_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        // First fill at 50000
        pm.add_fill(create_fill(1, 50000.0, 1.0), symbol.clone(), Side::Buy);

        // Second fill at 51000
        pm.add_fill(create_fill(2, 51000.0, 1.0), symbol.clone(), Side::Buy);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity, 2.0);
        assert_eq!(pos.average_entry_price, 50500.0); // (50000 + 51000) / 2
    }

    #[test]
    fn test_reduce_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        // Buy 2 BTC at 50000
        pm.add_fill(create_fill(1, 50000.0, 2.0), symbol.clone(), Side::Buy);

        // Sell 1 BTC at 52000
        pm.add_fill(create_fill(2, 52000.0, 1.0), symbol.clone(), Side::Sell);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity, 1.0);
        assert_eq!(pos.side, Side::Buy);
        assert_eq!(pos.realized_pnl, 2000.0); // (52000 - 50000) * 1
    }

    #[test]
    fn test_reverse_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        // Buy 1 BTC at 50000
        pm.add_fill(create_fill(1, 50000.0, 1.0), symbol.clone(), Side::Buy);

        // Sell 2 BTC at 52000 (close 1, reverse 1)
        pm.add_fill(create_fill(2, 52000.0, 2.0), symbol.clone(), Side::Sell);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity, 1.0);
        assert_eq!(pos.side, Side::Sell);
        assert_eq!(pos.average_entry_price, 52000.0);
        assert_eq!(pos.realized_pnl, 2000.0); // Profit from closing long position
    }

    #[test]
    fn test_fifo_accounting() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        // Buy at different prices
        pm.add_fill(create_fill(1, 50000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(2, 51000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(3, 52000.0, 1.0), symbol.clone(), Side::Buy);

        // Sell 1.5 BTC - should close first fill completely and half of second
        pm.add_fill(create_fill(4, 53000.0, 1.5), symbol.clone(), Side::Sell);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity, 1.5);

        // Realized P&L = (53000-50000)*1.0 + (53000-51000)*0.5 = 3000 + 1000 = 4000
        assert_eq!(pos.realized_pnl, 4000.0);
    }
}
