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

// ---------------------------------------------------------------------------
// MassiveTradeSchema
// ---------------------------------------------------------------------------

pub struct MassiveTradeSchema;

impl MassiveTradeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("price", DataType::Float64, false),
            Field::new("size", DataType::Float64, false),
            Field::new("sequence", DataType::Int64, true),
            Field::new("exchange_ts_ms", DataType::Int64, false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for MassiveTradeSchema {
    fn schema_name(&self) -> &str {
        "massive_trade"
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
        let mut price = Float64Builder::new();
        let mut size = Float64Builder::new();
        let mut sequence = Int64Builder::new();
        let mut exchange_ts_ms = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            // Each NATS message is a single Polygon event object (not an array).
            // Skip non-trade events (e.g. status messages with ev != "T").
            match json.get("ev").and_then(|v| v.as_str()) {
                Some("T") => {}
                Some(ev) => {
                    error!(ev, "massive trade: unexpected ev type, skipping");
                    continue;
                }
                None => {
                    error!("massive trade: missing ev field, skipping");
                    continue;
                }
            }

            let sym = match json.get("sym").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    error!("massive trade: missing sym, skipping");
                    continue;
                }
            };
            let p = match json.get("p").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("massive trade: missing p, skipping");
                    continue;
                }
            };
            let s = match json.get("s").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("massive trade: missing s, skipping");
                    continue;
                }
            };
            // q (sequence) is optional metadata — a trade missing only q is still a valid
            // trade and must be archived (Complete Data Archive pillar). Archive with null
            // sequence rather than dropping the row. sym/p/s/t are required for a trade
            // to be meaningful; a missing one of those still skips the row.
            let q = json.get("q").and_then(|v| v.as_i64());
            if q.is_none() {
                error!("massive trade: missing q, archiving with null sequence");
            }
            let t = match json.get("t").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!("massive trade: missing t, skipping");
                    continue;
                }
            };

            symbol.append_value(sym);
            price.append_value(p);
            size.append_value(s);
            match q {
                Some(v) => sequence.append_value(v),
                None => sequence.append_null(),
            };
            exchange_ts_ms.append_value(t);
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(symbol.finish()),
                Arc::new(price.finish()),
                Arc::new(size.finish()),
                Arc::new(sequence.finish()),
                Arc::new(exchange_ts_ms.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }
}

// ---------------------------------------------------------------------------
// MassiveQuoteSchema
// ---------------------------------------------------------------------------

pub struct MassiveQuoteSchema;

impl MassiveQuoteSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("bid", DataType::Float64, false),
            Field::new("bid_size", DataType::Float64, false),
            Field::new("ask", DataType::Float64, false),
            Field::new("ask_size", DataType::Float64, false),
            Field::new("exchange_ts_ms", DataType::Int64, false),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for MassiveQuoteSchema {
    fn schema_name(&self) -> &str {
        "massive_quote"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(Self::arrow_schema())
    }

    fn message_type(&self) -> &str {
        "quote"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        let mut symbol = StringBuilder::new();
        let mut bid = Float64Builder::new();
        let mut bid_size = Float64Builder::new();
        let mut ask = Float64Builder::new();
        let mut ask_size = Float64Builder::new();
        let mut exchange_ts_ms = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            // Each NATS message is a single Polygon event object (not an array).
            // Skip non-quote events.
            match json.get("ev").and_then(|v| v.as_str()) {
                Some("Q") => {}
                Some(ev) => {
                    error!(ev, "massive quote: unexpected ev type, skipping");
                    continue;
                }
                None => {
                    error!("massive quote: missing ev field, skipping");
                    continue;
                }
            }

            let sym = match json.get("sym").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    error!("massive quote: missing sym, skipping");
                    continue;
                }
            };
            let bp = match json.get("bp").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("massive quote: missing bp, skipping");
                    continue;
                }
            };
            let bs = match json.get("bs").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("massive quote: missing bs, skipping");
                    continue;
                }
            };
            let ap = match json.get("ap").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("massive quote: missing ap, skipping");
                    continue;
                }
            };
            let as_ = match json.get("as").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    error!("massive quote: missing as, skipping");
                    continue;
                }
            };
            let t = match json.get("t").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!("massive quote: missing t, skipping");
                    continue;
                }
            };

            symbol.append_value(sym);
            bid.append_value(bp);
            bid_size.append_value(bs);
            ask.append_value(ap);
            ask_size.append_value(as_);
            exchange_ts_ms.append_value(t);
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(symbol.finish()),
                Arc::new(bid.finish()),
                Arc::new(bid_size.finish()),
                Arc::new(ask.finish()),
                Arc::new(ask_size.finish()),
                Arc::new(exchange_ts_ms.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }
}

