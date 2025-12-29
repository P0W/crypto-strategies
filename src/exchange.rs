//! CoinDCX Exchange API client
//!
//! HTTP client for interacting with CoinDCX exchange API.

use anyhow::{Context, Result};
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const API_BASE_URL: &str = "https://api.coindcx.com";

#[derive(Debug, Clone)]
pub struct CoinDCXClient {
    api_key: String,
    api_secret: String,
    client: reqwest::Client,
}

impl CoinDCXClient {
    pub fn new(api_key: String, api_secret: String) -> Self {
        CoinDCXClient {
            api_key,
            api_secret,
            client: reqwest::Client::new(),
        }
    }

    fn generate_signature(&self, payload: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub async fn get_ticker(&self, symbol: &str) -> Result<Ticker> {
        let url = format!("{}/exchange/ticker", API_BASE_URL);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch ticker")?;

        let tickers: Vec<Ticker> = response.json().await.context("Failed to parse ticker")?;
        
        tickers
            .into_iter()
            .find(|t| t.market == symbol)
            .context(format!("Ticker not found for {}", symbol))
    }

    pub async fn place_order(&self, order: &OrderRequest) -> Result<OrderResponse> {
        let url = format!("{}/exchange/v1/orders/create", API_BASE_URL);
        
        let body = serde_json::to_string(order)?;
        let signature = self.generate_signature(&body);

        let response = self
            .client
            .post(&url)
            .header("X-AUTH-APIKEY", &self.api_key)
            .header("X-AUTH-SIGNATURE", signature)
            .json(order)
            .send()
            .await
            .context("Failed to place order")?;

        response.json().await.context("Failed to parse order response")
    }

    pub async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let url = format!("{}/exchange/v1/orders/cancel", API_BASE_URL);
        
        let request = CancelOrderRequest {
            id: order_id.to_string(),
        };

        let body = serde_json::to_string(&request)?;
        let signature = self.generate_signature(&body);

        self.client
            .post(&url)
            .header("X-AUTH-APIKEY", &self.api_key)
            .header("X-AUTH-SIGNATURE", signature)
            .json(&request)
            .send()
            .await
            .context("Failed to cancel order")?;

        Ok(())
    }

    pub async fn get_balances(&self) -> Result<Vec<Balance>> {
        let url = format!("{}/exchange/v1/users/balances", API_BASE_URL);
        
        let timestamp = Utc::now().timestamp_millis();
        let payload = format!("{{\"timestamp\":{}}}", timestamp);
        let signature = self.generate_signature(&payload);

        let response = self
            .client
            .post(&url)
            .header("X-AUTH-APIKEY", &self.api_key)
            .header("X-AUTH-SIGNATURE", signature)
            .json(&serde_json::json!({"timestamp": timestamp}))
            .send()
            .await
            .context("Failed to fetch balances")?;

        response.json().await.context("Failed to parse balances")
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ticker {
    pub market: String,
    pub last_price: String,
    pub bid: String,
    pub ask: String,
    pub volume: String,
    pub timestamp: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OrderRequest {
    pub side: String,          // "buy" or "sell"
    pub order_type: String,    // "limit_order" or "market_order"
    pub market: String,
    pub price_per_unit: Option<f64>,
    pub total_quantity: f64,
    pub timestamp: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OrderResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CancelOrderRequest {
    id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Balance {
    pub currency: String,
    pub balance: f64,
    pub locked_balance: f64,
}
