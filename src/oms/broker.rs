//! Broker-neutral spot execution contracts. Implementations must not retry mutations.

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use std::future::Future;

use super::{Order, OrderType};
use crate::{Money, Side, Symbol};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketRules {
    pub symbol: Symbol,
    pub asset: String,
    pub quote: String,
    pub data_pair: String,
    pub quantity_step: Money,
    pub price_tick: Money,
    pub min_quantity: Money,
    pub max_quantity: Money,
    pub max_market_quantity: Money,
    pub min_notional: Money,
    pub min_price: Money,
    pub max_price: Money,
    pub order_types: Vec<OrderType>,
}

impl MarketRules {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.asset.is_empty() && !self.quote.is_empty() && self.asset != self.quote,
            "Invalid currencies for {}",
            self.symbol
        );
        ensure!(
            !self.data_pair.is_empty()
                && self.quantity_step.is_positive()
                && self.price_tick.is_positive()
                && self.min_quantity.is_positive()
                && self.max_quantity >= self.min_quantity
                && self.max_market_quantity >= self.min_quantity
                && self.min_notional.is_positive()
                && self.min_price.is_positive()
                && self.max_price >= self.min_price,
            "Invalid market rules for {}",
            self.symbol
        );
        ensure!(
            [OrderType::Market, OrderType::Limit, OrderType::StopLimit]
                .iter().all(|kind| self.order_types.contains(kind)),
            "{} does not advertise market, limit and native stop-limit orders; real trading requires all three",
            self.symbol
        );
        Ok(())
    }

    pub fn quantity(&self, quantity: Money) -> Money {
        Self::quantize(quantity, self.quantity_step, false)
    }

    pub fn price(&self, price: Money, round_up: bool) -> Money {
        Self::quantize(price, self.price_tick, round_up)
    }

    fn quantize(value: Money, step: Money, round_up: bool) -> Money {
        let units = (value.inner() / step.inner()).round_dp_with_strategy(
            0,
            if round_up {
                RoundingStrategy::ToPositiveInfinity
            } else {
                RoundingStrategy::ToNegativeInfinity
            },
        );
        Money::from(units * step.inner())
    }

    pub fn order(&self, order: &Order, client_id: String, mark: Money) -> Result<BrokerOrder> {
        ensure!(order.symbol == self.symbol, "Order/market mismatch");
        let request = BrokerOrder {
            client_id,
            symbol: order.symbol.clone(),
            side: order.side,
            kind: order.order_type,
            quantity: self.quantity(order.quantity),
            limit: order
                .limit_price
                .map(|p| self.price(p, order.side == Side::Sell)),
            stop: order
                .stop_price
                .map(|p| self.price(p, order.side == Side::Sell)),
        };
        self.validate_order(&request, mark)?;
        Ok(request)
    }

    pub fn validate_order(&self, order: &BrokerOrder, mark: Money) -> Result<()> {
        ensure!(order.symbol == self.symbol, "Order/market mismatch");
        ensure!(
            self.order_types.contains(&order.kind),
            "Unsupported order type {:?}",
            order.kind
        );
        ensure!(
            order.quantity >= self.min_quantity
                && order.quantity <= self.max_quantity
                && order.quantity == self.quantity(order.quantity),
            "Invalid quantity for {}: {}",
            self.symbol,
            order.quantity
        );
        if order.kind == OrderType::Market {
            ensure!(
                order.quantity <= self.max_market_quantity,
                "Market quantity exceeds venue maximum"
            );
        }
        match order.kind {
            OrderType::Market => ensure!(
                order.limit.is_none() && order.stop.is_none(),
                "Invalid market payload"
            ),
            OrderType::Limit => ensure!(
                order.limit.is_some() && order.stop.is_none(),
                "Invalid limit payload"
            ),
            OrderType::Stop => ensure!(
                order.stop.is_some() && order.limit.is_none(),
                "Invalid stop payload"
            ),
            OrderType::StopLimit => ensure!(
                order.stop.is_some() && order.limit.is_some(),
                "Invalid stop-limit payload"
            ),
        }
        for price in [order.limit, order.stop].into_iter().flatten() {
            ensure!(
                price >= self.min_price
                    && price <= self.max_price
                    && price == self.price(price, false),
                "Invalid price for {}: {price}",
                self.symbol
            );
        }
        let value = order.limit.unwrap_or(mark);
        ensure!(
            value.is_positive() && order.quantity * value >= self.min_notional,
            "Order below minimum notional"
        );
        Ok(())
    }

    pub fn protective_prices(&self, stop: f64) -> Result<(Money, Money)> {
        ensure!(stop.is_finite() && stop > 0.0, "Invalid protective stop");
        let trigger = self.price(Money::from_f64(stop), true);
        // A stop-limit is not a guaranteed exit through a gap. The live quote path
        // cancels/reconciles a triggered, unfilled stop before a market reduction.
        let limit = self.price(trigger * Money::from(Decimal::new(99, 2)), false);
        ensure!(
            limit.is_positive() && limit < trigger,
            "Stop-limit buffer is below price precision"
        );
        Ok((trigger, limit))
    }

    pub fn validate_protection(&self, quantity: Money, stop: f64) -> Result<(Money, Money)> {
        let (trigger, limit) = self.protective_prices(stop)?;
        self.validate_order(
            &BrokerOrder {
                client_id: "protection-preflight".into(),
                symbol: self.symbol.clone(),
                side: Side::Sell,
                kind: OrderType::StopLimit,
                quantity,
                limit: Some(limit),
                stop: Some(trigger),
            },
            limit,
        )?;
        Ok((trigger, limit))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerOrder {
    pub client_id: String,
    pub symbol: Symbol,
    pub side: Side,
    pub kind: OrderType,
    pub quantity: Money,
    pub limit: Option<Money>,
    pub stop: Option<Money>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrokerStatus {
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

impl BrokerStatus {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Cancelled | Self::Rejected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerReport {
    pub exchange_id: String,
    pub order: BrokerOrder,
    pub status: BrokerStatus,
    pub filled: Money,
    pub average_price: Money,
    /// Cumulative fee in the market's quote currency, never an estimated fee.
    pub fee: Money,
    pub updated_at: DateTime<Utc>,
}

impl BrokerReport {
    pub fn validate(&self, request: &BrokerOrder) -> Result<()> {
        ensure!(
            !self.exchange_id.is_empty() && &self.order == request,
            "Broker order identity/payload mismatch"
        );
        ensure!(
            self.filled >= Money::ZERO
                && self.filled <= request.quantity
                && self.fee >= Money::ZERO,
            "Invalid cumulative quantity or fee"
        );
        ensure!(
            if self.filled.is_positive() {
                self.average_price.is_positive()
            } else {
                self.average_price.is_zero() && self.fee.is_zero()
            },
            "Invalid cumulative fill price/fee"
        );
        ensure!(
            (self.status == BrokerStatus::Filled) == (self.filled == request.quantity),
            "Broker filled status/quantity mismatch"
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Balance {
    pub currency: String,
    pub available: Money,
    pub locked: Money,
}

impl Balance {
    pub fn total(&self) -> Money {
        self.available + self.locked
    }
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub symbol: Symbol,
    pub bid: Money,
    pub ask: Money,
    pub timestamp: DateTime<Utc>,
}

impl Quote {
    pub fn validate(&self, now: DateTime<Utc>) -> Result<()> {
        ensure!(
            self.bid.is_positive() && self.ask >= self.bid,
            "Invalid bid/ask for {}",
            self.symbol
        );
        let age = now.signed_duration_since(self.timestamp).num_seconds();
        ensure!(
            (-5..=30).contains(&age),
            "Stale or future quote for {}",
            self.symbol
        );
        Ok(())
    }
}

/// A live broker must enforce unique client IDs and support lookup by that ID.
/// `submit`/`cancel` perform one network attempt; an error is an unknown outcome.
/// `lookup` returns None only for an authoritative order-not-found response.
pub trait Broker {
    /// Stable, non-secret account identity, including the broker name.
    fn account_key(&self) -> impl Future<Output = Result<String>> + Send;
    fn markets(&self, symbols: &[Symbol]) -> impl Future<Output = Result<Vec<MarketRules>>> + Send;
    fn balances(&self) -> impl Future<Output = Result<Vec<Balance>>> + Send;
    fn active_orders(
        &self,
        symbol: &Symbol,
    ) -> impl Future<Output = Result<Vec<BrokerReport>>> + Send;
    fn quotes(&self, symbols: &[Symbol]) -> impl Future<Output = Result<Vec<Quote>>> + Send;
    fn lookup(&self, client_id: &str) -> impl Future<Output = Result<Option<BrokerReport>>> + Send;
    fn submit(&self, order: &BrokerOrder) -> impl Future<Output = Result<BrokerReport>> + Send;
    fn cancel(&self, client_id: &str) -> impl Future<Output = Result<()>> + Send;
}

pub fn balance<'a>(balances: &'a [Balance], currency: &str) -> Result<&'a Balance> {
    let mut matches = balances.iter().filter(|b| b.currency == currency);
    let value = matches
        .next()
        .with_context(|| format!("Missing {currency} balance"))?;
    ensure!(
        matches.next().is_none() && value.available >= Money::ZERO && value.locked >= Money::ZERO,
        "Invalid or duplicate {currency} balance"
    );
    Ok(value)
}
