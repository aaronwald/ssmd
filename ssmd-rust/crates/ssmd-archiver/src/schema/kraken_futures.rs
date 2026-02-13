use std::sync::Arc;

use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::error::ArrowError;
use arrow::record_batch::RecordBatch;
use tracing::error;

use super::{hash_dedup_key, MessageSchema};

fn ts_type() -> DataType {
    DataType::Timestamp(TimeUnit::Microsecond, Some(Arc::from("UTC")))
}

/// Convert Kraken Futures epoch milliseconds to microseconds for Arrow timestamps.
fn epoch_ms_to_micros(ms: i64) -> i64 {
    ms * 1000
}

// ---------------------------------------------------------------------------
// KrakenFuturesTickerSchema
// ---------------------------------------------------------------------------

pub struct KrakenFuturesTickerSchema;

impl KrakenFuturesTickerSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("product_id", DataType::Utf8, false),
            Field::new("bid", DataType::Float64, false),
            Field::new("bid_size", DataType::Float64, false),
            Field::new("ask", DataType::Float64, false),
            Field::new("ask_size", DataType::Float64, false),
            Field::new("last", DataType::Float64, false),
            Field::new("volume", DataType::Float64, false),
            Field::new("volume_quote", DataType::Float64, true),
            Field::new("open", DataType::Float64, true),
            Field::new("high", DataType::Float64, true),
            Field::new("low", DataType::Float64, true),
            Field::new("change", DataType::Float64, true),
            Field::new("index_price", DataType::Float64, true),
            Field::new("mark_price", DataType::Float64, true),
            Field::new("open_interest", DataType::Float64, true),
            Field::new("funding_rate", DataType::Float64, true),
            Field::new("funding_rate_prediction", DataType::Float64, true),
            Field::new("next_funding_rate_time", DataType::Int64, true),
            Field::new("time", ts_type(), false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KrakenFuturesTickerSchema {
    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "ticker"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut product_id = StringBuilder::new();
        let mut bid = Float64Builder::new();
        let mut bid_size = Float64Builder::new();
        let mut ask = Float64Builder::new();
        let mut ask_size = Float64Builder::new();
        let mut last = Float64Builder::new();
        let mut volume_b = Float64Builder::new();
        let mut volume_quote = Float64Builder::new();
        let mut open = Float64Builder::new();
        let mut high = Float64Builder::new();
        let mut low = Float64Builder::new();
        let mut change = Float64Builder::new();
        let mut index_price = Float64Builder::new();
        let mut mark_price = Float64Builder::new();
        let mut open_interest = Float64Builder::new();
        let mut funding_rate = Float64Builder::new();
        let mut funding_rate_prediction = Float64Builder::new();
        let mut next_funding_rate_time = Int64Builder::new();
        let mut time = TimestampMicrosecondBuilder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            // Flat V1 format — no data[] wrapper
            let pid = match json.get("product_id").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    error!("Kraken Futures ticker missing 'product_id', skipping");
                    continue;
                }
            };

            macro_rules! req_f64 {
                ($field:expr) => {
                    match json.get($field).and_then(|v| v.as_f64()) {
                        Some(v) => v,
                        None => {
                            error!(field = $field, "Kraken Futures ticker missing required field, skipping");
                            continue;
                        }
                    }
                };
            }

            macro_rules! opt_f64 {
                ($builder:expr, $field:expr) => {
                    match json.get($field).and_then(|v| v.as_f64()) {
                        Some(v) => $builder.append_value(v),
                        None => $builder.append_null(),
                    }
                };
            }

            let ts_ms = match json.get("time").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures ticker missing 'time', skipping");
                    continue;
                }
            };

            product_id.append_value(pid);
            bid.append_value(req_f64!("bid"));
            bid_size.append_value(req_f64!("bid_size"));
            ask.append_value(req_f64!("ask"));
            ask_size.append_value(req_f64!("ask_size"));
            last.append_value(req_f64!("last"));
            volume_b.append_value(req_f64!("volume"));
            opt_f64!(volume_quote, "volumeQuote");
            opt_f64!(open, "open");
            opt_f64!(high, "high");
            opt_f64!(low, "low");
            opt_f64!(change, "change");
            opt_f64!(index_price, "index");
            opt_f64!(mark_price, "markPrice");
            opt_f64!(open_interest, "openInterest");
            opt_f64!(funding_rate, "funding_rate");
            opt_f64!(funding_rate_prediction, "funding_rate_prediction");
            match json.get("next_funding_rate_time").and_then(|v| v.as_i64()) {
                Some(v) => next_funding_rate_time.append_value(v),
                None => next_funding_rate_time.append_null(),
            }
            time.append_value(epoch_ms_to_micros(ts_ms));
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(product_id.finish()),
                Arc::new(bid.finish()),
                Arc::new(bid_size.finish()),
                Arc::new(ask.finish()),
                Arc::new(ask_size.finish()),
                Arc::new(last.finish()),
                Arc::new(volume_b.finish()),
                Arc::new(volume_quote.finish()),
                Arc::new(open.finish()),
                Arc::new(high.finish()),
                Arc::new(low.finish()),
                Arc::new(change.finish()),
                Arc::new(index_price.finish()),
                Arc::new(mark_price.finish()),
                Arc::new(open_interest.finish()),
                Arc::new(funding_rate.finish()),
                Arc::new(funding_rate_prediction.finish()),
                Arc::new(next_funding_rate_time.finish()),
                Arc::new(time.finish().with_timezone("UTC")),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let pid = json.get("product_id")?.as_str()?;
        let bid = format!("{}", json.get("bid")?.as_f64()?);
        let ask = format!("{}", json.get("ask")?.as_f64()?);
        let last = format!("{}", json.get("last")?.as_f64()?);
        let vol = format!("{}", json.get("volume")?.as_f64()?);
        Some(hash_dedup_key(&[pid, &bid, &ask, &last, &vol]))
    }
}

