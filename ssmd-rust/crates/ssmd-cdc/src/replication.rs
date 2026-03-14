// src/replication.rs
use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;
use crate::{Result, messages::{CdcEvent, CdcOperation}};
use regex::Regex;

pub struct ReplicationSlot {
    pool: Pool,
    slot_name: String,
    #[allow(dead_code)]
    publication_name: String,
}

impl ReplicationSlot {
    /// Connect to PostgreSQL for logical replication slot polling
    pub async fn connect(database_url: &str, slot_name: &str, publication_name: &str) -> Result<Self> {
        let pg_config: tokio_postgres::Config = database_url
            .parse()
            .map_err(|e: tokio_postgres::Error| crate::Error::Config(format!("invalid database URL: {}", e)))?;

        let mut cfg = Config::new();
        if let Some(host) = pg_config.get_hosts().first() {
            match host {
                tokio_postgres::config::Host::Tcp(h) => cfg.host = Some(h.clone()),
                #[cfg(unix)]
                tokio_postgres::config::Host::Unix(p) => {
                    cfg.host = Some(p.to_string_lossy().to_string())
                }
            }
        }
        if let Some(port) = pg_config.get_ports().first() {
            cfg.port = Some(*port);
        }
        if let Some(user) = pg_config.get_user() {
            cfg.user = Some(user.to_string());
        }
        if let Some(password) = pg_config.get_password() {
            cfg.password = Some(String::from_utf8_lossy(password).to_string());
        }
        if let Some(dbname) = pg_config.get_dbname() {
            cfg.dbname = Some(dbname.to_string());
        }

        // CDC polls sequentially — 2 connections is sufficient (peek + advance)
        cfg.pool = Some(deadpool_postgres::PoolConfig { max_size: 2, ..Default::default() });

        let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| crate::Error::Config(format!("failed to create pool: {}", e)))?;

        // Verify connectivity
        let client = pool.get().await
            .map_err(|e| crate::Error::Config(format!("failed to connect: {}", e)))?;
        drop(client);

        tracing::info!("PostgreSQL connection pool created");

        Ok(Self {
            pool,
            slot_name: slot_name.to_string(),
            publication_name: publication_name.to_string(),
        })
    }

    /// Ensure replication slot exists
    pub async fn ensure_slot(&self) -> Result<()> {
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;

        let exists = client
            .query_opt(
                "SELECT 1 FROM pg_replication_slots WHERE slot_name = $1",
                &[&self.slot_name],
            )
            .await?
            .is_some();

        if !exists {
            // Use test_decoding which is built into PostgreSQL
            client
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
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;
        let row = client
            .query_one("SELECT pg_current_wal_lsn()::text", &[])
            .await?;
        Ok(row.get(0))
    }

    /// Peek at up to `limit` changes without consuming them from the replication slot.
    /// Use `advance_slot()` after successful processing to consume.
    pub async fn peek_changes(&self, limit: i64) -> Result<Vec<CdcEvent>> {
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;

        let rows = client
            .query(
                "SELECT lsn::text, data FROM pg_logical_slot_peek_changes($1, NULL, NULL) LIMIT $2",
                &[&self.slot_name, &limit],
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
                let table = full_table.split('.').next_back().unwrap_or(full_table).to_string();
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

    /// Advance the replication slot past the given LSN, consuming all changes up to it.
    /// Call this only after all events have been successfully published.
    pub async fn advance_slot(&self, upto_lsn: &str) -> Result<()> {
        // Use format! for the LSN value — tokio-postgres cannot bind &str to pg_lsn.
        // The LSN comes from pg_logical_slot_peek_changes output, not user input.
        let sql = format!(
            "SELECT pg_replication_slot_advance($1, '{}'::pg_lsn)",
            upto_lsn.replace('\'', "")
        );
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;

        client
            .execute(&sql, &[&self.slot_name])
            .await?;
        tracing::debug!(slot = %self.slot_name, lsn = %upto_lsn, "Advanced replication slot");
        Ok(())
    }
}
