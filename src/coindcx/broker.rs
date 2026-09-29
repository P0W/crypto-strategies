//! Strict CoinDCX spot wire adapter; normalized execution lives in `oms`.

use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Number, Value};
use std::str::FromStr;

use super::client::ApiError;
use super::CoinDCXClient;
use crate::oms::broker::{
    Balance, Broker, BrokerOrder, BrokerReport, BrokerStatus, MarketRules, Quote,
};
use crate::oms::OrderType;
use crate::{Money, Side, Symbol};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
struct Amount(#[serde(deserialize_with = "decimal")] Decimal);

fn decimal<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Decimal, D::Error> {
    let text = match Value::deserialize(deserializer)? {
        Value::String(value) => value,
        Value::Number(value) => value.to_string(),
        _ => {
            return Err(serde::de::Error::custom(
                "Expected a decimal string or number",
            ))
        }
    };
    Decimal::from_str(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .map_err(serde::de::Error::custom)
}

impl From<Amount> for Money {
    fn from(value: Amount) -> Self {
        Money::from(value.0)
    }
}

fn number(value: Money) -> Result<Value> {
    Ok(Value::Number(
        Number::from_str(&value.inner().to_string()).context("Encoding exact decimal")?,
    ))
}

fn text_or_number<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(value) if !value.is_empty() => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(serde::de::Error::custom(
            "Expected ID/timestamp string or number",
        )),
    }
}

#[derive(Deserialize)]
struct WireMarket {
    coindcx_name: String,
    base_currency_short_name: String,
    target_currency_short_name: String,
    base_currency_precision: u32,
    target_currency_precision: u32,
    min_quantity: Amount,
    max_quantity: Amount,
    max_quantity_market: Option<Amount>,
    min_price: Amount,
    max_price: Amount,
    min_notional: Amount,
    step: Amount,
    order_types: Vec<String>,
    pair: String,
    status: String,
}

impl WireMarket {
    fn normalized(self) -> Result<MarketRules> {
        ensure!(
            self.base_currency_precision <= 28 && self.target_currency_precision <= 28,
            "Invalid venue precision"
        );
        let kind = |name: &str| match name {
            "market_order" => Some(OrderType::Market),
            "limit_order" => Some(OrderType::Limit),
            "stop_limit" => Some(OrderType::StopLimit),
            "stop_market" => Some(OrderType::Stop),
            _ => None,
        };
        let quantity_step = Money::from(self.step);
        let precision_step = Money::from(Decimal::new(1, self.target_currency_precision));
        ensure!(
            quantity_step >= precision_step,
            "Quantity step conflicts with precision"
        );
        Ok(MarketRules {
            symbol: Symbol::new(self.coindcx_name),
            asset: self.target_currency_short_name,
            quote: self.base_currency_short_name,
            data_pair: self.pair,
            quantity_step,
            price_tick: Money::from(Decimal::new(1, self.base_currency_precision)),
            min_quantity: self.min_quantity.into(),
            max_quantity: self.max_quantity.into(),
            max_market_quantity: self
                .max_quantity_market
                .map(Money::from)
                .unwrap_or(Money::ZERO),
            min_notional: self.min_notional.into(),
            min_price: self.min_price.into(),
            max_price: self.max_price.into(),
            order_types: self
                .order_types
                .iter()
                .filter_map(|name| kind(name))
                .collect(),
        })
    }
}

#[derive(Deserialize)]
struct WireOrder {
    #[serde(deserialize_with = "text_or_number")]
    id: String,
    client_order_id: String,
    market: String,
    order_type: String,
    side: String,
    status: String,
    total_quantity: Amount,
    remaining_quantity: Amount,
    avg_price: Amount,
    fee_amount: Amount,
    price_per_unit: Option<Amount>,
    stop_price: Option<Amount>,
    #[serde(deserialize_with = "text_or_number")]
    updated_at: String,
}