// ---------------------------------------------------------------------------
// OHLCV aggregate schemas (Polygon Starter plan: A. = 1s, AM. = 1m)
// ---------------------------------------------------------------------------

/// Shared Arrow schema for both the per-second and per-minute aggregate
/// schemas. The two schemas are identical in shape; only `schema_name`,
/// `message_type`, and the `ev` validation differ.
fn agg_arrow_schema() -> Schema {
    Schema::new(vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
        Field::new("vwap", DataType::Float64, true),
        Field::new("avg_trade_size", DataType::Float64, true),
        Field::new("accumulated_volume", DataType::Float64, true),
        Field::new("daily_vwap", DataType::Float64, true),
        Field::new("start_ts_ms", DataType::Int64, false),
        Field::new("end_ts_ms", DataType::Int64, false),
        Field::new("_nats_seq", DataType::UInt64, false),
        Field::new("_received_at", ts_type(), false),
    ])
}

/// Parse a batch of single-object aggregate JSON messages into a RecordBatch.
///
/// `expected_ev` selects which Polygon aggregate variant this batch accepts
/// (`"A"` for per-second, `"AM"` for per-minute). Rows whose `ev` does not
/// match are skipped (and logged) rather than silently coerced.
///
/// Required fields (o/h/l/c/v/s/e) missing → the row is skipped and logged
/// (a bar without OHLC or a window is not meaningful). Optional fields
/// (vw/av/a/z) absent → the corresponding column is null, preserving the row.
fn parse_agg_batch(
    expected_ev: &str,
    label: &str,
    messages: &[(Vec<u8>, u64, i64)],
) -> Result<RecordBatch, ArrowError> {
    let mut symbol = StringBuilder::new();
    let mut open = Float64Builder::new();
    let mut high = Float64Builder::new();
    let mut low = Float64Builder::new();
    let mut close = Float64Builder::new();
    let mut volume = Float64Builder::new();
    let mut vwap = Float64Builder::new();
    let mut avg_trade_size = Float64Builder::new();
    let mut accumulated_volume = Float64Builder::new();
    let mut daily_vwap = Float64Builder::new();
    let mut start_ts_ms = Int64Builder::new();
    let mut end_ts_ms = Int64Builder::new();
    let mut nats_seq = UInt64Builder::new();
    let mut received_at = TimestampMicrosecondBuilder::new();

    for (data, seq, recv_at) in messages {
        let json: serde_json::Value =
            serde_json::from_slice(data).map_err(|e| ArrowError::JsonError(e.to_string()))?;

        // Each NATS message is a single Polygon event object (not an array).
        // Skip events whose ev does not match this schema's aggregate variant.
        match json.get("ev").and_then(|v| v.as_str()) {
            Some(ev) if ev == expected_ev => {}
            Some(ev) => {
                error!(ev, label, "massive aggregate: unexpected ev type, skipping");
                continue;
            }
            None => {
                error!(label, "massive aggregate: missing ev field, skipping");
                continue;
            }
        }

        let sym = match json.get("sym").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                error!(label, "massive aggregate: missing sym, skipping");
                continue;
            }
        };
        let o = match json.get("o").and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing o, skipping");
                continue;
            }
        };
        let h = match json.get("h").and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing h, skipping");
                continue;
            }
        };
        let l = match json.get("l").and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing l, skipping");
                continue;
            }
        };
        let c = match json.get("c").and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing c, skipping");
                continue;
            }
        };
        let v = match json.get("v").and_then(|v| v.as_f64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing v, skipping");
                continue;
            }
        };
        let s_ts = match json.get("s").and_then(|v| v.as_i64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing s (window start), skipping");
                continue;
            }
        };
        let e_ts = match json.get("e").and_then(|v| v.as_i64()) {
            Some(v) => v,
            None => {
                error!(label, "massive aggregate: missing e (window end), skipping");
                continue;
            }
        };

        // Optional fields — null when absent rather than dropping the bar.
        let vw = json.get("vw").and_then(|v| v.as_f64());
        let z = json.get("z").and_then(|v| v.as_f64());
        let av = json.get("av").and_then(|v| v.as_f64());
        let a = json.get("a").and_then(|v| v.as_f64());

        symbol.append_value(sym);
        open.append_value(o);
        high.append_value(h);
        low.append_value(l);
        close.append_value(c);
        volume.append_value(v);
        vwap.append_option(vw);
        avg_trade_size.append_option(z);
        accumulated_volume.append_option(av);
        daily_vwap.append_option(a);
        start_ts_ms.append_value(s_ts);
        end_ts_ms.append_value(e_ts);
        nats_seq.append_value(*seq);
        received_at.append_value(*recv_at);
    }

    RecordBatch::try_new(
        Arc::new(agg_arrow_schema()),
        vec![
            Arc::new(symbol.finish()),
            Arc::new(open.finish()),
            Arc::new(high.finish()),
            Arc::new(low.finish()),
            Arc::new(close.finish()),
            Arc::new(volume.finish()),
            Arc::new(vwap.finish()),
            Arc::new(avg_trade_size.finish()),
            Arc::new(accumulated_volume.finish()),
            Arc::new(daily_vwap.finish()),
            Arc::new(start_ts_ms.finish()),
            Arc::new(end_ts_ms.finish()),
            Arc::new(nats_seq.finish()),
            Arc::new(received_at.finish().with_timezone("UTC")),
        ],
    )
}

