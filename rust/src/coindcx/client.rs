//! CoinDCX API Client
//!
//! A production-grade HTTP client for the CoinDCX exchange with:
//! - Automatic retry with exponential backoff
//! - Rate limiting
//! - Circuit breaker pattern for fault tolerance
//! - Comprehensive error handling
//!
//! # Example
//!
//! ```no_run
//! use crypto_strategies::coindcx::{CoinDCXClient, ClientConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create client with API credentials
//!     let client = CoinDCXClient::new("api_key", "api_secret");
//!
//!     // Get ticker for a market
//!     let ticker = client.get_ticker("BTCINR").await?;
//!     println!("BTC/INR price: {}", ticker.last_price);
//!
//!     // Get account balances
//!     let balances = client.get_balances().await?;
//!     for balance in balances {
//!         println!("{}: {}", balance.currency, balance.balance);
//!     }
//!
//!     Ok(())
//! }
//! ```

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::auth::{sign_request, Credentials};
use super::types::*;
use crate::common::{CircuitBreaker, CircuitBreakerConfig, RateLimiter, RateLimiterConfig};

/// Base URL for CoinDCX API
pub const API_BASE_URL: &str = "https://api.coindcx.com";

/// Base URL for public market data endpoints
pub const PUBLIC_BASE_URL: &str = "https://public.coindcx.com";

/// Client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Request timeout duration
    pub timeout: Duration,
    /// Rate limiter configuration
    pub rate_limiter: RateLimiterConfig,
    /// Circuit breaker configuration
    pub circuit_breaker: CircuitBreakerConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            timeout: Duration::from_secs(30),
            rate_limiter: RateLimiterConfig::default(),
            circuit_breaker: CircuitBreakerConfig::default(),
        }
    }
}

impl ClientConfig {
    /// Set maximum retry attempts
    pub fn with_max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set rate limit (requests per second)
    pub fn with_rate_limit(mut self, requests_per_second: usize) -> Self {
        self.rate_limiter = self.rate_limiter.with_rate(requests_per_second);
        self
    }

    /// Set circuit breaker failure threshold
    pub fn with_circuit_breaker_threshold(mut self, threshold: u32) -> Self {
        self.circuit_breaker = self.circuit_breaker.with_failure_threshold(threshold);
        self
    }
}

/// CoinDCX Exchange API Client
///
/// Provides methods to interact with the CoinDCX API including:
/// - Public endpoints (ticker, markets, orderbook)
/// - Authenticated endpoints (orders, balances, trades)
#[derive(Clone)]
pub struct CoinDCXClient {
    credentials: Credentials,
    http_client: Client,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    rate_limiter: RateLimiter,
    max_retries: u32,
}

