use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::error::ArrowError;
use arrow::record_batch::RecordBatch;
use tracing::error;

use crate::{hash_dedup_key, MessageSchema};

fn ts_type() -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, Some(Arc::from("UTC")))
}

/// Helper to append an optional i64 from a JSON value.
fn append_optional_i64(builder: &mut Int64Builder, value: Option<&serde_json::Value>) {
    match value.and_then(|v| v.as_i64()) {
        Some(v) => builder.append_value(v),
        None => builder.append_null(),
    }
}

// ---------------------------------------------------------------------------
// KalshiTickerSchema
// ---------------------------------------------------------------------------

pub struct KalshiTickerSchema;

impl KalshiTickerSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("market_ticker", DataType::Utf8, false),
            Field::new("yes_bid", DataType::Int64, true),
            Field::new("yes_ask", DataType::Int64, true),
            Field::new("no_bid", DataType::Int64, true),
            Field::new("no_ask", DataType::Int64, true),
            Field::new("last_price", DataType::Int64, true),
            Field::new("volume", DataType::Int64, true),
            Field::new("open_interest", DataType::Int64, true),
            Field::new("ts", ts_type(), false),
            Field::new("exchange_clock", DataType::Int64, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KalshiTickerSchema {
    fn schema_name(&self) -> &str {
        "kalshi_ticker"
    }

    fn schema_version(&self) -> &str {
        "1.1.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "ticker"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut market_ticker = StringBuilder::new();
        let mut yes_bid = Int64Builder::new();
        let mut yes_ask = Int64Builder::new();
        let mut no_bid = Int64Builder::new();
        let mut no_ask = Int64Builder::new();
        let mut last_price = Int64Builder::new();
        let mut volume = Int64Builder::new();
        let mut open_interest = Int64Builder::new();
        let mut ts = TimestampMicrosecondBuilder::new();
        let mut exchange_clock = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let msg = match json.get("msg") {
                Some(m) => m,
                None => {
                    error!("Kalshi ticker missing 'msg' field, skipping");
                    continue;
                }
            };

            let ticker = match msg.get("market_ticker").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => {
                    error!("Kalshi ticker missing 'market_ticker', skipping");
                    continue;
                }
            };

            let ts_secs = match msg.get("ts").and_then(|v| v.as_i64()) {
                Some(t) => t,
                None => {
                    error!("Kalshi ticker missing 'ts', skipping");
                    continue;
                }
            };

            market_ticker.append_value(ticker);
            append_optional_i64(&mut yes_bid, msg.get("yes_bid"));
            append_optional_i64(&mut yes_ask, msg.get("yes_ask"));
            append_optional_i64(&mut no_bid, msg.get("no_bid"));
            append_optional_i64(&mut no_ask, msg.get("no_ask"));
            append_optional_i64(&mut last_price, msg.get("price"));
            append_optional_i64(&mut volume, msg.get("volume"));
            append_optional_i64(&mut open_interest, msg.get("open_interest"));
            ts.append_value(ts_secs * 1_000_000);
            append_optional_i64(&mut exchange_clock, msg.get("Clock"));
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(market_ticker.finish()),
                Arc::new(yes_bid.finish()),
                Arc::new(yes_ask.finish()),
                Arc::new(no_bid.finish()),
                Arc::new(no_ask.finish()),
                Arc::new(last_price.finish()),
                Arc::new(volume.finish()),
                Arc::new(open_interest.finish()),
                Arc::new(ts.finish().with_timezone("UTC")),
                Arc::new(exchange_clock.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let msg = json.get("msg")?;
        let ticker = msg.get("market_ticker")?.as_str()?;
        let ts = msg.get("ts")?.as_i64()?.to_string();
        Some(hash_dedup_key(&["ticker", ticker, &ts]))
    }
}

// ---------------------------------------------------------------------------
// KalshiTradeSchema
// ---------------------------------------------------------------------------

pub struct KalshiTradeSchema;

impl KalshiTradeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("market_ticker", DataType::Utf8, false),
            Field::new("price", DataType::Int64, false),
            Field::new("count", DataType::Int64, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("ts", ts_type(), false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("exchange_seq", DataType::Int64, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KalshiTradeSchema {
    fn schema_name(&self) -> &str {
        "kalshi_trade"
    }

    fn schema_version(&self) -> &str {
        "1.1.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "trade"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut market_ticker = StringBuilder::new();
        let mut price = Int64Builder::new();
        let mut count = Int64Builder::new();
        let mut side = StringBuilder::new();
        let mut ts = TimestampMicrosecondBuilder::new();
        let mut trade_id = StringBuilder::new();
        let mut exchange_seq = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let msg = match json.get("msg") {
                Some(m) => m,
                None => {
                    error!("Kalshi trade missing 'msg' field, skipping");
                    continue;
                }
            };

            let ticker = match msg.get("market_ticker").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => {
                    error!("Kalshi trade missing 'market_ticker', skipping");
                    continue;
                }
            };
            let tid = match msg.get("trade_id").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!(
                        ticker = ticker,
                        "Kalshi trade missing 'trade_id', skipping"
                    );
                    continue;
                }
            };
            // Kalshi WS sends "yes_price", connector aliases to "price"
            let p = match msg
                .get("yes_price")
                .or_else(|| msg.get("price"))
                .and_then(|v| v.as_i64())
            {
                Some(v) => v,
                None => {
                    error!(
                        ticker = ticker,
                        "Kalshi trade missing 'yes_price'/'price', skipping"
                    );
                    continue;
                }
            };
            let c = match msg.get("count").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!(
                        ticker = ticker,
                        "Kalshi trade missing 'count', skipping"
                    );
                    continue;
                }
            };
            // Kalshi WS sends "taker_side", connector aliases to "side"
            let s = match msg
                .get("taker_side")
                .or_else(|| msg.get("side"))
                .and_then(|v| v.as_str())
            {
                Some(v) => v,
                None => {
                    error!(
                        ticker = ticker,
                        "Kalshi trade missing 'taker_side'/'side', skipping"
                    );
                    continue;
                }
            };
            let t = match msg.get("ts").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!(
                        ticker = ticker,
                        "Kalshi trade missing 'ts', skipping"
                    );
                    continue;
                }
            };

            market_ticker.append_value(ticker);
            price.append_value(p);
            count.append_value(c);
            side.append_value(s);
            ts.append_value(t * 1_000_000);
            trade_id.append_value(tid);
            append_optional_i64(&mut exchange_seq, json.get("seq"));
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(market_ticker.finish()),
                Arc::new(price.finish()),
                Arc::new(count.finish()),
                Arc::new(side.finish()),
                Arc::new(ts.finish().with_timezone("UTC")),
                Arc::new(trade_id.finish()),
                Arc::new(exchange_seq.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let msg = json.get("msg")?;
        let trade_id = msg.get("trade_id")?.as_str()?;
        Some(hash_dedup_key(&["trade", trade_id]))
    }
}

