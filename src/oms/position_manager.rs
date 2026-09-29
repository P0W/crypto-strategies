//! Position management with FIFO P&L calculation

use crate::oms::types::{Fill, Position};
use crate::{Money, Side, Symbol, Trade};
use std::collections::{BTreeMap, HashMap};

/// Position manager for tracking multiple positions per symbol
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PositionManager {
    positions: BTreeMap<Symbol, Position>,
}

impl PositionManager {
    /// Create new position manager
    pub fn new() -> Self {
        Self {
            positions: BTreeMap::new(),
        }
    }

    /// Add a fill to positions (FIFO accounting)
    pub fn add_fill(&mut self, fill: Fill, symbol: Symbol, side: Side) -> Option<Trade> {
        let needs_new_position = if let Some(position) = self.positions.get(&symbol) {
            position.side != side && position.quantity <= fill.quantity
        } else {
            true
        };

        if needs_new_position && !self.positions.contains_key(&symbol) {
            let new_position = Position::from_fill(fill, symbol.clone(), side);
            self.positions.insert(symbol, new_position);
            None
        } else if let Some(position) = self.positions.get_mut(&symbol) {
            if position.side == side {
                // Same side - add to position (FIFO weighted average)
                let prev_total_value = position.average_entry_price * position.quantity;
                let new_value = fill.price * fill.quantity;
                let new_total_qty = position.quantity + fill.quantity;

                position.average_entry_price = (prev_total_value + new_value) / new_total_qty;
                position.quantity += fill.quantity;
                position.fills.push(fill.clone());
                position.last_update_time = fill.timestamp;
                None
            } else {
                // Opposite side - reduce or reverse position
                let original_side = position.side;
                let close_quantity = position.quantity.min(fill.quantity);
                if !close_quantity.is_positive() {
                    return None;
                }
                let closes_position = close_quantity == position.quantity;

                let exit_commission = if close_quantity == fill.quantity {
                    fill.commission
                } else {
                    fill.commission * close_quantity / fill.quantity
                };
                let mut remaining_close = close_quantity;
                let mut entry_value = Money::ZERO;
                let mut entry_commission = Money::ZERO;
                let mut gross_pnl = Money::ZERO;
                let entry_time = position
                    .fills
                    .first()
                    .map(|entry| entry.timestamp)
                    .unwrap_or(position.first_entry_time);

                while !position.fills.is_empty()
                    && (closes_position || remaining_close.is_positive())
                {
                    let first_fill = &mut position.fills[0];
                    let lot_quantity = first_fill.quantity;
                    let matched_quantity = if closes_position {
                        lot_quantity
                    } else {
                        remaining_close.min(lot_quantity)
                    };
                    let matched_entry_commission = if matched_quantity == lot_quantity {
                        first_fill.commission
                    } else {
                        first_fill.commission * matched_quantity / lot_quantity
                    };

                    entry_value += first_fill.price * matched_quantity;
                    entry_commission += matched_entry_commission;
                    gross_pnl += match original_side {
                        Side::Buy => (fill.price - first_fill.price) * matched_quantity,
                        Side::Sell => (first_fill.price - fill.price) * matched_quantity,
                    };

                    first_fill.quantity -= matched_quantity;
                    first_fill.commission -= matched_entry_commission;
                    remaining_close -= matched_quantity;

                    if first_fill.quantity.is_zero() {
                        position.fills.remove(0);
                    }
                }

                // FIFO lots are authoritative, including at Decimal's precision boundary.
                position.quantity = position.fills.iter().map(|entry| entry.quantity).sum();
                let total_commission = entry_commission + exit_commission;
                let net_pnl = gross_pnl - total_commission;
                position.realized_pnl += net_pnl;

                // If remaining, reverse position
                let reversal_quantity = fill.quantity - close_quantity;
                if reversal_quantity.is_positive() {
                    position.side = match position.side {
                        Side::Buy => Side::Sell,
                        Side::Sell => Side::Buy,
                    };
                    position.quantity = reversal_quantity;
                    position.average_entry_price = fill.price;
                    position.fills = vec![Fill {
                        order_id: fill.order_id,
                        price: fill.price,
                        quantity: reversal_quantity,
                        timestamp: fill.timestamp,
                        commission: fill.commission - exit_commission,
                        is_maker: fill.is_maker,
                    }];
                    position.first_entry_time = fill.timestamp;
                    position.risk_amount = Money::ZERO;
                } else if position.quantity.is_positive() {
                    let remaining_value = position
                        .fills
                        .iter()
                        .map(|entry| entry.price * entry.quantity)
                        .sum::<Money>();
                    position.average_entry_price = remaining_value / position.quantity;
                } else {
                    position.average_entry_price = Money::ZERO;
                }

                position.last_update_time = fill.timestamp;
                Some(Trade {
                    symbol,
                    side: original_side,
                    entry_price: entry_value / close_quantity,
                    exit_price: fill.price,
                    quantity: close_quantity,
                    entry_time,
                    exit_time: fill.timestamp,
                    pnl: gross_pnl,
                    commission: total_commission,
                    net_pnl,
                })
            }
        } else {
            None
        }
    }

