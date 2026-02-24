use async_trait::async_trait;
use chrono::DateTime;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, warn};
use uuid::Uuid;

use harman::error::ExchangeError;
use harman::exchange::ExchangeAdapter;
use harman::types::{
    Action, Balance, ExchangeFill, ExchangeOrderState, ExchangeOrderStatus, OrderRequest,
    Position, Side,
};
use ssmd_connector_lib::kalshi::auth::KalshiCredentials;

use crate::types::*;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MIN_REQUEST_GAP: Duration = Duration::from_millis(200);

/// Kalshi REST trading client
pub struct KalshiClient {
    http: Client,
    credentials: KalshiCredentials,
    base_url: String,
    last_request: tokio::sync::Mutex<tokio::time::Instant>,
}

impl KalshiClient {
    /// Create a new Kalshi client
    ///
    /// `base_url` should be either:
    /// - `https://demo-api.kalshi.co` for demo
    /// - `https://trading-api.kalshi.com` for production
    pub fn new(credentials: KalshiCredentials, base_url: String) -> Self {
        let http = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .expect("failed to build HTTP client");

        Self {
            http,
            credentials,
            base_url,
            last_request: tokio::sync::Mutex::new(tokio::time::Instant::now()),
        }
    }

    /// Enforce minimum gap between requests (rate limiting)
    async fn throttle(&self) {
        let mut last = self.last_request.lock().await;
        let elapsed = last.elapsed();
        if elapsed < MIN_REQUEST_GAP {
            tokio::time::sleep(MIN_REQUEST_GAP - elapsed).await;
        }
        *last = tokio::time::Instant::now();
    }

    /// Make an authenticated GET request
    async fn get(&self, path: &str) -> Result<reqwest::Response, ExchangeError> {
        self.throttle().await;

        let (timestamp, signature) = self
            .credentials
            .sign_rest_request("GET", path)
            .map_err(|e| ExchangeError::Auth(e.to_string()))?;

        let url = format!("{}{}", self.base_url, path);
        debug!(url = %url, "GET request");

        let resp = self
            .http
            .get(&url)
            .header("KALSHI-ACCESS-KEY", &self.credentials.api_key)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ExchangeError::Timeout {
                        timeout_ms: DEFAULT_TIMEOUT.as_millis() as u64,
                    }
                } else {
                    ExchangeError::Connection(e.to_string())
                }
            })?;

        self.check_rate_limit(&resp)?;
        Ok(resp)
    }

    /// Make an authenticated POST request
    async fn post<B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<reqwest::Response, ExchangeError> {
        self.throttle().await;

        let (timestamp, signature) = self
            .credentials
            .sign_rest_request("POST", path)
            .map_err(|e| ExchangeError::Auth(e.to_string()))?;

        let url = format!("{}{}", self.base_url, path);
        debug!(url = %url, "POST request");

        let resp = self
            .http
            .post(&url)
            .header("KALSHI-ACCESS-KEY", &self.credentials.api_key)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ExchangeError::Timeout {
                        timeout_ms: DEFAULT_TIMEOUT.as_millis() as u64,
                    }
                } else {
                    ExchangeError::Connection(e.to_string())
                }
            })?;

        self.check_rate_limit(&resp)?;
        Ok(resp)
    }

    /// Make an authenticated DELETE request
    async fn delete(&self, path: &str) -> Result<reqwest::Response, ExchangeError> {
        self.throttle().await;

        let (timestamp, signature) = self
            .credentials
            .sign_rest_request("DELETE", path)
            .map_err(|e| ExchangeError::Auth(e.to_string()))?;

        let url = format!("{}{}", self.base_url, path);
        debug!(url = %url, "DELETE request");

        let resp = self
            .http
            .delete(&url)
            .header("KALSHI-ACCESS-KEY", &self.credentials.api_key)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ExchangeError::Timeout {
                        timeout_ms: DEFAULT_TIMEOUT.as_millis() as u64,
                    }
                } else {
                    ExchangeError::Connection(e.to_string())
                }
            })?;

        self.check_rate_limit(&resp)?;
        Ok(resp)
    }

    /// Check if response is rate limited
    fn check_rate_limit(&self, resp: &reqwest::Response) -> Result<(), ExchangeError> {
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1000);
            return Err(ExchangeError::RateLimited {
                retry_after_ms: retry_after * 1000,
            });
        }
        Ok(())
    }

    /// Map a Kalshi order status string to our ExchangeOrderState
    fn map_order_status(order: &KalshiOrder) -> ExchangeOrderState {
        match order.status.as_str() {
            "resting" => ExchangeOrderState::Resting,
            "executed" => ExchangeOrderState::Executed,
            "canceled" | "cancelled" => ExchangeOrderState::Cancelled,
            _ => {
                warn!(status = %order.status, "unknown Kalshi order status");
                ExchangeOrderState::NotFound
            }
        }
    }

    /// Parse a Kalshi side string to our Side enum
    fn parse_side(s: &str) -> Side {
        match s {
            "no" => Side::No,
            _ => Side::Yes,
        }
    }

    /// Parse a Kalshi action string to our Action enum
    fn parse_action(s: &str) -> Action {
        match s {
            "sell" => Action::Sell,
            _ => Action::Buy,
        }
    }
}