// ---------------------------------------------------------------------------
// KrakenFuturesTradeSchema
// ---------------------------------------------------------------------------

pub struct KrakenFuturesTradeSchema;

impl KrakenFuturesTradeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("product_id", DataType::Utf8, false),
            Field::new("uid", DataType::Utf8, false),
            Field::new("side", DataType::Utf8, false),
            Field::new("trade_type", DataType::Utf8, false),
            Field::new("seq", DataType::Int64, false),
            Field::new("qty", DataType::Float64, false),
            Field::new("price", DataType::Float64, false),
            Field::new("time", ts_type(), false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for KrakenFuturesTradeSchema {
    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "trade"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut product_id = StringBuilder::new();
        let mut uid = StringBuilder::new();
        let mut side = StringBuilder::new();
        let mut trade_type = StringBuilder::new();
        let mut seq_b = Int64Builder::new();
        let mut qty = Float64Builder::new();
        let mut price = Float64Builder::new();
        let mut time = TimestampMicrosecondBuilder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            let pid = match json.get("product_id").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    error!("Kraken Futures trade missing 'product_id', skipping");
                    continue;
                }
            };
            let u = match json.get("uid").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    error!("Kraken Futures trade missing 'uid', skipping");
                    continue;
                }
            };
            let s = match json.get("side").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures trade missing 'side', skipping");
                    continue;
                }
            };
            let tt = match json.get("type").and_then(|v| v.as_str()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures trade missing 'type', skipping");
                    continue;
                }
            };
            let sq = match json.get("seq").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures trade missing 'seq', skipping");
                    continue;
                }
            };
            let q = match json.get("qty").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures trade missing 'qty', skipping");
                    continue;
                }
            };
            let p = match json.get("price").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures trade missing 'price', skipping");
                    continue;
                }
            };
            let ts_ms = match json.get("time").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!("Kraken Futures trade missing 'time', skipping");
                    continue;
                }
            };

            product_id.append_value(pid);
            uid.append_value(u);
            side.append_value(s);
            trade_type.append_value(tt);
            seq_b.append_value(sq);
            qty.append_value(q);
            price.append_value(p);
            time.append_value(epoch_ms_to_micros(ts_ms));
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(product_id.finish()),
                Arc::new(uid.finish()),
                Arc::new(side.finish()),
                Arc::new(trade_type.finish()),
                Arc::new(seq_b.finish()),
                Arc::new(qty.finish()),
                Arc::new(price.finish()),
                Arc::new(time.finish().with_timezone("UTC")),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }

    fn dedup_key(&self, json: &serde_json::Value) -> Option<u64> {
        let uid = json.get("uid")?.as_str()?;
        Some(hash_dedup_key(&["trade", uid]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_futures_ticker() {
        let schema = KrakenFuturesTickerSchema;
        let json = br#"{"time":1770920339237,"product_id":"PF_ETHUSD","funding_rate":-0.005,"funding_rate_prediction":-0.014,"next_funding_rate_time":1770922800000,"feed":"ticker","bid":1907.3,"bid_size":0.097,"ask":1907.4,"ask_size":0.112,"volume":56514.738,"index":1907.83,"last":1907.7,"change":-1.45,"openInterest":17059.147,"markPrice":1907.459,"volumeQuote":110391688.155,"open":1935.7,"high":2000.2,"low":1895.0}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 1, 1000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 21);

        let pid = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(pid.value(0), "PF_ETHUSD");

        let bid_val = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(bid_val.value(0), 1907.3);

        let fr = batch.column(15).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(fr.value(0), -0.005);

        // time: epoch ms → micros
        let ts = batch.column(18).as_any().downcast_ref::<TimestampMicrosecondArray>().unwrap();
        assert_eq!(ts.value(0), 1770920339237 * 1000);
    }

    #[test]
    fn test_parse_futures_trade() {
        let schema = KrakenFuturesTradeSchema;
        let json = br#"{"feed":"trade","product_id":"PF_XBTUSD","uid":"16dc1852-127b-40c9-acbb-5916ce98ee04","side":"sell","type":"fill","seq":96905,"time":1770920339688,"qty":0.0003,"price":65368.0}"#;
        let batch = schema
            .parse_batch(&[(json.to_vec(), 10, 5000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 10);

        let pid = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(pid.value(0), "PF_XBTUSD");

        let uid_val = batch.column(1).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(uid_val.value(0), "16dc1852-127b-40c9-acbb-5916ce98ee04");

        let side_val = batch.column(2).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(side_val.value(0), "sell");

        let tt = batch.column(3).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(tt.value(0), "fill");

        let seq_val = batch.column(4).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(seq_val.value(0), 96905);

        let price_val = batch.column(6).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price_val.value(0), 65368.0);

        let ts = batch.column(7).as_any().downcast_ref::<TimestampMicrosecondArray>().unwrap();
        assert_eq!(ts.value(0), 1770920339688 * 1000);
    }

    #[test]
    fn test_dedup_key_futures_ticker() {
        let schema = KrakenFuturesTickerSchema;
        let json: serde_json::Value = serde_json::from_str(
            r#"{"product_id":"PF_XBTUSD","bid":65360.0,"ask":65361.0,"last":65367.0,"volume":5826.47}"#,
        ).unwrap();
        let key = schema.dedup_key(&json);
        assert!(key.is_some());
    }

    #[test]
    fn test_dedup_key_futures_trade() {
        let schema = KrakenFuturesTradeSchema;
        let json1: serde_json::Value = serde_json::from_str(
            r#"{"uid":"16dc1852-127b-40c9-acbb-5916ce98ee04"}"#,
        ).unwrap();
        let json2: serde_json::Value = serde_json::from_str(
            r#"{"uid":"d57af348-e45f-4191-a15a-9946dbf1870b"}"#,
        ).unwrap();
        let key1 = schema.dedup_key(&json1);
        let key2 = schema.dedup_key(&json2);
        assert!(key1.is_some());
        assert!(key2.is_some());
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_skip_missing_product_id() {
        let schema = KrakenFuturesTickerSchema;
        let json = br#"{"feed":"ticker","bid":1907.3}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn test_empty_batch() {
        let schema = KrakenFuturesTickerSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 21);

        let schema = KrakenFuturesTradeSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 10);
    }
}