    /// Get position for symbol (returns None if position quantity is 0 or negative)
    pub fn get_position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions
            .get(symbol)
            .filter(|p| p.quantity.is_positive())
    }

    /// Get raw position for symbol (even if qty=0, for trade creation)
    pub fn get_position_raw(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(symbol)
    }

    /// Get mutable position for symbol
    pub fn get_position_mut(&mut self, symbol: &Symbol) -> Option<&mut Position> {
        self.positions.get_mut(symbol)
    }

    /// Get all open positions as an iterator over (Symbol, &Position)
    pub fn get_all_positions(&self) -> impl Iterator<Item = (&Symbol, &Position)> {
        self.positions
            .iter()
            .filter(|(_, p)| p.quantity.is_positive())
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
        self.positions
            .values()
            .map(|p| p.unrealized_pnl.to_f64())
            .sum()
    }

    /// Get total realized P&L across all positions
    pub fn total_realized_pnl(&self) -> f64 {
        self.positions
            .values()
            .map(|p| p.realized_pnl.to_f64())
            .sum()
    }

    /// Clear all positions
    pub fn clear(&mut self) {
        self.positions.clear();
    }

    /// Get count of open positions (only counts positions with quantity > 0)
    pub fn open_position_count(&self) -> usize {
        self.positions
            .values()
            .filter(|p| p.quantity.is_positive())
            .count()
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
        Fill::from_f64(order_id, price, quantity, Utc::now(), 0.0, true)
    }

    fn create_fill_with_commission(
        order_id: OrderId,
        price: f64,
        quantity: f64,
        commission: f64,
    ) -> Fill {
        Fill::from_f64(order_id, price, quantity, Utc::now(), commission, true)
    }

    #[test]
    fn test_new_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        let fill = create_fill(1, 50000.0, 1.0);
        pm.add_fill(fill, symbol.clone(), Side::Buy);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity.to_f64(), 1.0);
        assert_eq!(pos.average_entry_price.to_f64(), 50000.0);
        assert_eq!(pos.side, Side::Buy);
    }

    #[test]
    fn test_add_to_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        pm.add_fill(create_fill(1, 50000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(2, 51000.0, 1.0), symbol.clone(), Side::Buy);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity.to_f64(), 2.0);
        assert_eq!(pos.average_entry_price.to_f64(), 50500.0);
    }

    #[test]
    fn test_reduce_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        pm.add_fill(create_fill(1, 50000.0, 2.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(2, 52000.0, 1.0), symbol.clone(), Side::Sell);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity.to_f64(), 1.0);
        assert_eq!(pos.side, Side::Buy);
        assert_eq!(pos.realized_pnl.to_f64(), 2000.0);
    }

    #[test]
    fn test_reverse_position() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        pm.add_fill(create_fill(1, 50000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(2, 52000.0, 2.0), symbol.clone(), Side::Sell);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity.to_f64(), 1.0);
        assert_eq!(pos.side, Side::Sell);
        assert_eq!(pos.average_entry_price.to_f64(), 52000.0);
        assert_eq!(pos.realized_pnl.to_f64(), 2000.0);
    }

    #[test]
    fn test_fifo_accounting() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        pm.add_fill(create_fill(1, 50000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(2, 51000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(3, 52000.0, 1.0), symbol.clone(), Side::Buy);
        pm.add_fill(create_fill(4, 53000.0, 1.5), symbol.clone(), Side::Sell);

        let pos = pm.get_position(&symbol).unwrap();
        assert_eq!(pos.quantity.to_f64(), 1.5);
        // Realized P&L = (53000-50000)*1.0 + (53000-51000)*0.5 = 3000 + 1000 = 4000
        assert_eq!(pos.realized_pnl.to_f64(), 4000.0);
    }

    #[test]
    fn test_partial_exit_returns_trade_and_prorates_commission() {
        let mut pm = PositionManager::new();
        let symbol = Symbol::new("BTCUSDT");

        let _ = pm.add_fill(
            create_fill_with_commission(1, 100.0, 2.0, 2.0),
            symbol.clone(),
            Side::Buy,
        );
        let trade = pm
            .add_fill(
                create_fill_with_commission(2, 110.0, 1.0, 1.0),
                symbol.clone(),
                Side::Sell,
            )
            .unwrap();

        assert_eq!(trade.quantity.to_f64(), 1.0);
        assert_eq!(trade.pnl.to_f64(), 10.0);
        assert_eq!(trade.commission.to_f64(), 2.0);
        assert_eq!(trade.net_pnl.to_f64(), 8.0);

        let position = pm.get_position(&symbol).unwrap();
        assert_eq!(position.quantity.to_f64(), 1.0);
        assert_eq!(position.average_entry_price.to_f64(), 100.0);
        assert_eq!(position.fills[0].commission.to_f64(), 1.0);
        assert_eq!(position.realized_pnl.to_f64(), 8.0);
    }

    #[test]
    fn test_full_exit_closes_fractional_lots_without_dust() {
        let mut manager = PositionManager::new();
        let symbol = Symbol::new("ETHINR");
        for (id, quantity) in [(1, 0.1), (2, 0.2), (3, 5e-13)] {
            manager.add_fill(create_fill(id, 100.0, quantity), symbol.clone(), Side::Buy);
        }
        let quantity = manager.get_position(&symbol).unwrap().quantity;
        let mut exit = create_fill(4, 120.0, 1.0);
        exit.quantity = quantity;
        manager.add_fill(exit, symbol.clone(), Side::Sell).unwrap();
        let position = manager.get_position_raw(&symbol).unwrap();
        assert_eq!(position.quantity, Money::ZERO);
        assert!(position.fills.is_empty());
        assert!(manager.get_position(&symbol).is_none());
    }

    #[test]
    fn test_fifo_quantity_matches_lots_at_decimal_precision_limit() {
        for partial_exit in [true, false] {
            let mut manager = PositionManager::new();
            let symbol = Symbol::new("SOLINR");
            manager.add_fill(create_fill(1, 100.0, 10.0), symbol.clone(), Side::Buy);
            let mut fractional = create_fill(2, 100.0, 1.0);
            fractional.quantity =
                serde_json::from_value(serde_json::json!("0.1234567890123456789012345671"))
                    .unwrap();
            let remaining = fractional.quantity;
            manager.add_fill(fractional, symbol.clone(), Side::Buy);
            if partial_exit {
                manager.add_fill(create_fill(3, 120.0, 10.0), symbol.clone(), Side::Sell);
                let position = manager.get_position(&symbol).unwrap();
                assert_eq!(position.quantity, remaining);
                assert_eq!(position.fills[0].quantity, remaining);
            }
            let mut exit = create_fill(4, 120.0, 1.0);
            exit.quantity = manager.get_position(&symbol).unwrap().quantity;
            manager.add_fill(exit, symbol.clone(), Side::Sell);
            assert!(manager.get_position(&symbol).is_none());
            assert!(manager.get_position_raw(&symbol).unwrap().fills.is_empty());
        }
    }
}
