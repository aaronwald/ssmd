// src/messages.rs
use serde::{Deserialize, Serialize};

/// wal2json output format
#[derive(Debug, Deserialize)]
pub struct WalJsonMessage {
    pub xid: Option<u64>,
    #[serde(default)]
    pub change: Vec<WalJsonChange>,
}

#[derive(Debug, Deserialize)]
pub struct WalJsonChange {
    pub kind: String,        // "insert", "update", "delete"
    pub schema: String,      // "public"
    pub table: String,       // "markets"
    #[serde(default)]
    pub columnnames: Vec<String>,
    #[serde(default)]
    pub columnvalues: Vec<serde_json::Value>,
    #[serde(default)]
    pub oldkeys: Option<OldKeys>,
}

#[derive(Debug, Deserialize)]
pub struct OldKeys {
    #[serde(default)]
    pub keynames: Vec<String>,
    #[serde(default)]
    pub keyvalues: Vec<serde_json::Value>,
}

/// Published CDC event (to NATS)
#[derive(Debug, Serialize, Deserialize)]
pub struct CdcEvent {
    pub lsn: String,
    pub table: String,
    pub op: CdcOperation,
    pub key: serde_json::Value,
    pub data: Option<serde_json::Value>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl CdcEvent {
    /// Unique dedup ID for NATS JetStream. LSN alone is NOT unique —
    /// a single transaction can produce events for multiple tables at the same LSN.
    pub fn dedup_id(&self) -> String {
        format!("{}:{}:{}", self.lsn, self.table, self.op.as_str())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CdcOperation {
    Insert,
    Update,
    Delete,
}

impl CdcOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            CdcOperation::Insert => "insert",
            CdcOperation::Update => "update",
            CdcOperation::Delete => "delete",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wal2json_insert() {
        let json = r#"{
            "xid": 12345,
            "change": [{
                "kind": "insert",
                "schema": "public",
                "table": "markets",
                "columnnames": ["ticker", "status"],
                "columnvalues": ["INXD-25-B4000", "active"]
            }]
        }"#;

        let msg: WalJsonMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.change.len(), 1);
        assert_eq!(msg.change[0].kind, "insert");
        assert_eq!(msg.change[0].table, "markets");
    }

    #[test]
    fn test_dedup_id_unique_per_table_and_op() {
        let event1 = CdcEvent {
            lsn: "22/1D4AE960".into(),
            table: "events".into(),
            op: CdcOperation::Update,
            key: serde_json::json!({"event_ticker": "KXBTCD-26MAR1413"}),
            data: None,
            timestamp: chrono::Utc::now(),
        };
        let event2 = CdcEvent {
            lsn: "22/1D4AE960".into(),
            table: "markets".into(),
            op: CdcOperation::Update,
            key: serde_json::json!({"ticker": "KXBTCD-26MAR1413-T70000"}),
            data: None,
            timestamp: chrono::Utc::now(),
        };
        assert_ne!(event1.dedup_id(), event2.dedup_id());
        assert_eq!(event1.dedup_id(), event1.dedup_id());
    }

    #[test]
    fn test_cdc_operation_serialization() {
        let event = CdcEvent {
            lsn: "0/16B3748".into(),
            table: "markets".into(),
            op: CdcOperation::Update,
            key: serde_json::json!({"ticker": "INXD-25-B4000"}),
            data: Some(serde_json::json!({"status": "active"})),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"op\":\"update\""));
    }
}
