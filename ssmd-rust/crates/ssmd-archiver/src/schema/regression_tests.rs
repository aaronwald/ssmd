//! Regression tests using real captured WebSocket messages from GCS archives.
//!
//! Fixture files in `testdata/` contain actual JSONL lines extracted from
//! `gs://ssmd-data/{exchange}/.../*.jsonl.gz` archives (2026-02-12).
//!
//! These tests ensure that schema parsers handle real production traffic,
//! not just hand-crafted test JSON.

use std::path::PathBuf;

use arrow::array::RecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use super::kalshi::{KalshiTickerSchema, KalshiTradeSchema};
use super::kraken::{KrakenTickerSchema, KrakenTradeSchema};
use super::kraken_futures::{KrakenFuturesTickerSchema, KrakenFuturesTradeSchema};
use super::polymarket::{PolymarketBookSchema, PolymarketTradeSchema};
use super::MessageSchema;

/// Load a fixture file from testdata/ directory.
/// Returns each non-empty line as raw bytes.
fn load_fixture(name: &str) -> Vec<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/schema/testdata")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("fixture file not found: {}", path.display()))
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.as_bytes().to_vec())
        .collect()
}

/// Convert fixture lines to parse_batch input tuples.
fn to_input(messages: &[Vec<u8>]) -> Vec<(Vec<u8>, u64, i64)> {
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| (m.clone(), i as u64, 1_000_000 * (1770900000 + i as i64)))
        .collect()
}

/// Verify RecordBatch schema matches expected column names.
fn assert_column_names(batch: &RecordBatch, expected: &[&str]) {
    let schema = batch.schema();
    let actual: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    assert_eq!(actual, expected, "Column names mismatch");
}

/// Write a RecordBatch to Parquet in-memory and read it back.
fn parquet_roundtrip(batch: &RecordBatch) -> RecordBatch {
    let mut buf = Vec::new();
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props)).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();

    let reader = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(buf))
        .unwrap()
        .build()
        .unwrap();
    let batches: Vec<RecordBatch> = reader.into_iter().map(|r| r.unwrap()).collect();
    assert_eq!(batches.len(), 1, "Expected exactly one batch from parquet");
    batches.into_iter().next().unwrap()
}

// ---------------------------------------------------------------------------
// Kalshi Ticker — real data uses: {"type":"ticker","sid":N,"msg":{...}}
// msg contains: market_ticker, yes_bid, yes_ask, price, volume, open_interest, ts
// ---------------------------------------------------------------------------

#[test]
fn test_kalshi_ticker_real_data() {
    let messages = load_fixture("kalshi_ticker.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KalshiTickerSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(
        batch.num_rows(),
        messages.len(),
        "All ticker messages must parse — if this fails, schema field names don't match real WS JSON"
    );
    assert_eq!(batch.num_columns(), 11);
    assert_column_names(
        &batch,
        &[
            "market_ticker",
            "yes_bid",
            "yes_ask",
            "no_bid",
            "no_ask",
            "last_price",
            "volume",
            "open_interest",
            "ts",
            "_nats_seq",
            "_received_at",
        ],
    );
}

#[test]
fn test_kalshi_ticker_parquet_roundtrip() {
    let messages = load_fixture("kalshi_ticker.jsonl");
    let schema = KalshiTickerSchema;
    let batch = schema.parse_batch(&to_input(&messages)).unwrap();

    let roundtripped = parquet_roundtrip(&batch);
    assert_eq!(roundtripped.num_rows(), batch.num_rows());
    assert_eq!(roundtripped.num_columns(), batch.num_columns());
    assert_eq!(roundtripped.schema(), batch.schema());
}

// ---------------------------------------------------------------------------
// Kalshi Trade — real WS uses: yes_price, taker_side (not price/side)
// This was the bug that caused zero trade parquet files.
// ---------------------------------------------------------------------------

#[test]
fn test_kalshi_trade_real_data() {
    let messages = load_fixture("kalshi_trade.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KalshiTradeSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(
        batch.num_rows(),
        messages.len(),
        "All trade messages must parse — real WS uses yes_price/taker_side, \
         schema must handle these field names"
    );
    assert_eq!(batch.num_columns(), 7);
    assert_column_names(
        &batch,
        &[
            "market_ticker",
            "price",
            "count",
            "side",
            "ts",
            "_nats_seq",
            "_received_at",
        ],
    );
}

