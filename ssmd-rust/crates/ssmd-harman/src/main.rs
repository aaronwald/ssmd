use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use clap::Parser;
use dashmap::DashMap;
use tracing::{error, info};

use ssmd_harman::{api, recovery, shutdown, AppState, Metrics};

/// ssmd-harman: PostgreSQL-backed order gateway
#[derive(Parser)]
#[command(name = "ssmd-harman")]
struct Args {
    /// Database URL (e.g., postgresql://user:pass@host:5432/harman)
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// Listen address for the HTTP API
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    listen_addr: String,

    /// Maximum notional exposure in dollars
    #[arg(long, env = "MAX_NOTIONAL", default_value = "100")]
    max_notional: f64,

    /// Kalshi API base URL
    #[arg(
        long,
        env = "KALSHI_BASE_URL",
        default_value = "https://demo-api.kalshi.co"
    )]
    kalshi_base_url: String,
}

#[tokio::main]
async fn main() {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ssmd_harman=info,harman=info".into()),
        )
        .json()
        .init();

    let args = Args::parse();

    // Load tokens from environment only — not CLI args — to avoid /proc/PID/cmdline exposure
    let api_token = std::env::var("HARMAN_API_TOKEN")
        .expect("HARMAN_API_TOKEN must be set");
    let admin_token = std::env::var("HARMAN_ADMIN_TOKEN")
        .expect("HARMAN_ADMIN_TOKEN must be set");

    // Optional: data-ts auth validation URL for API key support
    let auth_validate_url = std::env::var("AUTH_VALIDATE_URL").ok();
    if let Some(ref url) = auth_validate_url {
        info!(url, "API key validation enabled via data-ts");
    } else {
        info!("API key validation disabled (AUTH_VALIDATE_URL not set), static tokens only");
    }

    info!(listen_addr = %args.listen_addr, "ssmd-harman starting");

    // Create DB pool
    let pool = harman::db::create_pool(&args.database_url).expect("failed to create DB pool");

    // Run migrations
    harman::db::run_migrations(&pool)
        .await
        .expect("migration failed");

    // Create exchange client
    let kalshi_config = ssmd_connector_lib::kalshi::config::KalshiConfig::from_env()
        .expect("Kalshi credentials not configured");
    let credentials = ssmd_connector_lib::kalshi::auth::KalshiCredentials::new(
        kalshi_config.api_key,
        &kalshi_config.private_key_pem,
    )
    .expect("invalid Kalshi credentials");
    let exchange: Arc<dyn harman::exchange::ExchangeAdapter> = Arc::new(
        ssmd_exchange_kalshi::client::KalshiClient::new(credentials, args.kalshi_base_url),
    );

    // Check balance on startup
    match exchange.get_balance().await {
        Ok(balance) => info!(
            available_dollars = %balance.available_dollars,
            total_dollars = %balance.total_dollars,
            "connected to exchange"
        ),
        Err(e) => {
            error!(error = %e, "failed to fetch balance on startup");
            std::process::exit(1);
        }
    }

    let risk_limits = harman::risk::RiskLimits {
        max_notional: rust_decimal::Decimal::from_f64_retain(args.max_notional)
            .unwrap_or(rust_decimal::Decimal::new(100, 0)),
    };

    // Get or create startup session (key_prefix = None for backward compat)
    let startup_session_id = harman::db::get_or_create_session(&pool, "kalshi", None)
        .await
        .expect("failed to get or create session");
    info!(startup_session_id, "startup session initialized");

    let state = Arc::new(AppState {
        pool,
        exchange,
        risk_limits,
        shutting_down: AtomicBool::new(false),
        metrics: Metrics::new(),
        api_token,
        admin_token,
        startup_session_id,
        auth_validate_url,
        http_client: reqwest::Client::new(),
        session_semaphores: DashMap::new(),
        suspended_sessions: DashMap::new(),
        auth_cache: tokio::sync::RwLock::new(HashMap::new()),
        key_sessions: DashMap::new(),
        pump_semaphore: tokio::sync::Semaphore::new(1),
    });

    // Run recovery before starting API server
    if let Err(e) = recovery::run(&state).await {
        error!(error = %e, "recovery failed, exiting");
        std::process::exit(1);
    }

    // Spawn shutdown handler
    let shutdown_state = state.clone();
    let shutdown_handle = tokio::spawn(async move {
        shutdown::wait_for_shutdown(shutdown_state).await;
    });

    // Start API server
    let app = api::router(state.clone());
    let listener = tokio::net::TcpListener::bind(&args.listen_addr)
        .await
        .expect("failed to bind");
    info!(addr = %args.listen_addr, "API server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            shutdown_handle.await.ok();
        })
        .await
        .expect("server error");

    info!("ssmd-harman stopped");
}
