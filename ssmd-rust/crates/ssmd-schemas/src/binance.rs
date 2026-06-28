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
// BinanceTradeSchema
// ---------------------------------------------------------------------------
//
// The connector publishes the whole Binance combined-stream `@trade` frame
// verbatim:
//
//   {"stream":"btcusdt@trade","data":{"e":"trade","E":<ms>,"s":"BTCUSDT",
//    "t":<id>,"p":"<price-str>","q":"<qty-str>","T":<tradeMs>,"m":<bool>,
//    "M":<bool>}}
//
// The trade is read from the NESTED `data` object (same as the bar-cache
// `parse_binance_trade`). Raw wire keys are renamed to normalized columns,
// mirroring the massive trade schema (`sym`/`t` → `symbol`/`exchange_ts_ms`):
//
//   data.s → symbol         (Utf8,    uppercased)
//   data.p → price          (Float64, parsed from a decimal STRING)
//   data.q → qty            (Float64, parsed from a decimal STRING)
//   data.T → exchange_ts_ms (Int64,   epoch millis integer — the trade time)
//   data.t → trade_id       (Int64,   nullable — archived null if absent)
//
// The column names `symbol` and `exchange_ts_ms` MUST match
// `binance.yaml`'s `identifier_field: symbol` / `timestamp_field:
// exchange_ts_ms` so DQ SQL lines up.
//
// Following the massive precedent, `_nats_seq` and `_received_at` envelope
// columns are appended. Like massive, a row missing only the optional id
// (`t`) is archived with a null `trade_id` rather than dropped (Complete Data
// Archive pillar); a row missing a required field (s/p/q/T) or carrying an
// unparseable price/qty is skipped and logged. The Binance `m`/`M` maker flags
// are intentionally not materialized — the plan's locked schema contract is
// the five fields above, matching the bar-cache parser and the massive trade
// schema (which likewise carries no side/maker flag).

pub struct BinanceTradeSchema;

impl BinanceTradeSchema {
    fn arrow_schema() -> Schema {
        Schema::new(vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("price", DataType::Float64, false),
            Field::new("qty", DataType::Float64, false),
            Field::new("exchange_ts_ms", DataType::Int64, false),
            Field::new("trade_id", DataType::Int64, true),
            Field::new("_nats_seq", DataType::UInt64, false),
            Field::new("_received_at", ts_type(), false),
        ])
    }
}