impl WireOrder {
    fn normalized(self) -> Result<BrokerReport> {
        let kind = match self.order_type.as_str() {
            "market_order" => OrderType::Market,
            "limit_order" => OrderType::Limit,
            "stop_limit" => OrderType::StopLimit,
            "stop_market" => OrderType::Stop,
            other => bail!("Unsupported CoinDCX order type {other}"),
        };
        let side = match self.side.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            other => bail!("Unsupported CoinDCX order side {other}"),
        };
        let status = match self.status.as_str() {
            "init" => BrokerStatus::Pending,
            "open" => BrokerStatus::Open,
            "partially_filled" => BrokerStatus::PartiallyFilled,
            "filled" => BrokerStatus::Filled,
            "cancelled" | "partially_cancelled" => BrokerStatus::Cancelled,
            "rejected" => BrokerStatus::Rejected,
            other => bail!("Unknown CoinDCX spot status {other}"),
        };
        let updated_at = if let Ok(milliseconds) = self.updated_at.parse::<i64>() {
            DateTime::from_timestamp_millis(milliseconds).context("Invalid order timestamp")?
        } else {
            DateTime::parse_from_rfc3339(&self.updated_at)
                .context("Invalid order timestamp")?
                .with_timezone(&Utc)
        };
        let order = BrokerOrder {
            client_id: self.client_order_id,
            symbol: Symbol::new(self.market),
            side,
            kind,
            quantity: self.total_quantity.into(),
            limit: match kind {
                OrderType::Limit | OrderType::StopLimit => {
                    Some(self.price_per_unit.context("Missing limit price")?.into())
                }
                _ => None,
            },
            stop: match kind {
                OrderType::Stop | OrderType::StopLimit => {
                    Some(self.stop_price.context("Missing stop price")?.into())
                }
                _ => None,
            },
        };
        let report = BrokerReport {
            exchange_id: self.id,
            filled: order.quantity - Money::from(self.remaining_quantity),
            order,
            status,
            average_price: self.avg_price.into(),
            fee: self.fee_amount.into(),
            updated_at,
        };
        report.validate(&report.order)?;
        Ok(report)
    }
}

#[derive(Deserialize)]
struct WireOrders {
    orders: Vec<WireOrder>,
}

fn timestamp() -> Value {
    json!({"timestamp": Utc::now().timestamp_millis()})
}

