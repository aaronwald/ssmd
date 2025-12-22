//! ssmd-connector: Market data collection binary
//!
//! Connects to market data sources and writes to local storage.

use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ssmd_connector_lib::{
    EnvResolver, FileWriter, KeyResolver, Runner, ServerState, WebSocketConnector,
};
use ssmd_metadata::{Environment, Feed, FeedType, KeyType};

#[derive(Parser, Debug)]
#[command(name = "ssmd-connector")]
#[command(about = "Market data connector for SSMD")]
struct Args {
    /// Path to feed configuration file
    #[arg(short, long)]
    feed: PathBuf,

    /// Path to environment configuration file
    #[arg(short, long)]
    env: PathBuf,

    /// Health server bind address
    #[arg(long, default_value = "0.0.0.0:8080")]
    health_addr: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load feed configuration
    let feed = Feed::load(&args.feed)?;
    info!(feed = %feed.name, "Loaded feed configuration");

    // Load environment configuration
    let env_config = Environment::load(&args.env)?;
    info!(env = %env_config.name, "Loaded environment configuration");

    // Get latest version
    let version = feed.get_latest_version().ok_or("No feed versions defined")?;

    // Determine connection URL based on feed type
    let url = match feed.feed_type {
        FeedType::Websocket => version.endpoint.clone(),
        FeedType::Rest => {
            error!("REST feeds not yet supported");
            return Err("REST feeds not yet supported".into());
        }
        FeedType::Multicast => {
            error!("Multicast feeds not yet supported");
            return Err("Multicast feeds not yet supported".into());
        }
    };

    // Resolve credentials from environment config
    let creds: Option<HashMap<String, String>> = if let Some(ref keys) = env_config.keys {
        // Find the API key spec for this feed
        let api_key_spec = keys.values().find(|k| k.key_type == KeyType::ApiKey);

        if let Some(key_spec) = api_key_spec {
            if let Some(ref source) = key_spec.source {
                let resolver = EnvResolver::new();
                match resolver.resolve(source) {
                    Ok(resolved_keys) => Some(resolved_keys),
                    Err(e) => {
                        error!(error = %e, "Failed to resolve credentials");
                        return Err(e.into());
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Get output path from environment storage config
    let output_path = env_config
        .storage
        .path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));

    // Create connector and writer
    let connector = WebSocketConnector::new(&url, creds);
    let writer = FileWriter::new(&output_path, &feed.name);

    // Create runner
    let mut runner = Runner::new(&feed.name, connector, writer);
    let connected_handle = runner.connected_handle();

    // Setup shutdown signal
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Handle Ctrl+C
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Received shutdown signal");
        shutdown_tx_clone.send(true).ok();
    });

    // Start health server
    let health_addr: SocketAddr = args.health_addr.parse()?;
    let server_state = ServerState::new(&feed.name, Arc::clone(&connected_handle));
    tokio::spawn(async move {
        if let Err(e) = ssmd_connector_lib::run_server(health_addr, server_state).await {
            error!(error = %e, "Health server error");
        }
    });
    info!(addr = %health_addr, "Health server started");

    // Run the connector (blocks until shutdown or disconnect)
    match runner.run(shutdown_rx).await {
        Ok(()) => {
            info!("Connector stopped gracefully");
        }
        Err(e) => {
            error!(error = %e, "Connector error");
            // Exit with error code for K8s to restart
            std::process::exit(1);
        }
    }

    Ok(())
}
