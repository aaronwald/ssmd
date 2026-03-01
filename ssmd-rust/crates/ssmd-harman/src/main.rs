use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use dashmap::DashMap;
use lru::LruCache;
use tokio::sync::RwLock;
use tracing::{error, info};

use ssmd_harman::{api, shutdown, AppState, MonitorMetrics};
use ssmd_harman_ems::{Ems, EmsMetrics};
use ssmd_harman_oms::runner::OmsRunner;
use ssmd_harman_oms::{Oms, OmsMetrics};

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

    /// Enable auto-pump after order mutations
    #[arg(long, env = "AUTO_PUMP", default_value = "false")]
    auto_pump: bool,

    /// Auto-reconcile interval in seconds (0 = disabled)
    #[arg(long, env = "RECONCILE_INTERVAL_SECS", default_value = "0")]
    reconcile_interval_secs: u64,
}

#[tokio::main]
async fn main() {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ssmd_harman=info,harman=info,ssmd_harman_ems=info,ssmd_harman_oms=info".into()),
        )
        .json()
        .init();

    let args = Args::parse();

    // Load tokens from environment only -- not CLI args -- to avoid /proc/PID/cmdline exposure
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

    // Exchange type and environment
    let exchange_type = std::env::var("EXCHANGE_TYPE").unwrap_or_else(|_| "kalshi".to_string());
    let environment = std::env::var("EXCHANGE_ENVIRONMENT").unwrap_or_else(|_| "demo".to_string());

    // Cloudflare Access JWT config
    let cf_jwks_url = std::env::var("CF_JWKS_URL").ok();
    let cf_aud = std::env::var("CF_AUD").ok();
    let data_ts_api_key = std::env::var("DATA_TS_API_KEY").ok();
    let data_ts_base_url = std::env::var("DATA_TS_BASE_URL").ok();

    if cf_jwks_url.is_some() && cf_aud.is_some() {
        info!("Cloudflare Access JWT auth enabled");
    } else {
        info!("Cloudflare Access JWT auth disabled (CF_JWKS_URL/CF_AUD not set)");
    }

    // Startup env validation: detect base URL / environment mismatch
    let base_url_lower = args.kalshi_base_url.to_lowercase();
    if environment == "prod" && base_url_lower.contains("demo") {
        error!(
            environment = %environment,
            base_url = %args.kalshi_base_url,
            "FATAL: EXCHANGE_ENVIRONMENT=prod but KALSHI_BASE_URL contains 'demo'"
        );
        std::process::exit(1);
    }
    if environment == "demo" && !base_url_lower.contains("demo") {
        error!(
            environment = %environment,
            base_url = %args.kalshi_base_url,
            "FATAL: EXCHANGE_ENVIRONMENT=demo but KALSHI_BASE_URL does not contain 'demo'"
        );
        std::process::exit(1);
    }

    info!(listen_addr = %args.listen_addr, exchange_type = %exchange_type, environment = %environment, "ssmd-harman starting");

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

    // Reset stale processing items (watchdog: clear items stuck in processing state)
    if let Ok(client) = pool.get().await {
        let _ = client
            .execute(
                "UPDATE order_queue SET processing = FALSE WHERE processing = TRUE AND created_at < NOW() - INTERVAL '60 seconds'",
                &[],
            )
            .await
            .map(|n| {
                if n > 0 {
                    tracing::warn!(count = n, "reset stale processing order_queue items");
                }
            });
    }

    // Get or create startup session (key_prefix = None for backward compat)
    let startup_session_id = harman::db::get_or_create_session(&pool, &exchange_type, &environment, None)
        .await
        .expect("failed to get or create session");
    info!(startup_session_id, "startup session initialized");

    // Create shared registry, EMS metrics first, then OMS metrics
    let registry = prometheus::Registry::new();
    let ems_metrics = EmsMetrics::new(&registry);
    let ems = Arc::new(Ems::new(pool.clone(), exchange.clone(), risk_limits, ems_metrics));

    let oms_metrics = OmsMetrics::new(&registry);
    let oms = Arc::new(Oms::new(pool.clone(), exchange, ems.clone(), oms_metrics));
    let monitor_metrics = MonitorMetrics::new(&registry);

    let reconcile_interval = if args.reconcile_interval_secs > 0 {
        Some(Duration::from_secs(args.reconcile_interval_secs))
    } else {
        None
    };
    let runner = Arc::new(OmsRunner::new(oms.clone(), reconcile_interval, startup_session_id));
    let pump_trigger = runner.pump_trigger();
    if args.auto_pump {
        info!("auto-pump enabled");
    }
    if let Some(interval) = reconcile_interval {
        info!(interval_secs = interval.as_secs(), "auto-reconcile enabled");
    }

    // Optional Redis connection for monitor data
    let redis_conn = match std::env::var("REDIS_URL") {
        Ok(url) => match redis::Client::open(url.as_str()) {
            Ok(client) => match client.get_multiplexed_async_connection().await {
                Ok(conn) => {
                    info!("Connected to Redis for monitor data");
                    Some(conn)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to connect to Redis, monitor endpoints will return empty");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "Invalid Redis URL, monitor endpoints will return empty");
                None
            }
        },
        Err(_) => {
            info!("REDIS_URL not set, monitor endpoints will return empty");
            None
        }
    };

    let state = Arc::new(AppState {
        ems,
        oms: oms.clone(),
        pool,
        registry,
        api_token,
        admin_token,
        startup_session_id,
        auth_validate_url,
        http_client: reqwest::Client::new(),
        runner: runner.clone(),
        auto_pump: args.auto_pump,
        pump_trigger,
        session_semaphores: DashMap::new(),
        auth_cache: RwLock::new(LruCache::new(NonZeroUsize::new(512).unwrap())),
        key_sessions: DashMap::new(),
        ticker_cache: tokio::sync::RwLock::new(None),
        pump_semaphore: tokio::sync::Semaphore::new(1),
        redis_conn,
        monitor_metrics,
        exchange_type,
        environment,
        cf_jwks_url,
        cf_aud,
        cf_jwks: RwLock::new(None),
        data_ts_api_key,
        data_ts_base_url,
    });

    // Run recovery before starting API server
    if let Err(e) = oms.run_recovery(startup_session_id).await {
        error!(error = %e, "recovery failed, exiting");
        std::process::exit(1);
    }

    // Spawn OMS background runner (auto-pump + auto-reconcile)
    let runner_state = state.clone();
    tokio::spawn(async move {
        runner_state.runner.run(&runner_state.session_semaphores).await;
    });

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