impl Broker for CoinDCXClient {
    async fn account_key(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Account {
            coindcx_id: String,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Response {
            Single(Account),
            List(Vec<Account>),
        }
        let response: Response = self
            .post_once("/exchange/v1/users/info", &timestamp())
            .await?;
        let account = match response {
            Response::Single(account) => account,
            Response::List(mut accounts) => {
                ensure!(accounts.len() == 1, "Expected exactly one CoinDCX account");
                accounts.pop().context("Missing account")?
            }
        };
        ensure!(
            !account.coindcx_id.is_empty(),
            "Missing CoinDCX account identity"
        );
        Ok(format!("coindcx-spot:{}", account.coindcx_id))
    }

    async fn markets(&self, symbols: &[Symbol]) -> Result<Vec<MarketRules>> {
        let markets: Vec<Value> = self.get_once("/exchange/v1/markets_details").await?;
        markets
            .into_iter()
            .filter(|market| {
                symbols
                    .iter()
                    .any(|symbol| market["coindcx_name"].as_str() == Some(symbol.as_str()))
            })
            .map(|market| {
                serde_json::from_value::<WireMarket>(market)
                    .context("Invalid requested market metadata")
            })
            .filter_map(|market| match market {
                Ok(market) if market.status != "active" => None,
                other => Some(other.and_then(WireMarket::normalized)),
            })
            .collect()
    }

    async fn balances(&self) -> Result<Vec<Balance>> {
        #[derive(Deserialize)]
        struct WireBalance {
            currency: String,
            balance: Amount,
            locked_balance: Amount,
        }
        let balances: Vec<WireBalance> = self
            .post_once("/exchange/v1/users/balances", &timestamp())
            .await?;
        balances
            .into_iter()
            .map(|b| {
                let balance = Balance {
                    currency: b.currency,
                    available: b.balance.into(),
                    locked: b.locked_balance.into(),
                };
                ensure!(
                    balance.available >= Money::ZERO && balance.locked >= Money::ZERO,
                    "Negative venue balance"
                );
                Ok(balance)
            })
            .collect()
    }

    async fn active_orders(&self, symbol: &Symbol) -> Result<Vec<BrokerReport>> {
        let mut body = timestamp();
        body["market"] = json!(symbol.as_str());
        let response: WireOrders = self
            .post_once("/exchange/v1/orders/active_orders", &body)
            .await?;
        response
            .orders
            .into_iter()
            .map(WireOrder::normalized)
            .collect()
    }

    async fn quotes(&self, symbols: &[Symbol]) -> Result<Vec<Quote>> {
        #[derive(Deserialize)]
        struct WireTicker {
            market: String,
            bid: Amount,
            ask: Amount,
            timestamp: i64,
        }
        let values: Vec<Value> = self.get_once("/exchange/ticker").await?;
        let tickers: Vec<WireTicker> = values
            .into_iter()
            .filter(|ticker| {
                symbols
                    .iter()
                    .any(|symbol| ticker["market"].as_str() == Some(symbol.as_str()))
            })
            .map(|ticker| serde_json::from_value(ticker).context("Invalid requested ticker"))
            .collect::<Result<_>>()?;
        symbols
            .iter()
            .map(|symbol| {
                let mut matches = tickers.iter().filter(|t| t.market == symbol.as_str());
                let ticker = matches.next().context("Missing market ticker")?;
                ensure!(matches.next().is_none(), "Duplicate market ticker");
                Ok(Quote {
                    symbol: symbol.clone(),
                    bid: ticker.bid.into(),
                    ask: ticker.ask.into(),
                    timestamp: DateTime::from_timestamp(ticker.timestamp, 0)
                        .context("Invalid ticker timestamp")?,
                })
            })
            .collect()
    }

    async fn lookup(&self, client_id: &str) -> Result<Option<BrokerReport>> {
        let mut body = timestamp();
        body["client_order_id"] = json!(client_id);
        match self
            .post_once::<_, WireOrder>("/exchange/v1/orders/status", &body)
            .await
        {
            Ok(order) => Ok(Some(order.normalized()?)),
            Err(error) => {
                if let Some(api) = error.downcast_ref::<ApiError>() {
                    let body: Value = serde_json::from_str(&api.body)
                        .context("Invalid CoinDCX error response")?;
                    if api.status == 404
                        && body["message"]
                            .as_str()
                            .is_some_and(|s| s.eq_ignore_ascii_case("Order not found"))
                    {
                        return Ok(None);
                    }
                }
                Err(error.context("Looking up CoinDCX client order ID"))
            }
        }
    }

    async fn submit(&self, order: &BrokerOrder) -> Result<BrokerReport> {
        let mut body = timestamp();
        body["client_order_id"] = json!(order.client_id);
        body["market"] = json!(order.symbol.as_str());
        body["side"] = json!(match order.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        });
        body["order_type"] = json!(match order.kind {
            OrderType::Market => "market_order",
            OrderType::Limit => "limit_order",
            OrderType::Stop => "stop_market",
            OrderType::StopLimit => "stop_limit",
        });
        body["total_quantity"] = number(order.quantity)?;
        if let Some(price) = order.limit {
            body["price_per_unit"] = number(price)?;
        }
        if let Some(price) = order.stop {
            body["stop_price"] = number(price)?;
        }
        let response: WireOrders = self.post_once("/exchange/v1/orders/create", &body).await?;
        ensure!(
            response.orders.len() == 1,
            "Expected exactly one created order"
        );
        let report = response
            .orders
            .into_iter()
            .next()
            .context("Missing created order")?
            .normalized()?;
        report.validate(order)?;
        Ok(report)
    }

