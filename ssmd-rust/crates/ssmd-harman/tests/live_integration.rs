//! Live integration tests for harman OMS.
//!
//! These tests hit a deployed harman instance connected to Kalshi demo.
//! They exercise the full stack: test → harman API → Kalshi demo exchange.
//!
//! Required env vars:
//!   HARMAN_URL          - e.g. http://35.231.246.102:8080
//!   HARMAN_TEST_TICKER  - active Kalshi ticker (e.g. KXBTCD-26FEB25-T97500)
//!
//! Auth (one of):
//!   HARMAN_API_KEY      - ssmd API key with harman:admin scope (preferred)
//!   HARMAN_ADMIN_TOKEN  - static admin bearer token (legacy)
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
        // Prefer API key (per-key session isolation) over legacy static token
        let token = std::env::var("HARMAN_API_KEY")
            .or_else(|_| std::env::var("HARMAN_ADMIN_TOKEN"))
            .expect("HARMAN_API_KEY or HARMAN_ADMIN_TOKEN required for live integration tests");
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

    async fn resume(&self) -> reqwest::StatusCode {
        let resp = self
            .client
            .post(format!("{}/v1/admin/resume", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("resume request");
        resp.status()
    }

    async fn positions(&self) -> (reqwest::StatusCode, Value) {
        let resp = self
            .client
            .get(format!("{}/v1/admin/positions", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("positions request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("positions json");
        (status, json)
    }

    // --- Groups ---

    async fn create_bracket(
        &self,
        ticker: &str,
        entry_price: &str,
        tp_price: &str,
        sl_price: &str,
        qty: &str,
    ) -> (reqwest::StatusCode, Value) {
        let body = serde_json::json!({
            "entry": {
                "client_order_id": Uuid::new_v4(),
                "ticker": ticker,
                "side": "yes",
                "action": "buy",
                "quantity": qty,
                "price_dollars": entry_price,
                "time_in_force": "gtc"
            },
            "take_profit": {
                "client_order_id": Uuid::new_v4(),
                "ticker": ticker,
                "side": "yes",
                "action": "sell",
                "quantity": qty,
                "price_dollars": tp_price,
                "time_in_force": "gtc"
            },
            "stop_loss": {
                "client_order_id": Uuid::new_v4(),
                "ticker": ticker,
                "side": "yes",
                "action": "sell",
                "quantity": qty,
                "price_dollars": sl_price,
                "time_in_force": "gtc"
            }
        });
        let resp = self
            .client
            .post(format!("{}/v1/groups/bracket", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .expect("create_bracket request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("create_bracket json");
        (status, json)
    }

    async fn create_oco(
        &self,
        ticker: &str,
        leg1_price: &str,
        leg2_price: &str,
        qty: &str,
    ) -> (reqwest::StatusCode, Value) {
        let body = serde_json::json!({
            "leg1": {
                "client_order_id": Uuid::new_v4(),
                "ticker": ticker,
                "side": "yes",
                "action": "buy",
                "quantity": qty,
                "price_dollars": leg1_price,
                "time_in_force": "gtc"
            },
            "leg2": {
                "client_order_id": Uuid::new_v4(),
                "ticker": ticker,
                "side": "no",
                "action": "buy",
                "quantity": qty,
                "price_dollars": leg2_price,
                "time_in_force": "gtc"
            }
        });
        let resp = self
            .client
            .post(format!("{}/v1/groups/oco", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .expect("create_oco request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("create_oco json");
        (status, json)
    }

    async fn list_groups(&self, state_filter: Option<&str>) -> Value {
        let mut url = format!("{}/v1/groups", self.base_url);
        if let Some(s) = state_filter {
            url.push_str(&format!("?state={}", s));
        }
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("list_groups request");
        resp.json().await.expect("list_groups json")
    }

    async fn get_group(&self, id: i64) -> (reqwest::StatusCode, Value) {
        let resp = self
            .client
            .get(format!("{}/v1/groups/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("get_group request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("get_group json");
        (status, json)
    }

    async fn cancel_group(&self, id: i64) -> (reqwest::StatusCode, Value) {
        let resp = self
            .client
            .delete(format!("{}/v1/groups/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("cancel_group request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("cancel_group json");
        (status, json)
    }

    // --- Fills & Audit ---

    async fn list_fills(&self) -> Value {
        let resp = self
            .client
            .get(format!("{}/v1/fills", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("list_fills request");
        resp.json().await.expect("list_fills json")
    }

    async fn list_audit(&self) -> Value {
        let resp = self
            .client
            .get(format!("{}/v1/audit", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("list_audit request");
        resp.json().await.expect("list_audit json")
    }

    // --- Tickers ---

    async fn search_tickers(&self, q: &str) -> (reqwest::StatusCode, Value) {
        let resp = self
            .client
            .get(format!("{}/v1/tickers?q={}", self.base_url, q))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("search_tickers request");
        let status = resp.status();
        let json: Value = resp.json().await.expect("search_tickers json");
        (status, json)
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
/// Tolerates 422 (order already in terminal state) for cleanup after reconcile.
async fn cancel_and_pump(c: &TestClient, id: i64) {
    let (status, _) = c.cancel_order(id).await;
    assert!(
        status.is_success() || status == 422,
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

    // Resume session in case reconcile suspended it
    c.resume().await;

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
    assert!(
        result["cancelled"].as_u64().unwrap_or(0) >= 3,
        "mass cancel should cancel at least 3 orders: {:?}",
        result
    );

    // Mass cancel only cancels on the exchange side.
    // Cancel each order via the API + pump to update DB state.
    tokio::time::sleep(Duration::from_millis(500)).await;
    for id in [id1, id2, id3] {
        let _ = c.cancel_order(id).await;
    }
    let pump = c.pump().await;
    println!("mass_cancel cleanup pump: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify all cancelled
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

// =============================================================================
// Test 13: Positions endpoint
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_positions() {
    let c = TestClient::from_env();
    let (status, json) = c.positions().await;
    assert_eq!(status, 200, "positions endpoint failed");

    // Response has both exchange and local positions
    assert!(
        json.get("exchange").is_some(),
        "response missing exchange field: {:?}",
        json
    );
    assert!(
        json.get("local").is_some(),
        "response missing local field: {:?}",
        json
    );

    let exchange = json["exchange"].as_array().expect("exchange array");
    println!("exchange positions: {}", exchange.len());
    for pos in exchange {
        assert!(pos.get("ticker").is_some(), "exchange position missing ticker");
        assert!(pos.get("side").is_some(), "exchange position missing side");
        assert!(pos.get("quantity").is_some(), "exchange position missing quantity");
        assert!(
            pos.get("market_value_dollars").is_some(),
            "exchange position missing market_value_dollars"
        );
    }

    let local = json["local"].as_array().expect("local array");
    println!("local positions: {}", local.len());
    for pos in local {
        assert!(pos.get("ticker").is_some(), "local position missing ticker");
        assert!(pos.get("net_quantity").is_some(), "local position missing net_quantity");
    }
}

// =============================================================================
// Test 14: Reconcile does not suspend on position mismatches
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_reconcile_no_suspend() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    // Create and pump an order
    let id = create_and_pump(&c, &ticker, "0.02", "1").await;

    // Run reconcile — even with external positions, should NOT suspend
    let result = c.reconcile().await;
    println!("reconcile result: {:?}", result);

    // Verify not suspended: health should be "healthy", not "suspended"
    let health_status = c.health().await;
    assert_eq!(health_status, 200, "health check failed after reconcile");

    // Verify we can still create orders (session not suspended)
    let (status, json) = c.create_order(&ticker, "0.03", "1").await;
    assert_eq!(
        status, 201,
        "should be able to create order after reconcile (not suspended): {:?}",
        json
    );
    let id2 = json["id"].as_i64().expect("order id");

    // Clean up
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel_and_pump(&c, id).await;
    cancel_and_pump(&c, id2).await;
}

/// Helper: cancel a group and pump to process the leg cancels.
/// Tolerates non-success (group may already be terminal).
async fn cancel_group_and_pump(c: &TestClient, group_id: i64) {
    let (status, _) = c.cancel_group(group_id).await;
    assert!(
        status.is_success() || status == 422 || status == 500,
        "cancel group {} failed: {}",
        group_id,
        status
    );
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// Helper: find an order by leg_role in a group's orders array.
fn find_order_by_role<'a>(orders: &'a [Value], role: &str) -> &'a Value {
    orders
        .iter()
        .find(|o| o["leg_role"].as_str() == Some(role))
        .unwrap_or_else(|| panic!("no order with leg_role={} in group", role))
}

// =============================================================================
// Test 15: Create bracket group
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_create_bracket() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_bracket(&ticker, "0.02", "0.05", "0.01", "1").await;
    assert_eq!(status, 201, "create bracket failed: {:?}", json);

    assert_eq!(json["group_type"].as_str(), Some("bracket"));
    assert_eq!(json["state"].as_str(), Some("active"));

    let orders = json["orders"].as_array().expect("orders array");
    assert_eq!(orders.len(), 3, "bracket should have 3 orders");

    // Verify leg roles
    let entry = find_order_by_role(orders, "entry");
    let tp = find_order_by_role(orders, "take_profit");
    let sl = find_order_by_role(orders, "stop_loss");

    assert_eq!(entry["state"].as_str(), Some("pending"));
    assert_eq!(tp["state"].as_str(), Some("staged"));
    assert_eq!(sl["state"].as_str(), Some("staged"));

    let group_id = json["id"].as_i64().expect("group id");
    println!("bracket group_id={}, entry={}, tp={}, sl={}",
        group_id,
        entry["id"].as_i64().unwrap(),
        tp["id"].as_i64().unwrap(),
        sl["id"].as_i64().unwrap(),
    );

    // Verify group appears in list
    let groups = c.list_groups(Some("active")).await;
    let groups_arr = groups["groups"].as_array().expect("groups array");
    let found = groups_arr.iter().any(|g| g["id"].as_i64() == Some(group_id));
    assert!(found, "bracket group {} not found in active groups list", group_id);

    // Cleanup
    cancel_group_and_pump(&c, group_id).await;
}

// =============================================================================
// Test 16: Bracket entry pumped to acknowledged, exits stay staged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_bracket_pump_entry() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_bracket(&ticker, "0.02", "0.05", "0.01", "1").await;
    assert_eq!(status, 201, "create bracket failed: {:?}", json);
    let group_id = json["id"].as_i64().expect("group id");

    // Pump entry to exchange
    let pump = c.pump().await;
    println!("bracket pump result: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify entry acknowledged, exits still staged
    let (_, group) = c.get_group(group_id).await;
    let orders = group["orders"].as_array().expect("orders array");

    let entry = find_order_by_role(orders, "entry");
    let tp = find_order_by_role(orders, "take_profit");
    let sl = find_order_by_role(orders, "stop_loss");

    assert_eq!(
        entry["state"].as_str(),
        Some("acknowledged"),
        "entry should be acknowledged: {:?}",
        entry["state"]
    );
    assert_eq!(
        tp["state"].as_str(),
        Some("staged"),
        "take_profit should stay staged: {:?}",
        tp["state"]
    );
    assert_eq!(
        sl["state"].as_str(),
        Some("staged"),
        "stop_loss should stay staged: {:?}",
        sl["state"]
    );

    // Cleanup
    cancel_group_and_pump(&c, group_id).await;
}

// =============================================================================
// Test 17: Cancel bracket group
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_cancel_bracket() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_bracket(&ticker, "0.02", "0.05", "0.01", "1").await;
    assert_eq!(status, 201);
    let group_id = json["id"].as_i64().expect("group id");

    // Pump entry to acknowledged
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Cancel group
    let (status, cancel_json) = c.cancel_group(group_id).await;
    assert!(status.is_success(), "cancel bracket failed: {:?}", cancel_json);

    // Pump to process the cancel on exchange
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify group cancelled
    let (_, group) = c.get_group(group_id).await;
    assert_eq!(
        group["state"].as_str(),
        Some("cancelled"),
        "group should be cancelled: {:?}",
        group["state"]
    );

    // All orders should be terminal
    let orders = group["orders"].as_array().expect("orders array");
    for order in orders {
        let state = order["state"].as_str().unwrap_or("unknown");
        assert!(
            state == "cancelled" || state == "filled" || state == "rejected",
            "order {} should be terminal, got: {}",
            order["id"],
            state
        );
    }
}

// =============================================================================
// Test 18: Create OCO group
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_create_oco() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_oco(&ticker, "0.02", "0.02", "1").await;
    assert_eq!(status, 201, "create OCO failed: {:?}", json);

    assert_eq!(json["group_type"].as_str(), Some("oco"));
    assert_eq!(json["state"].as_str(), Some("active"));

    let orders = json["orders"].as_array().expect("orders array");
    assert_eq!(orders.len(), 2, "OCO should have 2 orders");

    // Both legs should be oco_leg and pending
    for order in orders {
        assert_eq!(
            order["leg_role"].as_str(),
            Some("oco_leg"),
            "OCO order should have leg_role=oco_leg: {:?}",
            order["leg_role"]
        );
        assert_eq!(
            order["state"].as_str(),
            Some("pending"),
            "OCO leg should be pending: {:?}",
            order["state"]
        );
    }

    let group_id = json["id"].as_i64().expect("group id");
    println!("OCO group_id={}", group_id);

    // Cleanup
    cancel_group_and_pump(&c, group_id).await;
}

// =============================================================================
// Test 19: OCO pump both legs acknowledged
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_oco_pump() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_oco(&ticker, "0.02", "0.02", "1").await;
    assert_eq!(status, 201);
    let group_id = json["id"].as_i64().expect("group id");

    // Pump both legs
    let pump = c.pump().await;
    println!("OCO pump result: {:?}", pump);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify both acknowledged
    let (_, group) = c.get_group(group_id).await;
    let orders = group["orders"].as_array().expect("orders array");

    for order in orders {
        assert_eq!(
            order["state"].as_str(),
            Some("acknowledged"),
            "OCO leg {} should be acknowledged: {:?}",
            order["id"],
            order["state"]
        );
    }

    // Cleanup
    cancel_group_and_pump(&c, group_id).await;
}

// =============================================================================
// Test 20: Cancel OCO group
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_cancel_oco() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    let (status, json) = c.create_oco(&ticker, "0.02", "0.02", "1").await;
    assert_eq!(status, 201);
    let group_id = json["id"].as_i64().expect("group id");

    // Pump to acknowledged
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Cancel group
    let (status, cancel_json) = c.cancel_group(group_id).await;
    assert!(status.is_success(), "cancel OCO failed: {:?}", cancel_json);

    // Pump to process cancels on exchange
    c.pump().await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify group cancelled
    let (_, group) = c.get_group(group_id).await;
    assert_eq!(
        group["state"].as_str(),
        Some("cancelled"),
        "OCO group should be cancelled: {:?}",
        group["state"]
    );

    // Both legs should be cancelled
    let orders = group["orders"].as_array().expect("orders array");
    for order in orders {
        let state = order["state"].as_str().unwrap_or("unknown");
        assert!(
            state == "cancelled" || state == "filled",
            "OCO leg {} should be cancelled or filled, got: {}",
            order["id"],
            state
        );
    }
}

// =============================================================================
// Test 21: Ticker search
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_ticker_search() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    // Extract prefix from the test ticker (e.g. "KXBTCD" from "KXBTCD-26FEB25-T97500")
    let prefix = ticker.split('-').next().unwrap_or(&ticker);

    // Search with known prefix — should return results
    let (status, json) = c.search_tickers(prefix).await;
    assert_eq!(status, 200, "ticker search failed: {:?}", json);

    let tickers = json["tickers"].as_array().expect("tickers array");
    assert!(
        !tickers.is_empty(),
        "ticker search for '{}' should return results",
        prefix
    );
    println!("ticker search '{}': {} results", prefix, tickers.len());

    // Verify at least one result matches the prefix
    let has_match = tickers
        .iter()
        .any(|t| t.as_str().map(|s| s.starts_with(prefix)).unwrap_or(false));
    assert!(has_match, "no ticker starts with '{}': {:?}", prefix, tickers);

    // Search with nonsense — should return empty
    let (status, json) = c.search_tickers("ZZZZNONEXISTENT").await;
    assert_eq!(status, 200, "ticker search for nonsense failed");
    let tickers = json["tickers"].as_array().expect("tickers array");
    assert!(
        tickers.is_empty(),
        "ticker search for nonsense should be empty, got: {:?}",
        tickers
    );
}

// =============================================================================
// Test 22: Fills and audit endpoints
// =============================================================================

#[tokio::test]
#[ignore]
async fn test_live_fills_and_audit() {
    let c = TestClient::from_env();
    let ticker = TestClient::ticker();

    // Fills endpoint should return 200 with array
    let fills = c.list_fills().await;
    assert!(
        fills.get("fills").is_some(),
        "fills response missing 'fills' field: {:?}",
        fills
    );
    let fills_arr = fills["fills"].as_array().expect("fills array");
    println!("existing fills: {}", fills_arr.len());

    // Audit endpoint should return 200 with array
    let audit = c.list_audit().await;
    assert!(
        audit.get("audit").is_some(),
        "audit response missing 'audit' field: {:?}",
        audit
    );
    let audit_before = audit["audit"].as_array().expect("audit array").len();
    println!("existing audit entries: {}", audit_before);

    // Create order, pump, cancel, pump — generates audit entries
    let id = create_and_pump(&c, &ticker, "0.02", "1").await;
    cancel_and_pump(&c, id).await;

    // Verify audit entries exist for the order lifecycle
    let audit = c.list_audit().await;
    let audit_arr = audit["audit"].as_array().expect("audit array");
    println!("audit entries after order lifecycle: {}", audit_arr.len());
    assert!(
        audit_arr.len() > audit_before,
        "audit should have new entries after order lifecycle"
    );

    // Check that at least one audit entry references our order
    let has_order_audit = audit_arr
        .iter()
        .any(|e| e["order_id"].as_i64() == Some(id));
    assert!(
        has_order_audit,
        "audit should contain entries for order {}: {:?}",
        id,
        &audit_arr[..audit_arr.len().min(5)]
    );
}
