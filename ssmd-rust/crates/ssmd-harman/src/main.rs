use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use dashmap::DashMap;
use lru::LruCache;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use ssmd_harman::{api, shutdown, AppState, MonitorMetrics};
use ssmd_harman_ems::{Ems, EmsMetrics};
use ssmd_harman_oms::price_feed::NatsPriceFeed;
use ssmd_harman_oms::price_monitor::PriceMonitor;
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

    /// Maximum total notional exposure in dollars
    #[arg(long, env = "MAX_NOTIONAL", default_value = "100")]
    max_notional: f64,

    /// Maximum notional for a single order (fat-finger protection) in dollars
    #[arg(long, env = "MAX_ORDER_NOTIONAL", default_value = "25")]
    max_order_notional: f64,

    /// Maximum daily realized loss in dollars (session suspended when exceeded)
    #[arg(long, env = "DAILY_LOSS_LIMIT", default_value = "50")]
    daily_loss_limit: f64,

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
    let cf_iss = std::env::var("CF_ISS").ok();
    let data_ts_api_key = std::env::var("DATA_TS_API_KEY").ok();
    let data_ts_base_url = std::env::var("DATA_TS_BASE_URL").ok();

    if cf_jwks_url.is_some() && cf_aud.is_some() {
        info!("Cloudflare Access JWT auth enabled");
    } else {
        info!("Cloudflare Access JWT auth disabled (CF_JWKS_URL/CF_AUD not set)");
    }

    info!(listen_addr = %args.listen_addr, exchange_type = %exchange_type, environment = %environment, "ssmd-harman starting");

    // Create DB pool
    let pool = harman::db::create_pool(&args.database_url).expect("failed to create DB pool");

    // Run migrations
    harman::db::run_migrations(&pool)
        .await
        .expect("migration failed");

    // Create exchange client based on EXCHANGE_TYPE
    let exchange_base_url = std::env::var("EXCHANGE_BASE_URL")
        .unwrap_or_else(|_| args.kalshi_base_url.clone());

    let exchange: Arc<dyn harman::exchange::ExchangeAdapter> = match exchange_type.as_str() {
        "kalshi" => {
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

            let kalshi_config = ssmd_connector_lib::kalshi::config::KalshiConfig::from_env()
                .expect("Kalshi credentials not configured");
            let credentials = ssmd_connector_lib::kalshi::auth::KalshiCredentials::new(
                kalshi_config.api_key,
                &kalshi_config.private_key_pem,
            )
            .expect("invalid Kalshi credentials");
            Arc::new(ssmd_exchange_kalshi::client::KalshiRestClient::new(
                credentials,
                args.kalshi_base_url.clone(),
            ))
        }
        "test" => {
            // Test exchange — uses Kalshi protocol against harman-test-exchange.
            // No real credentials needed; dummy RSA key satisfies the type system
            // and the test-exchange ignores auth headers entirely.
            info!(base_url = %exchange_base_url, "using test exchange (Kalshi protocol)");
            let credentials = ssmd_connector_lib::kalshi::auth::KalshiCredentials::dummy();
            Arc::new(ssmd_exchange_kalshi::client::KalshiRestClient::new(
                credentials,
                exchange_base_url.clone(),
            ))
        }
        other => {
            error!(exchange_type = %other, "unsupported EXCHANGE_TYPE (expected: kalshi, test)");
            std::process::exit(1);
        }
    };

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
        max_order_notional: rust_decimal::Decimal::from_f64_retain(args.max_order_notional)
            .unwrap_or(rust_decimal::Decimal::new(25, 0)),
        daily_loss_limit: rust_decimal::Decimal::from_f64_retain(args.daily_loss_limit)
            .unwrap_or(rust_decimal::Decimal::new(50, 0)),
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

    // Find existing session (prefers authenticated over NULL-key placeholder).
    // Only create a NULL-key placeholder on very first boot when no session exists.
    let startup_session_id = match harman::db::find_startup_session(&pool, &exchange_type, &environment).await {
        Ok(Some(id)) => id,
        _ => {
            harman::db::get_or_create_session(&pool, &exchange_type, &environment, None)
                .await
                .expect("failed to create startup session")
        }
    };
    info!(startup_session_id, "startup session resolved");

    // Create audit channel for exchange audit logging
    let (audit_sender, audit_writer) = harman::audit::create_audit_channel(pool.clone());

    // Create shared registry, EMS metrics first, then OMS metrics
    let registry = prometheus::Registry::new();
    let ems_metrics = EmsMetrics::new(&registry);
    let ems = Arc::new(Ems::new(pool.clone(), exchange.clone(), risk_limits, ems_metrics, audit_sender.clone()));

    let oms_metrics = Arc::new(OmsMetrics::new(&registry));
    let oms = Arc::new(Oms::new(pool.clone(), exchange.clone(), ems.clone(), oms_metrics, audit_sender));
    let monitor_metrics = MonitorMetrics::new(&registry);

    // Optional WebSocket event stream for real-time order/fill/settlement events.
    // Requires KALSHI_WS_URL and Kalshi credentials (KALSHI_API_KEY + KALSHI_PRIVATE_KEY).
    let event_stream: Option<Arc<dyn harman::exchange::EventStream>> =
        match (std::env::var("KALSHI_WS_URL"), exchange_type.as_str()) {
            (Ok(ws_url), "kalshi" | "test") => {
                let kalshi_config = ssmd_connector_lib::kalshi::config::KalshiConfig::from_env()
                    .expect("Kalshi credentials required for WS");
                let ws_credentials = ssmd_connector_lib::kalshi::auth::KalshiCredentials::new(
                    kalshi_config.api_key,
                    &kalshi_config.private_key_pem,
                )
                .expect("invalid Kalshi credentials for WS");
                info!(ws_url = %ws_url, "WS event stream enabled");
                Some(Arc::new(ssmd_exchange_kalshi::ws::client::KalshiWsClient::new(
                    ws_credentials, ws_url,
                )))
            }
            (Ok(_), _) => {
                warn!("KALSHI_WS_URL set but EXCHANGE_TYPE is not kalshi/test, WS disabled");
                None
            }
            (Err(_), _) => {
                info!("KALSHI_WS_URL not set, WS event stream disabled (REST-only mode)");
                None
            }
        };

    let reconcile_interval = if args.reconcile_interval_secs > 0 {
        Some(Duration::from_secs(args.reconcile_interval_secs))
    } else {
        None
    };
    // Create shared PumpTrigger before runner and PriceMonitor
    let pump_trigger = ssmd_harman_oms::runner::PumpTrigger::new();

    // Optional NATS price feed for PriceMonitor (SL trigger monitoring)
    let (price_monitor_handle, price_monitor_task) = match std::env::var("NATS_URL") {
        Ok(nats_url) => {
            let nats_subjects: Vec<String> = std::env::var("NATS_TICKER_SUBJECTS")
                .unwrap_or_else(|_| "prod.kalshi.crypto.json.ticker.>".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            info!(nats_url = %nats_url, subjects = ?nats_subjects, "connecting PriceMonitor to NATS");

            match NatsPriceFeed::connect(&nats_url, &nats_subjects).await {
                Ok(feed) => {
                    let (monitor, handle) = PriceMonitor::new(
                        pool.clone(),
                        pump_trigger.clone(),
                        oms.audit.clone(),
                        Box::new(feed),
                    );
                    info!("PriceMonitor created, will start after recovery");
                    (Some(handle), Some(monitor))
                }
                Err(e) => {
                    error!(error = %e, "failed to connect PriceMonitor to NATS — SL monitoring disabled");
                    (None, None)
                }
            }
        }
        Err(_) => {
            info!("NATS_URL not set, PriceMonitor disabled (no SL trigger monitoring)");
            (None, None)
        }
    };

    let runner = Arc::new(OmsRunner::new_with_pump_trigger(
        oms.clone(),
        reconcile_interval,
        startup_session_id,
        event_stream,
        price_monitor_handle,
        pump_trigger.clone(),
    ));
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
        cf_iss,
        cf_jwks: RwLock::new(None),
        data_ts_api_key,
        data_ts_base_url,
    });

    // Run recovery before starting API server
    if let Err(e) = oms.run_recovery(startup_session_id).await {
        error!(error = %e, "recovery failed, exiting");
        std::process::exit(1);
    }

    // Spawn audit writer (background batch INSERT to exchange_audit_log)
    let audit_handle = tokio::spawn(async move {
        audit_writer.run().await;
    });

    // Spawn OMS background runner (auto-pump + auto-reconcile)
    let runner_state = state.clone();
    tokio::spawn(async move {
        runner_state.runner.run(&runner_state.session_semaphores).await;
    });

    // Spawn PriceMonitor if configured
    if let Some(monitor) = price_monitor_task {
        // Crash recovery: reload any orders in Monitoring state
        if let Some(handle) = state.runner.price_monitor_handle() {
            match harman::db::list_orders(&state.pool, startup_session_id, Some(harman::state::OrderState::Monitoring)).await {
                Ok(orders) => {
                    if !orders.is_empty() {
                        info!(count = orders.len(), "reloading monitoring triggers from DB");
                        for order in orders {
                            if let (Some(trigger_price), Some(group_id)) = (order.trigger_price, order.group_id) {
                                handle.arm(ssmd_harman_oms::price_monitor::Trigger {
                                    order_id: order.id,
                                    session_id: order.session_id,
                                    group_id,
                                    ticker: order.ticker.clone(),
                                    side: order.side,
                                    action: order.action,
                                    trigger_price,
                                    submit_price: order.price_dollars,
                                    quantity: order.quantity,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "failed to reload monitoring triggers — SL orders may be unarmed");
                }
            }
        }

        tokio::spawn(async move {
            monitor.run().await;
        });
        info!("PriceMonitor spawned");
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

    // Drop our Arc<AppState> so AuditSenders are released and the channel closes.
    drop(state);

    // Wait for the audit writer to drain remaining events (max 5s).
    info!("waiting for audit writer to drain...");
    match tokio::time::timeout(std::time::Duration::from_secs(5), audit_handle).await {
        Ok(Ok(())) => info!("audit writer drained successfully"),
        Ok(Err(e)) => error!(error = %e, "audit writer task panicked"),
        Err(_) => warn!("audit writer drain timed out after 5s"),
    }

    info!("ssmd-harman stopped");
}
