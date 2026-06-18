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
}
