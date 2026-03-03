//! Kalshi WebSocket client for private trading channels.
//!
//! Pure event source — no DB access, no business logic.
//! Connects, authenticates, subscribes to private channels, and emits
//! `ExchangeEvent` via a broadcast channel. Handles reconnection internally.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::{http::Request, Message};
use tracing::{debug, error, info, warn};

use harman::exchange::{ExchangeEvent, EventStream};
use ssmd_connector_lib::kalshi::auth::KalshiCredentials;

use super::messages::WsPrivateMessage;

/// Private channels to subscribe to on connect.
const PRIVATE_CHANNELS: &[&str] = &[
    "fill",
    "user_orders",
    "market_positions",
    "market_lifecycle_v2",
];

/// Broadcast channel capacity for events.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Reconnect backoff parameters.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Read timeout — if no data for this long, treat connection as dead.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Subscription confirmation timeout.
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(15);

/// Kalshi private WebSocket client.
///
/// Implements `EventStream` — consumers call `subscribe()` to get a
/// `broadcast::Receiver<ExchangeEvent>`. The client runs a background
/// task that connects, subscribes, and forwards events.
pub struct KalshiWsClient {
    tx: broadcast::Sender<ExchangeEvent>,
}

impl KalshiWsClient {
    /// Create a new WS client and spawn the background event loop.
    ///
    /// `ws_url` should be:
    /// - `wss://demo-api.kalshi.co/trade-api/ws/v2` for demo
    /// - `wss://api.kalshi.com/trade-api/ws/v2` for production
    pub fn new(credentials: KalshiCredentials, ws_url: String) -> Self {
        let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            event_loop(credentials, ws_url, tx_clone).await;
        });

        Self { tx }
    }
}

impl EventStream for KalshiWsClient {
    fn subscribe(&self) -> broadcast::Receiver<ExchangeEvent> {
        self.tx.subscribe()
    }
}

