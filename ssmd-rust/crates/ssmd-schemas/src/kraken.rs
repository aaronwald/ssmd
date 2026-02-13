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

/// Parse a Kraken data array from the JSON message.
/// Returns None if there's no `data` array (control message).
fn get_data_array(json: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    json.get("data")?.as_array()
}

// ---------------------------------------------------------------------------
// KrakenTickerSchema
// ---------------------------------------------------------------------------

pub struct KrakenTickerSchema;

impl KrakenTickerSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("bid", DataType::Float64, false),
            Field::new("bid_qty", DataType::Float64, false),
            Field::new("ask", DataType::Float64, false),
            Field::new("ask_qty", DataType::Float64, false),
            Field::new("last", DataType::Float64, false),
            Field::new("volume", DataType::Float64, false),
            Field::new("vwap", DataType::Float64, false),
            Field::new("high", DataType::Float64, false),
            Field::new("low", DataType::Float64, false),
            Field::new("change", DataType::Float64, false),
            Field::new("change_pct", DataType::Float64, false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KrakenTickerSchema {
    fn schema_name(&self) -> &str {
        "kraken_ticker"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "ticker"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut symbol = StringBuilder::new();
        let mut bid = Float64Builder::new();
        let mut bid_qty = Float64Builder::new();
        let mut ask = Float64Builder::new();
        let mut ask_qty = Float64Builder::new();
        let mut last = Float64Builder::new();
        let mut volume_b = Float64Builder::new();
        let mut vwap = Float64Builder::new();
        let mut high = Float64Builder::new();
        let mut low = Float64Builder::new();
        let mut change = Float64Builder::new();
        let mut change_pct = Float64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let items = match get_data_array(&json) {
                Some(arr) => arr,
                None => continue,
            };

            for item in items {
                macro_rules! req_f64 {
                    ($field:expr) => {
                        match item.get($field).and_then(|v| v.as_f64()) {
                            Some(v) => v,
                            None => {
                                error!(
                                    field = $field,
                                    "Kraken ticker missing required field, skipping item"
                                );
                                continue;
                            }
                        }
                    };
                }

                let sym = match item.get("symbol").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => {
                        error!("Kraken ticker missing 'symbol', skipping item");
                        continue;
                    }
                };

                symbol.append_value(sym);
                bid.append_value(req_f64!("bid"));
                bid_qty.append_value(req_f64!("bid_qty"));
                ask.append_value(req_f64!("ask"));
                ask_qty.append_value(req_f64!("ask_qty"));
                last.append_value(req_f64!("last"));
                volume_b.append_value(req_f64!("volume"));
                vwap.append_value(req_f64!("vwap"));
                high.append_value(req_f64!("high"));
                low.append_value(req_f64!("low"));
                change.append_value(req_f64!("change"));
                change_pct.append_value(req_f64!("change_pct"));
                nats_seq.append_value(*seq);
                received_at.append_value(*recv_at);
            }
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(symbol.finish()),
                Arc::new(bid.finish()),
                Arc::new(bid_qty.finish()),
                Arc::new(ask.finish()),
                Arc::new(ask_qty.finish()),
                Arc::new(last.finish()),
                Arc::new(volume_b.finish()),
                Arc::new(vwap.finish()),
                Arc::new(high.finish()),
                Arc::new(low.finish()),
                Arc::new(change.finish()),
                Arc::new(change_pct.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let items = get_data_array(json)?;
        let item = items.first()?;
        let sym = item.get("symbol")?.as_str()?;
        let bid = format!("{}", item.get("bid")?.as_f64()?);
        let ask = format!("{}", item.get("ask")?.as_f64()?);
        let last = format!("{}", item.get("last")?.as_f64()?);
        let vol = format!("{}", item.get("volume")?.as_f64()?);
        Some(hash_dedup_key(&[sym, &bid, &ask, &last, &vol]))
    }
}

// ---------------------------------------------------------------------------
// KrakenTradeSchema
// ---------------------------------------------------------------------------

pub struct KrakenTradeSchema;

impl KrakenTradeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("price", DataType::Float64, false),
            Field::new("qty", DataType::Float64, false),
            Field::new("ord_type", DataType::Utf8, false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("timestamp", ts_type(), false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KrakenTradeSchema {
    fn schema_name(&self) -> &str {
        "kraken_trade"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "trade"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut symbol = StringBuilder::new();
        let mut side = StringBuilder::new();
        let mut price = Float64Builder::new();
        let mut qty = Float64Builder::new();
        let mut ord_type = StringBuilder::new();
        let mut trade_id = StringBuilder::new();
        let mut timestamp = TimestampMicrosecondBuilder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let items = match get_data_array(&json) {
                Some(arr) => arr,
                None => continue,
            };

            for item in items {
                let sym = match item.get("symbol").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => {
                        error!("Kraken trade missing 'symbol', skipping");
                        continue;
                    }
                };
                let s = match item.get("side").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => {
                        error!("Kraken trade missing 'side', skipping");
                        continue;
                    }
                };
                let p = match item.get("price").and_then(|v| v.as_f64()) {
                    Some(v) => v,
                    None => {
                        error!("Kraken trade missing 'price', skipping");
                        continue;
                    }
                };
                let q = match item.get("qty").and_then(|v| v.as_f64()) {
                    Some(v) => v,
                    None => {
                        error!("Kraken trade missing 'qty', skipping");
                        continue;
                    }
                };
                let ot = match item.get("ord_type").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => {
                        error!("Kraken trade missing 'ord_type', skipping");
                        continue;
                    }
                };
                let tid = match item.get("trade_id").and_then(|v| v.as_str()) {
                    // trade_id may come as integer in some API versions
                    Some(v) => v.to_string(),
                    None => match item.get("trade_id").and_then(|v| v.as_u64()) {
                        Some(v) => v.to_string(),
                        None => {
                            error!("Kraken trade missing 'trade_id', skipping");
                            continue;
                        }
                    },
                };
                let ts_str = match item.get("timestamp").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => {
                        error!("Kraken trade missing 'timestamp', skipping");
                        continue;
                    }
                };

                let ts_micros = match chrono::DateTime::parse_from_rfc3339(ts_str) {
                    Ok(dt) => dt.timestamp_micros(),
                    Err(e) => {
                        error!(error = %e, ts = ts_str, "Failed to parse Kraken timestamp, skipping");
                        continue;
                    }
                };

                symbol.append_value(sym);
                side.append_value(s);
                price.append_value(p);
                qty.append_value(q);
                ord_type.append_value(ot);
                trade_id.append_value(&tid);
                timestamp.append_value(ts_micros);
                nats_seq.append_value(*seq);
                received_at.append_value(*recv_at);
            }
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(symbol.finish()),
                Arc::new(side.finish()),
                Arc::new(price.finish()),
                Arc::new(qty.finish()),
                Arc::new(ord_type.finish()),
                Arc::new(trade_id.finish()),
                Arc::new(timestamp.finish().with_timezone("UTC")),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let items = get_data_array(json)?;
        let item = items.first()?;
        let sym = item.get("symbol")?.as_str()?;
        let tid = item
            .get("trade_id")
            .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string())))?;
        Some(hash_dedup_key(&[sym, &tid]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kraken_ticker() {
        let schema = KrakenTickerSchema;
        let json = br#"{"channel":"ticker","type":"update","data":[{"symbol":"BTC/USD","bid":97000.0,"bid_qty":0.5,"ask":97000.1,"ask_qty":1.0,"last":97000.0,"volume":1234.56,"vwap":96500.0,"high":98000.0,"low":95000.0,"change":500.0,"change_pct":0.52}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 14);

        let sym = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(sym.value(0), "BTC/USD");

        let bid = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert_eq!(bid.value(0), 97000.0);

        let vol = batch
            .column(6)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert_eq!(vol.value(0), 1234.56);
    }

    #[test]
    fn test_parse_kraken_ticker_multi_data() {
        let schema = KrakenTickerSchema;
        // data array with two items → two rows
        let json = br#"{"channel":"ticker","type":"update","data":[
            {"symbol":"BTC/USD","bid":97000.0,"bid_qty":0.5,"ask":97000.1,"ask_qty":1.0,"last":97000.0,"volume":1234.0,"vwap":96500.0,"high":98000.0,"low":95000.0,"change":500.0,"change_pct":0.52},
            {"symbol":"ETH/USD","bid":3000.0,"bid_qty":10.0,"ask":3001.0,"ask_qty":5.0,"last":3000.5,"volume":5000.0,"vwap":2999.0,"high":3100.0,"low":2900.0,"change":50.0,"change_pct":1.7}
        ]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 2000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let sym = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(sym.value(0), "BTC/USD");
        assert_eq!(sym.value(1), "ETH/USD");

        // Both rows share the same nats_seq
        let seq = batch
            .column(12)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(seq.value(0), 42);
        assert_eq!(seq.value(1), 42);
    }

    #[test]
    fn test_parse_kraken_trade() {
        let schema = KrakenTradeSchema;
        let json = br#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","side":"buy","price":97000.0,"qty":0.001,"ord_type":"market","trade_id":"12345","timestamp":"2026-02-06T12:00:00.000000Z"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 10, 5000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        let sym = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(sym.value(0), "BTC/USD");

        let side = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(side.value(0), "buy");

        let price = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert_eq!(price.value(0), 97000.0);

        let tid = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tid.value(0), "12345");

        // timestamp parsed from ISO 8601
        let ts = batch
            .column(6)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        // 2026-02-06T12:00:00Z in micros
        let expected = chrono::DateTime::parse_from_rfc3339("2026-02-06T12:00:00.000000Z")
            .unwrap()
            .timestamp_micros();
        assert_eq!(ts.value(0), expected);
    }

    #[test]
    fn test_skip_heartbeat() {
        let schema = KrakenTickerSchema;
        // Heartbeat has no "data" field
        let json = br#"{"channel":"heartbeat","type":"update"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_skip_subscription_result() {
        let schema = KrakenTickerSchema;
        let json = br#"{"method":"subscribe","success":true,"result":{"channel":"ticker","symbol":"BTC/USD"}}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_dedup_key_kraken_ticker() {
        let schema = KrakenTickerSchema;
        let json: serde_json::Value = serde_json::from_str(
            r#"{"channel":"ticker","type":"update","data":[{"symbol":"BTC/USD","bid":97000.0,"ask":97000.1,"last":97000.0,"volume":1234.0}]}"#,
        )
        .unwrap();
        let key = schema.dedup_key(&json);
        assert!(key.is_some());
    }

    #[test]
    fn test_dedup_key_kraken_trade() {
        let schema = KrakenTradeSchema;
        let json: serde_json::Value = serde_json::from_str(
            r#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","trade_id":"12345"}]}"#,
        )
        .unwrap();
        let key = schema.dedup_key(&json);
        assert!(key.is_some());

        // Different trade_id → different key
        let json2: serde_json::Value = serde_json::from_str(
            r#"{"channel":"trade","type":"update","data":[{"symbol":"BTC/USD","trade_id":"12346"}]}"#,
        )
        .unwrap();
        assert_ne!(schema.dedup_key(&json), schema.dedup_key(&json2));
    }

    #[test]
    fn test_empty_batch() {
        let schema = KrakenTickerSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 14);
    }
}
