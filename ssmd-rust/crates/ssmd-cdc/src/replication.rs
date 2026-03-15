// src/replication.rs
use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;
use crate::{Result, messages::{CdcEvent, CdcOperation}};
use once_cell::sync::Lazy;
use regex::Regex;

static TABLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^table ([^:]+): (INSERT|UPDATE|DELETE):(.*)$").unwrap()
});
static COL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\w+)\[([^\]]+)\]:('(?:[^'\\]|\\.)*'|[^\s]+)").unwrap()
});

pub struct ReplicationSlot {
    pool: Pool,
    slot_name: String,
}

impl ReplicationSlot {
    /// Get a reference to the connection pool (for health checks)
    pub fn pool(&self) -> &deadpool_postgres::Pool {
        &self.pool
    }

    /// Connect to PostgreSQL for logical replication slot polling
    pub async fn connect(database_url: &str, slot_name: &str) -> Result<Self> {
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

        // CDC polls sequentially (peek then advance) — 1 connection is sufficient
        cfg.pool = Some(deadpool_postgres::PoolConfig { max_size: 1, ..Default::default() });

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

    /// Close the connection pool, ending any active slot usage.
    /// The replication slot itself persists server-side — this just drops
    /// the DB connections so the slot is no longer held active.
    pub fn close(&self) {
        self.pool.close();
        tracing::info!(slot = %self.slot_name, "Connection pool closed");
    }

    /// Get and consume up to `limit` changes from the replication slot.
    /// Changes are atomically consumed — they won't appear in subsequent calls.
    /// If the caller crashes before processing, changes are lost. Use NATS dedup
    /// and cache warmer full-refresh on restart to handle this.
    pub async fn get_changes(&self, limit: i64) -> Result<Vec<CdcEvent>> {
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;

        client.execute("BEGIN", &[]).await?;
        client.execute("SET LOCAL statement_timeout = '30s'", &[]).await?;

        let result = client
            .query(
                "SELECT lsn::text, data FROM pg_logical_slot_get_changes($1, NULL, NULL) LIMIT $2",
                &[&self.slot_name, &limit],
            )
            .await;

        client.execute("COMMIT", &[]).await?;

        let rows = result?;
        Self::parse_test_decoding_rows(rows)
    }

    /// Peek at up to `limit` changes without consuming them from the replication slot.
    /// Changes remain in the slot — use get_changes() to consume.
    pub async fn peek_changes(&self, limit: i64) -> Result<Vec<CdcEvent>> {
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;

        // Guard against unbounded blocking when WAL backlog is large.
        // Use SET LOCAL inside a transaction so the timeout is automatically
        // reset when the transaction ends — no error-path leakage.
        client.execute("BEGIN", &[]).await?;
        client.execute("SET LOCAL statement_timeout = '30s'", &[]).await?;

        let result = client
            .query(
                "SELECT lsn::text, data FROM pg_logical_slot_peek_changes($1, NULL, NULL) LIMIT $2",
                &[&self.slot_name, &limit],
            )
            .await;

        // COMMIT ends the transaction, resetting statement_timeout regardless of success/failure
        client.execute("COMMIT", &[]).await?;

        let rows = result?;
        Self::parse_test_decoding_rows(rows)
    }

    /// Parse test_decoding output rows into CdcEvents
    fn parse_test_decoding_rows(rows: Vec<tokio_postgres::Row>) -> Result<Vec<CdcEvent>> {
        let mut events = Vec::new();

        // Parse test_decoding output format:
        // table schema.table: INSERT: col1[type]:value1 col2[type]:value2 ...
        // table schema.table: UPDATE: old-key: col1[type]:value1 col1[type]:value1 ...
        // table schema.table: DELETE: col1[type]:value1

        for row in rows {
            let lsn: String = row.get(0);
            let data: String = row.get(1);

            if let Some(caps) = TABLE_RE.captures(&data) {
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

                for cap in COL_RE.captures_iter(cols_str) {
                    let col_name = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                    let _col_type = cap.get(2).map(|m| m.as_str()).unwrap_or("");
                    let col_value_str = cap.get(3).map(|m| m.as_str()).unwrap_or("");

                    let value = if col_value_str.starts_with('\'') && col_value_str.ends_with('\'') {
                        let unquoted = &col_value_str[1..col_value_str.len()-1];
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
        let sql = build_advance_sql(upto_lsn);
        let client = self.pool.get().await
            .map_err(|e| crate::Error::Replication(format!("pool error: {}", e)))?;

        client
            .execute(&sql, &[&self.slot_name])
            .await?;
        tracing::debug!(slot = %self.slot_name, lsn = %upto_lsn, "Advanced replication slot");
        Ok(())
    }
}

/// Build the SQL for advancing a replication slot.
/// Uses format! with string literal — tokio-postgres cannot bind &str to pg_lsn.
/// The LSN comes from pg_logical_slot_peek_changes output, not user input.
fn build_advance_sql(lsn: &str) -> String {
    format!(
        "SELECT pg_replication_slot_advance($1, '{}'::pg_lsn)",
        lsn.replace('\'', "")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_advance_sql_normal_lsn() {
        let sql = build_advance_sql("22/1D4AE960");
        assert_eq!(sql, "SELECT pg_replication_slot_advance($1, '22/1D4AE960'::pg_lsn)");
    }

    #[test]
    fn test_build_advance_sql_strips_quotes() {
        let sql = build_advance_sql("22/1D4A'E960");
        assert_eq!(sql, "SELECT pg_replication_slot_advance($1, '22/1D4AE960'::pg_lsn)");
    }

    #[test]
    fn test_build_advance_sql_uses_single_bind_param() {
        // The slot name is $1 (bind param). The LSN is a string literal, NOT $2.
        // This is critical — tokio-postgres cannot bind &str to pg_lsn type.
        let sql = build_advance_sql("0/14A01058");
        assert!(!sql.contains("$2"), "LSN must not be a bind parameter — tokio-postgres cannot bind &str to pg_lsn");
        assert!(sql.contains("$1"), "Slot name must be a bind parameter");
        assert!(sql.contains("::pg_lsn"), "LSN must be cast to pg_lsn");
    }
}
