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
// Kraken — real data uses Futures V1 format (feed/product_id flat objects),
// NOT the V2 format (channel/data array) that schemas expect.
//
// These tests document the format mismatch: parse_batch produces 0 rows
// because the real messages have no "data" array.
// ---------------------------------------------------------------------------

#[test]
fn test_kraken_ticker_real_data_format_mismatch() {
    let messages = load_fixture("kraken_ticker.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KrakenTickerSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    // Real Kraken Futures data uses flat objects with "feed" field,
    // not the V2 "channel"+"data" array format the schema expects.
    // This results in 0 parsed rows — all messages skip the get_data_array() check.
    assert_eq!(
        batch.num_rows(),
        0,
        "Kraken Futures V1 format has no 'data' array — \
         schema expects V2 format with channel+data. \
         Fix: update Kraken schemas for Futures V1 or transform in connector."
    );
}

#[test]
fn test_kraken_trade_real_data_format_mismatch() {
    let messages = load_fixture("kraken_trade.jsonl");
    assert!(!messages.is_empty(), "fixture file empty");

    let schema = KrakenTradeSchema;
    let input = to_input(&messages);
    let batch = schema.parse_batch(&input).unwrap();

    // Same V1 vs V2 mismatch as ticker.
    assert_eq!(
        batch.num_rows(),
        0,
        "Kraken Futures V1 trade format has no 'data' array — \
         see kraken_trade.jsonl for real message shape"
    );
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
    let registry = super::SchemaRegistry::for_feed("kraken");

    // Real Kraken Futures messages use "feed" not "channel", and have no "data" array.
    // detect_message_type checks for "data" first, so these return None.
    let ticker_msg = &load_fixture("kraken_ticker.jsonl")[0];
    let json: serde_json::Value = serde_json::from_slice(ticker_msg).unwrap();
    assert!(
        registry.detect_and_get(&json).is_none(),
        "Kraken Futures V1 messages should not be detected — no 'data' array"
    );
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
