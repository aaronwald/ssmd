use anyhow::{bail, Result};
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

mod catalog;
mod gcs;
mod processor;

#[derive(Parser, Debug)]
#[command(
    name = "ssmd-parquet-gen",
    about = "Generate Parquet files from JSONL.gz archives in GCS"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Feed name (e.g., kalshi, kraken-futures, polymarket)
    #[arg(long)]
    feed: Option<String>,

    /// Stream name (e.g., crypto, futures, markets)
    #[arg(long)]
    stream: Option<String>,

    /// Date to process (YYYY-MM-DD)
    #[arg(long)]
    date: Option<NaiveDate>,

    /// GCS bucket name
    #[arg(long)]
    bucket: Option<String>,

    /// GCS path prefix (matches archiver storage.remote.prefix, defaults to feed name)
    #[arg(long)]
    prefix: Option<String>,

    /// Overwrite existing parquet files
    #[arg(long, default_value_t = false)]
    overwrite: bool,

    /// Dry run — list files without writing
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate root catalog.json from per-date manifests
    Catalog {
        /// GCS bucket name
        #[arg(long)]
        bucket: String,
        /// Output path in bucket for catalog.json
        #[arg(long, default_value = "catalog.json")]
        output: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    // Dispatch subcommand
    if let Some(Command::Catalog { bucket, output }) = args.command {
        info!(bucket = %bucket, output = %output, "Generating catalog");
        let gcs = gcs::GcsClient::from_env(&bucket)?;
        catalog::generate_catalog(&gcs, &output).await?;
        info!("Catalog generation complete");
        return Ok(());
    }

    // Backward-compatible flat args for CronJob invocation
    let feed = args
        .feed
        .ok_or_else(|| anyhow::anyhow!("--feed is required"))?;
    let stream = args
        .stream
        .ok_or_else(|| anyhow::anyhow!("--stream is required"))?;
    let date = args
        .date
        .ok_or_else(|| anyhow::anyhow!("--date is required"))?;
    let bucket = args
        .bucket
        .ok_or_else(|| anyhow::anyhow!("--bucket is required"))?;

    let prefix = args.prefix.as_deref().unwrap_or(&feed);

    info!(
        feed = %feed,
        stream = %stream,
        date = %date,
        bucket = %bucket,
        prefix = %prefix,
        overwrite = args.overwrite,
        dry_run = args.dry_run,
        "Starting parquet generation"
    );

    let gcs = gcs::GcsClient::from_env(&bucket)?;

    let stats = processor::process_date(
        &gcs,
        prefix,
        &feed,
        &stream,
        &date,
        args.overwrite,
        args.dry_run,
    )
    .await?;

    // Summary
    let total_records: usize = stats.iter().flat_map(|s| s.records_by_type.values()).sum();
    let total_files: usize = stats.iter().map(|s| s.parquet_files_written).sum();
    let total_bytes: usize = stats.iter().map(|s| s.bytes_written).sum();

    info!(
        total_records = total_records,
        total_parquet_files = total_files,
        total_bytes = total_bytes,
        "Processing complete"
    );

    if total_records == 0 {
        bail!("No records processed — check feed/stream/date");
    }

    Ok(())
}