impl MessageSchema for BinanceTradeSchema {
    fn schema_name(&self) -> &str {
        "binance_trade"
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
        let mut qty = Float64Builder::new();
        let mut exchange_ts_ms = Int64Builder::new();
        let mut trade_id = Int64Builder::new();
        let mut nats_seq = UInt64Builder::new();
        let mut received_at = TimestampMicrosecondBuilder::new();

        for (data, seq, recv_at) in messages {
            let json: serde_json::Value = serde_json::from_slice(data)
                .map_err(|e| ArrowError::JsonError(e.to_string()))?;

            // The trade lives under the nested `data` object of the combined
            // stream frame. A frame without `data` is a control frame
            // (e.g. a subscribe response) — skip it.
            let inner = match json.get("data") {
                Some(d) => d,
                None => {
                    error!("binance trade: missing data object, skipping");
                    continue;
                }
            };

            // Only `e == "trade"` payloads are materialized; other inner event
            // types (klines, tickers) are skipped.
            match inner.get("e").and_then(|v| v.as_str()) {
                Some("trade") => {}
                Some(ev) => {
                    error!(ev, "binance trade: unexpected inner event type, skipping");
                    continue;
                }
                None => {
                    error!("binance trade: missing inner event type, skipping");
                    continue;
                }
            }

            let sym = match inner.get("s").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => {
                    error!("binance trade: missing s (symbol), skipping");
                    continue;
                }
            };
            // Price arrives as a decimal STRING. A non-numeric string is a bad
            // payload, not a zero — skip it loudly rather than coercing.
            let p = match inner.get("p").and_then(|v| v.as_str()) {
                Some(s) => match s.parse::<f64>() {
                    Ok(v) => v,
                    Err(e) => {
                        error!(price = s, error = %e, "binance trade: unparseable p, skipping");
                        continue;
                    }
                },
                None => {
                    error!("binance trade: missing p (price), skipping");
                    continue;
                }
            };
            let q = match inner.get("q").and_then(|v| v.as_str()) {
                Some(s) => match s.parse::<f64>() {
                    Ok(v) => v,
                    Err(e) => {
                        error!(qty = s, error = %e, "binance trade: unparseable q, skipping");
                        continue;
                    }
                },
                None => {
                    error!("binance trade: missing q (qty), skipping");
                    continue;
                }
            };
            // Trade time `T` is an epoch-millis integer.
            let t_ms = match inner.get("T").and_then(|v| v.as_i64()) {
                Some(v) => v,
                None => {
                    error!("binance trade: missing T (trade time), skipping");
                    continue;
                }
            };
            // Trade id `t` is optional metadata — a trade missing only `t` is
            // still a valid trade and must be archived (Complete Data Archive
            // pillar). Archive with a null trade_id rather than dropping it.
            let tid = inner.get("t").and_then(|v| v.as_i64());
            if tid.is_none() {
                error!("binance trade: missing t (trade id), archiving with null trade_id");
            }

            symbol.append_value(sym.to_uppercase());
            price.append_value(p);
            qty.append_value(q);
            exchange_ts_ms.append_value(t_ms);
            match tid {
                Some(v) => trade_id.append_value(v),
                None => trade_id.append_null(),
            };
            nats_seq.append_value(*seq);
            received_at.append_value(*recv_at);
        }

        RecordBatch::try_new(
            Arc::new(Self::arrow_schema()),
            vec![
                Arc::new(symbol.finish()),
                Arc::new(price.finish()),
                Arc::new(qty.finish()),
                Arc::new(exchange_ts_ms.finish()),
                Arc::new(trade_id.finish()),
                Arc::new(nats_seq.finish()),
                Arc::new(received_at.finish().with_timezone("UTC")),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{detect_message_type, MessageSchema, SchemaRegistry};

    /// A real combined-stream `@trade` frame as the connector publishes it.
    fn frame(sym: &str, price: &str, qty: &str, trade_id: i64, t_ms: i64) -> Vec<u8> {
        let lower = sym.to_lowercase();
        format!(
            r#"{{"stream":"{lower}@trade","data":{{"e":"trade","E":{t_ms},"s":"{sym}","t":{trade_id},"p":"{price}","q":"{qty}","T":{t_ms},"m":false,"M":true}}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn trade_schema_metadata() {
        let s = BinanceTradeSchema;
        assert_eq!(s.schema_name(), "binance_trade");
        assert_eq!(s.message_type(), "trade");
        assert_eq!(s.schema_version(), "1.0.0");
        let names: Vec<_> = s.schema().fields().iter().map(|f| f.name().clone()).collect();
        // Column names DQ depends on must be present and exactly named.
        for f in [
            "symbol",
            "price",
            "qty",
            "exchange_ts_ms",
            "trade_id",
            "_nats_seq",
            "_received_at",
        ] {
            assert!(names.contains(&f.to_string()), "missing {f}");
        }
        assert_eq!(names.len(), 7);
    }

    #[test]
    fn trade_schema_column_types() {
        let s = BinanceTradeSchema;
        let schema = s.schema();
        let by_name = |n: &str| schema.field_with_name(n).unwrap().clone();
        assert_eq!(by_name("symbol").data_type(), &DataType::Utf8);
        assert_eq!(by_name("price").data_type(), &DataType::Float64);
        assert_eq!(by_name("qty").data_type(), &DataType::Float64);
        assert_eq!(by_name("exchange_ts_ms").data_type(), &DataType::Int64);
        assert_eq!(by_name("trade_id").data_type(), &DataType::Int64);
        // exchange_ts_ms is the DQ timestamp field — must be non-null.
        assert!(!by_name("exchange_ts_ms").is_nullable());
        assert!(!by_name("symbol").is_nullable());
        // trade_id is optional metadata — nullable.
        assert!(by_name("trade_id").is_nullable());
    }

    #[test]
    fn trade_parse_batch_round_trip() {
        let schema = BinanceTradeSchema;
        let json = frame("BTCUSDT", "67000.50", "0.0125", 88123456, 1718658000123);
        let batch = schema.parse_batch(&[(json, 42, 9999)]).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 7);

        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "BTCUSDT");

        let price = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 67000.50);

        let qty = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(qty.value(0), 0.0125);

        let ts = batch.column(3).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(ts.value(0), 1718658000123);

        let tid = batch.column(4).as_any().downcast_ref::<Int64Array>().unwrap();
        assert!(tid.is_valid(0));
        assert_eq!(tid.value(0), 88123456);

        let nats = batch.column(5).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 42);

