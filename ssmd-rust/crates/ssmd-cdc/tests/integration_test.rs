//! CDC integration tests — require PostgreSQL with wal_level=logical and NATS.
//!
//! Run with: cargo test -p ssmd-cdc --test integration_test -- --ignored --nocapture
//!
//! These tests verify the complete CDC cycle:
//!   CREATE TABLE → INSERT → peek_changes → advance_slot → verify slot advanced
//!
//! Environment variables:
//!   CDC_TEST_DATABASE_URL  — PostgreSQL with wal_level=logical (default: postgresql://test:test@localhost:5432/cdc_test)
//!   CDC_TEST_NATS_URL      — NATS server (default: nats://localhost:4222)

use ssmd_cdc::replication::ReplicationSlot;
use tokio_postgres::NoTls;

fn database_url() -> String {
    std::env::var("CDC_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://test:test@localhost:5432/cdc_test".to_string())
}

/// Helper: direct postgres connection for test setup (creating tables, inserting rows)
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

/// Helper: drop test slot if it exists (cleanup between tests)
async fn cleanup_slot(client: &tokio_postgres::Client, slot_name: &str) {
    // Drop slot if exists (ignore errors)
    let _ = client
        .execute(
            "SELECT pg_drop_replication_slot(slot_name) FROM pg_replication_slots WHERE slot_name = $1",
            &[&slot_name.to_string()],
        )
        .await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL with wal_level=logical
async fn test_full_cdc_cycle_insert_peek_advance() {
    let slot_name = "cdc_test_full_cycle";
    let client = setup_client().await;

    // Cleanup any previous test state
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();

    // Create a test table
    client
        .execute(
            "CREATE TABLE cdc_test_markets (ticker TEXT PRIMARY KEY, status TEXT, volume INT)",
            &[],
        )
        .await
        .unwrap();

    // Connect CDC replication slot
    let replication = ReplicationSlot::connect(&database_url(), slot_name)
        .await
        .expect("Failed to connect ReplicationSlot");
    replication.ensure_slot().await.expect("Failed to ensure slot");

    // Peek should return empty (no changes since slot creation)
    let events = replication.peek_changes(100).await.expect("peek failed");
    assert!(events.is_empty(), "Expected no events before any changes");

    // Insert a row
    client
        .execute(
            "INSERT INTO cdc_test_markets (ticker, status, volume) VALUES ('KXTEST-001', 'active', 100)",
            &[],
        )
        .await
        .unwrap();

    // Peek should return the insert event
    let events = replication.peek_changes(100).await.expect("peek failed");
    assert!(!events.is_empty(), "Expected at least one event after INSERT");

    let event = &events[0];
    assert_eq!(event.table, "cdc_test_markets");
    assert_eq!(event.op, ssmd_cdc::messages::CdcOperation::Insert);

    // Verify parsed data contains our columns
    let data = event.data.as_ref().expect("event should have data");
    assert_eq!(data.get("ticker").and_then(|v| v.as_str()), Some("KXTEST-001"));
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("active"));
    assert_eq!(data.get("volume").and_then(|v| v.as_i64()), Some(100));

    // Record last LSN
    let last_lsn = events.last().unwrap().lsn.clone();

    // Advance the slot past the insert
    replication.advance_slot(&last_lsn).await.expect("advance_slot failed");

    // Peek again — should return empty (we advanced past the insert)
    let events = replication.peek_changes(100).await.expect("peek failed after advance");
    assert!(events.is_empty(), "Expected no events after advance, got {}", events.len());

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
    let _ = replication.peek_changes(1000).await;

    // Insert, then update, then delete
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

    let events = replication.peek_changes(100).await.expect("peek failed");

    // Should have 3 events: INSERT, UPDATE, DELETE
    let ops: Vec<&str> = events.iter().map(|e| e.op.as_str()).collect();
    assert!(ops.contains(&"insert"), "Missing INSERT event, got: {:?}", ops);
    assert!(ops.contains(&"update"), "Missing UPDATE event, got: {:?}", ops);
    assert!(ops.contains(&"delete"), "Missing DELETE event, got: {:?}", ops);

    // Verify update has the new status
    let update_event = events.iter().find(|e| e.op == ssmd_cdc::messages::CdcOperation::Update).unwrap();
    let data = update_event.data.as_ref().unwrap();
    assert_eq!(data.get("status").and_then(|v| v.as_str()), Some("settled"));

    // Advance past all events
    let last_lsn = events.last().unwrap().lsn.clone();
    replication.advance_slot(&last_lsn).await.expect("advance_slot failed");

    // Cleanup
    replication.close();
    let client = setup_client().await;
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_same_transaction_multiple_tables() {
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

    let events = replication.peek_changes(100).await.expect("peek failed");

    // Should have events for BOTH tables
    let tables: Vec<&str> = events.iter().map(|e| e.table.as_str()).collect();
    assert!(
        tables.contains(&"cdc_test_events"),
        "Missing cdc_test_events, got: {:?}", tables
    );
    assert!(
        tables.contains(&"cdc_test_markets"),
        "Missing cdc_test_markets, got: {:?}", tables
    );

    // Events from the same transaction may share LSNs — verify dedup_id is unique
    let dedup_ids: Vec<String> = events.iter().map(|e| e.dedup_id()).collect();
    let unique_ids: std::collections::HashSet<&String> = dedup_ids.iter().collect();
    assert_eq!(
        dedup_ids.len(),
        unique_ids.len(),
        "dedup_ids must be unique, got duplicates: {:?}", dedup_ids
    );

    // Advance and verify clean
    let last_lsn = events.last().unwrap().lsn.clone();
    replication.advance_slot(&last_lsn).await.expect("advance_slot failed");
    let events = replication.peek_changes(100).await.expect("peek after advance failed");
    assert!(events.is_empty(), "Expected empty after advance");

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

    // First connection: create slot, insert, advance
    {
        let replication = ReplicationSlot::connect(&database_url(), slot_name)
            .await
            .expect("connect failed");
        replication.ensure_slot().await.expect("ensure_slot failed");

        client
            .execute("INSERT INTO cdc_test_markets (ticker, status) VALUES ('KXTEST-R1', 'active')", &[])
            .await
            .unwrap();

        let events = replication.peek_changes(100).await.expect("peek failed");
        assert!(!events.is_empty());
        let last_lsn = events.last().unwrap().lsn.clone();
        replication.advance_slot(&last_lsn).await.expect("advance failed");
        replication.close();
    }

    // Insert while disconnected
    client
        .execute("INSERT INTO cdc_test_markets (ticker, status) VALUES ('KXTEST-R2', 'active')", &[])
        .await
        .unwrap();

    // Second connection: reconnect, should see only the new insert
    {
        let replication = ReplicationSlot::connect(&database_url(), slot_name)
            .await
            .expect("reconnect failed");
        replication.ensure_slot().await.expect("ensure_slot failed");

        let events = replication.peek_changes(100).await.expect("peek failed");
        assert!(!events.is_empty(), "Should see insert made while disconnected");

        // Should NOT see the first insert (already advanced past it)
        let tickers: Vec<String> = events
            .iter()
            .filter_map(|e| e.data.as_ref()?.get("ticker")?.as_str().map(String::from))
            .collect();
        assert!(
            !tickers.contains(&"KXTEST-R1".to_string()),
            "Should not see already-advanced event"
        );
        assert!(
            tickers.contains(&"KXTEST-R2".to_string()),
            "Should see new event"
        );

        let last_lsn = events.last().unwrap().lsn.clone();
        replication.advance_slot(&last_lsn).await.expect("advance failed");
        replication.close();
    }

    // Cleanup
    let client = setup_client().await;
    cleanup_slot(&client, slot_name).await;
    client.execute("DROP TABLE IF EXISTS cdc_test_markets", &[]).await.unwrap();
}