    async fn cancel(&self, client_id: &str) -> Result<()> {
        let mut body = timestamp();
        body["client_order_id"] = json!(client_id);
        let response: Value = self.post_once("/exchange/v1/orders/cancel", &body).await?;
        ensure!(
            response["status"] == "success",
            "Cancellation was not acknowledged: {response}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coindcx::{auth::sign_request, ClientConfig};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn wire_order() -> Value {
        json!({
            "id": 123, "client_order_id": "test-client", "market": "BTCUSDT",
            "side": "buy", "order_type": "market_order", "status": "filled",
            "total_quantity": "0.123456789123456789", "remaining_quantity": 0,
            "avg_price": "100.25", "fee_amount": "0.01", "updated_at": 1780000000000i64
        })
    }

    #[test]
    fn strict_reports_accept_numeric_ids_and_both_timestamp_formats() {
        let report: WireOrder = serde_json::from_value(wire_order()).unwrap();
        let report = report.normalized().unwrap();
        assert_eq!(report.exchange_id, "123");
        assert_eq!(report.filled.inner().to_string(), "0.123456789123456789");
        let mut legacy = wire_order();
        legacy["id"] = json!("legacy-id");
        legacy["updated_at"] = json!("2026-01-01T00:00:00Z");
        serde_json::from_value::<WireOrder>(legacy)
            .unwrap()
            .normalized()
            .unwrap();
    }

    #[test]
    fn decimal_json_is_exact_in_both_directions() {
        let amount: Amount = serde_json::from_str("0.123456789123456789123456789").unwrap();
        assert_eq!(amount.0.to_string(), "0.123456789123456789123456789");
        assert_eq!(
            number(amount.into()).unwrap().to_string(),
            "0.123456789123456789123456789"
        );
        assert!(serde_json::from_str::<Amount>("null").is_err());
        assert!(serde_json::from_str::<Amount>("\"NaN\"").is_err());
    }

    #[test]
    fn missing_fees_unknown_status_and_missing_native_trigger_are_rejected() {
        let mut missing = wire_order();
        missing.as_object_mut().unwrap().remove("fee_amount");
        assert!(serde_json::from_value::<WireOrder>(missing).is_err());
        let mut unknown = wire_order();
        unknown["status"] = json!("undocumented");
        assert!(serde_json::from_value::<WireOrder>(unknown)
            .unwrap()
            .normalized()
            .is_err());
        let mut stop = wire_order();
        stop["order_type"] = json!("stop_limit");
        stop["price_per_unit"] = json!("90");
        assert!(serde_json::from_value::<WireOrder>(stop)
            .unwrap()
            .normalized()
            .is_err());
    }

    async fn http_fixture(
        status: &str,
        body: &str,
        extra: &str,
    ) -> (CoinDCXClient, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\n{extra}Connection: close\r\n\r\n{body}", body.len());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let size = stream.read(&mut buffer).await.unwrap();
                if size == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..size]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length: usize = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        let client = CoinDCXClient::with_config("test-key", "test-secret", ClientConfig::default())
            .test_endpoint(format!("http://{address}"));
        (client, task)
    }

