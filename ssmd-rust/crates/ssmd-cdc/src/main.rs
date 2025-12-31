use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "ssmd-cdc")]
#[command(about = "PostgreSQL CDC to NATS publisher")]
struct Args {
    /// PostgreSQL connection string
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// NATS server URL
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    /// Replication slot name
    #[arg(long, env = "REPLICATION_SLOT", default_value = "ssmd_cdc")]
    slot_name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    tracing::info!(database_url = %args.database_url, nats_url = %args.nats_url, "Starting ssmd-cdc");

    // TODO: Implement CDC loop
    Ok(())
}
