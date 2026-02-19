use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::error::ArrowError;
use arrow::record_batch::RecordBatch;
use tracing::error;

use crate::MessageSchema;

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

/// Parse a numeric value from a JSON string (e.g., "0.55") or number.
fn parse_f64_str(value: &serde_json::Value) -> Option<f64> {
    value
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| value.as_f64())
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
            Field::new("price", DataType::Float64, false),
            Field::new("side", DataType::Utf8, true),
            Field::new("size", DataType::Float64, true),
            Field::new("fee_rate_bps", DataType::Float64, true),
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
        "2.0.0"
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
        let mut price = Float64Builder::new();
        let mut side = StringBuilder::new();
        let mut size = Float64Builder::new();
        let mut fee_rate_bps = Float64Builder::new();
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
            let p = match json.get("price").and_then(parse_f64_str) {
                Some(v) => v,
                None => {
                    error!("Polymarket trade missing/invalid 'price', skipping");
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
            match json.get("size").and_then(parse_f64_str) {
                Some(v) => size.append_value(v),
                None => size.append_null(),
            }
            match json.get("fee_rate_bps").and_then(parse_f64_str) {
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
}

// ---------------------------------------------------------------------------
// PolymarketPriceChangeSchema
// ---------------------------------------------------------------------------

pub struct PolymarketPriceChangeSchema;

impl PolymarketPriceChangeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("market", DataType::Utf8, false),
            Field::new("timestamp_ms", DataType::Int64, true),
            Field::new("asset_id", DataType::Utf8, false),
            Field::new("price", DataType::Float64, false),
            Field::new("size", DataType::Float64, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("hash", DataType::Utf8, true),
            Field::new("best_bid", DataType::Float64, true),
            Field::new("best_ask", DataType::Float64, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for PolymarketPriceChangeSchema {
    fn schema_name(&self) -> &str {
        "polymarket_price_change"
    }

    fn schema_version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "price_change"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut market = StringBuilder::new();
        let mut timestamp_ms = Int64Builder::new();
        let mut asset_id = StringBuilder::new();
        let mut price = Float64Builder::new();
        let mut size = Float64Builder::new();
        let mut side = StringBuilder::new();
        let mut hash_b = StringBuilder::new();
        let mut best_bid = Float64Builder::new();
        let mut best_ask = Float64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let mkt = match json.get("market").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket price_change missing 'market', skipping");
                    continue;
                }
            };

            let ts = json.get("timestamp").and_then(parse_timestamp_ms);

            let changes = match json.get("price_changes").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => {
                    error!("Polymarket price_change missing 'price_changes' array, skipping");
                    continue;
                }
            };

            // Flatten: one row per PriceChangeItem
            for item in changes {
                let aid = match item.get("asset_id").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => {
                        error!("Polymarket price_change item missing 'asset_id', skipping item");
                        continue;
                    }
                };
                let p = match item.get("price").and_then(parse_f64_str) {
                    Some(v) => v,
                    None => {
                        error!("Polymarket price_change item missing/invalid 'price', skipping item");
                        continue;
                    }
                };
                let sz = match item.get("size").and_then(parse_f64_str) {
                    Some(v) => v,
                    None => {
                        error!("Polymarket price_change item missing/invalid 'size', skipping item");
                        continue;
                    }
                };
                let sd = match item.get("side").and_then(|v| v.as_str()) {
                    Some(v) => v,
                    None => {
                        error!("Polymarket price_change item missing 'side', skipping item");
                        continue;
                    }
                };

                market.append_value(mkt);
                match ts {
                    Some(ms) => timestamp_ms.append_value(ms),
                    None => timestamp_ms.append_null(),
                }
                asset_id.append_value(aid);
                price.append_value(p);
                size.append_value(sz);
                side.append_value(sd);

                match item.get("hash").and_then(|v| v.as_str()) {
                    Some(h) => hash_b.append_value(h),
                    None => hash_b.append_null(),
                }
                match item.get("best_bid").and_then(parse_f64_str) {
                    Some(v) => best_bid.append_value(v),
                    None => best_bid.append_null(),
                }
                match item.get("best_ask").and_then(parse_f64_str) {
                    Some(v) => best_ask.append_value(v),
                    None => best_ask.append_null(),
                }

                nats_seq.append_value(*seq);
                received_at.append_value(*recv_at);
            }
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(market.finish()),
                Arc::new(timestamp_ms.finish()),
                Arc::new(asset_id.finish()),
                Arc::new(price.finish()),
                Arc::new(size.finish()),
                Arc::new(side.finish()),
                Arc::new(hash_b.finish()),
                Arc::new(best_bid.finish()),
                Arc::new(best_ask.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }
}

