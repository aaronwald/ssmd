use std::collections::BTreeMap;

use anyhow::Result;
use bytes::Bytes;
use chrono::Utc;
use serde::Serialize;
use tracing::{info, warn};

use ssmd_schemas::SchemaRegistry;

use crate::gcs::GcsClient;
use crate::processor::{
    format_arrow_type, ManifestSchemaInfo, ParquetManifest, SchemaColumnDef,
};

/// Root catalog aggregating all feeds
#[derive(Debug, Serialize)]
pub struct Catalog {
    pub generated_at: String,
    pub version: String,
    pub feeds: Vec<FeedSummary>,
}

/// Summary for a single feed across all dates
#[derive(Debug, Serialize)]
pub struct FeedSummary {
    pub feed: String,
    pub stream: String,
    pub prefix: String,
    pub message_types: Vec<String>,
    pub date_min: String,
    pub date_max: String,
    pub total_files: usize,
    pub total_bytes: usize,
    pub total_rows: usize,
    pub dates: Vec<String>,
    pub schemas: BTreeMap<String, ManifestSchemaInfo>,
}

/// Feed definition matching CronJob configuration
struct FeedDef {
    feed: &'static str,
    stream: &'static str,
    prefix: &'static str,
}

const FEEDS: &[FeedDef] = &[
    FeedDef {
        feed: "kalshi",
        stream: "crypto",
        prefix: "kalshi",
    },
    FeedDef {
        feed: "kraken-futures",
        stream: "futures",
        prefix: "kraken-futures",
    },
    FeedDef {
        feed: "polymarket",
        stream: "markets",
        prefix: "polymarket",
    },
];

/// Generate catalog.json from per-date manifests and write to GCS bucket root
pub async fn generate_catalog(gcs: &GcsClient, output: &str) -> Result<()> {
    let mut feeds = Vec::new();

    for def in FEEDS {
        match build_feed_summary(gcs, def).await {
            Ok(Some(summary)) => {
                info!(
                    feed = %summary.feed,
                    dates = summary.dates.len(),
                    total_rows = summary.total_rows,
                    "Built feed summary"
                );
                feeds.push(summary);
            }
            Ok(None) => {
                info!(feed = %def.feed, "No manifests found, skipping");
            }
            Err(e) => {
                warn!(feed = %def.feed, error = %e, "Failed to build feed summary, skipping");
            }
        }
    }

    let catalog = Catalog {
        generated_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        version: "1.0.0".to_string(),
        feeds,
    };

    let json_bytes = serde_json::to_vec_pretty(&catalog)?;
    info!(path = %output, bytes = json_bytes.len(), "Writing catalog");
    gcs.put(output, Bytes::from(json_bytes)).await?;

    Ok(())
}

async fn build_feed_summary(gcs: &GcsClient, def: &FeedDef) -> Result<Option<FeedSummary>> {
    // List all parquet-manifest.json files under {prefix}/{prefix}/{stream}/
    let search_prefix = format!("{}/{}/{}", def.prefix, def.prefix, def.stream);
    let manifest_paths = gcs
        .list_files_with_suffix(&search_prefix, "parquet-manifest.json")
        .await?;

    if manifest_paths.is_empty() {
        return Ok(None);
    }

    info!(
        feed = %def.feed,
        count = manifest_paths.len(),
        "Found manifest files"
    );

    let registry = SchemaRegistry::for_feed(def.feed);
    let mut dates = Vec::new();
    let mut total_files: usize = 0;
    let mut total_bytes: usize = 0;
    let mut total_rows: usize = 0;
    let mut schemas: BTreeMap<String, ManifestSchemaInfo> = BTreeMap::new();
    let mut message_types_set = BTreeMap::<String, ()>::new();

    for manifest_path in &manifest_paths {
        let data = match gcs.get(manifest_path).await {
            Ok(d) => d,
            Err(e) => {
                warn!(path = %manifest_path, error = %e, "Failed to read manifest, skipping");
                continue;
            }
        };

        let manifest: ParquetManifest = match serde_json::from_slice(&data) {
            Ok(m) => m,
            Err(e) => {
                warn!(path = %manifest_path, error = %e, "Failed to parse manifest, skipping");
                continue;
            }
        };

        dates.push(manifest.date.clone());

        // Aggregate totals from manifest stats
        for (msg_type, count) in &manifest.totals.records_written {
            message_types_set.insert(msg_type.clone(), ());
            total_rows += count;
        }

        // Use v2.0.0 files field if present, otherwise estimate from stats
        if !manifest.files.is_empty() {
            total_files += manifest.files.len();
            total_bytes += manifest.files.iter().map(|f| f.bytes).sum::<usize>();
        } else {
            // v1.0.0 — count parquet files from records_written keys × hours
            total_files += manifest.totals.records_written.len() * manifest.hours.len().max(1);
        }

        // Merge schemas — prefer v2.0.0 manifest schemas, hydrate from registry for v1.0.0
        if !manifest.schemas.is_empty() {
            for (msg_type, schema_info) in manifest.schemas {
                schemas.entry(msg_type).or_insert(schema_info);
            }
        } else {
            // Hydrate from SchemaRegistry for v1.0.0 manifests
            for msg_type in manifest.totals.records_written.keys() {
                if schemas.contains_key(msg_type) {
                    continue;
                }
                if let Some(schema) = registry.get(msg_type) {
                    let columns = schema
                        .schema()
                        .fields()
                        .iter()
                        .map(|f| SchemaColumnDef {
                            name: f.name().clone(),
                            arrow_type: format_arrow_type(f.data_type()),
                            nullable: f.is_nullable(),
                        })
                        .collect();
                    schemas.insert(
                        msg_type.clone(),
                        ManifestSchemaInfo {
                            schema_name: schema.schema_name().to_string(),
                            schema_version: schema.schema_version().to_string(),
                            columns,
                        },
                    );
                }
            }
        }
    }

    dates.sort();

    let date_min = dates.first().cloned().unwrap_or_default();
    let date_max = dates.last().cloned().unwrap_or_default();
    let message_types: Vec<String> = message_types_set.into_keys().collect();

    Ok(Some(FeedSummary {
        feed: def.feed.to_string(),
        stream: def.stream.to_string(),
        prefix: def.prefix.to_string(),
        message_types,
        date_min,
        date_max,
        total_files,
        total_bytes,
        total_rows,
        dates,
        schemas,
    }))
}
