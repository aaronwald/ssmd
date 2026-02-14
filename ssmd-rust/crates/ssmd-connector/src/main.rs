//! ssmd-connector: Market data collection binary
//!
//! Connects to market data sources and publishes to NATS.
//! Raw JSON passthrough - no transformation.

use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info, warn};
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
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
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
        "kraken" => {
            run_kraken_connector(&feed, &env_config, health_addr, shutdown_rx).await
        }
        "kraken-futures" => {
            run_kraken_futures_connector(&feed, &env_config, health_addr, shutdown_rx).await
        }
        "polymarket" => {
            run_polymarket_connector(&feed, &env_config, health_addr, shutdown_rx).await
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

    // Extract WebSocket URL from feed config (overrides hardcoded constants)
    let ws_url = feed.get_latest_version().map(|v| v.endpoint.clone());

    // Check if lifecycle mode is enabled (dedicated lifecycle collector)
    let lifecycle_enabled = env_config.lifecycle.as_ref().is_some_and(|c| c.enabled);

    // Build series filter if configured (simple HashSet of series tickers for O(1) lookup)
    let series_filter: Option<HashSet<String>> = if lifecycle_enabled {
        if let Some(ref lifecycle_config) = env_config.lifecycle {
            if !lifecycle_config.series.is_empty() {
                let filter: HashSet<String> = lifecycle_config.series.iter().cloned().collect();
                info!(
                    series = ?lifecycle_config.series,
                    "Series filter enabled for lifecycle events"
                );
                Some(filter)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Create connector with optional secmaster filtering, CDC, or lifecycle mode
    let connector = if lifecycle_enabled {
        // Lifecycle-only mode: subscribe only to market_lifecycle_v2 channel
        let lifecycle_config = env_config.lifecycle.clone().unwrap_or_default();
        info!(
            use_demo = use_demo,
            has_series_filter = series_filter.is_some(),
            series_count = series_filter.as_ref().map(|f| f.len()).unwrap_or(0),
            "Creating Kalshi connector (lifecycle mode)"
        );
        KalshiConnector::with_lifecycle(credentials, use_demo, lifecycle_config, ws_url.clone())
    } else if let Some(ref secmaster) = env_config.secmaster {
        if !secmaster.categories.is_empty() {
            // Inject API key from environment variable if not set in config
            let mut secmaster_config = secmaster.clone();
            if secmaster_config.api_key.is_none() {
                if let Ok(api_key) = std::env::var("SSMD_DATA_API_KEY") {
                    secmaster_config.api_key = Some(api_key);
                }
            }

            // Check if CDC is enabled
            let cdc_enabled = env_config.cdc.as_ref().is_some_and(|c| c.enabled);
            let nats_url = env_config.transport.url.clone();

            info!(
                categories = ?secmaster_config.categories,
                use_demo = use_demo,
                cdc_enabled = cdc_enabled,
                "Creating Kalshi connector with category filtering"
            );

            if cdc_enabled {
                if let (Some(cdc_config), Some(nats_url)) = (env_config.cdc.clone(), nats_url) {
                    KalshiConnector::with_cdc(
                        credentials,
                        use_demo,
                        secmaster_config,
                        env_config.subscription.clone(),
                        cdc_config,
                        nats_url,
                        ws_url.clone(),
                    )
                } else {
                    warn!("CDC enabled but NATS URL not configured, falling back to static subscriptions");
                    KalshiConnector::with_secmaster(
                        credentials,
                        use_demo,
                        secmaster_config,
                        env_config.subscription.clone(),
                        ws_url.clone(),
                    )
                }
            } else {
                KalshiConnector::with_secmaster(
                    credentials,
                    use_demo,
                    secmaster_config,
                    env_config.subscription.clone(),
                    ws_url.clone(),
                )
            }
        } else {
            info!(use_demo = use_demo, "Creating Kalshi connector (global mode)");
            KalshiConnector::new(credentials, use_demo, ws_url.clone())
        }
    } else {
        info!(use_demo = use_demo, "Creating Kalshi connector (global mode)");
        KalshiConnector::new(credentials, use_demo, ws_url)
    };

    // NATS transport required
    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using NATS writer (raw JSON)");
            let transport = MiddlewareFactory::create_nats_transport_validated(env_config).await?;
            let writer = create_nats_writer(transport, env_config, feed, series_filter);
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

/// Run Kraken connector for spot market data
async fn run_kraken_connector(
    feed: &Feed,
    env_config: &Environment,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse symbols from environment or use defaults
    let symbols = std::env::var("KRAKEN_SYMBOLS")
        .map(|s| {
            s.split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_else(|_| vec!["BTC/USD".to_string(), "ETH/USD".to_string()]);

    // Extract WebSocket URL from feed config (overrides hardcoded constants)
    let ws_url = feed.get_latest_version().map(|v| v.endpoint.clone());

    info!(symbols = ?symbols, "Creating Kraken connector");

    let connector = ssmd_connector_lib::kraken::KrakenConnector::new(symbols, ws_url);

    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using Kraken NATS writer");
            let transport = MiddlewareFactory::create_nats_transport_validated(env_config).await?;
            let writer = create_kraken_nats_writer(transport, env_config, feed);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        _ => {
            error!("Only NATS transport is supported for Kraken connector");
            Err("Only NATS transport is supported".into())
        }
    }
}

/// Run Kraken Futures connector for perpetual contract data
async fn run_kraken_futures_connector(
    feed: &Feed,
    env_config: &Environment,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse product IDs from environment or use defaults
    let product_ids = std::env::var("KRAKEN_FUTURES_SYMBOLS")
        .map(|s| {
            s.split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_else(|_| vec![
            "PF_XBTUSD".to_string(),
            "PF_ETHUSD".to_string(),
        ]);

    // Extract WebSocket URL from feed config (overrides hardcoded constants)
    let ws_url = feed.get_latest_version().map(|v| v.endpoint.clone());

    info!(product_ids = ?product_ids, "Creating Kraken Futures connector");

    let connector = ssmd_connector_lib::kraken_futures::KrakenFuturesConnector::new(product_ids, ws_url);

    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using Kraken Futures NATS writer");
            let transport = MiddlewareFactory::create_nats_transport_validated(env_config).await?;
            let writer = create_kraken_futures_nats_writer(transport, env_config, feed);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        _ => {
            error!("Only NATS transport is supported for Kraken Futures connector");
            Err("Only NATS transport is supported".into())
        }
    }
}

/// Create KrakenFuturesNatsWriter with optional custom subject prefix
fn create_kraken_futures_nats_writer(
    transport: Arc<dyn ssmd_middleware::Transport>,
    env_config: &Environment,
    feed: &Feed,
) -> ssmd_connector_lib::kraken_futures::KrakenFuturesNatsWriter {
    if let (Some(ref prefix), Some(ref stream)) = (
        &env_config.transport.subject_prefix,
        &env_config.transport.stream,
    ) {
        info!(
            subject_prefix = %prefix,
            stream = %stream,
            "Using custom subject prefix"
        );
        ssmd_connector_lib::kraken_futures::KrakenFuturesNatsWriter::with_prefix(
            transport, prefix.clone(), stream.clone(),
        )
    } else {
        info!(
            subject_prefix = format!("{}.{}", env_config.name, feed.name),
            "Using default subject prefix"
        );
        ssmd_connector_lib::kraken_futures::KrakenFuturesNatsWriter::new(
            transport, &env_config.name, &feed.name,
        )
    }
}

/// Run Polymarket connector for prediction market data
async fn run_polymarket_connector(
    feed: &Feed,
    env_config: &Environment,
    health_addr: SocketAddr,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check for static token IDs from environment (for testing/manual override)
    let static_tokens: Option<Vec<String>> = std::env::var("POLYMARKET_TOKEN_IDS")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        });

    // Extract WebSocket URL from feed config (overrides hardcoded constants)
    let ws_url = feed.get_latest_version().map(|v| v.endpoint.clone());

    let connector = if let Some(tokens) = static_tokens {
        // Priority 1: Static token IDs from environment variable
        info!(tokens = tokens.len(), "Creating Polymarket connector with static token IDs");
        ssmd_connector_lib::polymarket::PolymarketConnector::new(tokens, ws_url)
    } else if let Some(ref secmaster) = env_config.secmaster {
        if !secmaster.categories.is_empty() {
            // Priority 2: Secmaster-driven filtering by category
            let mut secmaster_config = secmaster.clone();
            if secmaster_config.api_key.is_none() {
                if let Ok(api_key) = std::env::var("SSMD_DATA_API_KEY") {
                    secmaster_config.api_key = Some(api_key);
                }
            }

            info!(
                categories = ?secmaster_config.categories,
                "Creating Polymarket connector with secmaster category filtering"
            );

            ssmd_connector_lib::polymarket::PolymarketConnector::with_secmaster(secmaster_config, ws_url)
        } else {
            // Secmaster configured but no categories — fall through to Gamma discovery
            let min_volume = std::env::var("POLYMARKET_MIN_VOLUME")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(10000.0);

            let min_liquidity = std::env::var("POLYMARKET_MIN_LIQUIDITY")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(5000.0);

            info!(
                min_volume = min_volume,
                min_liquidity = min_liquidity,
                "Creating Polymarket connector with market discovery"
            );

            let discovery = ssmd_connector_lib::polymarket::MarketDiscovery::new()
                .with_min_volume(min_volume)
                .with_min_liquidity(min_liquidity);

            ssmd_connector_lib::polymarket::PolymarketConnector::with_discovery(discovery, ws_url)
        }
    } else {
        // Priority 3: Fallback to Gamma REST API discovery
        let min_volume = std::env::var("POLYMARKET_MIN_VOLUME")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10000.0);

        let min_liquidity = std::env::var("POLYMARKET_MIN_LIQUIDITY")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5000.0);

        info!(
            min_volume = min_volume,
            min_liquidity = min_liquidity,
            "Creating Polymarket connector with market discovery"
        );

        let discovery = ssmd_connector_lib::polymarket::MarketDiscovery::new()
            .with_min_volume(min_volume)
            .with_min_liquidity(min_liquidity);

        ssmd_connector_lib::polymarket::PolymarketConnector::with_discovery(discovery, ws_url)
    };

    match env_config.transport.transport_type {
        TransportType::Nats => {
            info!(transport = "nats", "Using Polymarket NATS writer");
            let transport = MiddlewareFactory::create_nats_transport_validated(env_config).await?;
            let writer = create_polymarket_nats_writer(transport, env_config, feed);
            run_with_writer(feed, connector, writer, health_addr, shutdown_rx).await
        }
        _ => {
            error!("Only NATS transport is supported for Polymarket connector");
            Err("Only NATS transport is supported".into())
        }
    }
}

/// Create PolymarketNatsWriter with optional custom subject prefix
fn create_polymarket_nats_writer(
    transport: Arc<dyn ssmd_middleware::Transport>,
    env_config: &Environment,
    feed: &Feed,
) -> ssmd_connector_lib::polymarket::PolymarketNatsWriter {
    if let (Some(ref prefix), Some(ref stream)) = (
        &env_config.transport.subject_prefix,
        &env_config.transport.stream,
    ) {
        info!(
            subject_prefix = %prefix,
            stream = %stream,
            "Using custom subject prefix"
        );
        ssmd_connector_lib::polymarket::PolymarketNatsWriter::with_prefix(
            transport,
            prefix.clone(),
            stream.clone(),
        )
    } else {
        info!(
            subject_prefix = format!("{}.{}", env_config.name, feed.name),
            "Using default subject prefix"
        );
        ssmd_connector_lib::polymarket::PolymarketNatsWriter::new(
            transport,
            &env_config.name,
            &feed.name,
        )
    }
}

/// Create KrakenNatsWriter with optional custom subject prefix
fn create_kraken_nats_writer(
    transport: Arc<dyn ssmd_middleware::Transport>,
    env_config: &Environment,
    feed: &Feed,
) -> ssmd_connector_lib::kraken::KrakenNatsWriter {
    if let (Some(ref prefix), Some(ref stream)) = (
        &env_config.transport.subject_prefix,
        &env_config.transport.stream,
    ) {
        info!(
            subject_prefix = %prefix,
            stream = %stream,
            "Using custom subject prefix"
        );
        ssmd_connector_lib::kraken::KrakenNatsWriter::with_prefix(
            transport,
            prefix.clone(),
            stream.clone(),
        )
    } else {
        info!(
            subject_prefix = format!("{}.{}", env_config.name, feed.name),
            "Using default subject prefix"
        );
        ssmd_connector_lib::kraken::KrakenNatsWriter::new(transport, &env_config.name, &feed.name)
    }
}

/// Create NatsWriter with optional custom subject prefix for sharding and series filter
fn create_nats_writer(
    transport: Arc<dyn ssmd_middleware::Transport>,
    env_config: &Environment,
    feed: &Feed,
    series_filter: Option<HashSet<String>>,
) -> NatsWriter {
    // Use custom subject_prefix and stream if configured, otherwise default
    let writer = if let (Some(ref prefix), Some(ref stream)) = (
        &env_config.transport.subject_prefix,
        &env_config.transport.stream,
    ) {
        info!(
            subject_prefix = %prefix,
            stream = %stream,
            "Using custom subject prefix for sharding"
        );
        NatsWriter::with_prefix(transport, prefix.clone(), stream.clone())
    } else {
        // Default: use {env_name}.{feed_name} as prefix
        info!(
            subject_prefix = format!("{}.{}", env_config.name, feed.name),
            "Using default subject prefix"
        );
        NatsWriter::new(transport, &env_config.name, &feed.name)
    };

    // Apply series filter if configured
    if let Some(filter) = series_filter {
        info!(
            series_count = filter.len(),
            "Applying series filter to lifecycle events"
        );
        writer.with_series_filter(filter)
    } else {
        writer
    }
}

/// Staleness threshold in seconds - if no messages for this long, health check fails
const STALE_THRESHOLD_SECS: u64 = 300; // 5 minutes

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
    // Use activity handle (tracks WebSocket ping/pong + data messages) for health checks
    // This prevents false staleness during quiet market periods when pings are succeeding
    let activity_handle = runner.activity_handle();

    // Start health server with staleness tracking
    let server_state = ServerState::with_last_message(
        &feed.name,
        Arc::clone(&connected_handle),
        Arc::clone(&activity_handle),
        STALE_THRESHOLD_SECS,
    );
    tokio::spawn(async move {
        if let Err(e) = ssmd_connector_lib::run_server(health_addr, server_state).await {
            error!(error = %e, "Health server error");
        }
    });
    info!(addr = %health_addr, stale_threshold_secs = STALE_THRESHOLD_SECS, "Health server started");

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
            let transport = MiddlewareFactory::create_nats_transport_validated(env_config).await?;
            let connector = WebSocketConnector::new(&url, creds);
            let writer = create_nats_writer(transport, env_config, feed, None);
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
