//! Types and models for CoinDCX API
//!
//! This module contains all the request and response types
//! following the official CoinDCX API documentation.

use serde::{Deserialize, Serialize};

/// Market ticker information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker {
    /// Market pair name (e.g., "BTCINR")
    pub market: String,
    /// Last traded price
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub last_price: String,
    /// Highest bid price in orderbook
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub bid: String,
    /// Lowest ask price in orderbook
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub ask: String,
    /// 24-hour trading volume
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub volume: String,
    /// Timestamp when ticker was generated
    #[serde(default)]
    pub timestamp: i64,
    /// 24-hour price change percentage
    #[serde(default)]
    pub change_24_hour: Option<String>,
    /// 24-hour high price
    #[serde(default)]
    pub high: Option<String>,
    /// 24-hour low price
    #[serde(default)]
    pub low: Option<String>,
}

impl Ticker {
    /// Parse last_price as f64
    pub fn last_price_f64(&self) -> Option<f64> {
        self.last_price.parse().ok()
    }

    /// Parse bid as f64
    pub fn bid_f64(&self) -> Option<f64> {
        self.bid.parse().ok()
    }

    /// Parse ask as f64
    pub fn ask_f64(&self) -> Option<f64> {
        self.ask.parse().ok()
    }

    /// Parse volume as f64
    pub fn volume_f64(&self) -> Option<f64> {
        self.volume.parse().ok()
    }
}

/// Order side (buy or sell)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "buy"),
            OrderSide::Sell => write!(f, "sell"),
        }
    }
}

/// Order type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    MarketOrder,
    LimitOrder,
    StopLimit,
    TakeProfit,
}

impl std::fmt::Display for OrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderType::MarketOrder => write!(f, "market_order"),
            OrderType::LimitOrder => write!(f, "limit_order"),
            OrderType::StopLimit => write!(f, "stop_limit"),
            OrderType::TakeProfit => write!(f, "take_profit"),
        }
    }
}

/// Order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    PartiallyCancelled,
    Init,
}

/// Request to create a new order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    /// Order side: "buy" or "sell"
    pub side: String,
    /// Order type: "limit_order" or "market_order"
    pub order_type: String,
    /// Market pair (e.g., "BTCINR")
    pub market: String,
    /// Price per unit (required for limit orders)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_per_unit: Option<f64>,
    /// Total quantity to trade
    pub total_quantity: f64,
    /// Request timestamp in milliseconds
    pub timestamp: i64,
    /// Optional client order ID for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
}

impl OrderRequest {
    /// Create a new market order request
    pub fn market(side: OrderSide, market: impl Into<String>, quantity: f64) -> Self {
        Self {
            side: side.to_string(),
            order_type: OrderType::MarketOrder.to_string(),
            market: market.into(),
            price_per_unit: None,
            total_quantity: quantity,
            timestamp: chrono::Utc::now().timestamp_millis(),
            client_order_id: None,
        }
    }

    /// Create a new limit order request
    pub fn limit(side: OrderSide, market: impl Into<String>, quantity: f64, price: f64) -> Self {
        Self {
            side: side.to_string(),
            order_type: OrderType::LimitOrder.to_string(),
            market: market.into(),
            price_per_unit: Some(price),
            total_quantity: quantity,
            timestamp: chrono::Utc::now().timestamp_millis(),
            client_order_id: None,
        }
    }

    /// Set a client order ID for tracking
    pub fn with_client_order_id(mut self, id: impl Into<String>) -> Self {
        self.client_order_id = Some(id.into());
        self
    }
}

