//! CDC integration tests — require PostgreSQL with wal_level=logical.
//!
//! Run with: cargo test -p ssmd-cdc --test integration_test -- --ignored --nocapture
//!
//! These tests verify the complete CDC cycle using get_changes (atomic consume):
//!   CREATE TABLE → INSERT → get_changes → verify consumed → get_changes returns empty
//!
//! Environment variables:
//!   CDC_TEST_DATABASE_URL — PostgreSQL with wal_level=logical (default: postgresql://test:test@localhost:5432/cdc_test)

use ssmd_cdc::replication::ReplicationSlot;
use tokio_postgres::NoTls;

fn database_url() -> String {
    std::env::var("CDC_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://test:test@localhost:5432/cdc_test".to_string())
}

async fn setup_client() -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(&database_url(), NoTls)
        .await
        .expect("Failed to connect to test database");
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });
    client
}

async fn cleanup_slot(client: &tokio_postgres::Client, slot_name: &str) {
    let _ = client
        .execute(
            "SELECT pg_drop_replication_slot(slot_name) FROM pg_replication_slots WHERE slot_name = $1",
            &[&slot_name.to_string()],
        )
        .await;
}

#[tokio::test]
#[ignore]
async fn test_full_cdc_cycle_insert_get_consume() {
    let slot_name = "cdc_test_full_cycle";
    let client = setup_client().await;

    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
    client
        .execute(
            "CREATE TABLE cdc_test_markets (ticker TEXT PRIMARY KEY, status TEXT, volume INT)",
            &[],
        )
        .await
        .unwrap();

    let replication = ReplicationSlot::connect(&database_url(), slot_name)
        .await
        .expect("Failed to connect ReplicationSlot");
    replication.ensure_slot().await.expect("Failed to ensure slot");

    // get_changes should return empty (no changes since slot creation)
    let events = replication.get_changes(100).await.expect("get_changes failed");
    assert!(events.is_empty(), "Expected no events before any changes");

    // Insert a row
    client
        .execute(
            "INSERT INTO cdc_test_markets (ticker, status, volume) VALUES ('KXTEST-001', 'active', 100)",
            &[],
        )
        .await
        .unwrap();

    // get_changes should return the insert event AND consume it
    let events = replication.get_changes(100).await.expect("get_changes failed");
    assert!(!events.is_empty(), "Expected at least one event after INSERT");

    let event = &events[0];
    assert_eq!(event.table, "cdc_test_markets");
    assert_eq!(event.op, ssmd_cdc::messages::CdcOperation::Insert);

    let data = event.data.as_ref().expect("event should have data");
    assert_eq!(data.get("ticker").and_then(|v| v.as_str()), Some("KXTEST-001"));
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("active"));
    assert_eq!(data.get("volume").and_then(|v| v.as_i64()), Some(100));

    // get_changes again — should return empty (changes were consumed)
    let events = replication.get_changes(100).await.expect("get_changes failed after consume");
    assert!(events.is_empty(), "Expected no events after consume, got {}", events.len());

    // Cleanup
    replication.close();
    let client = setup_client().await;
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_update_and_delete_events() {
    let slot_name = "cdc_test_update_delete";
    let client = setup_client().await;

    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
    client
        .execute(
            "CREATE TABLE cdc_test_markets (ticker TEXT PRIMARY KEY, status TEXT)",
            &[],
        )
        .await
        .unwrap();

    let replication = ReplicationSlot::connect(&database_url(), slot_name)
        .await
        .expect("connect failed");
    replication.ensure_slot().await.expect("ensure_slot failed");

    // Drain any initial events
    let _ = replication.get_changes(1000).await;

    // Insert, update, delete
    client
        .execute("INSERT INTO cdc_test_markets (ticker, status) VALUES ('KXTEST-002', 'active')", &[])
        .await
        .unwrap();
    client
        .execute("UPDATE cdc_test_markets SET status = 'settled' WHERE ticker = 'KXTEST-002'", &[])
        .await
        .unwrap();
    client
        .execute("DELETE FROM cdc_test_markets WHERE ticker = 'KXTEST-002'", &[])
        .await
        .unwrap();

    let events = replication.get_changes(100).await.expect("get_changes failed");

    let ops: Vec<&str> = events.iter().map(|e| e.op.as_str()).collect();
    assert!(ops.contains(&"insert"), "Missing INSERT event, got: {:?}", ops);
    assert!(ops.contains(&"update"), "Missing UPDATE event, got: {:?}", ops);
    assert!(ops.contains(&"delete"), "Missing DELETE event, got: {:?}", ops);

    let update_event = events.iter().find(|e| e.op == ssmd_cdc::messages::CdcOperation::Update).unwrap();
    let data = update_event.data.as_ref().unwrap();
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("settled"));

    // Cleanup
    replication.close();
    let client = setup_client().await;
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_same_transaction_multiple_tables_dedup_id() {
    let slot_name = "cdc_test_multi_table";
    let client = setup_client().await;

    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_events", &[]).await.unwrap();
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
    client
        .execute("CREATE TABLE cdc_test_events (event_ticker TEXT PRIMARY KEY, title TEXT)", &[])
        .await
        .unwrap();
    client
        .execute(
            "CREATE TABLE cdc_test_markets (ticker TEXT PRIMARY KEY, event_ticker TEXT, status TEXT)",
            &[],
        )
        .await
        .unwrap();

    let replication = ReplicationSlot::connect(&database_url(), slot_name)
        .await
        .expect("connect failed");
    replication.ensure_slot().await.expect("ensure_slot failed");

    // Insert into both tables in the same transaction
    client.execute("BEGIN", &[]).await.unwrap();
    client
        .execute("INSERT INTO cdc_test_events (event_ticker, title) VALUES ('KXTEST-EV1', 'Test Event')", &[])
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO cdc_test_markets (ticker, event_ticker, status) VALUES ('KXTEST-M1', 'KXTEST-EV1', 'active')",
            &[],
        )
        .await
        .unwrap();
    client.execute("COMMIT", &[]).await.unwrap();

    let events = replication.get_changes(100).await.expect("get_changes failed");

    // Should have events for BOTH tables
    let tables: Vec<&str> = events.iter().map(|e| e.table.as_str()).collect();
    assert!(tables.contains(&"cdc_test_events"), "Missing cdc_test_events, got: {:?}", tables);
    assert!(tables.contains(&"cdc_test_markets"), "Missing cdc_test_markets, got: {:?}", tables);

    // Events from the same transaction may share LSNs — verify dedup_id is unique
    let dedup_ids: Vec<String> = events.iter().map(|e| e.dedup_id()).collect();
    let unique_ids: std::collections::HashSet<&String> = dedup_ids.iter().collect();
    assert_eq!(
        dedup_ids.len(),
        unique_ids.len(),
        "dedup_ids must be unique, got duplicates: {:?}", dedup_ids
    );

    // get_changes again — should be empty (consumed)
    let events = replication.get_changes(100).await.expect("get_changes after consume failed");
    assert!(events.is_empty(), "Expected empty after consume, got {}", events.len());

    // Cleanup
    replication.close();
    let client = setup_client().await;
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
    client.execute("DROP TABLE IF EXISTS cdc_test_events", &[]).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_slot_survives_reconnect() {
    let slot_name = "cdc_test_reconnect";
    let client = setup_client().await;

    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
    client
        .execute("CREATE TABLE cdc_test_markets (ticker TEXT PRIMARY KEY, status TEXT)", &[])
        .await
        .unwrap();

    // First connection: create slot, insert, consume
    {
        let replication = ReplicationSlot::connect(&database_url(), slot_name)
            .await
            .expect("connect failed");
        replication.ensure_slot().await.expect("ensure_slot failed");

        client
            .execute("INSERT INTO cdc_test_markets (ticker, status) VALUES ('KXTEST-R1', 'active')", &[])
            .await
            .unwrap();

        let events = replication.get_changes(100).await.expect("get_changes failed");
        assert!(!events.is_empty());
        replication.close();
    }

    // Insert while disconnected
    client
        .execute("INSERT INTO cdc_test_markets (ticker, status) VALUES ('KXTEST-R2', 'active')", &[])
        .await
        .unwrap();

    // Second connection: should see only the new insert (first was consumed)
    {
        let replication = ReplicationSlot::connect(&database_url(), slot_name)
            .await
            .expect("reconnect failed");
        replication.ensure_slot().await.expect("ensure_slot failed");

        let events = replication.get_changes(100).await.expect("get_changes failed");
        assert!(!events.is_empty(), "Should see insert made while disconnected");

        let tickers: Vec<String> = events
            .iter()
            .filter_map(|e| e.data.as_ref()?.get("ticker")?.as_str().map(String::from))
            .collect();
        assert!(
            tickers.contains(&"KXTEST-R2".to_string()),
            "Should see new event, got: {:?}", tickers
        );

        replication.close();
    }

    // Cleanup
    let client = setup_client().await;
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
}
