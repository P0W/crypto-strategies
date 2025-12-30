//! Robust CoinDCX Exchange API client
//!
//! Production-grade HTTP client with:
//! - Exponential backoff with retries
//! - Rate limiting
//! - Circuit breaker pattern
//! - Request timeouts
//! - Comprehensive error handling

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::time::{sleep, Instant};

type HmacSha256 = Hmac<Sha256>;

const API_BASE_URL: &str = "https://api.coindcx.com";
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_RATE_LIMIT_PER_SECOND: usize = 10;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failing, reject requests
    HalfOpen, // Testing if service recovered
}

/// Circuit breaker for managing API failures
struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    failure_threshold: u32,
    success_threshold: u32,
    last_failure_time: Option<Instant>,
    timeout: Duration,
}

impl CircuitBreaker {
    fn new(failure_threshold: u32, timeout: Duration) -> Self {
        CircuitBreaker {
            state: CircuitState::Closed,
            failure_count: 0,
            failure_threshold,
            success_threshold: 2, // Need 2 successes in HalfOpen to close
            last_failure_time: None,
            timeout,
        }
    }

    fn can_attempt(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= self.timeout {
                        tracing::info!("Circuit breaker moving to HalfOpen state");
                        self.state = CircuitState::HalfOpen;
                        self.failure_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.failure_count += 1;
                if self.failure_count >= self.success_threshold {
                    tracing::info!("Circuit breaker closed after successful recovery");
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                }
            }
            CircuitState::Open => {}
        }
    }

    fn record_failure(&mut self) {
        self.last_failure_time = Some(Instant::now());

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.failure_threshold {
                    tracing::warn!("Circuit breaker opened due to failures");
                    self.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                tracing::warn!("Circuit breaker re-opened due to failure in HalfOpen state");
                self.state = CircuitState::Open;
                self.failure_count = 0;
            }
            CircuitState::Open => {}
        }
    }
}

/// Rate limiter using token bucket algorithm
struct RateLimiter {
    permits: Arc<Semaphore>,
    refill_rate: usize,
    last_refill: Arc<Mutex<Instant>>,
}

impl RateLimiter {
    fn new(max_requests_per_second: usize) -> Self {
        RateLimiter {
            permits: Arc::new(Semaphore::new(max_requests_per_second)),
            refill_rate: max_requests_per_second,
            last_refill: Arc::new(Mutex::new(Instant::now())),
        }
    }

    async fn acquire(&self) {
        // Refill permits if a second has passed
        {
            let mut last_refill = self.last_refill.lock().await;
            let elapsed = last_refill.elapsed();
            if elapsed >= Duration::from_secs(1) {
                // Add available permits (up to max)
                let to_add = self
                    .refill_rate
                    .saturating_sub(self.permits.available_permits());
                if to_add > 0 {
                    self.permits.add_permits(to_add);
                }
                *last_refill = Instant::now();
            }
        }

        // Wait for a permit
        let _ = self.permits.acquire().await.expect("Semaphore closed");
    }
}

/// Robust CoinDCX exchange client
#[derive(Clone)]
pub struct RobustCoinDCXClient {
    api_key: String,
    api_secret: String,
    client: reqwest::Client,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    rate_limiter: Arc<RateLimiter>,
    max_retries: u32,
}

