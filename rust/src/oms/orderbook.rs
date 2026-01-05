//! High-performance order book with price-time priority
//!
//! Uses BTreeMap for efficient price-sorted storage and VecDeque for FIFO ordering.

use crate::oms::types::{Order, OrderId, OrderState};
use crate::Side;
use ordered_float::OrderedFloat;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Ultra-fast order book using BTreeMap for price-time priority
pub struct OrderBook {
    /// Buy orders sorted by price (descending) - best bid first
    buy_orders: BTreeMap<OrderedFloat<f64>, VecDeque<OrderId>>,

    /// Sell orders sorted by price (ascending) - best ask first
    sell_orders: BTreeMap<OrderedFloat<f64>, VecDeque<OrderId>>,

    /// Fast lookup: OrderId → Order
    orders: HashMap<OrderId, Order>,
}

impl OrderBook {
    /// Create a new order book
    pub fn new() -> Self {
        Self {
            buy_orders: BTreeMap::new(),
            sell_orders: BTreeMap::new(),
            orders: HashMap::new(),
        }
    }

    /// Add order with price-time priority
    pub fn add_order(&mut self, mut order: Order) {
        order.state = OrderState::Open;
        order.updated_at = chrono::Utc::now();

        let order_id = order.id;
        let price = self.get_order_price(&order);

        // Add to price-time queue
        match order.side {
            Side::Buy => {
                self.buy_orders
                    .entry(OrderedFloat(price))
                    .or_default()
                    .push_back(order_id);
            }
            Side::Sell => {
                self.sell_orders
                    .entry(OrderedFloat(price))
                    .or_default()
                    .push_back(order_id);
            }
        }

        // Add to lookup map
        self.orders.insert(order_id, order);
    }

    /// Cancel order
    pub fn cancel_order(&mut self, order_id: OrderId) -> Option<Order> {
        let mut order = self.orders.remove(&order_id)?;
        order.state = OrderState::Cancelled;
        order.updated_at = chrono::Utc::now();

        let price = self.get_order_price(&order);

        // Remove from price-time queue
        match order.side {
            Side::Buy => {
                if let Some(queue) = self.buy_orders.get_mut(&OrderedFloat(price)) {
                    queue.retain(|&id| id != order_id);
                    if queue.is_empty() {
                        self.buy_orders.remove(&OrderedFloat(price));
                    }
                }
            }
            Side::Sell => {
                if let Some(queue) = self.sell_orders.get_mut(&OrderedFloat(price)) {
                    queue.retain(|&id| id != order_id);
                    if queue.is_empty() {
                        self.sell_orders.remove(&OrderedFloat(price));
                    }
                }
            }
        }

        Some(order)
    }

    /// Get orders that would fill at given price
    /// Returns: Vec<OrderId> sorted by priority (price-time)
    pub fn get_fillable_orders(&self, price: f64, side: Side) -> Vec<OrderId> {
        let mut fillable = Vec::new();

        match side {
            // For buy side, check sell orders with limit_price <= execution price
            Side::Buy => {
                for (&limit_price, queue) in &self.sell_orders {
                    if limit_price.0 <= price {
                        fillable.extend(queue.iter().copied());
                    } else {
                        break; // BTreeMap is sorted, so we can stop early
                    }
                }
            }
            // For sell side, check buy orders with limit_price >= execution price
            Side::Sell => {
                for (&limit_price, queue) in self.buy_orders.iter().rev() {
                    if limit_price.0 >= price {
                        fillable.extend(queue.iter().copied());
                    } else {
                        break;
                    }
                }
            }
        }

        fillable
    }

    /// Get best bid price
    pub fn best_bid(&self) -> Option<f64> {
        self.buy_orders.keys().next_back().map(|&p| p.0)
    }

    /// Get best ask price
    pub fn best_ask(&self) -> Option<f64> {
        self.sell_orders.keys().next().map(|&p| p.0)
    }

    /// Get order by ID
    pub fn get_order(&self, order_id: OrderId) -> Option<&Order> {
        self.orders.get(&order_id)
    }

    /// Get mutable order by ID
    pub fn get_order_mut(&mut self, order_id: OrderId) -> Option<&mut Order> {
        self.orders.get_mut(&order_id)
    }

    /// Get all active orders
    pub fn get_all_orders(&self) -> Vec<&Order> {
        self.orders.values().collect()
    }

    /// Get all active order IDs
    pub fn get_all_order_ids(&self) -> Vec<OrderId> {
        self.orders.keys().copied().collect()
    }

    /// Clear all orders
    pub fn clear(&mut self) {
        self.buy_orders.clear();
        self.sell_orders.clear();
        self.orders.clear();
    }

    /// Get number of active orders
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Check if order book is empty
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Helper to get price from order based on type
    fn get_order_price(&self, order: &Order) -> f64 {
        match order.order_type {
            crate::oms::types::OrderType::Limit | crate::oms::types::OrderType::StopLimit => {
                order.limit_price.unwrap_or(0.0)
            }
            crate::oms::types::OrderType::Stop => order.stop_price.unwrap_or(0.0),
            crate::oms::types::OrderType::Market => {
                // Market orders shouldn't be in the book, but handle gracefully
                0.0
            }
        }
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oms::types::{OrderType, TimeInForce};
    use crate::Symbol;

    #[test]
    fn test_add_and_cancel_order() {
        let mut book = OrderBook::new();

        let order = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );

        let order_id = order.id;
        book.add_order(order);

        assert_eq!(book.len(), 1);
        assert_eq!(book.best_bid(), Some(50000.0));

        let cancelled = book.cancel_order(order_id);
        assert!(cancelled.is_some());
        assert_eq!(book.len(), 0);
    }

    #[test]
    fn test_price_time_priority() {
        let mut book = OrderBook::new();

        // Add three buy orders at same price
        let order1 = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );
        let id1 = order1.id;

        let order2 = Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        );
        let id2 = order2.id;

        book.add_order(order1);
        book.add_order(order2);

        // Get fillable orders - should maintain time priority
        let fillable = book.get_fillable_orders(50000.0, Side::Sell);
        assert_eq!(fillable.len(), 2);
        assert_eq!(fillable[0], id1); // First order has priority
        assert_eq!(fillable[1], id2);
    }

    #[test]
    fn test_best_bid_ask() {
        let mut book = OrderBook::new();

        book.add_order(Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(49000.0),
            None,
            TimeInForce::GTC,
            None,
        ));

        book.add_order(Order::new(
            Symbol::new("BTCUSDT"),
            Side::Buy,
            OrderType::Limit,
            1.0,
            Some(50000.0),
            None,
            TimeInForce::GTC,
            None,
        ));

        book.add_order(Order::new(
            Symbol::new("BTCUSDT"),
            Side::Sell,
            OrderType::Limit,
            1.0,
            Some(51000.0),
            None,
            TimeInForce::GTC,
            None,
        ));

        assert_eq!(book.best_bid(), Some(50000.0)); // Highest buy
        assert_eq!(book.best_ask(), Some(51000.0)); // Lowest sell
    }
}