    #[tokio::test]
    async fn active_order_envelope_is_parsed_and_request_is_signed() {
        let mut wire = wire_order();
        wire["status"] = json!("open");
        wire["remaining_quantity"] = wire["total_quantity"].clone();
        wire["avg_price"] = json!(0);
        wire["fee_amount"] = json!(0);
        let body = json!({"orders": [wire]}).to_string();
        let (client, server) = http_fixture("200 OK", &body, "").await;
        let reports = client.active_orders(&Symbol::new("BTCUSDT")).await.unwrap();
        assert_eq!(reports.len(), 1);
        let request = server.await.unwrap();
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(headers
            .to_ascii_lowercase()
            .contains("x-auth-apikey: test-key"));
        assert!(headers.contains(&sign_request(body, "test-secret")));
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap()["market"],
            "BTCUSDT"
        );
    }

    #[tokio::test]
    async fn mutation_transport_does_not_retry_or_follow_redirects() {
        for (status, extra) in [
            ("500 Internal Server Error", ""),
            (
                "307 Temporary Redirect",
                "Location: http://127.0.0.1:1/credential-sink\r\n",
            ),
        ] {
            let (client, server) = http_fixture(status, "{}", extra).await;
            let order = BrokerOrder {
                client_id: "no-retry".into(),
                symbol: Symbol::new("BTCUSDT"),
                side: Side::Buy,
                kind: OrderType::Market,
                quantity: Money::ONE,
                limit: None,
                stop: None,
            };
            let result =
                tokio::time::timeout(std::time::Duration::from_millis(800), client.submit(&order))
                    .await;
            let error = result
                .expect("Mutation unexpectedly entered retry/backoff")
                .unwrap_err();
            assert!(error.downcast_ref::<ApiError>().is_some());
            let request = server.await.unwrap();
            assert!(request.starts_with("POST /exchange/v1/orders/create "));
        }
    }

    #[tokio::test]
    async fn only_explicit_order_not_found_maps_to_absence() {
        let (client, server) =
            http_fixture("404 Not Found", r#"{"message":"Order not found"}"#, "").await;
        assert!(client.lookup("missing").await.unwrap().is_none());
        server.await.unwrap();
        let (client, server) =
            http_fixture("404 Not Found", r#"{"message":"Route not found"}"#, "").await;
        assert!(client.lookup("missing").await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn public_ticker_uses_seconds_not_order_timestamp_milliseconds() {
        let now = Utc::now();
        let body =
            json!([{"market":"BTCUSDT","bid":"100.10","ask":"100.20","timestamp":now.timestamp()}])
                .to_string();
        let (client, server) = http_fixture("200 OK", &body, "").await;
        let quotes = client.quotes(&[Symbol::new("BTCUSDT")]).await.unwrap();
        quotes[0].validate(now).unwrap();
        assert_eq!(quotes[0].timestamp.timestamp(), now.timestamp());
        server.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "read-only public CoinDCX contract check; requires internet"]
    async fn public_spot_contract() {
        let client = CoinDCXClient::new("", "");
        let symbols = [Symbol::new("BTCUSDT"), Symbol::new("ETHUSDT")];
        let markets = client.markets(&symbols).await.unwrap();
        for symbol in &symbols {
            markets
                .iter()
                .find(|m| &m.symbol == symbol)
                .unwrap()
                .validate()
                .unwrap();
        }
        let quotes = client.quotes(&symbols).await.unwrap();
        assert_eq!(quotes.len(), symbols.len());
        for quote in quotes {
            quote.validate(Utc::now()).unwrap();
        }
    }

    #[tokio::test]
    async fn unrelated_market_metadata_cannot_break_a_requested_market() {
        let valid = json!({
            "coindcx_name":"BTCUSDT","base_currency_short_name":"USDT",
            "target_currency_short_name":"BTC","base_currency_precision":2,
            "target_currency_precision":5,"min_quantity":0.00001,
            "max_quantity":100,"max_quantity_market":10,"min_price":0.01,
            "max_price":1000000,"min_notional":5,"step":0.00001,
            "order_types":["limit_order","market_order","stop_limit"],
            "pair":"B-BTC_USDT","status":"active"
        });
        let body =
            json!([valid, {"coindcx_name":"INVALID","step":0,"status":"active"}]).to_string();
        let (client, server) = http_fixture("200 OK", &body, "").await;
        let markets = client.markets(&[Symbol::new("BTCUSDT")]).await.unwrap();
        assert_eq!(markets.len(), 1);
        markets[0].validate().unwrap();
        assert_eq!(markets[0].quote, "USDT");
        assert_eq!(markets[0].asset, "BTC");
        server.await.unwrap();
        let (client, server) = http_fixture("200 OK", &body, "").await;
        assert!(client.markets(&[Symbol::new("INVALID")]).await.is_err());
        server.await.unwrap();
    }
}
