// src/replication.rs
use tokio_postgres::{Client, NoTls};
use crate::{Result, messages::{CdcEvent, CdcOperation}};
use regex::Regex;

pub struct ReplicationSlot {
    client: Client,
    slot_name: String,
    #[allow(dead_code)]
    publication_name: String,
}

impl ReplicationSlot {
    /// Connect to PostgreSQL for logical replication slot polling
    pub async fn connect(database_url: &str, slot_name: &str, publication_name: &str) -> Result<Self> {
        // Note: We don't need replication=database since we poll via pg_logical_slot_get_changes
        // rather than using the streaming replication protocol
        let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "PostgreSQL connection error");
            }
        });

        Ok(Self {
            client,
            slot_name: slot_name.to_string(),
            publication_name: publication_name.to_string(),
        })
    }

    /// Ensure replication slot exists
    pub async fn ensure_slot(&self) -> Result<()> {
        let exists = self.client
            .query_opt(
                "SELECT 1 FROM pg_replication_slots WHERE slot_name = $1",
                &[&self.slot_name],
            )
            .await?
            .is_some();

        if !exists {
            // Use test_decoding which is built into PostgreSQL
            self.client
                .execute(
                    "SELECT pg_create_logical_replication_slot($1, 'test_decoding')",
                    &[&self.slot_name],
                )
                .await?;
            tracing::info!(slot = %self.slot_name, "Created replication slot with test_decoding");
        } else {
            tracing::info!(slot = %self.slot_name, "Replication slot exists");
        }

        Ok(())
    }

    /// Get current WAL LSN
    pub async fn current_lsn(&self) -> Result<String> {
        let row = self.client
            .query_one("SELECT pg_current_wal_lsn()::text", &[])
            .await?;
        Ok(row.get(0))
    }

    /// Poll for changes from the replication slot
    pub async fn poll_changes(&self) -> Result<Vec<CdcEvent>> {
        let rows = self.client
            .query(
                "SELECT lsn::text, data FROM pg_logical_slot_get_changes($1, NULL, NULL)",
                &[&self.slot_name],
            )
            .await?;

        let mut events = Vec::new();

        // Parse test_decoding output format:
        // table schema.table: INSERT: col1[type]:value1 col2[type]:value2 ...
        // table schema.table: UPDATE: old-key: col1[type]:value1 col1[type]:value1 ...
        // table schema.table: DELETE: col1[type]:value1
        let table_re = Regex::new(r"^table ([^:]+): (INSERT|UPDATE|DELETE):(.*)$").unwrap();
        let col_re = Regex::new(r"(\w+)\[([^\]]+)\]:('(?:[^'\\]|\\.)*'|[^\s]+)").unwrap();

        for row in rows {
            let lsn: String = row.get(0);
            let data: String = row.get(1);

            if let Some(caps) = table_re.captures(&data) {
                let full_table = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let table = full_table.split('.').last().unwrap_or(full_table).to_string();
                let op_str = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let cols_str = caps.get(3).map(|m| m.as_str()).unwrap_or("");

                let op = match op_str {
                    "INSERT" => CdcOperation::Insert,
                    "UPDATE" => CdcOperation::Update,
                    "DELETE" => CdcOperation::Delete,
                    _ => continue,
                };

                // Parse columns
                let mut columns: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
                let mut first_col_name = String::new();
                let mut first_col_value = serde_json::Value::Null;

                for cap in col_re.captures_iter(cols_str) {
                    let col_name = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                    let _col_type = cap.get(2).map(|m| m.as_str()).unwrap_or("");
                    let col_value_str = cap.get(3).map(|m| m.as_str()).unwrap_or("");

                    // Parse value - remove quotes if present
                    let value = if col_value_str.starts_with('\'') && col_value_str.ends_with('\'') {
                        let unquoted = &col_value_str[1..col_value_str.len()-1];
                        // Unescape single quotes
                        let unescaped = unquoted.replace("''", "'");
                        serde_json::Value::String(unescaped)
                    } else if col_value_str == "null" {
                        serde_json::Value::Null
                    } else if let Ok(n) = col_value_str.parse::<i64>() {
                        serde_json::Value::Number(n.into())
                    } else if let Ok(f) = col_value_str.parse::<f64>() {
                        serde_json::Number::from_f64(f)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::String(col_value_str.to_string()))
                    } else if col_value_str == "true" {
                        serde_json::Value::Bool(true)
                    } else if col_value_str == "false" {
                        serde_json::Value::Bool(false)
                    } else {
                        serde_json::Value::String(col_value_str.to_string())
                    };

                    if first_col_name.is_empty() {
                        first_col_name = col_name.clone();
                        first_col_value = value.clone();
                    }

                    columns.insert(col_name, value);
                }

                // Build key from first column (assumed to be PK)
                let key = if !first_col_name.is_empty() {
                    serde_json::json!({ first_col_name: first_col_value })
                } else {
                    serde_json::Value::Null
                };

                let data = if columns.is_empty() {
                    None
                } else {
                    Some(serde_json::Value::Object(columns))
                };

                events.push(CdcEvent {
                    lsn: lsn.clone(),
                    table,
                    op,
                    key,
                    data,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        Ok(events)
    }
}