        let recv = batch.column(6).as_any().downcast_ref::<TimestampMicrosecondArray>().unwrap();
        assert_eq!(recv.value(0), 9999);
    }

    /// A Binance-exclusive fan token (PSGUSDT) must parse like any other symbol.
    #[test]
    fn trade_parse_fan_token() {
        let schema = BinanceTradeSchema;
        let json = frame("PSGUSDT", "2.345", "10.0", 777, 1718658002000);
        let batch = schema.parse_batch(&[(json, 7, 1234)]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "PSGUSDT");
        let price = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 2.345);
    }

    /// String price/qty parse to f64 (the whole point — Binance sends decimals
    /// as strings, unlike massive which sends JSON numbers).
    #[test]
    fn trade_string_numbers_parse_to_f64() {
        let schema = BinanceTradeSchema;
        let json = frame("ETHUSDT", "3000", "2", 1, 90000);
        let batch = schema.parse_batch(&[(json, 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        let price = batch.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(price.value(0), 3000.0);
        let qty = batch.column(2).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(qty.value(0), 2.0);
    }

    /// Lowercase wire symbol (defensive) is uppercased to a stable column value.
    #[test]
    fn trade_uppercases_symbol() {
        let schema = BinanceTradeSchema;
        let raw = br#"{"stream":"btcusdt@trade","data":{"e":"trade","E":90000,"s":"btcusdt","t":7,"p":"1.0","q":"2.0","T":90000,"m":false,"M":true}}"#;
        let batch = schema.parse_batch(&[(raw.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "BTCUSDT");
    }

    /// A non-trade inner event (e.g. a kline frame) is skipped, not aborted.
    #[test]
    fn trade_skip_non_trade_frame() {
        let schema = BinanceTradeSchema;
        let kline = br#"{"stream":"btcusdt@kline_1m","data":{"e":"kline","s":"BTCUSDT","k":{}}}"#;
        let batch = schema.parse_batch(&[(kline.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 7);
    }

    /// A control frame without a `data` object is skipped.
    #[test]
    fn trade_skip_control_frame() {
        let schema = BinanceTradeSchema;
        let control = br#"{"result":null,"id":1}"#;
        let batch = schema.parse_batch(&[(control.to_vec(), 1, 1000)]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    /// Malformed JSON aborts the batch (mirrors massive/kraken: a corrupt line
    /// is a hard JsonError so the caller can react, not a silent zero-row).
    #[test]
    fn trade_malformed_json_errors() {
        let schema = BinanceTradeSchema;
        let bad = br#"{not json"#;
        let result = schema.parse_batch(&[(bad.to_vec(), 1, 1000)]);
        assert!(result.is_err());
    }

    /// An unparseable price string skips the row (does not coerce to 0.0).
    #[test]
    fn trade_unparseable_price_skipped() {
        let schema = BinanceTradeSchema;
        let bad = br#"{"stream":"btcusdt@trade","data":{"e":"trade","E":1,"s":"BTCUSDT","t":1,"p":"not-a-number","q":"1.0","T":1,"m":false,"M":true}}"#;
        let good = frame("ETHUSDT", "3000.0", "1.0", 2, 2);
        let batch = schema
            .parse_batch(&[(bad.to_vec(), 1, 1000), (good, 2, 2000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 1, "bad-price row skipped, good row kept");
        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "ETHUSDT");
    }

    /// A trade missing only the optional `t` (trade id) must NOT be dropped —
    /// archived with a null trade_id (Complete Data Archive pillar).
    #[test]
    fn trade_missing_trade_id_archived_null() {
        let schema = BinanceTradeSchema;
        let no_id = br#"{"stream":"btcusdt@trade","data":{"e":"trade","E":5,"s":"BTCUSDT","p":"100.0","q":"1.0","T":5,"m":false,"M":true}}"#;
        let with_id = frame("ETHUSDT", "200.0", "2.0", 555, 6);
        let batch = schema
            .parse_batch(&[(no_id.to_vec(), 10, 1000), (with_id, 11, 2000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 2, "trade missing t must not be dropped");
        let tid = batch.column(4).as_any().downcast_ref::<Int64Array>().unwrap();
        assert!(tid.is_null(0), "trade_id null when t absent");
        assert!(tid.is_valid(1));
        assert_eq!(tid.value(1), 555);
    }

    #[test]
    fn trade_empty_batch() {
        let schema = BinanceTradeSchema;
        let batch = schema.parse_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.num_columns(), 7);
    }

    #[test]
    fn trade_parse_multiple_symbols() {
        let schema = BinanceTradeSchema;
        let m1 = frame("BTCUSDT", "67000.0", "0.5", 1, 1000);
        let m2 = frame("PSGUSDT", "2.0", "100.0", 2, 2000);
        let batch = schema
            .parse_batch(&[(m1, 10, 1000), (m2, 11, 2000)])
            .unwrap();
        assert_eq!(batch.num_rows(), 2);
        let sym = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sym.value(0), "BTCUSDT");
        assert_eq!(sym.value(1), "PSGUSDT");
        let nats = batch.column(5).as_any().downcast_ref::<UInt64Array>().unwrap();
        assert_eq!(nats.value(0), 10);
        assert_eq!(nats.value(1), 11);
    }

    // ── registry + detect wiring ──────────────────────────────────────────────

    #[test]
    fn registry_routes_binance_trade() {
        let reg = SchemaRegistry::for_feed("binance");
        let schema = reg.get("trade").expect("binance trade schema registered");
        assert_eq!(schema.schema_name(), "binance_trade");
        assert!(reg.get("ticker").is_none());
    }

    #[test]
    fn detect_binance_trade_frame() {
        let json: serde_json::Value = serde_json::from_slice(&frame(
            "BTCUSDT", "1.0", "1.0", 1, 1,
        ))
        .unwrap();
        assert_eq!(detect_message_type("binance", &json), Some("trade".into()));
    }

    #[test]
    fn detect_binance_control_frame_skipped() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"result":null,"id":1}"#).unwrap();
        assert_eq!(detect_message_type("binance", &json), None);
    }

    #[test]
    fn detect_binance_non_trade_frame() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"stream":"btcusdt@kline_1m","data":{"e":"kline","s":"BTCUSDT"}}"#)
                .unwrap();
        // Detects "kline" — not in the registry, so it is dropped downstream.
        assert_eq!(detect_message_type("binance", &json), Some("kline".into()));
        let reg = SchemaRegistry::for_feed("binance");
        assert!(reg.get("kline").is_none());
    }
}