#[async_trait]
impl ExchangeAdapter for KalshiClient {
    async fn submit_order(&self, order: &OrderRequest) -> Result<String, ExchangeError> {
        // Kalshi API always uses yes_price for both Yes and No side orders.
        // For No-side orders, the exchange interprets yes_price as the complement
        // (i.e., no_price = 100 - yes_price). We pass our price_cents directly.
        let body = KalshiOrderRequest {
            ticker: order.ticker.clone(),
            client_order_id: order.client_order_id.to_string(),
            side: order.side.to_string(),
            action: order.action.to_string(),
            order_type: "limit".to_string(),
            count: order.quantity,
            yes_price: order.price_cents,
            time_in_force: order.time_in_force.to_string(),
            subaccount: 0,
        };

        let resp = self
            .post("/trade-api/v2/portfolio/orders", &body)
            .await?;

        let status = resp.status();
        if status.is_success() {
            let order_resp: KalshiOrderResponse = resp
                .json()
                .await
                .map_err(|e| ExchangeError::Unexpected(e.to_string()))?;
            Ok(order_resp.order.order_id)
        } else {
            let error_body = resp.text().await.unwrap_or_default();
            Err(ExchangeError::Rejected {
                reason: format!("HTTP {}: {}", status, error_body),
            })
        }
    }

    async fn cancel_order(&self, exchange_order_id: &str) -> Result<(), ExchangeError> {
        let path = format!("/trade-api/v2/portfolio/orders/{}", exchange_order_id);
        let resp = self.delete(&path).await?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else if status == reqwest::StatusCode::NOT_FOUND {
            Err(ExchangeError::NotFound(Uuid::nil()))
        } else {
            let error_body = resp.text().await.unwrap_or_default();
            Err(ExchangeError::Rejected {
                reason: format!("cancel failed HTTP {}: {}", status, error_body),
            })
        }
    }

    async fn cancel_all_orders(&self) -> Result<i32, ExchangeError> {
        let resp = self
            .delete("/trade-api/v2/portfolio/orders/batched")
            .await?;

        let status = resp.status();
        if status.is_success() {
            let cancel_resp: KalshiBatchCancelResponse = resp
                .json()
                .await
                .map_err(|e| ExchangeError::Unexpected(e.to_string()))?;
            Ok(cancel_resp.orders_cancelled)
        } else {
            let error_body = resp.text().await.unwrap_or_default();
            Err(ExchangeError::Rejected {
                reason: format!("mass cancel failed HTTP {}: {}", status, error_body),
            })
        }
    }