/// Response from order creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    /// Unique order identifier
    pub id: String,
    /// Order status
    pub status: String,
    /// Client order ID (if provided)
    #[serde(default)]
    pub client_order_id: Option<String>,
    /// Market pair
    #[serde(default)]
    pub market: Option<String>,
    /// Order type
    #[serde(default)]
    pub order_type: Option<String>,
    /// Order side
    #[serde(default)]
    pub side: Option<String>,
    /// Fee amount charged
    #[serde(default)]
    pub fee_amount: Option<f64>,
    /// Total quantity
    #[serde(default)]
    pub total_quantity: Option<f64>,
    /// Remaining unfilled quantity
    #[serde(default)]
    pub remaining_quantity: Option<f64>,
    /// Average fill price
    #[serde(default)]
    pub avg_price: Option<f64>,
    /// Price per unit
    #[serde(default)]
    pub price_per_unit: Option<f64>,
    /// Order creation timestamp
    #[serde(default)]
    pub created_at: Option<String>,
    /// Last update timestamp
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Response containing multiple orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrdersResponse {
    pub orders: Vec<OrderResponse>,
}

/// Request to cancel an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    /// Order ID to cancel
    pub id: String,
    /// Request timestamp
    pub timestamp: i64,
}

impl CancelOrderRequest {
    pub fn new(order_id: impl Into<String>) -> Self {
        Self {
            id: order_id.into(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Request to cancel order by client order ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderByClientIdRequest {
    pub client_order_id: String,
    pub timestamp: i64,
}

/// User balance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    /// Currency code (e.g., "BTC", "INR")
    pub currency: String,
    /// Available balance
    #[serde(deserialize_with = "deserialize_f64_or_string")]
    pub balance: f64,
    /// Balance locked in open orders
    #[serde(deserialize_with = "deserialize_f64_or_string")]
    pub locked_balance: f64,
}

impl Balance {
    /// Get total balance (available + locked)
    pub fn total(&self) -> f64 {
        self.balance + self.locked_balance
    }
}

/// Market details information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDetails {
    /// CoinDCX internal name
    pub coindcx_name: String,
    /// Base currency short name (e.g., "BTC")
    pub base_currency_short_name: String,
    /// Target currency short name (e.g., "INR")
    pub target_currency_short_name: String,
    /// Minimum order quantity
    pub min_quantity: f64,
    /// Maximum order quantity
    pub max_quantity: f64,
    /// Minimum order price
    pub min_price: f64,
    /// Maximum order price
    pub max_price: f64,
    /// Minimum notional value
    pub min_notional: f64,
    /// Base currency decimal precision
    pub base_currency_precision: u32,
    /// Target currency decimal precision
    pub target_currency_precision: u32,
    /// Minimum price/quantity step
    pub step: f64,
    /// Available order types
    #[serde(default)]
    pub order_types: Vec<String>,
    /// Market status ("active" or "inactive")
    #[serde(default)]
    pub status: String,
    /// Exchange code
    #[serde(default)]
    pub ecode: Option<String>,
    /// Trading pair identifier
    #[serde(default)]
    pub pair: Option<String>,
}

/// Trade history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    /// Trade ID
    pub id: i64,
    /// Order ID
    pub order_id: String,
    /// Trade side
    pub side: String,
    /// Fee amount
    pub fee_amount: String,
    /// Exchange code
    pub ecode: String,
    /// Trade quantity
    pub quantity: f64,
    /// Trade price
    pub price: f64,
    /// Market symbol
    pub symbol: String,
    /// Trade timestamp
    pub timestamp: i64,
}

/// User info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// CoinDCX user ID
    pub coindcx_id: String,
    /// First name
    #[serde(default)]
    pub first_name: Option<String>,
    /// Last name
    #[serde(default)]
    pub last_name: Option<String>,
    /// Mobile number
    #[serde(default)]
    pub mobile_number: Option<String>,
    /// Email address
    #[serde(default)]
    pub email: Option<String>,
}

/// Candle/OHLCV data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    /// Open price
    pub open: f64,
    /// High price
    pub high: f64,
    /// Low price
    pub low: f64,
    /// Close price
    pub close: f64,
    /// Volume
    pub volume: f64,
    /// Candle open time in milliseconds
    pub time: i64,
}

