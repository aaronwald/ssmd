//! Integration tests for the Kalshi demo environment.
//!
//! These tests hit the live Kalshi demo API at `https://demo-api.kalshi.co`.
//! They require real credentials and are marked `#[ignore]` so they don't run
//! in CI. Run them with:
//!
//! ```bash
//! KALSHI_API_KEY=... KALSHI_PRIVATE_KEY_PEM="$(cat key.pem)" \
//!   cargo test -p ssmd-exchange-kalshi -- --ignored
//! ```
//!
//! The demo environment may have limited markets or liquidity. Order tests
//! use deeply out-of-the-money prices (1 cent) to avoid accidental fills.

use harman::error::ExchangeError;
use harman::exchange::ExchangeAdapter;
use harman::types::{Action, OrderRequest, Side, TimeInForce};
use rust_decimal::Decimal;
use serde::Deserialize;
use ssmd_connector_lib::kalshi::auth::KalshiCredentials;
use ssmd_exchange_kalshi::client::KalshiClient;
use uuid::Uuid;

const DEMO_BASE_URL: &str = "https://demo-api.kalshi.co";

/// A minimal market object from the Kalshi markets endpoint.
#[derive(Debug, Deserialize)]
struct MarketInfo {
    ticker: String,
}

/// Response from GET /trade-api/v2/markets
#[derive(Debug, Deserialize)]
struct MarketsResponse {
    markets: Vec<MarketInfo>,
}

/// Build a KalshiClient from environment variables.
/// Returns None if credentials are not set (test should skip).
fn make_client() -> Option<KalshiClient> {
    let api_key = std::env::var("KALSHI_API_KEY").ok()?;
    let pem = std::env::var("KALSHI_PRIVATE_KEY_PEM").ok()?;

    if api_key.is_empty() || pem.is_empty() {
        return None;
    }

    let credentials = KalshiCredentials::new(api_key, &pem)
        .expect("failed to parse KALSHI_PRIVATE_KEY_PEM");
    Some(KalshiClient::new(credentials, DEMO_BASE_URL.to_string()))
}

/// Discover an active market on the demo environment.
/// Makes a direct HTTP call since KalshiClient doesn't expose a markets endpoint.
async fn discover_active_market() -> Option<String> {
    let api_key = std::env::var("KALSHI_API_KEY").ok()?;
    let pem = std::env::var("KALSHI_PRIVATE_KEY_PEM").ok()?;
    let credentials = KalshiCredentials::new(api_key, &pem).ok()?;

    let path = "/trade-api/v2/markets?status=open&limit=1";
    let (timestamp, signature) = credentials.sign_rest_request("GET", path).ok()?;

    let url = format!("{}{}", DEMO_BASE_URL, path);
    let resp = reqwest::Client::new()
        .get(&url)
        .header("KALSHI-ACCESS-KEY", &credentials.api_key)
        .header("KALSHI-ACCESS-SIGNATURE", &signature)
        .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
        .header("Content-Type", "application/json")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        eprintln!("market discovery failed: HTTP {}", resp.status());
        return None;
    }

    let body: MarketsResponse = resp.json().await.ok()?;
    body.markets.into_iter().next().map(|m| m.ticker)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn test_demo_auth_balance() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("KALSHI_API_KEY / KALSHI_PRIVATE_KEY_PEM not set — skipping");
            return;
        }
    };

    let balance = client
        .get_balance()
        .await
        .expect("get_balance failed on demo API");

    println!(
        "Demo balance: available=${}, total=${}",
        balance.available_dollars, balance.total_dollars
    );
}

#[tokio::test]
#[ignore]
async fn test_demo_get_positions() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let positions = client
        .get_positions()
        .await
        .expect("get_positions failed on demo API");

    println!("Demo positions: {} entries", positions.len());
    for p in &positions {
        println!("  {} side={:?} qty={}", p.ticker, p.side, p.quantity);
    }
}

#[tokio::test]
#[ignore]
async fn test_demo_get_fills() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let fills = client
        .get_fills()
        .await
        .expect("get_fills failed on demo API");

    println!("Demo fills: {} entries", fills.len());
    for f in fills.iter().take(5) {
        println!(
            "  {} {} {:?}/{:?} price=${} qty={}",
            f.trade_id, f.ticker, f.side, f.action, f.price_dollars, f.quantity
        );
    }
}

