//! Live integration tests for harman OMS.
//!
//! These tests hit a deployed harman instance connected to Kalshi demo.
//! They exercise the full stack: test → harman API → Kalshi demo exchange.
//!
//! Required env vars:
//!   HARMAN_URL          - e.g. http://35.231.246.102:8080
//!   HARMAN_ADMIN_TOKEN  - admin bearer token
//!   HARMAN_TEST_TICKER  - active Kalshi ticker (e.g. KXBTCD-26FEB25-T97500)
//!
//! Run with:
//!   cargo test -p ssmd-harman --test live_integration -- --ignored --nocapture

use rust_decimal::Decimal;
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

/// Simple HTTP client for live tests (no TUI dependency).
struct TestClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl TestClient {
    fn from_env() -> Self {
        let base_url =
            std::env::var("HARMAN_URL").expect("HARMAN_URL required for live integration tests");
        let token = std::env::var("HARMAN_ADMIN_TOKEN")
            .expect("HARMAN_ADMIN_TOKEN required for live integration tests");
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    fn ticker() -> String {
        std::env::var("HARMAN_TEST_TICKER")
            .expect("HARMAN_TEST_TICKER required for live integration tests")
    }

    async fn health(&self) -> reqwest::StatusCode {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .expect("health request");
        resp.status()
    }

    async fn create_order(
        &self,
        ticker: &str,
        price: &str,
        quantity: &str,
    ) -> (reqwest::StatusCode, Value) {
        let coid = Uuid::new_v4();
        let body = serde_json::json!({
            "client_order_id": coid,
            "ticker": ticker,
            "side": "yes",
            "action": "buy",
            "quantity": quantity,
            "price_dollars": price,
            "time_in_force": "gtc"
        });
        let resp = self
            .client
            .post(format!("{}/v1/orders", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .expect("create_order request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("create_order json");
        (status, json)
    }

    async fn get_order(&self, id: i64) -> (reqwest::StatusCode, Value) {
        let resp = self
            .client
            .get(format!("{}/v1/orders/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("get_order request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("get_order json");
        (status, json)
    }

    async fn list_orders(&self) -> Value {
        let resp = self
            .client
            .get(format!("{}/v1/orders", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("list_orders request");
        resp.json().await.expect("list_orders json")
    }

    async fn cancel_order(&self, id: i64) -> (reqwest::StatusCode, Value) {
        let resp = self
            .client
            .delete(format!("{}/v1/orders/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("cancel_order request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("cancel_order json");
        (status, json)
    }

    async fn amend_order(
        &self,
        id: i64,
        new_price: Option<&str>,
        new_qty: Option<&str>,
    ) -> (reqwest::StatusCode, Value) {
        let mut body = serde_json::Map::new();
        if let Some(p) = new_price {
            body.insert("new_price_dollars".into(), Value::String(p.to_string()));
        }
        if let Some(q) = new_qty {
            body.insert("new_quantity".into(), Value::String(q.to_string()));
        }
        let resp = self
            .client
            .post(format!("{}/v1/orders/{}/amend", self.base_url, id))
            .bearer_auth(&self.token)
            .json(&Value::Object(body))
            .send()
            .await
            .expect("amend_order request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("amend_order json");
        (status, json)
    }

    async fn decrease_order(
        &self,
        id: i64,
        reduce_by: &str,
    ) -> (reqwest::StatusCode, Value) {
        let body = serde_json::json!({"reduce_by": reduce_by});
        let resp = self
            .client
            .post(format!("{}/v1/orders/{}/decrease", self.base_url, id))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .expect("decrease_order request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("decrease_order json");
        (status, json)
    }

    async fn pump(&self) -> Value {
        let resp = self
            .client
            .post(format!("{}/v1/admin/pump", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("pump request");
        resp.json().await.expect("pump json")
    }

    async fn reconcile(&self) -> Value {
        let resp = self
            .client
            .post(format!("{}/v1/admin/reconcile", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("reconcile request");
        resp.json().await.expect("reconcile json")
    }

    async fn risk(&self) -> Value {
        let resp = self
            .client
            .get(format!("{}/v1/admin/risk", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("risk request");
        resp.json().await.expect("risk json")
    }

    async fn mass_cancel(&self) -> Value {
        let resp = self
            .client
            .post(format!("{}/v1/orders/mass-cancel", self.base_url))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({"confirm": true}))
            .send()
            .await
            .expect("mass_cancel request");
        resp.json().await.expect("mass_cancel json")
    }
}

/// Helper: create order, pump it to acknowledged, return the order ID.
async fn create_and_pump(c: &TestClient, ticker: &str, price: &str, qty: &str) -> i64 {
    let (status, json) = c.create_order(ticker, price, qty).await;
    assert_eq!(status, 201, "create failed: {:?}", json);
    let id = json["id"].as_i64().expect("order id");

    let pump = c.pump().await;
    assert_eq!(
        pump["errors"].as_array().map(|a| a.len()),
        Some(0),
        "pump errors: {:?}",
        pump["errors"]
    );

    // Wait briefly for exchange to process
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_, order) = c.get_order(id).await;
    assert_eq!(
        order["state"].as_str(),
        Some("acknowledged"),
        "order {} not acknowledged: state={:?}",
        id,
        order["state"]
    );
    id
}

/// Helper: cancel an acknowledged order and pump the cancel.
async fn cancel_and_pump(c: &TestClient, id: i64) {
    let (status, _) = c.cancel_order(id).await;
    assert!(
        status.is_success(),
        "cancel order {} failed: status {}",
        id,
        status
    );

    let pump = c.pump().await;
    assert!(
        pump["cancelled"].as_u64().unwrap_or(0) > 0
            || pump["errors"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
        "cancel pump issue: {:?}",
        pump
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
}

// =============================================================================
// Test 1: Health check
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_health() {
    let c = TestClient::from_env();
    let status = c.health().await;
    assert_eq!(status, 200, "health check failed");
}

// =============================================================================
// Test 2: Create order and list
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_create_and_list() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_order(&ticker, "0.02", "1").await;
    assert_eq!(status, 201, "create failed: {:?}", json);
    let id = json["id"].as_i64().expect("order id");

    let orders = c.list_orders().await;
    let found = orders["orders"]
        .as_array()
        .expect("orders array")
        .iter()
        .any(|o| o["id"].as_i64() == Some(id));
    assert!(found, "created order {} not found in list", id);

    // Clean up: pump + cancel
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 3: Create, pump, cancel
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_create_pump_cancel() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "1").await;

    cancel_and_pump(&c, id).await;

    let (_, order) = c.get_order(id).await;
    assert_eq!(
        order["state"].as_str(),
        Some("cancelled"),
        "order not cancelled: {:?}",
        order["state"]
    );
}

// =============================================================================
// Test 4: Amend price
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_amend_price() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "1").await;

    // Amend price
    let (status, json) = c.amend_order(id, Some("0.03"), None).await;
    assert_eq!(status, 200, "amend failed: {:?}", json);
    assert_eq!(json["status"].as_str(), Some("pending_amend"));

    // Pump the amend
    let pump = c.pump().await;
    println!("amend pump result: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify new price
    let (_, order) = c.get_order(id).await;
    assert_eq!(
        order["state"].as_str(),
        Some("acknowledged"),
        "order not back to acknowledged: {:?}",
        order["state"]
    );
    let new_price: Decimal = order["price_dollars"]
        .as_str()
        .expect("price_dollars")
        .parse()
        .expect("parse price");
    assert_eq!(
        new_price,
        Decimal::new(3, 2),
        "price not updated to 0.03"
    );

    // Clean up
    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 5: Amend quantity
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_amend_quantity() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "2").await;

    // Amend quantity
    let (status, json) = c.amend_order(id, None, Some("3")).await;
    assert_eq!(status, 200, "amend failed: {:?}", json);

    let pump = c.pump().await;
    println!("amend qty pump result: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_, order) = c.get_order(id).await;
    assert_eq!(order["state"].as_str(), Some("acknowledged"));
    let new_qty: Decimal = order["quantity"]
        .as_str()
        .expect("quantity")
        .parse()
        .expect("parse qty");
    assert_eq!(new_qty, Decimal::from(3), "quantity not updated to 3");

    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 6: Amend both price and quantity
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_amend_price_and_quantity() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "2").await;

    let (status, json) = c.amend_order(id, Some("0.04"), Some("5")).await;
    assert_eq!(status, 200, "amend failed: {:?}", json);

    let pump = c.pump().await;
    println!("amend both pump result: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_, order) = c.get_order(id).await;
    assert_eq!(order["state"].as_str(), Some("acknowledged"));
    let new_price: Decimal = order["price_dollars"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let new_qty: Decimal = order["quantity"].as_str().unwrap().parse().unwrap();
    assert_eq!(new_price, Decimal::new(4, 2));
    assert_eq!(new_qty, Decimal::from(5));

    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 7: Decrease quantity
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_decrease() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "3").await;

    let (status, json) = c.decrease_order(id, "1").await;
    assert_eq!(status, 200, "decrease failed: {:?}", json);
    assert_eq!(json["status"].as_str(), Some("pending_decrease"));

    let pump = c.pump().await;
    println!("decrease pump result: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let (_, order) = c.get_order(id).await;
    assert_eq!(order["state"].as_str(), Some("acknowledged"));
    let new_qty: Decimal = order["quantity"].as_str().unwrap().parse().unwrap();
    assert_eq!(new_qty, Decimal::from(2), "quantity not decreased to 2");

    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 8: Amend rejected for wrong state (pending, not yet pumped)
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_amend_rejected_state() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    // Create order but do NOT pump — it stays in pending
    let (status, json) = c.create_order(&ticker, "0.02", "1").await;
    assert_eq!(status, 201);
    let id = json["id"].as_i64().expect("order id");

    // Try to amend — should fail with 422
    let (status, json) = c.amend_order(id, Some("0.03"), None).await;
    assert_eq!(
        status, 422,
        "amend should be rejected for pending order: {:?}",
        json
    );

    // Clean up
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 9: Decrease rejected for wrong state
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_decrease_rejected_state() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_order(&ticker, "0.02", "2").await;
    assert_eq!(status, 201);
    let id = json["id"].as_i64().expect("order id");

    let (status, json) = c.decrease_order(id, "1").await;
    assert_eq!(
        status, 422,
        "decrease should be rejected for pending order: {:?}",
        json
    );

    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 10: Reconcile discovers fills/positions
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_reconcile() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "1").await;

    let result = c.reconcile().await;
    println!("reconcile result: {:?}", result);
    // Just verify it doesn't error out
    assert!(
        result.get("errors").is_some(),
        "reconcile response missing errors field"
    );

    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 11: Risk exposure
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_risk() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let id = create_and_pump(&c, &ticker, "0.02", "1").await;

    let risk = c.risk().await;
    println!("risk: {:?}", risk);
    let open: Decimal = risk["open_notional"]
        .as_str()
        .expect("open_notional")
        .parse()
        .expect("parse");
    assert!(open > Decimal::ZERO, "should have open notional");

    cancel_and_pump(&c, id).await;
}

// =============================================================================
// Test 12: Mass cancel
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_mass_cancel() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    // Create 3 orders
    let id1 = create_and_pump(&c, &ticker, "0.02", "1").await;
    let id2 = create_and_pump(&c, &ticker, "0.02", "1").await;
    let id3 = create_and_pump(&c, &ticker, "0.02", "1").await;

    // Mass cancel on exchange
    let result = c.mass_cancel().await;
    println!("mass_cancel result: {:?}", result);

    // Now reconcile to pick up the cancellations
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.reconcile().await;

    // Verify all cancelled (or at least no longer acknowledged)
    for id in [id1, id2, id3] {
        let (_, order) = c.get_order(id).await;
        let state = order["state"].as_str().unwrap_or("unknown");
        assert!(
            state == "cancelled" || state == "filled",
            "order {} should be cancelled or filled, got: {}",
            id,
            state
        );
    }
}