#[test]
fn test_kalshi_trade_parquet_roundtrip() {
    let messages = load_fixture("kalshi_trade.jsonl");
    let schema = KalshiTradeSchema;
    let batch = schema.parse_batch(&to_input(&messages)).unwrap();
    assert!(batch.num_rows() > 0, "Must have rows to roundtrip");

    let roundtripped = parquet_roundtrip(&batch);
    assert_eq!(roundtripped.num_rows(), batch.num_rows());
    assert_eq!(roundtripped.schema(), batch.schema());
}

// ---------------------------------------------------------------------------
// Kraken Spot V2 schemas vs Futures V1 real data — format mismatch.
// The Spot V2 schemas expect channel+data[] wrappers; Futures V1 is flat.
// These tests document the mismatch (0 rows). Use kraken-futures feed instead.
// ---------------------------------------------------------------------------

#[test]
fn test_kraken_spot_v2_ticker_vs_futures_v1_data() {
    let messages = load_fixture("kraken_ticker.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KrakenTickerSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    // Spot V2 schema expects "data" array — Futures V1 has none → 0 rows.
    assert_eq!(batch.num_rows(), 0);
}

#[test]
fn test_kraken_spot_v2_trade_vs_futures_v1_data() {
    let messages = load_fixture("kraken_trade.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KrakenTradeSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(batch.num_rows(), 0);
}

// ---------------------------------------------------------------------------
// Kraken Futures — real data uses V1 format (flat feed/product_id objects).
// The kraken_futures module handles this format correctly.
// ---------------------------------------------------------------------------

#[test]
fn test_kraken_futures_ticker_real_data() {
    let messages = load_fixture("kraken_ticker.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KrakenFuturesTickerSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(
        batch.num_rows(),
        messages.len(),
        "All Futures V1 ticker messages must parse"
    );
    assert_eq!(batch.num_columns(), 21);
    assert_column_names(
        &batch,
        &[
            "product_id", "bid", "bid_size", "ask", "ask_size", "last",
            "volume", "volume_quote", "open", "high", "low", "change",
            "index_price", "mark_price", "open_interest", "funding_rate",
            "funding_rate_prediction", "next_funding_rate_time",
            "time", "_nats_seq", "_received_at",
        ],
    );
}

#[test]
fn test_kraken_futures_ticker_parquet_roundtrip() {
    let messages = load_fixture("kraken_ticker.jsonl");
    let schema = KrakenFuturesTickerSchema;
    let batch = schema.parse_batch(&to_input(&messages)).unwrap();
    assert!(batch.num_rows() > 0);

    let roundtripped = parquet_roundtrip(&batch);
    assert_eq!(roundtripped.num_rows(), batch.num_rows());
    assert_eq!(roundtripped.schema(), batch.schema());
}

#[test]
fn test_kraken_futures_trade_real_data() {
    let messages = load_fixture("kraken_trade.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KrakenFuturesTradeSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(
        batch.num_rows(),
        messages.len(),
        "All Futures V1 trade messages must parse"
    );
    assert_eq!(batch.num_columns(), 10);
    assert_column_names(
        &batch,
        &[
            "product_id", "uid", "side", "trade_type", "seq",
            "qty", "price", "time", "_nats_seq", "_received_at",
        ],
    );
}

#[test]
fn test_kraken_futures_trade_parquet_roundtrip() {
    let messages = load_fixture("kraken_trade.jsonl");
    let schema = KrakenFuturesTradeSchema;
    let batch = schema.parse_batch(&to_input(&messages)).unwrap();
    assert!(batch.num_rows() > 0);

    let roundtripped = parquet_roundtrip(&batch);
    assert_eq!(roundtripped.num_rows(), batch.num_rows());
    assert_eq!(roundtripped.schema(), batch.schema());
}

// ---------------------------------------------------------------------------
// Polymarket Book — real data uses: event_type, asset_id, market, bids, asks,
// timestamp (string ms), hash
// ---------------------------------------------------------------------------

#[test]
fn test_polymarket_book_real_data() {
    let messages = load_fixture("polymarket_book.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = PolymarketBookSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(
        batch.num_rows(),
        messages.len(),
        "All book messages must parse — if this fails, check field name aliases (bids/buys, asks/sells)"
    );
    assert_eq!(batch.num_columns(), 8);
    assert_column_names(
        &batch,
        &[
            "asset_id",
            "market",
            "timestamp_ms",
            "hash",
            "bids_json",
            "asks_json",
            "_nats_seq",
            "_received_at",
        ],
    );
}

#[test]
fn test_polymarket_book_parquet_roundtrip() {
    let messages = load_fixture("polymarket_book.jsonl");
    let schema = PolymarketBookSchema;
    let batch = schema.parse_batch(&to_input(&messages)).unwrap();

    let roundtripped = parquet_roundtrip(&batch);
    assert_eq!(roundtripped.num_rows(), batch.num_rows());
    assert_eq!(roundtripped.schema(), batch.schema());
}

// ---------------------------------------------------------------------------
// Polymarket Trade (last_trade_price) — real data uses: event_type, asset_id,
// market, price (string), side, size, fee_rate_bps, timestamp (string ms)
// ---------------------------------------------------------------------------

#[test]
fn test_polymarket_trade_real_data() {
    let messages = load_fixture("polymarket_trade.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = PolymarketTradeSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    assert_eq!(
        batch.num_rows(),
        messages.len(),
        "All trade messages must parse — check price/side/size field types"
    );
    assert_eq!(batch.num_columns(), 9);
    assert_column_names(
        &batch,
        &[
            "asset_id",
            "market",
            "price",
            "side",
            "size",
            "fee_rate_bps",
            "timestamp_ms",
            "_nats_seq",
            "_received_at",
        ],
    );
}

#[test]
fn test_polymarket_trade_parquet_roundtrip() {
    let messages = load_fixture("polymarket_trade.jsonl");
    let schema = PolymarketTradeSchema;
    let batch = schema.parse_batch(&to_input(&messages)).unwrap();

    let roundtripped = parquet_roundtrip(&batch);
    assert_eq!(roundtripped.num_rows(), batch.num_rows());
    assert_eq!(roundtripped.schema(), batch.schema());
}

// ---------------------------------------------------------------------------
// Cross-schema detection via SchemaRegistry
// Verify detect_message_type routes real messages to the correct schema.
// ---------------------------------------------------------------------------

#[test]
fn test_detect_real_kalshi_messages() {
    let registry = super::SchemaRegistry::for_feed("kalshi");

    // Ticker
    let ticker_msg = &load_fixture("kalshi_ticker.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(ticker_msg).unwrap();
    let (msg_type, _schema) = registry.detect_and_get(&json).unwrap();
    assert_eq!(msg_type, "ticker");

    // Trade
    let trade_msg = &load_fixture("kalshi_trade.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(trade_msg).unwrap();
    let (msg_type, _schema) = registry.detect_and_get(&json).unwrap();
    assert_eq!(msg_type, "trade");
}

#[test]
fn test_detect_real_kraken_messages() {
    // Spot V2 registry doesn't detect Futures V1 messages (no "data" array)
    let registry_spot = super::SchemaRegistry::for_feed("kraken");
    let ticker_msg = &load_fixture("kraken_ticker.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(ticker_msg).unwrap();
    assert!(
        registry_spot.detect_and_get(&json).is_none(),
        "Kraken Futures V1 messages should not be detected by Spot V2 registry"
    );

    // Futures registry correctly detects V1 messages
    let registry_futures = super::SchemaRegistry::for_feed("kraken-futures");

    let json: serde_json::Value = serde_json::from_slice(ticker_msg).unwrap();
    let (msg_type, _schema) = registry_futures.detect_and_get(&json).unwrap();
    assert_eq!(msg_type, "ticker");

    let trade_msg = &load_fixture("kraken_trade.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(trade_msg).unwrap();
    let (msg_type, _schema) = registry_futures.detect_and_get(&json).unwrap();
    assert_eq!(msg_type, "trade");
}

#[test]
fn test_detect_real_polymarket_messages() {
    let registry = super::SchemaRegistry::for_feed("polymarket");

    // Book
    let book_msg = &load_fixture("polymarket_book.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(book_msg).unwrap();
    let (msg_type, _schema) = registry.detect_and_get(&json).unwrap();
    assert_eq!(msg_type, "book");

    // Trade
    let trade_msg = &load_fixture("polymarket_trade.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(trade_msg).unwrap();
    let (msg_type, _schema) = registry.detect_and_get(&json).unwrap();
    assert_eq!(msg_type, "last_trade_price");
}
