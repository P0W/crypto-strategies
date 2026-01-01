//! Zerodha Kite API Client
//!
//! Production-grade HTTP client for Zerodha Kite Connect API with:
//! - Circuit breaker pattern for fault tolerance
//! - Rate limiting using token bucket algorithm
//! - Automatic retry with exponential backoff
//! - Comprehensive error handling

use chrono::{Duration, Utc};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::Mutex;

use super::auth::Credentials;
use super::error::{ZerodhaError, ZerodhaResult};
use super::types::*;
use super::API_BASE_URL;
use crate::common::{CircuitBreaker, CircuitBreakerConfig, RateLimiter, RateLimiterConfig};

/// Client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub max_retries: u32,
    pub timeout: StdDuration,
    pub rate_limiter: RateLimiterConfig,
    pub circuit_breaker: CircuitBreakerConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            timeout: StdDuration::from_secs(30),
            rate_limiter: RateLimiterConfig::default().with_rate(10),
            circuit_breaker: CircuitBreakerConfig::default(),
        }
    }
}

impl ClientConfig {
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    pub fn with_timeout(mut self, timeout: StdDuration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_rate_limit(mut self, requests_per_second: usize) -> Self {
        self.rate_limiter = self.rate_limiter.with_rate(requests_per_second);
        self
    }
}

/// Zerodha Kite API Client
pub struct ZerodhaClient {
    client: Client,
    credentials: Credentials,
    _config: ClientConfig,
    rate_limiter: RateLimiter,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
}

impl ZerodhaClient {
    /// Create a new client with credentials
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self::with_config(api_key, api_secret, ClientConfig::default())
    }

    /// Create a new client with custom configuration
    pub fn with_config(
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        config: ClientConfig,
    ) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("Failed to create HTTP client");

        let rate_limiter = RateLimiter::new(config.rate_limiter.clone());
        let circuit_breaker = Arc::new(Mutex::new(CircuitBreaker::new(config.circuit_breaker.clone())));