impl RobustCoinDCXClient {
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self::with_config(
            api_key,
            api_secret,
            DEFAULT_MAX_RETRIES,
            DEFAULT_RATE_LIMIT_PER_SECOND,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        )
    }

    pub fn with_config(
        api_key: String,
        api_secret: String,
        max_retries: u32,
        rate_limit_per_second: usize,
        timeout: Duration,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("Failed to build HTTP client");

        RobustCoinDCXClient {
            api_key,
            api_secret,
            client,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new(
                5,                       // Open circuit after 5 failures
                Duration::from_secs(60), // Stay open for 60 seconds
            ))),
            rate_limiter: Arc::new(RateLimiter::new(rate_limit_per_second)),
            max_retries,
        }
    }

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

    pub async fn get_ticker(&self, symbol: &str) -> Result<Ticker> {
        let symbol = symbol.to_string();
        self.execute_with_retry(|| {
            let url = format!("{}/exchange/ticker", API_BASE_URL);
            let client = self.client.clone();
            let symbol = symbol.clone();

            async move {
                let response = client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to fetch ticker")?;

                let text = response.text().await.context("Failed to read response")?;

                let tickers: Vec<Ticker> =
                    serde_json::from_str(&text).context(format!("Failed to parse ticker JSON"))?;

                tickers
                    .into_iter()
                    .find(|t| t.market == symbol)
                    .context(format!("Ticker not found for {}", symbol))
            }
        })
        .await
    }

    pub async fn place_order(&self, order: &OrderRequest) -> Result<OrderResponse> {
        let order = order.clone();
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();

        self.execute_with_retry(|| {
            let url = format!("{}/exchange/v1/orders/create", API_BASE_URL);
            let client = self.client.clone();
            let order = order.clone();
            let api_key = api_key.clone();
            let api_secret = api_secret.clone();

            async move {
                let body = serde_json::to_string(&order)?;
                let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes())
                    .expect("HMAC can take key of any size");
                mac.update(body.as_bytes());
                let signature = hex::encode(mac.finalize().into_bytes());

                let response = client
                    .post(&url)
                    .header("X-AUTH-APIKEY", api_key)
                    .header("X-AUTH-SIGNATURE", signature)
                    .json(&order)
                    .send()
                    .await
                    .context("Failed to place order")?;

                response
                    .json()
                    .await
                    .context("Failed to parse order response")
            }
        })
        .await
    }

    pub async fn cancel_order(&self, order_id: &str) -> Result<()> {
        let order_id = order_id.to_string();
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();

        self.execute_with_retry(|| {
            let url = format!("{}/exchange/v1/orders/cancel", API_BASE_URL);
            let client = self.client.clone();
            let order_id = order_id.clone();
            let api_key = api_key.clone();
            let api_secret = api_secret.clone();

            async move {
                let request = CancelOrderRequest {
                    id: order_id.clone(),
                };

                let body = serde_json::to_string(&request)?;
                let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes())
                    .expect("HMAC can take key of any size");
                mac.update(body.as_bytes());
                let signature = hex::encode(mac.finalize().into_bytes());

                client
                    .post(&url)
                    .header("X-AUTH-APIKEY", api_key)
                    .header("X-AUTH-SIGNATURE", signature)
                    .json(&request)
                    .send()
                    .await
                    .context("Failed to cancel order")?;

                Ok(())
            }
        })
        .await
    }

    pub async fn get_balances(&self) -> Result<Vec<Balance>> {
        let api_key = self.api_key.clone();
        let api_secret = self.api_secret.clone();

        self.execute_with_retry(|| {
            let url = format!("{}/exchange/v1/users/balances", API_BASE_URL);
            let client = self.client.clone();
            let api_key = api_key.clone();
            let api_secret = api_secret.clone();

            async move {
                let timestamp = Utc::now().timestamp_millis();
                let payload = format!("{{\"timestamp\":{}}}", timestamp);
                let mut mac = HmacSha256::new_from_slice(api_secret.as_bytes())
                    .expect("HMAC can take key of any size");
                mac.update(payload.as_bytes());
                let signature = hex::encode(mac.finalize().into_bytes());

                let response = client
                    .post(&url)
                    .header("X-AUTH-APIKEY", api_key)
                    .header("X-AUTH-SIGNATURE", signature)
                    .json(&serde_json::json!({"timestamp": timestamp}))
                    .send()
                    .await
                    .context("Failed to fetch balances")?;

                response.json().await.context("Failed to parse balances")
            }
        })
        .await
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ticker {
    pub market: String,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub last_price: String,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub bid: String,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub ask: String,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub volume: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub change_24_hour: Option<String>,
    #[serde(default)]
    pub high: Option<String>,
    #[serde(default)]
    pub low: Option<String>,
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrNumber;

    impl<'de> de::Visitor<'de> for StringOrNumber {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub side: String,       // "buy" or "sell"
    pub order_type: String, // "limit_order" or "market_order"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_transitions() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(5));

        // Start in Closed state
        assert_eq!(cb.state, CircuitState::Closed);
        assert!(cb.can_attempt());

        // Record failures
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open); // Should open after 3 failures
        assert!(!cb.can_attempt()); // Immediately after opening, should reject
    }
}
