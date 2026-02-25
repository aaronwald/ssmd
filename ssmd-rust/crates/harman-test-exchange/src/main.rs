use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    Router,
};
use clap::Parser;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod routes;
mod state;

use routes::AppState;
use state::ExchangeState;

#[derive(Parser)]
#[command(name = "harman-test-exchange")]
#[command(about = "Test exchange server mimicking Kalshi REST API for harman E2E testing")]
struct Args {
    /// Listen address
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    listen_addr: String,

    /// Starting balance in cents (1_000_000 = $10,000)
    #[arg(long, env = "STARTING_BALANCE", default_value = "1000000")]
    starting_balance: i64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let state: AppState = Arc::new(Mutex::new(ExchangeState::new(args.starting_balance)));

    let app = Router::new()
        .route(
            "/trade-api/v2/portfolio/orders",
            post(routes::submit_order).get(routes::list_orders),
        )
        .route(
            "/trade-api/v2/portfolio/orders/batched",
            delete(routes::batch_cancel),
        )
        .route(
            "/trade-api/v2/portfolio/orders/:id",
            delete(routes::cancel_order),
        )
        .route(
            "/trade-api/v2/portfolio/fills",
            get(routes::list_fills),
        )
        .route(
            "/trade-api/v2/portfolio/positions",
            get(routes::list_positions),
        )
        .route(
            "/trade-api/v2/portfolio/balance",
            get(routes::get_balance),
        )
        .route("/health", get(routes::health))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&args.listen_addr)
        .await
        .expect("failed to bind listener");

    tracing::info!(
        addr = %args.listen_addr,
        balance_cents = args.starting_balance,
        "harman-test-exchange started"
    );

    axum::serve(listener, app)
        .await
        .expect("server error");
}