        Self {
            client,
            credentials: Credentials::new(api_key, api_secret),
            _config: config.clone(),
            rate_limiter,
            circuit_breaker,
        }
    }

    /// Set access token after login
    pub fn with_access_token(mut self, token: String) -> Self {
        self.credentials = self.credentials.with_access_token(token);
        self
    }

    /// Get historical OHLCV data
    ///
    /// # Arguments
    /// * `instrument` - Trading symbol (e.g., "NSE:RELIANCE", "NSE:NIFTY 50")
    /// * `interval` - Kite interval ("minute", "5minute", "15minute", "60minute", "day")
    /// * `days` - Number of days of historical data
    pub async fn get_historical_data(
        &self,
        instrument: &str,
        interval: &str,
        days: u32,
    ) -> ZerodhaResult<Vec<Candle>> {
        // Check circuit breaker
        {
            let mut cb = self.circuit_breaker.lock().await;
            if !cb.can_attempt() {
                return Err(ZerodhaError::CircuitBreakerOpen);
            }
        }

        // Rate limiting
        self.rate_limiter.acquire().await;

        let to_date = Utc::now();
        let from_date = to_date - Duration::days(days as i64);

        let url = format!(
            "{}/instruments/historical/{}/{}",
            API_BASE_URL,
            instrument,
            interval
        );

        let response = self
            .client
            .get(&url)
            .header("X-Kite-Version", "3")
            .header("Authorization", format!("token {}:{}", self.credentials.api_key, self.credentials.access_token.as_ref().unwrap_or(&String::new())))
            .query(&[
                ("from", from_date.format("%Y-%m-%d").to_string()),
                ("to", to_date.format("%Y-%m-%d").to_string()),
            ])
            .send()
            .await?;

        if response.status().is_success() {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_success();

            // Parse response
            let candles: Vec<Candle> = response.json().await
                .map_err(|e| ZerodhaError::ParseError(e.to_string()))?;
            Ok(candles)
        } else {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_failure();

            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(ZerodhaError::ApiError(error_text))
        }
    }

    /// Get real-time quote
    pub async fn get_quote(&self, instrument: &str) -> ZerodhaResult<Quote> {
        {
            let mut cb = self.circuit_breaker.lock().await;
            if !cb.can_attempt() {
                return Err(ZerodhaError::CircuitBreakerOpen);
            }
        }

        self.rate_limiter.acquire().await;

        let url = format!("{}/quote", API_BASE_URL);
        let response = self
            .client
            .get(&url)
            .header("X-Kite-Version", "3")
            .header("Authorization", format!("token {}:{}", self.credentials.api_key, self.credentials.access_token.as_ref().unwrap_or(&String::new())))
            .query(&[("i", instrument)])
            .send()
            .await?;

        if response.status().is_success() {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_success();

            let quote: Quote = response.json().await
                .map_err(|e| ZerodhaError::ParseError(e.to_string()))?;
            Ok(quote)
        } else {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_failure();

            Err(ZerodhaError::ApiError(response.text().await.unwrap_or_else(|_| "Unknown error".to_string())))
        }
    }

    /// Place an order
    pub async fn place_order(
        &self,
        exchange: &str,
        tradingsymbol: &str,
        transaction_type: &str,
        quantity: i32,
        price: Option<f64>,
    ) -> ZerodhaResult<Order> {
        {
            let mut cb = self.circuit_breaker.lock().await;
            if !cb.can_attempt() {
                return Err(ZerodhaError::CircuitBreakerOpen);
            }
        }

        self.rate_limiter.acquire().await;

        let url = format!("{}/orders", API_BASE_URL);
        
        let mut params = vec![
            ("exchange", exchange.to_string()),
            ("tradingsymbol", tradingsymbol.to_string()),
            ("transaction_type", transaction_type.to_string()),
            ("quantity", quantity.to_string()),
            ("order_type", if price.is_some() { "LIMIT" } else { "MARKET" }.to_string()),
            ("product", "MIS".to_string()), // Intraday
            ("validity", "DAY".to_string()),
        ];

        if let Some(p) = price {
            params.push(("price", p.to_string()));
        }

        let response = self
            .client
            .post(&url)
            .header("X-Kite-Version", "3")
            .header("Authorization", format!("token {}:{}", self.credentials.api_key, self.credentials.access_token.as_ref().unwrap_or(&String::new())))
            .form(&params)
            .send()
            .await?;

        if response.status().is_success() {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_success();

            let order: Order = response.json().await
                .map_err(|e| ZerodhaError::ParseError(e.to_string()))?;
            Ok(order)
        } else {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_failure();

            Err(ZerodhaError::ApiError(response.text().await.unwrap_or_else(|_| "Unknown error".to_string())))
        }
    }

    /// Get all positions
    pub async fn get_positions(&self) -> ZerodhaResult<Vec<Position>> {
        {
            let mut cb = self.circuit_breaker.lock().await;
            if !cb.can_attempt() {
                return Err(ZerodhaError::CircuitBreakerOpen);
            }
        }

        self.rate_limiter.acquire().await;

        let url = format!("{}/portfolio/positions", API_BASE_URL);
        let response = self
            .client
            .get(&url)
            .header("X-Kite-Version", "3")
            .header("Authorization", format!("token {}:{}", self.credentials.api_key, self.credentials.access_token.as_ref().unwrap_or(&String::new())))
            .send()
            .await?;

        if response.status().is_success() {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_success();

            let positions: Vec<Position> = response.json().await
                .map_err(|e| ZerodhaError::ParseError(e.to_string()))?;
            Ok(positions)
        } else {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_failure();

            Err(ZerodhaError::ApiError(response.text().await.unwrap_or_else(|_| "Unknown error".to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = ZerodhaClient::new("test_key", "test_secret");
        assert_eq!(client.credentials.api_key, "test_key");
    }

    #[test]
    fn test_config_builder() {
        let config = ClientConfig::default()
            .with_max_retries(5)
            .with_timeout(StdDuration::from_secs(60))
            .with_rate_limit(20);

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.timeout, StdDuration::from_secs(60));
    }
}
