//! Strategy interface types for OMS

use crate::oms::types::{Order, OrderType, Position, TimeInForce};
use crate::{Candle, Money, MultiTimeframeCandles, Side, Symbol};

/// Context provided to strategy for decision-making
#[derive(Debug)]
pub struct StrategyContext<'a> {
    pub symbol: &'a Symbol,
    pub candles: &'a [Candle],
    /// Multi-timeframe candles (if strategy requires multiple timeframes)
    pub mtf_candles: Option<&'a MultiTimeframeCandles<'a>>,
    pub current_position: Option<&'a Position>,
    pub open_orders: &'a [Order],
    pub cash_available: f64,
    pub equity: f64,
    /// Peak equity for drawdown calculation
    pub peak_equity: f64,
}

impl<'a> StrategyContext<'a> {
    /// Create a single-timeframe context
    pub fn single_timeframe(
        symbol: &'a Symbol,
        candles: &'a [Candle],
        current_position: Option<&'a Position>,
        open_orders: &'a [Order],
        cash_available: f64,
        equity: f64,
    ) -> Self {
        Self {
            symbol,
            candles,
            mtf_candles: None,
            current_position,
            open_orders,
            cash_available,
            equity,
            peak_equity: equity,
        }
    }

    /// Create a multi-timeframe context
    pub fn multi_timeframe(
        symbol: &'a Symbol,
        mtf_candles: &'a MultiTimeframeCandles<'a>,
        current_position: Option<&'a Position>,
        open_orders: &'a [Order],
        cash_available: f64,
        equity: f64,
    ) -> Self {
        Self {
            symbol,
            candles: mtf_candles.primary(),
            mtf_candles: Some(mtf_candles),
            current_position,
            open_orders,
            cash_available,
            equity,
            peak_equity: equity,
        }
    }

    /// Set peak equity for drawdown calculation
    pub fn with_peak_equity(mut self, peak: f64) -> Self {
        self.peak_equity = peak;
        self
    }

    /// Get candles for a specific timeframe (multi-timeframe mode)
    pub fn get_timeframe(&self, tf: &str) -> Option<&'a [Candle]> {
        self.mtf_candles.and_then(|mtf| mtf.get(tf))
    }

    /// Check if this is a multi-timeframe context
    pub fn is_multi_timeframe(&self) -> bool {
        self.mtf_candles.is_some()
    }
}

/// Order request from strategy
#[derive(Debug, Clone)]
pub struct OrderRequest {
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Money,
    pub limit_price: Option<Money>,
    pub stop_price: Option<Money>,
    pub time_in_force: TimeInForce,
    pub client_id: Option<String>,
}

impl OrderRequest {
    pub fn market_buy(symbol: Symbol, quantity: f64) -> Self {
        Self {
            symbol,
            side: Side::Buy,
            order_type: OrderType::Market,
            quantity: Money::from_f64(quantity),
            limit_price: None,
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            client_id: None,
        }
    }

    pub fn market_sell(symbol: Symbol, quantity: f64) -> Self {
        Self {
            symbol,
            side: Side::Sell,
            order_type: OrderType::Market,
            quantity: Money::from_f64(quantity),
            limit_price: None,
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            client_id: None,
        }
    }

    pub fn limit_buy(symbol: Symbol, quantity: f64, limit_price: f64) -> Self {
        Self {
            symbol,
            side: Side::Buy,
            order_type: OrderType::Limit,
            quantity: Money::from_f64(quantity),
            limit_price: Some(Money::from_f64(limit_price)),
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            client_id: None,
        }
    }

    pub fn limit_sell(symbol: Symbol, quantity: f64, limit_price: f64) -> Self {
        Self {
            symbol,
            side: Side::Sell,
            order_type: OrderType::Limit,
            quantity: Money::from_f64(quantity),
            limit_price: Some(Money::from_f64(limit_price)),
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            client_id: None,
        }
    }

    pub fn stop_buy(symbol: Symbol, quantity: f64, stop_price: f64) -> Self {
        Self {
            symbol,
            side: Side::Buy,
            order_type: OrderType::Stop,
            quantity: Money::from_f64(quantity),
            limit_price: None,
            stop_price: Some(Money::from_f64(stop_price)),
            time_in_force: TimeInForce::GTC,
            client_id: None,
        }
    }

    pub fn stop_sell(symbol: Symbol, quantity: f64, stop_price: f64) -> Self {
        Self {
            symbol,
            side: Side::Sell,
            order_type: OrderType::Stop,
            quantity: Money::from_f64(quantity),
            limit_price: None,
            stop_price: Some(Money::from_f64(stop_price)),
            time_in_force: TimeInForce::GTC,
            client_id: None,
        }
    }

    pub fn with_client_id(mut self, client_id: String) -> Self {
        self.client_id = Some(client_id);
        self
    }

    pub fn with_time_in_force(mut self, tif: TimeInForce) -> Self {
        self.time_in_force = tif;
        self
    }

    pub fn into_order(self) -> Order {
        Order::new(
            self.symbol,
            self.side,
            self.order_type,
            self.quantity,
            self.limit_price,
            self.stop_price,
            self.time_in_force,
            self.client_id,
        )
    }

    pub fn to_order(&self) -> Order {
        Order::new(
            self.symbol.clone(),
            self.side,
            self.order_type,
            self.quantity,
            self.limit_price,
            self.stop_price,
            self.time_in_force,
            self.client_id.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_buy_request() {
        let req = OrderRequest::market_buy(Symbol::new("BTCUSDT"), 1.0);
        assert_eq!(req.side, Side::Buy);
        assert_eq!(req.order_type, OrderType::Market);
        assert_eq!(req.quantity.to_f64(), 1.0);
        assert!(req.limit_price.is_none());
    }

    #[test]
    fn test_limit_sell_request() {
        let req = OrderRequest::limit_sell(Symbol::new("BTCUSDT"), 1.0, 52000.0);
        assert_eq!(req.side, Side::Sell);
        assert_eq!(req.order_type, OrderType::Limit);
        assert_eq!(req.limit_price.unwrap().to_f64(), 52000.0);
    }

    #[test]
    fn test_stop_sell_request() {
        let req = OrderRequest::stop_sell(Symbol::new("BTCUSDT"), 1.0, 48000.0);
        assert_eq!(req.side, Side::Sell);
        assert_eq!(req.order_type, OrderType::Stop);
        assert_eq!(req.stop_price.unwrap().to_f64(), 48000.0);
    }

    #[test]
    fn test_with_client_id() {
        let req = OrderRequest::market_buy(Symbol::new("BTCUSDT"), 1.0)
            .with_client_id("test123".to_string());
        assert_eq!(req.client_id, Some("test123".to_string()));
    }
}