// ---------------------------------------------------------------------------
// KalshiLifecycleSchema
// ---------------------------------------------------------------------------

pub struct KalshiLifecycleSchema;

impl KalshiLifecycleSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("market_ticker", DataType::Utf8, false),
            Field::new("event_type", DataType::Utf8, false),
            Field::new("open_ts", ts_type(), true),
            Field::new("close_ts", ts_type(), true),
            Field::new("additional_metadata", DataType::Utf8, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KalshiLifecycleSchema {
    fn schema_name(&self) -> &str {
        "kalshi_lifecycle"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "market_lifecycle_v2"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut market_ticker = StringBuilder::new();
        let mut event_type = StringBuilder::new();
        let mut open_ts = TimestampMicrosecondBuilder::new();
        let mut close_ts = TimestampMicrosecondBuilder::new();
        let mut additional_metadata = StringBuilder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let msg = match json.get("msg") {
                Some(m) => m,
                None => {
                    error!("Kalshi lifecycle missing 'msg' field, skipping");
                    continue;
                }
            };

            let ticker = match msg.get("market_ticker").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => {
                    error!("Kalshi lifecycle missing 'market_ticker', skipping");
                    continue;
                }
            };
            let et = match msg.get("event_type").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => {
                    error!("Kalshi lifecycle missing 'event_type', skipping");
                    continue;
                }
            };

            market_ticker.append_value(ticker);
            event_type.append_value(et);

            match msg.get("open_ts").and_then(|v| v.as_i64()) {
                Some(v) => open_ts.append_value(v * 1_000_000),
                None => open_ts.append_null(),
            }
            match msg.get("close_ts").and_then(|v| v.as_i64()) {
                Some(v) => close_ts.append_value(v * 1_000_000),
                None => close_ts.append_null(),
            }
            match msg.get("additional_metadata") {
                Some(v) if !v.is_null() => {
                    additional_metadata.append_value(v.to_string());
                }
                _ => additional_metadata.append_null(),
            }

            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(market_ticker.finish()),
                Arc::new(event_type.finish()),
                Arc::new(open_ts.finish().with_timezone("UTC")),
                Arc::new(close_ts.finish().with_timezone("UTC")),
                Arc::new(additional_metadata.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let msg = json.get("msg")?;
        let ticker = msg.get("market_ticker")?.as_str()?;
        let et = msg.get("event_type")?.as_str()?;
        Some(hash_dedup_key(&[ticker, et]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kalshi_ticker() {
        let schema = KalshiTickerSchema;
        let json = br#"{"type":"ticker","sid":1,"msg":{"market_ticker":"KXBTCD-26FEB12-T50049.99","yes_bid":50,"yes_ask":52,"no_bid":48,"no_ask":50,"price":51,"volume":1000,"open_interest":500,"ts":1707667200,"Clock":13281241747}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1707667200_000_000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 12);

        let col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(col.value(0), "KXBTCD-26FEB12-T50049.99");

        let col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(col.value(0), 50); // yes_bid

        let col = batch
            .column(5)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(col.value(0), 51); // last_price (from "price")

        // ts: 1707667200 seconds → 1707667200_000_000 micros
        let col = batch
            .column(8)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        assert_eq!(col.value(0), 1707667200_000_000);

        // exchange_clock
        let col = batch
            .column(9)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(col.value(0), 13281241747);

        // _nats_seq
        let col = batch
            .column(10)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(col.value(0), 1);
    }

    #[test]
    fn test_parse_kalshi_ticker_nullable_fields() {
        let schema = KalshiTickerSchema;
        // Minimal ticker: only required fields (market_ticker, ts)
        let json = br#"{"type":"ticker","msg":{"market_ticker":"KXTEST","ts":1707667200}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        // yes_bid should be null
        let col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(col.is_null(0));

        // volume should be null
        let col = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(col.is_null(0));

        // exchange_clock should be null
        let col = batch
            .column(9)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(col.is_null(0));
    }

    #[test]
    fn test_parse_kalshi_trade() {
        let schema = KalshiTradeSchema;
        let json = br#"{"type":"trade","sid":1,"seq":10,"msg":{"trade_id":"f851595a-1234","market_ticker":"KXBTC-123","price":55,"count":10,"side":"yes","ts":1707667200}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 1707667200_000_000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 9);

        let ticker = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(ticker.value(0), "KXBTC-123");

        let price = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(price.value(0), 55);

        let count = batch
            .column(2)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(count.value(0), 10);

        let side = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(side.value(0), "yes");

        // trade_id
        let tid = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tid.value(0), "f851595a-1234");

        // exchange_seq
        let eseq = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(eseq.value(0), 10);
    }

    #[test]
    fn test_parse_kalshi_trade_ws_field_names() {
        // Raw Kalshi WS uses "yes_price" and "taker_side" (not "price"/"side")
        let schema = KalshiTradeSchema;
        let json = br#"{"type":"trade","sid":1,"seq":5,"msg":{"trade_id":"abc-123","market_ticker":"KXBTC-123","yes_price":55,"count":10,"taker_side":"yes","ts":1707667200}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 1707667200_000_000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 9);

        let price = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(price.value(0), 55);

        let side = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(side.value(0), "yes");
    }

    #[test]
    fn test_parse_kalshi_trade_mixed_field_names() {
        // Both old ("price"/"side") and new ("yes_price"/"taker_side") should work
        let schema = KalshiTradeSchema;
        let old_format = br#"{"type":"trade","seq":1,"msg":{"trade_id":"tid-1","market_ticker":"A","price":10,"count":1,"side":"yes","ts":100}}"#;
        let new_format = br#"{"type":"trade","seq":2,"msg":{"trade_id":"tid-2","market_ticker":"B","yes_price":20,"count":2,"taker_side":"no","ts":200}}"#;
        let batch = schema
            .parse_batch(&[
                (old_format.to_vec(), 1, 100_000_000),
                (new_format.to_vec(), 2, 200_000_000),
            ])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let prices = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(prices.value(0), 10);
        assert_eq!(prices.value(1), 20);

        let sides = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(sides.value(0), "yes");
        assert_eq!(sides.value(1), "no");
    }

    #[test]
    fn test_parse_kalshi_trade_no_envelope_seq() {
        // exchange_seq should be null when top-level "seq" is absent
        let schema = KalshiTradeSchema;
        let json = br#"{"type":"trade","sid":1,"msg":{"trade_id":"tid-no-seq","market_ticker":"KXBTC-123","price":55,"count":10,"side":"yes","ts":1707667200}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 1707667200_000_000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        // exchange_seq should be null
        let eseq = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(eseq.is_null(0));

        // trade_id should still be present
        let tid = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tid.value(0), "tid-no-seq");
    }

    #[test]
    fn test_parse_kalshi_trade_missing_trade_id() {
        // Rows without trade_id should be skipped
        let schema = KalshiTradeSchema;
        let json = br#"{"type":"trade","seq":10,"msg":{"market_ticker":"KXBTC-123","price":55,"count":10,"side":"yes","ts":1707667200}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 1707667200_000_000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_parse_kalshi_lifecycle() {
        let schema = KalshiLifecycleSchema;
        let json = br#"{"type":"market_lifecycle_v2","sid":13,"msg":{"market_ticker":"KXBTCD-26JAN2310-T105000","event_type":"activated","open_ts":1737554400,"close_ts":1737558000,"additional_metadata":{"settlement_value":null}}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 100, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        let ticker = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(ticker.value(0), "KXBTCD-26JAN2310-T105000");

        let et = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(et.value(0), "activated");

        // open_ts: 1737554400 → micros
        let open = batch
            .column(2)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        assert_eq!(open.value(0), 1737554400_000_000);

        // additional_metadata is a JSON string
        let meta = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(!meta.is_null(0));
        assert!(meta.value(0).contains("settlement_value"));
    }

    #[test]
    fn test_parse_kalshi_lifecycle_no_timestamps() {
        let schema = KalshiLifecycleSchema;
        let json = br#"{"type":"market_lifecycle_v2","sid":1,"msg":{"market_ticker":"KXTEST","event_type":"created"}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        // open_ts should be null
        let open = batch
            .column(2)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        assert!(open.is_null(0));

        // additional_metadata should be null
        let meta = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(meta.is_null(0));
    }

    #[test]
    fn test_skip_missing_msg_field() {
        let schema = KalshiTickerSchema;
        let json = br#"{"type":"ticker"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_multi_message_batch() {
        let schema = KalshiTradeSchema;
        let msg1 = br#"{"type":"trade","seq":1,"msg":{"trade_id":"tid-1","market_ticker":"A","price":10,"count":1,"side":"yes","ts":100}}"#;
        let msg2 = br#"{"type":"trade","seq":2,"msg":{"trade_id":"tid-2","market_ticker":"B","price":20,"count":2,"side":"no","ts":200}}"#;
        let batch = schema
            .parse_batch(&[
                (msg1.to_vec(), 1, 100_000_000),
                (msg2.to_vec(), 2, 200_000_000),
            ])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let tickers = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tickers.value(0), "A");
        assert_eq!(tickers.value(1), "B");
    }

    #[test]
    fn test_empty_batch() {
        let schema = KalshiTickerSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 12);
    }

    #[test]
    fn test_dedup_key_ticker() {
        let schema = KalshiTickerSchema;
        let json: serde_json::Value = serde_json::from_str(
            r#"{"type":"ticker","msg":{"market_ticker":"KXBTC","ts":1707667200}}"#,
        )
        .unwrap();
        let key = schema.dedup_key(&json);
        assert!(key.is_some());

        // Same input → same key
        let key2 = schema.dedup_key(&json);
        assert_eq!(key, key2);

        // Different ts → different key
        let json2: serde_json::Value = serde_json::from_str(
            r#"{"type":"ticker","msg":{"market_ticker":"KXBTC","ts":1707667201}}"#,
        )
        .unwrap();
        assert_ne!(schema.dedup_key(&json), schema.dedup_key(&json2));
    }

    #[test]
    fn test_dedup_key_missing_fields() {
        let schema = KalshiTickerSchema;
        let json: serde_json::Value = serde_json::from_str(r#"{"type":"ticker"}"#).unwrap();
        assert!(schema.dedup_key(&json).is_none());
    }
}