/// Per-second OHLCV aggregate schema (`"ev":"A"`).
pub struct MassiveOhlcv1sSchema;

impl MessageSchema for MassiveOhlcv1sSchema {
    fn schema_name(&self) -> &str {
        "massive_ohlcv_1s"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(agg_arrow_schema())
    }

    fn message_type(&self) -> &str {
        "ohlcv_1s"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        parse_agg_batch("A", "ohlcv_1s", messages)
    }
}

/// Per-minute OHLCV aggregate schema (`"ev":"AM"`).
pub struct MassiveOhlcv1mSchema;

impl MessageSchema for MassiveOhlcv1mSchema {
    fn schema_name(&self) -> &str {
        "massive_ohlcv_1m"
    }

    fn schema_version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Arc<Schema> {
        Arc::new(agg_arrow_schema())
    }

    fn message_type(&self) -> &str {
        "ohlcv_1m"
    }

    fn parse_batch(&self, messages: &[(Vec<u8>, u64, i64)]) -> Result<RecordBatch, ArrowError> {
        parse_agg_batch("AM", "ohlcv_1m", messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageSchema;

    #[test]
    fn trade_schema_metadata() {
        let s = MassiveTradeSchema;
        assert_eq!(s.schema_name(), "massive_trade");
        assert_eq!(s.message_type(), "trade");
        assert_eq!(s.schema_version(), "1.0.0");
        let names: Vec<_> = s.schema().fields().iter().map(|f| f.name().clone()).collect();
        assert!(names.contains(&"symbol".to_string()));
        assert!(names.contains(&"price".to_string()));
        assert!(names.contains(&"size".to_string()));
        assert!(names.contains(&"sequence".to_string()));
        assert!(names.contains(&"_received_at".to_string()));
    }

    #[test]
    fn quote_schema_metadata() {
        let s = MassiveQuoteSchema;
        assert_eq!(s.schema_name(), "massive_quote");
        assert_eq!(s.message_type(), "quote");
        let names: Vec<_> = s.schema().fields().iter().map(|f| f.name().clone()).collect();
        for f in ["symbol", "bid", "bid_size", "ask", "ask_size", "_received_at"] {
            assert!(names.contains(&f.to_string()), "missing {f}");
        }
    }

    #[test]
    fn trade_parse_batch_round_trip() {
        let schema = MassiveTradeSchema;
        let json = br#"{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987,"x":11,"c":[14],"z":3}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 42, 9999)]).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 7);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "AAPL");

        let price = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 189.42);

        let size = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(size.value(0), 100.0);

        let sequence = batch.column(3).as_any().downcast_ref::<Int64Array>().unwrap();
        assert!(sequence.is_valid(0), "sequence should be non-null when q is present");
        assert_eq!(sequence.value(0), 987);

        let ts = batch.column(4).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(ts.value(0), 1718658000123);

        let nats = batch.column(5).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 42);

        let recv = batch.column(6).as_any().downcast_ref::<TimestampMicrosecondArray>().unwrap();
        assert_eq!(recv.value(0), 9999);
    }