/// Main event loop with automatic reconnection.
async fn event_loop(
    credentials: KalshiCredentials,
    ws_url: String,
    tx: broadcast::Sender<ExchangeEvent>,
) {
    let mut backoff = INITIAL_BACKOFF;

    loop {
        info!(url = %ws_url, "connecting to Kalshi private WebSocket");

        match connect_and_run(&credentials, &ws_url, &tx).await {
            Ok(()) => {
                info!("WebSocket connection closed gracefully");
            }
            Err(e) => {
                error!(error = %e, "WebSocket connection error");
            }
        }

        // Emit Disconnected event
        let _ = tx.send(ExchangeEvent::Disconnected {
            reason: "connection lost, reconnecting".to_string(),
        });

        info!(backoff_secs = backoff.as_secs(), "reconnecting after backoff");
        tokio::time::sleep(backoff).await;

        // Exponential backoff with cap
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Connect, authenticate, subscribe, and process messages until disconnect.
async fn connect_and_run(
    credentials: &KalshiCredentials,
    ws_url: &str,
    tx: &broadcast::Sender<ExchangeEvent>,
) -> Result<(), String> {
    // Sign the WS request
    let (timestamp, signature) = credentials
        .sign_websocket_request()
        .map_err(|e| format!("auth signing failed: {}", e))?;

    let host = ws_url
        .replace("wss://", "")
        .split('/')
        .next()
        .unwrap_or("api.kalshi.com")
        .to_string();

    let request = Request::builder()
        .uri(ws_url)
        .header("KALSHI-ACCESS-KEY", &credentials.api_key)
        .header("KALSHI-ACCESS-SIGNATURE", &signature)
        .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
        .header("Host", &host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .map_err(|e| format!("build request: {}", e))?;

    let (mut ws, response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("connect: {}", e))?;

    info!(status = ?response.status(), "WebSocket connected");

    // Subscribe to private channels
    for (i, channel) in PRIVATE_CHANNELS.iter().enumerate() {
        let cmd_id = (i + 1) as u64;
        let cmd = serde_json::json!({
            "id": cmd_id,
            "cmd": "subscribe",
            "params": {
                "channels": [channel]
            }
        });

        let msg_text = serde_json::to_string(&cmd)
            .map_err(|e| format!("serialize subscribe: {}", e))?;

        ws.send(Message::Text(msg_text))
            .await
            .map_err(|e| format!("send subscribe: {}", e))?;

        // Wait for subscription confirmation (with timeout)
        let confirmed = wait_for_subscribe_ack(&mut ws, cmd_id).await;
        match confirmed {
            Ok(()) => info!(channel = %channel, "subscribed"),
            Err(e) => warn!(channel = %channel, error = %e, "subscription may have failed"),
        }
    }

    // Emit Connected event (after all subscriptions)
    let _ = tx.send(ExchangeEvent::Connected);

    // Process messages
    loop {
        let recv = tokio::time::timeout(READ_TIMEOUT, ws.next()).await;

        match recv {
            Err(_) => {
                warn!("WebSocket read timeout, assuming connection dead");
                return Err("read timeout".to_string());
            }
            Ok(None) => {
                return Err("stream ended".to_string());
            }
            Ok(Some(Err(e))) => {
                return Err(format!("WebSocket error: {}", e));
            }
            Ok(Some(Ok(Message::Text(text)))) => {
                handle_message(&text, tx);
            }
            Ok(Some(Ok(Message::Ping(data)))) => {
                if let Err(e) = ws.send(Message::Pong(data)).await {
                    return Err(format!("pong failed: {}", e));
                }
            }
            Ok(Some(Ok(Message::Close(_)))) => {
                return Ok(());
            }
            Ok(Some(Ok(_))) => {}
        }
    }
}

/// Wait for a subscription acknowledgement for the given command ID.
async fn wait_for_subscribe_ack(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_id: u64,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + SUBSCRIBE_TIMEOUT;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("subscription confirmation timeout".to_string());
        }

        let recv = tokio::time::timeout(remaining, ws.next()).await;
        match recv {
            Err(_) => return Err("timeout".to_string()),
            Ok(None) => return Err("stream ended".to_string()),
            Ok(Some(Err(e))) => return Err(format!("ws error: {}", e)),
            Ok(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<WsPrivateMessage>(&text) {
                    Ok(WsPrivateMessage::Subscribed { id, .. }) if id == expected_id => {
                        return Ok(());
                    }
                    Ok(WsPrivateMessage::Ok { id, .. }) if id == expected_id => {
                        return Ok(());
                    }
                    Ok(WsPrivateMessage::Error { id: Some(id), msg }) if id == expected_id => {
                        let reason = msg
                            .map(|m| m.msg)
                            .unwrap_or_else(|| "unknown error".to_string());
                        return Err(format!("subscribe error: {}", reason));
                    }
                    _ => {
                        // Other messages (data, other subscriptions) — continue waiting
                        debug!(raw = %text, "received non-ack message while waiting for subscription");
                    }
                }
            }
            Ok(Some(Ok(Message::Ping(data)))) => {
                let _ = ws.send(Message::Pong(data)).await;
            }
            Ok(Some(Ok(_))) => {}
        }
    }
}

/// Parse a text message and emit the corresponding ExchangeEvent.
fn handle_message(text: &str, tx: &broadcast::Sender<ExchangeEvent>) {
    let msg = match serde_json::from_str::<WsPrivateMessage>(text) {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, text = %text, "failed to parse WS message");
            return;
        }
    };

    match msg {
        WsPrivateMessage::Fill { msg, .. } => {
            if let Some(event) = msg.to_exchange_event() {
                let _ = tx.send(event);
            }
        }
        WsPrivateMessage::UserOrder { msg, .. } => {
            if let Some(event) = msg.to_exchange_event() {
                let _ = tx.send(event);
            }
        }
        WsPrivateMessage::MarketPosition { msg, .. } => {
            let _ = tx.send(msg.to_exchange_event());
        }
        WsPrivateMessage::MarketLifecycleV2 { msg, .. } => {
            if let Some(event) = msg.to_exchange_event() {
                let _ = tx.send(event);
            }
        }
        WsPrivateMessage::Subscribed { .. }
        | WsPrivateMessage::Ok { .. }
        | WsPrivateMessage::Error { .. } => {
            // Control messages after initial subscription — log and ignore
            debug!(raw = %text, "control message in event loop");
        }
        WsPrivateMessage::Unknown => {
            debug!(raw = %text, "unknown WS message type");
        }
    }
}
