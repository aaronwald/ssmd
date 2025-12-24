//! Integration tests for NATS transport
//!
//! Run with: cargo test -p ssmd-middleware --test nats_integration -- --ignored
//! Requires: docker run -p 4222:4222 nats:latest -js

use bytes::Bytes;
use ssmd_middleware::{NatsTransport, SubjectBuilder, Transport};

#[tokio::test]
#[ignore]
async fn test_nats_publish_subscribe_roundtrip() {
    let transport = NatsTransport::connect("nats://localhost:4222")
        .await
        .expect("Failed to connect to NATS");

    let subjects = SubjectBuilder::new("test-env", "kalshi");

    // Subscribe first
    let mut sub = transport
        .subscribe(&subjects.trade("BTCUSD"))
        .await
        .expect("Failed to subscribe");

    // Publish
    transport
        .publish(&subjects.trade("BTCUSD"), Bytes::from("test message"))
        .await
        .expect("Failed to publish");

    // Receive
    let msg = sub.next().await.expect("Failed to receive");
    assert_eq!(msg.payload, Bytes::from("test message"));
}

#[tokio::test]
#[ignore]
async fn test_jetstream_stream_creation() {
    let transport = NatsTransport::connect("nats://localhost:4222")
        .await
        .expect("Failed to connect to NATS");

    let subjects = SubjectBuilder::new("test-env", "kalshi");

    transport
        .ensure_stream(subjects.stream_name(), vec![subjects.all().to_string()])
        .await
        .expect("Failed to create stream");
}