    #[test]
    fn quote_parse_batch_round_trip() {
        let schema = MassiveQuoteSchema;
        let json = br#"{"ev":"Q","sym":"SPY","bp":543.10,"bs":2,"ap":543.12,"as":3,"t":1718658000456,"z":3}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 77, 8888)]).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 8);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "SPY");

        let bid = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(bid.value(0), 543.10);

        let bid_size = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(bid_size.value(0), 2.0);

        let ask = batch.column(3).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(ask.value(0), 543.12);

        let ask_size = batch.column(4).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(ask_size.value(0), 3.0);

        let ts = batch.column(5).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(ts.value(0), 1718658000456);

        let nats = batch.column(6).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 77);

        let recv = batch.column(7).as_any().downcast_ref::<TimestampMicrosecondArray>().unwrap();
        assert_eq!(recv.value(0), 8888);
    }

    #[test]
    fn trade_skip_non_trade_ev() {
        let schema = MassiveTradeSchema;
        // status message with ev != "T" should be skipped
        let json = br#"{"ev":"status","status":"connected","message":"Connected Successfully"}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn quote_skip_non_quote_ev() {
        let schema = MassiveQuoteSchema;
        // status message with ev != "Q" should be skipped
        let json = br#"{"ev":"status","status":"auth_success"}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn trade_empty_batch() {
        let schema = MassiveTradeSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 7);
    }

    #[test]
    fn quote_empty_batch() {
        let schema = MassiveQuoteSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 8);
    }

    #[test]
    fn trade_parse_multiple_messages() {
        let schema = MassiveTradeSchema;
        let msg1 = br#"{"ev":"T","sym":"AAPL","p":189.42,"s":100,"t":1718658000123,"q":987}"#;
        let msg2 = br#"{"ev":"T","sym":"MSFT","p":420.00,"s":50,"t":1718658001000,"q":988}"#;
        let batch = schema
            .parse_batch(&[(msg1.to_vec(), 10, 1000), (msg2.to_vec(), 11, 2000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "AAPL");
        assert_eq!(sym.value(1), "MSFT");

        let nats = batch.column(5).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 10);
        assert_eq!(nats.value(1), 11);
    }

    /// Regression test: a trade missing the q (sequence) field must NOT be dropped.
    /// The row is archived with a null sequence; all other required fields are preserved.
    /// This ensures the Complete Data Archive pillar is upheld — q is optional metadata,
    /// unlike sym/p/s/t which are required for a trade to be meaningful.
    #[test]
    fn trade_missing_q_archived_with_null_sequence() {
        let schema = MassiveTradeSchema;

        // msg_no_q: valid trade, q field absent — must produce one row with null sequence
        let msg_no_q = br#"{"ev":"T","sym":"GOOG","p":175.50,"s":200,"t":1718658002000}"#;
        // msg_with_q: normal trade with q — must produce one row with non-null sequence
        let msg_with_q = br#"{"ev":"T","sym":"TSLA","p":250.00,"s":50,"t":1718658003000,"q":555}"#;

        let batch = schema
            .parse_batch(&[
                (msg_no_q.to_vec(), 20, 3000),
                (msg_with_q.to_vec(), 21, 4000),
            ])
            .unwrap();

        // Both rows are present — no silent drop
        assert_eq!(batch.num_rows(), 2, "trade missing q must not be dropped");
        assert_eq!(batch.num_columns(), 7);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "GOOG");
        assert_eq!(sym.value(1), "TSLA");

        let price = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 175.50);
        assert_eq!(price.value(1), 250.00);

        let size = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(size.value(0), 200.0);
        assert_eq!(size.value(1), 50.0);

        let sequence = batch.column(3).as_any().downcast_ref::<Int64Array>().unwrap();
        // Row 0 (no q): sequence must be null
        assert!(sequence.is_null(0), "sequence must be null when q is absent");
        // Row 1 (has q): sequence must be the provided value
        assert!(sequence.is_valid(1), "sequence must be non-null when q is present");
        assert_eq!(sequence.value(1), 555);

        let ts = batch.column(4).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(ts.value(0), 1718658002000);
        assert_eq!(ts.value(1), 1718658003000);

        let nats = batch.column(5).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 20);
        assert_eq!(nats.value(1), 21);
    }

    // ── OHLCV aggregate schemas ───────────────────────────────────────────────

    #[test]
    fn ohlcv_1s_schema_metadata() {
        let s = MassiveOhlcv1sSchema;
        assert_eq!(s.schema_name(), "massive_ohlcv_1s");
        assert_eq!(s.message_type(), "ohlcv_1s");
        assert_eq!(s.schema_version(), "1.0.0");
        let names: Vec<_> = s.schema().fields().iter().map(|f| f.name().clone()).collect();
        for f in [
            "symbol",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "vwap",
            "avg_trade_size",
            "accumulated_volume",
            "daily_vwap",
            "start_ts_ms",
            "end_ts_ms",
            "_nats_seq",
            "_received_at",
        ] {
            assert!(names.contains(&f.to_string()), "missing {f}");
        }
    }

    #[test]
    fn ohlcv_1m_schema_metadata() {
        let s = MassiveOhlcv1mSchema;
        assert_eq!(s.schema_name(), "massive_ohlcv_1m");
        assert_eq!(s.message_type(), "ohlcv_1m");
        assert_eq!(s.schema_version(), "1.0.0");
        // Same Arrow shape as the per-second schema.
        let names: Vec<_> = s.schema().fields().iter().map(|f| f.name().clone()).collect();
        assert_eq!(names.len(), 14);
    }

    #[test]
    fn ohlcv_1s_parse_batch_round_trip() {
        let schema = MassiveOhlcv1sSchema;
        let json = br#"{"ev":"A","sym":"AAPL","v":80,"av":144673,"vw":296.32,"o":296.32,"c":296.30,"h":296.40,"l":296.20,"a":296.6776,"z":80,"s":1782124955000,"e":1782124956000,"dv":"80.0","dav":"144673.243177"}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 42, 9999)]).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 14);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "AAPL");

        let open = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(open.value(0), 296.32);
        let high = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(high.value(0), 296.40);
        let low = batch.column(3).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(low.value(0), 296.20);
        let close = batch.column(4).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(close.value(0), 296.30);
        let volume = batch.column(5).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(volume.value(0), 80.0);

        let vwap = batch.column(6).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!(vwap.is_valid(0));
        assert_eq!(vwap.value(0), 296.32);
        let avg_trade_size = batch.column(7).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(avg_trade_size.value(0), 80.0);
        let accumulated_volume = batch.column(8).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(accumulated_volume.value(0), 144673.0);
        let daily_vwap = batch.column(9).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(daily_vwap.value(0), 296.6776);

        let start = batch.column(10).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(start.value(0), 1782124955000);
        let end = batch.column(11).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(end.value(0), 1782124956000);

        let nats = batch.column(12).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 42);
        let recv = batch.column(13).as_any().downcast_ref::<TimestampMicrosecondArray>().unwrap();
        assert_eq!(recv.value(0), 9999);
    }

    #[test]
    fn ohlcv_1m_parse_batch_round_trip() {
        let schema = MassiveOhlcv1mSchema;
        let json = br#"{"ev":"AM","sym":"SPY","v":2000,"av":500000,"vw":543.11,"o":543.10,"c":543.12,"h":543.20,"l":543.00,"a":543.05,"z":120,"s":1782124920000,"e":1782124980000}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 77, 8888)]).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 14);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "SPY");
        let close = batch.column(4).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(close.value(0), 543.12);
        let start = batch.column(10).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(start.value(0), 1782124920000);
        let end = batch.column(11).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(end.value(0), 1782124980000);
    }

    #[test]
    fn ohlcv_1s_integer_numbers_parse_as_f64() {
        // v, vw, z etc. may arrive as integer JSON numbers — as_f64 handles both.
        let schema = MassiveOhlcv1sSchema;
        let json = br#"{"ev":"A","sym":"AAPL","v":100,"vw":296,"o":296,"c":296,"h":296,"l":296,"z":10,"s":1,"e":2}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        let volume = batch.column(5).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(volume.value(0), 100.0);
        let vwap = batch.column(6).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(vwap.value(0), 296.0);
    }

    #[test]
    fn ohlcv_1s_skip_wrong_ev() {
        let schema = MassiveOhlcv1sSchema;
        // An AM event must be skipped by the per-second schema.
        let json = br#"{"ev":"AM","sym":"AAPL","o":1.0,"c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn ohlcv_1m_skip_wrong_ev() {
        let schema = MassiveOhlcv1mSchema;
        // An A event must be skipped by the per-minute schema.
        let json = br#"{"ev":"A","sym":"AAPL","o":1.0,"c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn ohlcv_1s_skip_status_ev() {
        let schema = MassiveOhlcv1sSchema;
        let json = br#"{"ev":"status","status":"connected"}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[test]
    fn ohlcv_1s_empty_batch() {
        let schema = MassiveOhlcv1sSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 14);
    }

    #[test]
    fn ohlcv_1m_empty_batch() {
        let schema = MassiveOhlcv1mSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 14);
    }

    #[test]
    fn ohlcv_1s_parse_multiple_messages() {
        let schema = MassiveOhlcv1sSchema;
        let msg1 = br#"{"ev":"A","sym":"AAPL","o":296.0,"c":296.5,"h":297.0,"l":295.0,"v":80,"vw":296.2,"s":1,"e":2}"#;
        let msg2 = br#"{"ev":"A","sym":"MSFT","o":420.0,"c":421.0,"h":422.0,"l":419.0,"v":50,"vw":420.5,"s":1,"e":2}"#;
        let batch = schema
            .parse_batch(&[(msg1.to_vec(), 10, 1000), (msg2.to_vec(), 11, 2000)])
            .unwrap();

        assert_eq!(batch.num_rows(), 2);
        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "AAPL");
        assert_eq!(sym.value(1), "MSFT");
        let nats = batch.column(12).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 10);
        assert_eq!(nats.value(1), 11);
    }

    /// Optional fields (vw/av/a/z) absent → null columns, row still archived.
    #[test]
    fn ohlcv_1s_optional_fields_absent_become_null() {
        let schema = MassiveOhlcv1sSchema;
        // Only required fields present — vw/av/a/z absent.
        let json = br#"{"ev":"A","sym":"GOOG","o":175.5,"c":175.6,"h":175.7,"l":175.4,"v":200,"s":1718658002000,"e":1718658003000}"#;
        let batch = schema.parse_batch(&[(json.to_vec(), 20, 3000)]).unwrap();

        assert_eq!(batch.num_rows(), 1, "row with absent optional fields must be archived");

        let vwap = batch.column(6).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!(vwap.is_null(0), "vwap must be null when vw absent");
        let avg_trade_size = batch.column(7).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!(avg_trade_size.is_null(0), "avg_trade_size must be null when z absent");
        let accumulated_volume = batch.column(8).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!(accumulated_volume.is_null(0), "accumulated_volume must be null when av absent");
        let daily_vwap = batch.column(9).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!(daily_vwap.is_null(0), "daily_vwap must be null when a absent");

        // Required fields still populated.
        let open = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(open.value(0), 175.5);
        let volume = batch.column(5).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(volume.value(0), 200.0);
    }

    /// A bar missing a required field (e.g. open) is skipped, not archived.
    #[test]
    fn ohlcv_1s_missing_required_field_skipped() {
        let schema = MassiveOhlcv1sSchema;
        // Missing "o" (open) — not a meaningful bar.
        let bad = br#"{"ev":"A","sym":"AAPL","c":2.0,"h":2.0,"l":1.0,"v":10,"s":1,"e":2}"#;
        let good = br#"{"ev":"A","sym":"MSFT","o":3.0,"c":4.0,"h":4.0,"l":3.0,"v":20,"s":1,"e":2}"#;
        let batch = schema
            .parse_batch(&[(bad.to_vec(), 1, 1000), (good.to_vec(), 2, 2000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 1, "bar missing required open must be skipped");
        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "MSFT");
    }
}
