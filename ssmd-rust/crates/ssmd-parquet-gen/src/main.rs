use anyhow::Result;
use chrono::NaiveDate;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod gcs;
mod processor;

#[derive(Parser, Debug)]
#[command(
    name = "ssmd-parquet-gen",
    about = "Generate Parquet files from JSONL.gz archives in GCS"
)]
struct Args {
    /// Feed name (e.g., kalshi, kraken-futures, polymarket)
    #[arg(long)]
    feed: String,

    /// Stream name (e.g., crypto, futures, markets)
    #[arg(long)]
    stream: String,

    /// Date to process (YYYY-MM-DD)
    #[arg(long)]
    date: NaiveDate,

    /// GCS bucket name
    #[arg(long)]
    bucket: String,

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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let prefix = args.prefix.as_deref().unwrap_or(&args.feed);

    info!(
        feed = %args.feed,
        stream = %args.stream,
        date = %args.date,
        bucket = %args.bucket,
        prefix = %prefix,
        overwrite = args.overwrite,
        dry_run = args.dry_run,
        "Starting parquet generation"
    );

    let gcs = gcs::GcsClient::from_env(&args.bucket)?;

    let stats = processor::process_date(
        &gcs,
        prefix,
        &args.feed,
        &args.stream,
        &args.date,
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

    Ok(())
}
