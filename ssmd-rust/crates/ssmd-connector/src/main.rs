//! ssmd-connector: Market data collection binary
//!
//! Connects to market data sources and publishes to NATS.
//! Raw JSON passthrough - no transformation.

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
    EnvResolver, KeyResolver, NatsWriter, Runner, ServerState, WebSocketConnector,
};
use ssmd_metadata::{Environment, Feed, FeedType, KeyType, TransportType};
use ssmd_middleware::MiddlewareFactory;

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
            run_kalshi_connector(&feed, &env_config, health_addr, shutdown_rx).await
        }
        _ => {
            run_generic_connector(&feed, &env_config, health_addr, shutdown_rx).await
        }
    }
}

/// Run Kalshi-specific connector with RSA authentication
async fn run_kalshi_connector(
    feed: &Feed,
    env_config: &Environment,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve credentials from env config keys (preferred) or fall back to hardcoded env vars
    let (api_key, private_key_pem) = if let Some(ref keys) = env_config.keys {
        let api_key_spec = keys.values().find(|k| k.key_type == KeyType::ApiKey);

        if let Some(key_spec) = api_key_spec {
            if let Some(ref source) = key_spec.source {
                let resolver = EnvResolver::new();
                let resolved = resolver.resolve(source).map_err(|e| {
                    error!(error = %e, source = %source, "Failed to resolve credentials from env config");
                    e
                })?;

                // Extract KALSHI_API_KEY and KALSHI_PRIVATE_KEY from resolved HashMap
                let api_key = resolved.get("KALSHI_API_KEY")
                    .ok_or("KALSHI_API_KEY not found in resolved keys")?
                    .clone();
                let private_key = resolved.get("KALSHI_PRIVATE_KEY")
                    .ok_or("KALSHI_PRIVATE_KEY not found in resolved keys")?
                    .clone();

                info!("Loaded credentials from env config keys");
                (api_key, private_key)
            } else {
                // No source specified, fall back to hardcoded env vars
                info!("No key source in env config, falling back to environment variables");
                let config = KalshiConfig::from_env()?;
                (config.api_key, config.private_key_pem)
            }
        } else {
            // No api_key type found, fall back
            info!("No api_key type in env config, falling back to environment variables");
            let config = KalshiConfig::from_env()?;
            (config.api_key, config.private_key_pem)
        }
    } else {
        // No keys section, fall back to hardcoded env vars (backwards compatibility)
        info!("No keys in env config, falling back to environment variables");
        let config = KalshiConfig::from_env()?;
        (config.api_key, config.private_key_pem)
    };

    let credentials = KalshiCredentials::new(api_key, &private_key_pem).map_err(|e| {
        error!(error = %e, "Failed to create Kalshi credentials");
        e
    })?;

    // Check for demo mode from environment
    let use_demo = std::env::var("KALSHI_USE_DEMO")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false);

    info!(use_demo = use_demo, "Creating Kalshi connector");

    let connector = KalshiConnector::new(credentials, use_demo);

    // NATS transport required
    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using NATS writer (raw JSON)");
            let transport = MiddlewareFactory::create_transport(env_config).await?;
            let writer = NatsWriter::new(transport, &env_config.name, &feed.name);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        TransportType::Memory => {
            error!("Memory transport not supported - use NATS transport");
            error!("Set transport.transport_type: nats in environment config");
            Err("Memory transport not supported - connector requires NATS".into())
        }
        TransportType::Mqtt => {
            error!("MQTT transport not yet supported");
            Err("MQTT transport not yet supported".into())
        }
    }
}

/// Run connector with a specific writer implementation
async fn run_with_writer<C, W>(
    feed: &Feed,
    connector: C,
    writer: W,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>>
where
    C: ssmd_connector_lib::traits::Connector,
    W: ssmd_connector_lib::traits::Writer,
{
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

    // NATS transport required
    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using NATS writer (raw JSON)");
            let transport = MiddlewareFactory::create_transport(env_config).await?;
            let connector = WebSocketConnector::new(&url, creds);
            let writer = NatsWriter::new(transport, &env_config.name, &feed.name);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        TransportType::Memory => {
            error!("Memory transport not supported - use NATS transport");
            Err("Memory transport not supported - connector requires NATS".into())
        }
        TransportType::Mqtt => {
            error!("MQTT transport not yet supported");
            Err("MQTT transport not yet supported".into())
        }
    }
}