// ---------------------------------------------------------------------------
// PolymarketBestBidAskSchema
// ---------------------------------------------------------------------------

pub struct PolymarketBestBidAskSchema;

impl PolymarketBestBidAskSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("market", DataType::Utf8, false),
            Field::new("asset_id", DataType::Utf8, false),
            Field::new("best_bid", DataType::Float64, true),
            Field::new("best_ask", DataType::Float64, true),
            Field::new("spread", DataType::Float64, true),
            Field::new("timestamp_ms", DataType::Int64, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for PolymarketBestBidAskSchema {
    fn schema_name(&self) -> &str {
        "polymarket_best_bid_ask"
    }

    fn schema_version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "best_bid_ask"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut market = StringBuilder::new();
        let mut asset_id = StringBuilder::new();
        let mut best_bid = Float64Builder::new();
        let mut best_ask = Float64Builder::new();
        let mut spread = Float64Builder::new();
        let mut timestamp_ms = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let mkt = match json.get("market").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket best_bid_ask missing 'market', skipping");
                    continue;
                }
            };
            let aid = match json.get("asset_id").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Polymarket best_bid_ask missing 'asset_id', skipping");
                    continue;
                }
            };

            market.append_value(mkt);
            asset_id.append_value(aid);

            match json.get("best_bid").and_then(parse_f64_str) {
                Some(v) => best_bid.append_value(v),
                None => best_bid.append_null(),
            }
            match json.get("best_ask").and_then(parse_f64_str) {
                Some(v) => best_ask.append_value(v),
                None => best_ask.append_null(),
            }
            match json.get("spread").and_then(parse_f64_str) {
                Some(v) => spread.append_value(v),
                None => spread.append_null(),
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
                Arc::new(market.finish()),
                Arc::new(asset_id.finish()),
                Arc::new(best_bid.finish()),
                Arc::new(best_ask.finish()),
                Arc::new(spread.finish()),
                Arc::new(timestamp_ms.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
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
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((price.value(0) - 0.55).abs() < f64::EPSILON);

        let side = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(side.value(0), "BUY");

        let size = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((size.value(0) - 100.0).abs() < f64::EPSILON);

        let fee = batch
            .column(5)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((fee.value(0) - 0.0).abs() < f64::EPSILON);

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
            .downcast_ref::<Float64Array>()
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
        let msg2 = br#"{"event_type":"last_trade_price","asset_id":"B","market":"0x2","price":"0.60","side":"SELL","size":"200"}"#;
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

    // -------------------------------------------------------------------
    // PolymarketPriceChangeSchema
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_polymarket_price_change() {
        let schema = PolymarketPriceChangeSchema;
        let json = br#"{"event_type":"price_change","market":"0x1234abcd","timestamp":"1706000000000","price_changes":[{"asset_id":"21742633143463","price":"0.55","size":"750","side":"BUY","hash":"order123","best_bid":"0.55","best_ask":"0.56"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 11);

        let mkt = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(mkt.value(0), "0x1234abcd");

        let ts = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(ts.value(0), 1706000000000);

        let aid = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aid.value(0), "21742633143463");

        let price = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((price.value(0) - 0.55).abs() < f64::EPSILON);

        let size = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((size.value(0) - 750.0).abs() < f64::EPSILON);

        let side = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(side.value(0), "BUY");

        let hash = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(hash.value(0), "order123");

        let best_bid = batch
            .column(7)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((best_bid.value(0) - 0.55).abs() < f64::EPSILON);

        let best_ask = batch
            .column(8)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((best_ask.value(0) - 0.56).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_polymarket_price_change_multiple_items() {
        let schema = PolymarketPriceChangeSchema;
        // Two items in price_changes array → two rows in output
        let json = br#"{"event_type":"price_change","market":"0xabc","timestamp":"1706000000000","price_changes":[{"asset_id":"A","price":"0.50","size":"100","side":"BUY"},{"asset_id":"B","price":"0.60","size":"200","side":"SELL","hash":"h2","best_bid":"0.59","best_ask":"0.61"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 42, 5000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let mkt = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        // Both rows share the parent market
        assert_eq!(mkt.value(0), "0xabc");
        assert_eq!(mkt.value(1), "0xabc");

        let aids = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aids.value(0), "A");
        assert_eq!(aids.value(1), "B");

        // First item has no hash → null
        let hash = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(hash.is_null(0));
        assert_eq!(hash.value(1), "h2");

        // Both rows get the same nats_seq from the parent message
        let seq = batch
            .column(9)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        assert_eq!(seq.value(0), 42);
        assert_eq!(seq.value(1), 42);
    }

    #[test]
    fn test_parse_polymarket_price_change_nullable_fields() {
        let schema = PolymarketPriceChangeSchema;
        // Minimal: no timestamp, no optional item fields
        let json = br#"{"event_type":"price_change","market":"0xabc","price_changes":[{"asset_id":"X","price":"0.70","size":"50","side":"SELL"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        // timestamp_ms should be null
        let ts = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(ts.is_null(0));

        // hash, best_bid, best_ask should be null
        let hash = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(hash.is_null(0));

        let best_bid = batch
            .column(7)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(best_bid.is_null(0));

        let best_ask = batch
            .column(8)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(best_ask.is_null(0));
    }

    #[test]
    fn test_price_change_skip_missing_market() {
        let schema = PolymarketPriceChangeSchema;
        let json = br#"{"event_type":"price_change","price_changes":[{"asset_id":"X","price":"0.70","size":"50","side":"SELL"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_price_change_skip_missing_price_changes() {
        let schema = PolymarketPriceChangeSchema;
        let json = br#"{"event_type":"price_change","market":"0xabc"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_price_change_skip_item_missing_required() {
        let schema = PolymarketPriceChangeSchema;
        // Item missing 'price' → skipped, but other items in same message still processed
        let json = br#"{"event_type":"price_change","market":"0xabc","price_changes":[{"asset_id":"A","size":"50","side":"BUY"},{"asset_id":"B","price":"0.60","size":"100","side":"SELL"}]}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        // Only second item survives
        assert_eq!(batch.num_rows(), 1);

        let aids = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aids.value(0), "B");
    }

    #[test]
    fn test_price_change_empty_batch() {
        let schema = PolymarketPriceChangeSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 11);
    }

    // -------------------------------------------------------------------
    // PolymarketBestBidAskSchema
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_polymarket_best_bid_ask() {
        let schema = PolymarketBestBidAskSchema;
        let json = br#"{"event_type":"best_bid_ask","market":"0x1234abcd","asset_id":"21742633143463","best_bid":"0.55","best_ask":"0.56","spread":"0.01","timestamp":"1706000000000"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 8);

        let mkt = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(mkt.value(0), "0x1234abcd");

        let aid = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aid.value(0), "21742633143463");

        let bid = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((bid.value(0) - 0.55).abs() < f64::EPSILON);

        let ask = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((ask.value(0) - 0.56).abs() < f64::EPSILON);

        let spread = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((spread.value(0) - 0.01).abs() < f64::EPSILON);

        let ts = batch
            .column(5)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(ts.value(0), 1706000000000);
    }

    #[test]
    fn test_parse_polymarket_best_bid_ask_nullable_fields() {
        let schema = PolymarketBestBidAskSchema;
        // Minimal: only required fields
        let json = br#"{"event_type":"best_bid_ask","market":"0xabc","asset_id":"123"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);

        let bid = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(bid.is_null(0));

        let ask = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(ask.is_null(0));

        let spread = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(spread.is_null(0));

        let ts = batch
            .column(5)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(ts.is_null(0));
    }

    #[test]
    fn test_best_bid_ask_skip_missing_required() {
        let schema = PolymarketBestBidAskSchema;
        // Missing asset_id
        let json = br#"{"event_type":"best_bid_ask","market":"0xabc"}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_best_bid_ask_empty_batch() {
        let schema = PolymarketBestBidAskSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 8);
    }

    #[test]
    fn test_best_bid_ask_multi_message() {
        let schema = PolymarketBestBidAskSchema;
        let msg1 = br#"{"event_type":"best_bid_ask","market":"0x1","asset_id":"A","best_bid":"0.50","best_ask":"0.60"}"#;
        let msg2 = br#"{"event_type":"best_bid_ask","market":"0x2","asset_id":"B","spread":"0.05"}"#;
        let batch = schema
            .parse_batch(&[
                (msg1.to_vec(), 1, 1000),
                (msg2.to_vec(), 2, 2000),
            ])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let aids = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(aids.value(0), "A");
        assert_eq!(aids.value(1), "B");
    }
}