#[tokio::test]
#[ignore]
async fn test_demo_submit_and_cancel() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping order tests");
            return;
        }
    };
    println!("Using demo market: {}", ticker);

    // Submit a deeply out-of-the-money limit order (1 cent yes) to avoid fills.
    let coid = Uuid::new_v4();
    let order = OrderRequest {
        client_order_id: coid,
        ticker: ticker.clone(),
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(1, 2), // $0.01
        time_in_force: TimeInForce::Gtc,
    };

    let exchange_id = client
        .submit_order(&order)
        .await
        .expect("submit_order failed on demo");
    println!("Order submitted: exchange_id={}", exchange_id);

    // Cancel the order
    client
        .cancel_order(&exchange_id)
        .await
        .expect("cancel_order failed on demo");
    println!("Order cancelled: {}", exchange_id);
}

#[tokio::test]
#[ignore]
async fn test_demo_mass_cancel() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping");
            return;
        }
    };
    println!("Using demo market: {}", ticker);

    // Submit 3 orders at 1 cent
    let mut submitted = Vec::new();
    for _ in 0..3 {
        let order = OrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: ticker.clone(),
            side: Side::Yes,
            action: Action::Buy,
            quantity: Decimal::from(1),
            price_dollars: Decimal::new(1, 2),
            time_in_force: TimeInForce::Gtc,
        };
        let eid = client
            .submit_order(&order)
            .await
            .expect("submit_order failed");
        submitted.push(eid);
    }
    println!("Submitted {} orders", submitted.len());

    // Mass cancel
    let cancelled = client
        .cancel_all_orders()
        .await
        .expect("cancel_all_orders failed");
    println!("Mass cancel result: {} orders cancelled", cancelled);

    // We expect at least our 3 orders cancelled (there may be others from prior runs)
    assert!(
        cancelled >= 3,
        "expected at least 3 cancellations, got {}",
        cancelled
    );
}

#[tokio::test]
#[ignore]
async fn test_demo_get_order_by_client_id() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping");
            return;
        }
    };
    println!("Using demo market: {}", ticker);

    let coid = Uuid::new_v4();
    let order = OrderRequest {
        client_order_id: coid,
        ticker: ticker.clone(),
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(1, 2),
        time_in_force: TimeInForce::Gtc,
    };

    let exchange_id = client
        .submit_order(&order)
        .await
        .expect("submit_order failed");
    println!("Order submitted: coid={} eid={}", coid, exchange_id);

    // Look up by client_order_id
    let status = client
        .get_order_by_client_id(coid)
        .await
        .expect("get_order_by_client_id failed");

    println!(
        "Order lookup: eid={} state={:?} filled={} remaining={}",
        status.exchange_order_id, status.status, status.filled_quantity, status.remaining_quantity
    );
    assert_eq!(status.exchange_order_id, exchange_id);

    // Clean up
    let _ = client.cancel_order(&exchange_id).await;
}

// ---------------------------------------------------------------------------
// Negative / Edge Case Tests
// ---------------------------------------------------------------------------

/// Double cancel: cancel the same order twice. Second cancel should return NotFound.
#[tokio::test]
#[ignore]
async fn test_demo_double_cancel() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping");
            return;
        }
    };
    println!("Using demo market: {}", ticker);

    // Submit and cancel
    let order = OrderRequest {
        client_order_id: Uuid::new_v4(),
        ticker,
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(1, 2),
        time_in_force: TimeInForce::Gtc,
    };

    let exchange_id = client
        .submit_order(&order)
        .await
        .expect("submit_order failed");
    println!("Order submitted: {}", exchange_id);

    client
        .cancel_order(&exchange_id)
        .await
        .expect("first cancel failed");
    println!("First cancel succeeded");

    // Second cancel — order already cancelled, should fail
    let result = client.cancel_order(&exchange_id).await;
    println!("Second cancel result: {:?}", result);
    assert!(
        result.is_err(),
        "expected second cancel to fail, but it succeeded"
    );
    match result.unwrap_err() {
        ExchangeError::NotFound(_) => println!("Got expected NotFound"),
        ExchangeError::Rejected { reason } => println!("Got Rejected (acceptable): {}", reason),
        e => panic!("unexpected error type: {:?}", e),
    }
}