    async fn get_order_by_client_id(
        &self,
        client_order_id: Uuid,
    ) -> Result<ExchangeOrderStatus, ExchangeError> {
        let path = format!(
            "/trade-api/v2/portfolio/orders?client_order_id={}",
            client_order_id
        );
        let resp = self.get(&path).await?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ExchangeError::Unexpected(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let orders_resp: KalshiOrdersResponse = resp
            .json()
            .await
            .map_err(|e| ExchangeError::Unexpected(e.to_string()))?;

        let order = orders_resp
            .orders
            .into_iter()
            .find(|o| {
                o.client_order_id
                    .as_deref()
                    == Some(&client_order_id.to_string())
            })
            .ok_or(ExchangeError::NotFound(client_order_id))?;

        let filled = order.filled_count();
        let remaining = order.effective_remaining();

        Ok(ExchangeOrderStatus {
            exchange_order_id: order.order_id.clone(),
            status: Self::map_order_status(&order),
            filled_quantity: filled,
            remaining_quantity: remaining,
        })
    }

    async fn get_positions(&self) -> Result<Vec<Position>, ExchangeError> {
        let resp = self
            .get("/trade-api/v2/portfolio/positions")
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ExchangeError::Unexpected(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let positions_resp: KalshiPositionsResponse = resp
            .json()
            .await
            .map_err(|e| ExchangeError::Unexpected(e.to_string()))?;

        Ok(positions_resp
            .market_positions
            .into_iter()
            .map(|p| Position {
                ticker: p.ticker,
                side: if p.position >= 0 {
                    Side::Yes
                } else {
                    Side::No
                },
                quantity: p.position.unsigned_abs() as i32,
                market_value_cents: p.market_exposure,
            })
            .collect())
    }

    async fn get_fills(&self) -> Result<Vec<ExchangeFill>, ExchangeError> {
        let resp = self.get("/trade-api/v2/portfolio/fills").await?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ExchangeError::Unexpected(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let fills_resp: KalshiFillsResponse = resp
            .json()
            .await
            .map_err(|e| ExchangeError::Unexpected(e.to_string()))?;

        Ok(fills_resp
            .fills
            .into_iter()
            .map(|f| {
                let filled_at = DateTime::parse_from_rfc3339(&f.created_time)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                ExchangeFill {
                    trade_id: f.trade_id,
                    order_id: f.order_id,
                    ticker: f.ticker,
                    side: Self::parse_side(&f.side),
                    action: Self::parse_action(&f.action),
                    price_cents: f.yes_price as i32,
                    quantity: f.count as i32,
                    is_taker: f.is_taker,
                    filled_at,
                }
            })
            .collect())
    }

    async fn get_balance(&self) -> Result<Balance, ExchangeError> {
        let resp = self
            .get("/trade-api/v2/portfolio/balance")
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_body = resp.text().await.unwrap_or_default();
            return Err(ExchangeError::Unexpected(format!(
                "HTTP {}: {}",
                status, error_body
            )));
        }

        let balance_resp: KalshiBalanceResponse = resp
            .json()
            .await
            .map_err(|e| ExchangeError::Unexpected(e.to_string()))?;

        Ok(Balance {
            available_cents: balance_resp.balance.balance,
            total_cents: balance_resp.balance.balance + balance_resp.balance.payout,
        })
    }
}

impl std::fmt::Debug for KalshiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KalshiClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn setup() -> (MockServer, KalshiClient) {
        let server = MockServer::start().await;

        // Generate a test RSA key for signing
        use rsa::pkcs8::EncodePrivateKey;
        use rsa::RsaPrivateKey;
        let mut rng = rand_core::OsRng;
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();

        let credentials =
            KalshiCredentials::new("test-api-key".to_string(), pem.as_str()).unwrap();
        let client = KalshiClient::new(credentials, server.uri());

        (server, client)
    }

    fn test_order_request() -> OrderRequest {
        OrderRequest {
            client_order_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            ticker: "KXBTCD-26FEB-T100000".to_string(),
            side: Side::Yes,
            action: Action::Buy,
            quantity: 10,
            price_cents: 50,
            time_in_force: harman::types::TimeInForce::Gtc,
        }
    }

    #[tokio::test]
    async fn test_submit_order_success() {
        let (server, client) = setup().await;

        Mock::given(method("POST"))
            .and(path("/trade-api/v2/portfolio/orders"))
            .and(header("KALSHI-ACCESS-KEY", "test-api-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "order": {
                    "order_id": "exch-order-123",
                    "client_order_id": "550e8400-e29b-41d4-a716-446655440000",
                    "ticker": "KXBTCD-26FEB-T100000",
                    "status": "resting",
                    "side": "yes",
                    "action": "buy",
                    "yes_price": 50,
                    "count_fp": 10.0,
                    "remaining_count_fp": 10.0
                }
            })))
            .mount(&server)
            .await;

        let result = client.submit_order(&test_order_request()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "exch-order-123");
    }

    #[tokio::test]
    async fn test_submit_order_rejected() {
        let (server, client) = setup().await;

        Mock::given(method("POST"))
            .and(path("/trade-api/v2/portfolio/orders"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(serde_json::json!({
                    "code": "invalid_ticker",
                    "message": "Ticker not found"
                })),
            )
            .mount(&server)
            .await;

        let result = client.submit_order(&test_order_request()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ExchangeError::Rejected { reason } => {
                assert!(reason.contains("400"));
            }
            e => panic!("expected Rejected, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_submit_order_rate_limited() {
        let (server, client) = setup().await;

        Mock::given(method("POST"))
            .and(path("/trade-api/v2/portfolio/orders"))
            .respond_with(
                ResponseTemplate::new(429).insert_header("retry-after", "2"),
            )
            .mount(&server)
            .await;

        let result = client.submit_order(&test_order_request()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ExchangeError::RateLimited { retry_after_ms } => {
                assert_eq!(retry_after_ms, 2000);
            }
            e => panic!("expected RateLimited, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_cancel_order_success() {
        let (server, client) = setup().await;

        Mock::given(method("DELETE"))
            .and(path("/trade-api/v2/portfolio/orders/exch-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let result = client.cancel_order("exch-123").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_order_not_found() {
        let (server, client) = setup().await;

        Mock::given(method("DELETE"))
            .and(path("/trade-api/v2/portfolio/orders/exch-999"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let result = client.cancel_order("exch-999").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExchangeError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_cancel_all_orders() {
        let (server, client) = setup().await;

        Mock::given(method("DELETE"))
            .and(path("/trade-api/v2/portfolio/orders/batched"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"orders_cancelled": 3})),
            )
            .mount(&server)
            .await;

        let result = client.cancel_all_orders().await;
        assert_eq!(result.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_get_order_by_client_id() {
        let (server, client) = setup().await;
        let cid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        Mock::given(method("GET"))
            .and(path("/trade-api/v2/portfolio/orders"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "orders": [{
                    "order_id": "exch-order-123",
                    "client_order_id": "550e8400-e29b-41d4-a716-446655440000",
                    "ticker": "KXBTCD-26FEB-T100000",
                    "status": "resting",
                    "side": "yes",
                    "action": "buy",
                    "yes_price": 50,
                    "count_fp": 10.0,
                    "remaining_count_fp": 7.0
                }]
            })))
            .mount(&server)
            .await;

        let result = client.get_order_by_client_id(cid).await.unwrap();
        assert_eq!(result.exchange_order_id, "exch-order-123");
        assert_eq!(result.status, ExchangeOrderState::Resting);
        assert_eq!(result.filled_quantity, 3);
        assert_eq!(result.remaining_quantity, 7);
    }

    #[tokio::test]
    async fn test_get_balance() {
        let (server, client) = setup().await;

        Mock::given(method("GET"))
            .and(path("/trade-api/v2/portfolio/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "balance": 10000,
                "payout": 500
            })))
            .mount(&server)
            .await;

        let result = client.get_balance().await.unwrap();
        assert_eq!(result.available_cents, 10000);
        assert_eq!(result.total_cents, 10500);
    }

    #[tokio::test]
    async fn test_get_fills() {
        let (server, client) = setup().await;

        Mock::given(method("GET"))
            .and(path("/trade-api/v2/portfolio/fills"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fills": [{
                    "trade_id": "trade-001",
                    "order_id": "exch-order-123",
                    "ticker": "KXBTCD-26FEB-T100000",
                    "side": "yes",
                    "action": "buy",
                    "yes_price": 50,
                    "count": 5,
                    "is_taker": true,
                    "created_time": "2026-02-24T12:00:00Z"
                }]
            })))
            .mount(&server)
            .await;

        let fills = client.get_fills().await.unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].trade_id, "trade-001");
        assert_eq!(fills[0].quantity, 5);
        assert!(fills[0].is_taker);
    }

    #[tokio::test]
    async fn test_get_positions() {
        let (server, client) = setup().await;

        Mock::given(method("GET"))
            .and(path("/trade-api/v2/portfolio/positions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "market_positions": [{
                    "ticker": "KXBTCD-26FEB-T100000",
                    "position": 10,
                    "market_exposure": 500,
                    "total_traded": 1000,
                    "realized_pnl": 50,
                    "resting_orders_count": 1
                }]
            })))
            .mount(&server)
            .await;

        let positions = client.get_positions().await.unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].ticker, "KXBTCD-26FEB-T100000");
        assert_eq!(positions[0].quantity, 10);
        assert_eq!(positions[0].market_value_cents, 500);
    }
}
