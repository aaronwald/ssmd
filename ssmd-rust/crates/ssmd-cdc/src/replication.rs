// src/replication.rs
use tokio_postgres::{Client, NoTls};
use crate::{Result, messages::{WalJsonMessage, CdcEvent, CdcOperation}};

pub struct ReplicationSlot {
    client: Client,
    slot_name: String,
    #[allow(dead_code)]
    publication_name: String,
}

impl ReplicationSlot {
    /// Connect to PostgreSQL with replication enabled
    pub async fn connect(database_url: &str, slot_name: &str, publication_name: &str) -> Result<Self> {
        // Add replication=database parameter for logical replication
        let url = if database_url.contains('?') {
            format!("{}&replication=database", database_url)
        } else {
            format!("{}?replication=database", database_url)
        };

        let (client, connection) = tokio_postgres::connect(&url, NoTls).await?;

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
            self.client
                .execute(
                    "SELECT pg_create_logical_replication_slot($1, 'wal2json')",
                    &[&self.slot_name],
                )
                .await?;
            tracing::info!(slot = %self.slot_name, "Created replication slot");
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
                "SELECT lsn::text, data FROM pg_logical_slot_get_changes($1, NULL, NULL,
                    'include-lsn', '1',
                    'include-timestamp', '1')",
                &[&self.slot_name],
            )
            .await?;

        let mut events = Vec::new();

        for row in rows {
            let lsn: String = row.get(0);
            let data: String = row.get(1);

            if let Ok(msg) = serde_json::from_str::<WalJsonMessage>(&data) {
                for change in msg.change {
                    let op = match change.kind.as_str() {
                        "insert" => CdcOperation::Insert,
                        "update" => CdcOperation::Update,
                        "delete" => CdcOperation::Delete,
                        _ => continue,
                    };

                    // Build key from primary key columns (first column assumed to be PK)
                    let key = if !change.columnnames.is_empty() && !change.columnvalues.is_empty() {
                        serde_json::json!({ &change.columnnames[0]: &change.columnvalues[0] })
                    } else if let Some(ref old) = change.oldkeys {
                        if !old.keynames.is_empty() && !old.keyvalues.is_empty() {
                            serde_json::json!({ &old.keynames[0]: &old.keyvalues[0] })
                        } else {
                            serde_json::Value::Null
                        }
                    } else {
                        serde_json::Value::Null
                    };

                    // Build data object from columns
                    let data = if change.columnnames.len() == change.columnvalues.len() {
                        let obj: serde_json::Map<String, serde_json::Value> = change.columnnames
                            .iter()
                            .zip(change.columnvalues.iter())
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        Some(serde_json::Value::Object(obj))
                    } else {
                        None
                    };

                    events.push(CdcEvent {
                        lsn: lsn.clone(),
                        table: change.table,
                        op,
                        key,
                        data,
                        timestamp: chrono::Utc::now(),
                    });
                }
            }
        }

        Ok(events)
    }
}
