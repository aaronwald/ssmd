mod api;
mod reconciliation;
mod recovery;
mod shutdown;
mod sweeper;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use clap::Parser;
use deadpool_postgres::Pool;
use tracing::{error, info};

use harman::exchange::ExchangeAdapter;
use harman::risk::RiskLimits;

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

    /// Bearer token for order endpoints (/v1/orders/*)
    #[arg(long, env = "HARMAN_API_TOKEN")]
    api_token: String,

    /// Bearer token for admin endpoints (/v1/admin/*, mass-cancel)
    #[arg(long, env = "HARMAN_ADMIN_TOKEN")]
    admin_token: String,
}

/// Metrics for prometheus
pub struct Metrics {
    pub registry: prometheus::Registry,
    pub orders_dequeued: prometheus::IntCounter,
    pub orders_submitted: prometheus::IntCounter,
    pub orders_rejected: prometheus::IntCounter,
    pub orders_cancelled: prometheus::IntCounter,
    pub fills_recorded: prometheus::IntCounter,
}

impl Metrics {
    fn new() -> Self {
        let registry = prometheus::Registry::new();

        let orders_dequeued =
            prometheus::IntCounter::new("harman_orders_dequeued_total", "Orders dequeued from queue")
                .unwrap();
        let orders_submitted =
            prometheus::IntCounter::new("harman_orders_submitted_total", "Orders submitted to exchange")
                .unwrap();
        let orders_rejected =
            prometheus::IntCounter::new("harman_orders_rejected_total", "Orders rejected by exchange")
                .unwrap();
        let orders_cancelled =
            prometheus::IntCounter::new("harman_orders_cancelled_total", "Orders cancelled")
                .unwrap();
        let fills_recorded =
            prometheus::IntCounter::new("harman_fills_recorded_total", "Fills recorded")
                .unwrap();

        registry.register(Box::new(orders_dequeued.clone())).unwrap();
        registry.register(Box::new(orders_submitted.clone())).unwrap();
        registry.register(Box::new(orders_rejected.clone())).unwrap();
        registry.register(Box::new(orders_cancelled.clone())).unwrap();
        registry.register(Box::new(fills_recorded.clone())).unwrap();

        Self {
            registry,
            orders_dequeued,
            orders_submitted,
            orders_rejected,
            orders_cancelled,
            fills_recorded,
        }
    }
}

/// Shared application state
pub struct AppState {
    pub pool: Pool,
    pub exchange: Arc<dyn ExchangeAdapter>,
    pub risk_limits: RiskLimits,
    pub shutting_down: AtomicBool,
    pub metrics: Metrics,
    pub session_id: i64,
    pub api_token: String,
    pub admin_token: String,
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
    let exchange: Arc<dyn ExchangeAdapter> = Arc::new(
        ssmd_exchange_kalshi::client::KalshiClient::new(credentials, args.kalshi_base_url),
    );

    // Check balance on startup
    match exchange.get_balance().await {
        Ok(balance) => info!(
            available_cents = balance.available_cents,
            total_cents = balance.total_cents,
            "connected to exchange"
        ),
        Err(e) => {
            error!(error = %e, "failed to fetch balance on startup");
            std::process::exit(1);
        }
    }

    let risk_limits = RiskLimits {
        max_notional: rust_decimal::Decimal::from_f64_retain(args.max_notional)
            .unwrap_or(rust_decimal::Decimal::new(100, 0)),
    };

    // Get or create session
    let session_id = harman::db::get_or_create_session(&pool, "kalshi")
        .await
        .expect("failed to get or create session");
    info!(session_id, "session initialized");

    let state = Arc::new(AppState {
        pool,
        exchange,
        risk_limits,
        shutting_down: AtomicBool::new(false),
        metrics: Metrics::new(),
        session_id,
        api_token: args.api_token,
        admin_token: args.admin_token,
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