/// Order book entry
#[derive(Debug, Clone)]
pub struct OrderBookEntry {
    pub price: f64,
    pub quantity: f64,
}

/// Order book data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    /// Bid orders (price -> quantity)
    pub bids: std::collections::HashMap<String, String>,
    /// Ask orders (price -> quantity)
    pub asks: std::collections::HashMap<String, String>,
}

impl OrderBook {
    /// Get sorted bid entries (highest price first)
    pub fn sorted_bids(&self) -> Vec<OrderBookEntry> {
        let mut entries: Vec<OrderBookEntry> = self
            .bids
            .iter()
            .filter_map(|(price, qty)| {
                Some(OrderBookEntry {
                    price: price.parse().ok()?,
                    quantity: qty.parse().ok()?,
                })
            })
            .collect();
        entries.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
        entries
    }

    /// Get sorted ask entries (lowest price first)
    pub fn sorted_asks(&self) -> Vec<OrderBookEntry> {
        let mut entries: Vec<OrderBookEntry> = self
            .asks
            .iter()
            .filter_map(|(price, qty)| {
                Some(OrderBookEntry {
                    price: price.parse().ok()?,
                    quantity: qty.parse().ok()?,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
        entries
    }

    /// Get best bid price
    pub fn best_bid(&self) -> Option<f64> {
        self.sorted_bids().first().map(|e| e.price)
    }

    /// Get best ask price
    pub fn best_ask(&self) -> Option<f64> {
        self.sorted_asks().first().map(|e| e.price)
    }

    /// Get bid-ask spread
    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }
}

/// Timestamp request body (used for authenticated requests)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampRequest {
    pub timestamp: i64,
}

impl TimestampRequest {
    pub fn new() -> Self {
        Self {
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl Default for TimestampRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Active orders request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveOrdersRequest {
    pub market: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    pub timestamp: i64,
}

impl ActiveOrdersRequest {
    pub fn new(market: impl Into<String>) -> Self {
        Self {
            market: market.into(),
            side: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn with_side(mut self, side: OrderSide) -> Self {
        self.side = Some(side.to_string());
        self
    }
}

/// Order status request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderStatusRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
    pub timestamp: i64,
}

impl OrderStatusRequest {
    pub fn by_id(order_id: impl Into<String>) -> Self {
        Self {
            id: Some(order_id.into()),
            client_order_id: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn by_client_id(client_order_id: impl Into<String>) -> Self {
        Self {
            id: None,
            client_order_id: Some(client_order_id.into()),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

// Custom deserializer for fields that can be string or number
fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct StringOrNumber;

    impl<'de> Visitor<'de> for StringOrNumber {
        type Value = String;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or a number")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(String::new())
        }
    }

    deserializer.deserialize_any(StringOrNumber)
}

// Custom deserializer for f64 that can handle string representation
fn deserialize_f64_or_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct F64OrString;

    impl<'de> Visitor<'de> for F64OrString {
        type Value = f64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a number or a string representing a number")
        }

        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v as f64)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(v as f64)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.parse().map_err(de::Error::custom)
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.parse().map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_any(F64OrString)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_side_display() {
        assert_eq!(OrderSide::Buy.to_string(), "buy");
        assert_eq!(OrderSide::Sell.to_string(), "sell");
    }

    #[test]
    fn test_order_type_display() {
        assert_eq!(OrderType::MarketOrder.to_string(), "market_order");
        assert_eq!(OrderType::LimitOrder.to_string(), "limit_order");
    }

    #[test]
    fn test_order_request_market() {
        let order = OrderRequest::market(OrderSide::Buy, "BTCINR", 0.001);
        assert_eq!(order.side, "buy");
        assert_eq!(order.order_type, "market_order");
        assert_eq!(order.market, "BTCINR");
        assert_eq!(order.total_quantity, 0.001);
        assert!(order.price_per_unit.is_none());
    }

    #[test]
    fn test_order_request_limit() {
        let order = OrderRequest::limit(OrderSide::Sell, "BTCINR", 0.001, 5000000.0);
        assert_eq!(order.side, "sell");
        assert_eq!(order.order_type, "limit_order");
        assert_eq!(order.market, "BTCINR");
        assert_eq!(order.total_quantity, 0.001);
        assert_eq!(order.price_per_unit, Some(5000000.0));
    }

    #[test]
    fn test_order_request_with_client_id() {
        let order = OrderRequest::market(OrderSide::Buy, "BTCINR", 0.001)
            .with_client_order_id("my-order-123");
        assert_eq!(order.client_order_id, Some("my-order-123".to_string()));
    }

    #[test]
    fn test_balance_total() {
        let balance = Balance {
            currency: "BTC".to_string(),
            balance: 1.5,
            locked_balance: 0.5,
        };
        assert_eq!(balance.total(), 2.0);
    }

    #[test]
    fn test_ticker_parsing() {
        let json = r#"{
            "market": "BTCINR",
            "last_price": "5000000",
            "bid": "4999000",
            "ask": "5001000",
            "volume": "100.5",
            "timestamp": 1234567890
        }"#;

        let ticker: Ticker = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.market, "BTCINR");
        assert_eq!(ticker.last_price_f64(), Some(5000000.0));
        assert_eq!(ticker.bid_f64(), Some(4999000.0));
        assert_eq!(ticker.ask_f64(), Some(5001000.0));
        assert_eq!(ticker.volume_f64(), Some(100.5));
    }

    #[test]
    fn test_ticker_numeric_values() {
        let json = r#"{
            "market": "BTCINR",
            "last_price": 5000000,
            "bid": 4999000,
            "ask": 5001000,
            "volume": 100.5,
            "timestamp": 1234567890
        }"#;

        let ticker: Ticker = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.last_price_f64(), Some(5000000.0));
    }

    #[test]
    fn test_cancel_order_request() {
        let req = CancelOrderRequest::new("order-123");
        assert_eq!(req.id, "order-123");
        assert!(req.timestamp > 0);
    }

    #[test]
    fn test_timestamp_request() {
        let req = TimestampRequest::new();
        assert!(req.timestamp > 0);
    }

    #[test]
    fn test_active_orders_request() {
        let req = ActiveOrdersRequest::new("BTCINR").with_side(OrderSide::Buy);
        assert_eq!(req.market, "BTCINR");
        assert_eq!(req.side, Some("buy".to_string()));
    }

    #[test]
    fn test_order_status_request() {
        let by_id = OrderStatusRequest::by_id("order-123");
        assert_eq!(by_id.id, Some("order-123".to_string()));
        assert!(by_id.client_order_id.is_none());

        let by_client = OrderStatusRequest::by_client_id("client-456");
        assert!(by_client.id.is_none());
        assert_eq!(by_client.client_order_id, Some("client-456".to_string()));
    }

    #[test]
    fn test_order_book_operations() {
        let mut bids = std::collections::HashMap::new();
        bids.insert("100.0".to_string(), "1.0".to_string());
        bids.insert("99.0".to_string(), "2.0".to_string());

        let mut asks = std::collections::HashMap::new();
        asks.insert("101.0".to_string(), "1.5".to_string());
        asks.insert("102.0".to_string(), "2.5".to_string());

        let book = OrderBook { bids, asks };

        assert_eq!(book.best_bid(), Some(100.0));
        assert_eq!(book.best_ask(), Some(101.0));
        assert_eq!(book.spread(), Some(1.0));

        let sorted_bids = book.sorted_bids();
        assert_eq!(sorted_bids[0].price, 100.0);
        assert_eq!(sorted_bids[1].price, 99.0);

        let sorted_asks = book.sorted_asks();
        assert_eq!(sorted_asks[0].price, 101.0);
        assert_eq!(sorted_asks[1].price, 102.0);
    }
}
