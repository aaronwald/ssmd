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
    kalshi::{KalshiConfig, KalshiConnector, KalshiCredentials},
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

    // Get output path from environment storage config
    let output_path = env_config
        .storage
        .path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));

    // Setup shutdown signal
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Handle Ctrl+C
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Received shutdown signal");
        shutdown_tx_clone.send(true).ok();
    });

    // Parse health server address
    let health_addr: SocketAddr = args.health_addr.parse()?;

    // Create and run connector based on feed name
    match feed.name.as_str() {
        "kalshi" => {
            run_kalshi_connector(&feed, &output_path, health_addr, shutdown_rx).await
        }
        _ => {
            run_generic_connector(&feed, &env_config, &output_path, health_addr, shutdown_rx).await
        }
    }
}

/// Run Kalshi-specific connector with RSA authentication
async fn run_kalshi_connector(
    feed: &Feed,
    output_path: &PathBuf,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load Kalshi config from environment
    let config = KalshiConfig::from_env().map_err(|e| {
        error!(error = %e, "Failed to load Kalshi config");
        e
    })?;

    let credentials = KalshiCredentials::new(config.api_key, &config.private_key_pem).map_err(|e| {
        error!(error = %e, "Failed to create Kalshi credentials");
        e
    })?;

    info!(use_demo = config.use_demo, "Creating Kalshi connector");

    let connector = KalshiConnector::new(credentials, config.use_demo);
    let writer = FileWriter::new(output_path, &feed.name);
    let mut runner = Runner::new(&feed.name, connector, writer);
    let connected_handle = runner.connected_handle();

    // Start health server
    let server_state = ServerState::new(&feed.name, Arc::clone(&connected_handle));
    tokio::spawn(async move {
        if let Err(e) = ssmd_connector_lib::run_server(health_addr, server_state).await {
            error!(error = %e, "Health server error");
        }
    });
    info!(addr = %health_addr, "Health server started");

    // Run the connector
    match runner.run(shutdown_rx).await {
        Ok(()) => {
            info!("Connector stopped gracefully");
            Ok(())
        }
        Err(e) => {
            error!(error = %e, "Connector error");
            std::process::exit(1);
        }
    }
}

/// Run generic WebSocket connector
async fn run_generic_connector(
    feed: &Feed,
    env_config: &Environment,
    output_path: &PathBuf,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
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

    let connector = WebSocketConnector::new(&url, creds);
    let writer = FileWriter::new(output_path, &feed.name);
    let mut runner = Runner::new(&feed.name, connector, writer);
    let connected_handle = runner.connected_handle();

    // Start health server
    let server_state = ServerState::new(&feed.name, Arc::clone(&connected_handle));
    tokio::spawn(async move {
        if let Err(e) = ssmd_connector_lib::run_server(health_addr, server_state).await {
            error!(error = %e, "Health server error");
        }
    });
    info!(addr = %health_addr, "Health server started");

    // Run the connector
    match runner.run(shutdown_rx).await {
        Ok(()) => {
            info!("Connector stopped gracefully");
            Ok(())
        }
        Err(e) => {
            error!(error = %e, "Connector error");
            std::process::exit(1);
        }
    }
}
