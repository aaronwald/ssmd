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

/// Parse timestamp_ms from a JSON value that may be a string or integer.
fn parse_timestamp_ms(value: &serde_json::Value) -> Option<i64> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_i64())
}

// ---------------------------------------------------------------------------
// PolymarketBookSchema
// ---------------------------------------------------------------------------

pub struct PolymarketBookSchema;

impl PolymarketBookSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("asset_id", DataType::Utf8, false),
            Field::new("market", DataType::Utf8, false),
            Field::new("timestamp_ms", DataType::Int64, true),
            Field::new("hash", DataType::Utf8, true),
            Field::new("bids_json", DataType::Utf8, false),
            Field::new("asks_json", DataType::Utf8, false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for PolymarketBookSchema {
    fn schema_name(&self) -> &str {
        "polymarket_book"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "book"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut asset_id = StringBuilder::new();
        let mut market = StringBuilder::new();
        let mut timestamp_ms = Int64Builder::new();
        let mut hash_b = StringBuilder::new();
        let mut bids_json = StringBuilder::new();
        let mut asks_json = StringBuilder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let aid = match json.get("asset_id").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket book missing 'asset_id', skipping");
                    continue;
                }
            };
            let mkt = match json.get("market").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket book missing 'market', skipping");
                    continue;
                }
            };

            // bids: try "buys" first, then "bids"
            let empty_array = serde_json::Value::Array(vec![]);
            let bids = json
                .get("buys")
                .or_else(|| json.get("bids"))
                .unwrap_or(&empty_array);
            let asks = json
                .get("sells")
                .or_else(|| json.get("asks"))
                .unwrap_or(&empty_array);

            let bids_str = serde_json::to_string(bids)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;
            let asks_str = serde_json::to_string(asks)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            asset_id.append_value(aid);
            market.append_value(mkt);

            match json.get("timestamp").and_then(parse_timestamp_ms) {
                Some(ms) => timestamp_ms.append_value(ms),
                None => timestamp_ms.append_null(),
            }

            match json.get("hash").and_then(|v| v.as_str()) {
                Some(h) => hash_b.append_value(h),
                None => hash_b.append_null(),
            }

            bids_json.append_value(&bids_str);
            asks_json.append_value(&asks_str);
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(asset_id.finish()),
                Arc::new(market.finish()),
                Arc::new(timestamp_ms.finish()),
                Arc::new(hash_b.finish()),
                Arc::new(bids_json.finish()),
                Arc::new(asks_json.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let aid = json.get("asset_id")?.as_str()?;
        let ts = json
            .get("timestamp")
            .and_then(parse_timestamp_ms)
            .map(|v| v.to_string())
            .unwrap_or_default();
        Some(hash_dedup_key(&[aid, &ts]))
    }
}

// ---------------------------------------------------------------------------
// PolymarketTradeSchema
// ---------------------------------------------------------------------------

pub struct PolymarketTradeSchema;