impl CoinDCXClient {
    /// Create a new client with API credentials
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self::with_config(api_key, api_secret, ClientConfig::default())
    }

    /// Create a new client with custom configuration
    pub fn with_config(
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        config: ClientConfig,
    ) -> Self {
        let http_client = Client::builder()
            .timeout(config.timeout)
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            credentials: Credentials::new(api_key, api_secret),
            http_client,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new(config.circuit_breaker))),
            rate_limiter: RateLimiter::new(config.rate_limiter),
            max_retries: config.max_retries,
        }
    }

    /// Create a client from environment variables
    ///
    /// Expects `COINDCX_API_KEY` and `COINDCX_API_SECRET`
    pub fn from_env() -> Result<Self> {
        let credentials = Credentials::from_env()
            .context("Failed to load CoinDCX credentials from environment")?;
        Ok(Self::with_config(
            credentials.api_key(),
            credentials.api_secret(),
            ClientConfig::default(),
        ))
    }

    /// Execute a request with retry logic, rate limiting, and circuit breaker
    async fn execute_with_retry<F, Fut, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        // Check circuit breaker
        {
            let mut cb = self.circuit_breaker.lock().await;
            if !cb.can_attempt() {
                return Err(anyhow!("Circuit breaker is open, rejecting request"));
            }
        }

        // Rate limiting
        self.rate_limiter.acquire().await;

        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                // Exponential backoff: 1s, 2s, 4s, 8s...
                let delay = Duration::from_secs(2u64.pow(attempt - 1));
                tracing::debug!("Retrying after {}ms", delay.as_millis());
                sleep(delay).await;
            }

            match operation().await {
                Ok(result) => {
                    // Record success in circuit breaker
                    let mut cb = self.circuit_breaker.lock().await;
                    cb.record_success();
                    return Ok(result);
                }
                Err(e) => {
                    tracing::warn!(
                        "Request failed (attempt {}/{}): {}",
                        attempt + 1,
                        self.max_retries + 1,
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        // All retries exhausted, record failure
        {
            let mut cb = self.circuit_breaker.lock().await;
            cb.record_failure();
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Request failed after retries")))
    }

    /// Make an authenticated POST request
    async fn authenticated_post<T, R>(&self, endpoint: &str, body: &T) -> Result<R>
    where
        T: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", API_BASE_URL, endpoint);
        let json_body = serde_json::to_string(body)?;
        let signature = sign_request(&json_body, self.credentials.api_secret());

        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-AUTH-APIKEY", self.credentials.api_key())
            .header("X-AUTH-SIGNATURE", signature)
            .body(json_body)
            .send()
            .await
            .context("Failed to send request")?;

        let status = response.status();
        let text = response.text().await.context("Failed to read response")?;

        if !status.is_success() {
            return Err(anyhow!("API error ({}): {}", status, text));
        }

        serde_json::from_str(&text).context("Failed to parse response")
    }

    // ==================== PUBLIC ENDPOINTS ====================

    /// Get ticker information for all markets
    pub async fn get_all_tickers(&self) -> Result<Vec<Ticker>> {
        self.execute_with_retry(|| {
            let url = format!("{}/exchange/ticker", API_BASE_URL);
            let client = self.http_client.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch tickers")?;

                let text = response.text().await.context("Failed to read response")?;
                serde_json::from_str(&text).context("Failed to parse tickers")
            }
        })
        .await
    }

    /// Get ticker information for a specific market
    pub async fn get_ticker(&self, symbol: &str) -> Result<Ticker> {
        let symbol = symbol.to_string();
        self.execute_with_retry(|| {
            let url = format!("{}/exchange/ticker", API_BASE_URL);
            let client = self.http_client.clone();
            let symbol = symbol.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch ticker")?;

                let text = response.text().await.context("Failed to read response")?;
                let tickers: Vec<Ticker> =
                    serde_json::from_str(&text).context("Failed to parse ticker JSON")?;

                tickers
                    .into_iter()
                    .find(|t| t.market == symbol)
                    .ok_or_else(|| anyhow!("Ticker not found for {}", symbol))
            }
        })
        .await
    }

    /// Get list of all available markets
    pub async fn get_markets(&self) -> Result<Vec<String>> {
        self.execute_with_retry(|| {
            let url = format!("{}/exchange/v1/markets", API_BASE_URL);
            let client = self.http_client.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch markets")?;

                let text = response.text().await.context("Failed to read response")?;
                serde_json::from_str(&text).context("Failed to parse markets")
            }
        })
        .await
    }

    /// Get detailed information for all markets
    pub async fn get_markets_details(&self) -> Result<Vec<MarketDetails>> {
        self.execute_with_retry(|| {
            let url = format!("{}/exchange/v1/markets_details", API_BASE_URL);
            let client = self.http_client.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch market details")?;

                let text = response.text().await.context("Failed to read response")?;
                serde_json::from_str(&text).context("Failed to parse market details")
            }
        })
        .await
    }

    /// Get order book for a market pair
    pub async fn get_orderbook(&self, pair: &str) -> Result<OrderBook> {
        let pair = pair.to_string();
        self.execute_with_retry(|| {
            let url = format!(
                "{}/market_data/orderbook?pair={}",
                PUBLIC_BASE_URL,
                pair.clone()
            );
            let client = self.http_client.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch orderbook")?;

                let text = response.text().await.context("Failed to read response")?;
                serde_json::from_str(&text).context("Failed to parse orderbook")
            }
        })
        .await
    }

    /// Get candle/OHLCV data for a market
    ///
    /// # Arguments
    /// * `pair` - Trading pair (e.g., "B-BTC_USDT")
    /// * `interval` - Candle interval (1m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 1d, 3d, 1w, 1M)
    /// * `limit` - Number of candles (default 500, max 1000)
    pub async fn get_candles(
        &self,
        pair: &str,
        interval: &str,
        limit: Option<u32>,
    ) -> Result<Vec<Candle>> {
        let pair = pair.to_string();
        let interval = interval.to_string();
        self.execute_with_retry(|| {
            let mut url = format!(
                "{}/market_data/candles?pair={}&interval={}",
                PUBLIC_BASE_URL,
                pair.clone(),
                interval.clone()
            );
            if let Some(l) = limit {
                url.push_str(&format!("&limit={}", l));
            }
            let client = self.http_client.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch candles")?;

                let text = response.text().await.context("Failed to read response")?;
                serde_json::from_str(&text).context("Failed to parse candles")
            }
        })
        .await
    }

    // ==================== AUTHENTICATED ENDPOINTS ====================

    /// Get user balances
    pub async fn get_balances(&self) -> Result<Vec<Balance>> {
        let request = TimestampRequest::new();
        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/users/balances";
            let req = request.clone();
            let this = self.clone();

            async move { this.authenticated_post(endpoint, &req).await }
        })
        .await
    }

    /// Get user info
    pub async fn get_user_info(&self) -> Result<Vec<UserInfo>> {
        let request = TimestampRequest::new();
        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/users/info";
            let req = request.clone();
            let this = self.clone();

            async move { this.authenticated_post(endpoint, &req).await }
        })
        .await
    }

    /// Place a new order
    pub async fn place_order(&self, order: &OrderRequest) -> Result<OrdersResponse> {
        let order = order.clone();
        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/orders/create";
            let ord = order.clone();
            let this = self.clone();

            async move { this.authenticated_post(endpoint, &ord).await }
        })
        .await
    }

    /// Cancel an order by ID
    pub async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let request = CancelOrderRequest::new(order_id);
        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/orders/cancel";
            let req = request.clone();
            let this = self.clone();

            async move {
                let _: serde_json::Value = this.authenticated_post(endpoint, &req).await?;
                Ok(())
            }
        })
        .await
    }

    /// Get order status
    pub async fn get_order_status(&self, order_id: &str) -> Result<OrderResponse> {
        let request = OrderStatusRequest::by_id(order_id);
        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/orders/status";
            let req = request.clone();
            let this = self.clone();

            async move { this.authenticated_post(endpoint, &req).await }
        })
        .await
    }

    /// Get active orders for a market
    pub async fn get_active_orders(&self, market: &str) -> Result<Vec<OrderResponse>> {
        let request = ActiveOrdersRequest::new(market);
        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/orders/active_orders";
            let req = request.clone();
            let this = self.clone();

            async move { this.authenticated_post(endpoint, &req).await }
        })
        .await
    }

    /// Cancel all orders for a market
    pub async fn cancel_all_orders(&self, market: &str, side: Option<OrderSide>) -> Result<()> {
        let mut request = ActiveOrdersRequest::new(market);
        if let Some(s) = side {
            request = request.with_side(s);
        }

        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/orders/cancel_all";
            let req = request.clone();
            let this = self.clone();

            async move {
                let _: serde_json::Value = this.authenticated_post(endpoint, &req).await?;
                Ok(())
            }
        })
        .await
    }

    /// Get trade history
    pub async fn get_trade_history(&self, limit: Option<u32>) -> Result<Vec<Trade>> {
        #[derive(serde::Serialize, Clone)]
        struct TradeHistoryRequest {
            timestamp: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<u32>,
        }

        let request = TradeHistoryRequest {
            timestamp: chrono::Utc::now().timestamp_millis(),
            limit,
        };

        self.execute_with_retry(|| {
            let endpoint = "/exchange/v1/orders/trade_history";
            let req = request.clone();
            let this = self.clone();

            async move { this.authenticated_post(endpoint, &req).await }
        })
        .await
    }

    // ==================== UTILITY METHODS ====================

    /// Check if the API is reachable
    pub async fn health_check(&self) -> Result<bool> {
        match self.get_markets().await {
            Ok(markets) => Ok(!markets.is_empty()),
            Err(_) => Ok(false),
        }
    }

    /// Get the current circuit breaker state
    pub async fn circuit_breaker_state(&self) -> crate::common::CircuitState {
        let cb = self.circuit_breaker.lock().await;
        cb.state()
    }

    /// Reset the circuit breaker
    pub async fn reset_circuit_breaker(&self) {
        let mut cb = self.circuit_breaker.lock().await;
        cb.reset();
    }

    /// Get available rate limit permits
    pub fn available_rate_limit(&self) -> usize {
        self.rate_limiter.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_client_config_builder() {
        let config = ClientConfig::default()
            .with_max_retries(5)
            .with_timeout(Duration::from_secs(60))
            .with_rate_limit(20)
            .with_circuit_breaker_threshold(10);

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.timeout, Duration::from_secs(60));
        assert_eq!(config.rate_limiter.max_requests_per_second, 20);
        assert_eq!(config.circuit_breaker.failure_threshold, 10);
    }

    #[test]
    fn test_client_creation() {
        let client = CoinDCXClient::new("test_key", "test_secret");
        // Should not panic
        assert_eq!(client.max_retries, 3);
    }

    #[test]
    fn test_client_with_config() {
        let config = ClientConfig::default().with_max_retries(5);
        let client = CoinDCXClient::with_config("test_key", "test_secret", config);
        assert_eq!(client.max_retries, 5);
    }

    #[tokio::test]
    async fn test_circuit_breaker_state() {
        let client = CoinDCXClient::new("test_key", "test_secret");
        let state = client.circuit_breaker_state().await;
        assert_eq!(state, crate::common::CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_reset_circuit_breaker() {
        let client = CoinDCXClient::new("test_key", "test_secret");

        // Manually record failures to open the circuit
        {
            let mut cb = client.circuit_breaker.lock().await;
            for _ in 0..10 {
                cb.record_failure();
            }
        }

        // Reset
        client.reset_circuit_breaker().await;

        // Should be closed again
        let state = client.circuit_breaker_state().await;
        assert_eq!(state, crate::common::CircuitState::Closed);
    }

    #[test]
    fn test_api_urls() {
        assert_eq!(API_BASE_URL, "https://api.coindcx.com");
        assert_eq!(PUBLIC_BASE_URL, "https://public.coindcx.com");
    }
}