/// Cancel a non-existent order ID. Should return NotFound or Rejected.
#[tokio::test]
#[ignore]
async fn test_demo_cancel_nonexistent_order() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let fake_id = Uuid::new_v4().to_string();
    println!("Cancelling non-existent order: {}", fake_id);

    let result = client.cancel_order(&fake_id).await;
    println!("Result: {:?}", result);
    assert!(
        result.is_err(),
        "expected cancel of non-existent order to fail"
    );
    match result.unwrap_err() {
        ExchangeError::NotFound(_) => println!("Got expected NotFound"),
        ExchangeError::Rejected { reason } => println!("Got Rejected (acceptable): {}", reason),
        e => panic!("unexpected error type: {:?}", e),
    }
}

/// Submit order with bogus ticker. Should be rejected by the exchange.
#[tokio::test]
#[ignore]
async fn test_demo_submit_invalid_ticker() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let order = OrderRequest {
        client_order_id: Uuid::new_v4(),
        ticker: "NONEXISTENT-TICKER-12345".to_string(),
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(50, 2), // $0.50
        time_in_force: TimeInForce::Gtc,
    };

    let result = client.submit_order(&order).await;
    println!("Invalid ticker result: {:?}", result);
    assert!(result.is_err(), "expected invalid ticker to be rejected");
    match result.unwrap_err() {
        ExchangeError::Rejected { reason } => {
            println!("Got expected Rejected: {}", reason);
        }
        e => panic!("expected Rejected, got: {:?}", e),
    }
}

/// Submit order with zero quantity. Should be rejected.
#[tokio::test]
#[ignore]
async fn test_demo_submit_zero_quantity() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping");
            return;
        }
    };

    let order = OrderRequest {
        client_order_id: Uuid::new_v4(),
        ticker,
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(0),
        price_dollars: Decimal::new(50, 2),
        time_in_force: TimeInForce::Gtc,
    };

    let result = client.submit_order(&order).await;
    println!("Zero quantity result: {:?}", result);
    assert!(result.is_err(), "expected zero quantity to be rejected");
}

/// Submit order with price out of range (> $0.99). Should be rejected.
#[tokio::test]
#[ignore]
async fn test_demo_submit_invalid_price() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping");
            return;
        }
    };

    let order = OrderRequest {
        client_order_id: Uuid::new_v4(),
        ticker,
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(150, 2), // $1.50 — out of range
        time_in_force: TimeInForce::Gtc,
    };

    let result = client.submit_order(&order).await;
    println!("Invalid price result: {:?}", result);
    assert!(result.is_err(), "expected price > $0.99 to be rejected");
}

/// Submit duplicate client_order_id. Second should be rejected.
#[tokio::test]
#[ignore]
async fn test_demo_submit_duplicate_client_order_id() {
    let client = match make_client() {
        Some(c) => c,
        None => {
            eprintln!("credentials not set — skipping");
            return;
        }
    };

    let ticker = match discover_active_market().await {
        Some(t) => t,
        None => {
            eprintln!("no active market found on demo — skipping");
            return;
        }
    };
    println!("Using demo market: {}", ticker);

    let coid = Uuid::new_v4();
    let order = OrderRequest {
        client_order_id: coid,
        ticker: ticker.clone(),
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(1, 2),
        time_in_force: TimeInForce::Gtc,
    };

    let exchange_id = client
        .submit_order(&order)
        .await
        .expect("first submit failed");
    println!("First submit succeeded: {}", exchange_id);

    // Same client_order_id again
    let order2 = OrderRequest {
        client_order_id: coid,
        ticker,
        side: Side::Yes,
        action: Action::Buy,
        quantity: Decimal::from(1),
        price_dollars: Decimal::new(1, 2),
        time_in_force: TimeInForce::Gtc,
    };

    let result = client.submit_order(&order2).await;
    println!("Duplicate coid result: {:?}", result);
    assert!(
        result.is_err(),
        "expected duplicate client_order_id to be rejected"
    );

    // Clean up
    let _ = client.cancel_order(&exchange_id).await;
}