impl PolymarketTradeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("asset_id", DataType::Utf8, false),
            Field::new("market", DataType::Utf8, false),
            Field::new("price", DataType::Utf8, false),
            Field::new("side", DataType::Utf8, true),
            Field::new("size", DataType::Utf8, true),
            Field::new("fee_rate_bps", DataType::Utf8, true),
            Field::new("timestamp_ms", DataType::Int64, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for PolymarketTradeSchema {
    fn schema_name(&self) -> &str {
        "polymarket_trade"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "last_trade_price"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut asset_id = StringBuilder::new();
        let mut market = StringBuilder::new();
        let mut price = StringBuilder::new();
        let mut side = StringBuilder::new();
        let mut size = StringBuilder::new();
        let mut fee_rate_bps = StringBuilder::new();
        let mut timestamp_ms = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let aid = match json.get("asset_id").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket trade missing 'asset_id', skipping");
                    continue;
                }
            };
            let mkt = match json.get("market").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket trade missing 'market', skipping");
                    continue;
                }
            };
            let p = match json.get("price").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket trade missing 'price', skipping");
                    continue;
                }
            };

            asset_id.append_value(aid);
            market.append_value(mkt);
            price.append_value(p);

            match json.get("side").and_then(|v| v.as_str()) {
                Some(v) => side.append_value(v),
                None => side.append_null(),
            }
            match json.get("size").and_then(|v| v.as_str()) {
                Some(v) => size.append_value(v),
                None => size.append_null(),
            }
            match json.get("fee_rate_bps").and_then(|v| v.as_str()) {
                Some(v) => fee_rate_bps.append_value(v),
                None => fee_rate_bps.append_null(),
            }
            match json.get("timestamp").and_then(parse_timestamp_ms) {
                Some(ms) => timestamp_ms.append_value(ms),
                None => timestamp_ms.append_null(),
            }

            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(asset_id.finish()),
                Arc::new(market.finish()),
                Arc::new(price.finish()),
                Arc::new(side.finish()),
                Arc::new(size.finish()),
                Arc::new(fee_rate_bps.finish()),
                Arc::new(timestamp_ms.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let aid = json.get("asset_id")?.as_str()?;
        let p = json.get("price")?.as_str().unwrap_or("");
        let ts = json
            .get("timestamp")
            .and_then(parse_timestamp_ms)
            .map(|v| v.to_string())
            .unwrap_or_default();
        Some(hash_dedup_key(&[aid, p, &ts]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_polymarket_book() {
        let schema = PolymarketBookSchema;
        let json = br#"{"event_type":"book","asset_id":"21742633143463906290569050155826241533067272736897614950488156847949938836455","market":"0x1234abcd","timestamp":"1706000000000","hash":"abc123","buys":[{"price":"0.55","size":"1000"}],"sells":[{"price":"0.56","size":"750"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 8);

        let aid = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(aid.value(0).starts_with("21742633"));

        let mkt = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(mkt.value(0), "0x1234abcd");

        let ts = batch
            .column(2)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(ts.value(0), 1706000000000);

        let hash = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(hash.value(0), "abc123");

        // bids_json contains the buys array as JSON
        let bids = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(bids.value(0).contains("0.55"));

        // asks_json contains the sells array as JSON
        let asks = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(asks.value(0).contains("0.56"));
    }

    #[test]
    fn test_parse_polymarket_book_bids_asks_aliases() {
        let schema = PolymarketBookSchema;
        // Use "bids"/"asks" instead of "buys"/"sells"
        let json = br#"{"event_type":"book","asset_id":"123","market":"0xabc","bids":[{"price":"0.50","size":"500"}],"asks":[{"price":"0.60","size":"300"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        let bids = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(bids.value(0).contains("0.50"));

        // timestamp_ms should be null
        let ts = batch
            .column(2)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(ts.is_null(0));
    }

    #[test]
    fn test_parse_polymarket_trade() {
        let schema = PolymarketTradeSchema;
        let json = br#"{"event_type":"last_trade_price","asset_id":"21742633143463","market":"0x1234abcd","price":"0.55","side":"BUY","size":"100","fee_rate_bps":"0","timestamp":"1706000000000"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 5000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 9);

        let aid = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aid.value(0), "21742633143463");

        let price = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(price.value(0), "0.55");

        let side = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(side.value(0), "BUY");

        let ts = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(ts.value(0), 1706000000000);
    }

    #[test]
    fn test_parse_polymarket_trade_nullable_fields() {
        let schema = PolymarketTradeSchema;
        // Minimal: only required fields
        let json = br#"{"event_type":"last_trade_price","asset_id":"123","market":"0xabc","price":"0.50"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        // side should be null
        let side = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(side.is_null(0));

        // size should be null
        let size = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(size.is_null(0));

        // timestamp_ms should be null
        let ts = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(ts.is_null(0));
    }

    #[test]
    fn test_skip_missing_required_fields() {
        let schema = PolymarketBookSchema;
        // Missing asset_id
        let json = br#"{"event_type":"book","market":"0xabc"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_dedup_key_book() {
        let schema = PolymarketBookSchema;
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"book","asset_id":"123","timestamp":"1706000000000"}"#,
        )
        .unwrap();
        let key = schema.dedup_key(&json);
        assert!(key.is_some());

        // Different timestamp → different key
        let json2: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"book","asset_id":"123","timestamp":"1706000001000"}"#,
        )
        .unwrap();
        assert_ne!(schema.dedup_key(&json), schema.dedup_key(&json2));
    }

    #[test]
    fn test_dedup_key_trade() {
        let schema = PolymarketTradeSchema;
        let json: serde_json::Value = serde_json::from_str(
            r#"{"event_type":"last_trade_price","asset_id":"123","price":"0.55","timestamp":"1706000000000"}"#,
        )
        .unwrap();
        let key = schema.dedup_key(&json);
        assert!(key.is_some());
    }

    #[test]
    fn test_empty_batch() {
        let schema = PolymarketBookSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 8);
    }

    #[test]
    fn test_multi_message_batch() {
        let schema = PolymarketTradeSchema;
        let msg1 = br#"{"event_type":"last_trade_price","asset_id":"A","market":"0x1","price":"0.50"}"#;
        let msg2 = br#"{"event_type":"last_trade_price","asset_id":"B","market":"0x2","price":"0.60","side":"SELL"}"#;
        let batch = schema
            .parse_batch(&[
                (msg1.to_vec(), 1, 1000),
                (msg2.to_vec(), 2, 2000),
            ])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let aids = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aids.value(0), "A");
        assert_eq!(aids.value(1), "B");
    }
}
