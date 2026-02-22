use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use url::Url;

use crate::error::ConnectorError;
use crate::traits::{Connector, TimestampedMsg};
use ssmd_middleware::now_tsc;

/// WebSocket connector for Kalshi
pub struct WebSocketConnector {
    url: String,
    creds: Option<HashMap<String, String>>,
    tx: Option<mpsc::Sender<TimestampedMsg>>,
    rx: Option<mpsc::Receiver<TimestampedMsg>>,
}

impl WebSocketConnector {
    pub fn new(url: impl Into<String>, creds: Option<HashMap<String, String>>) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            url: url.into(),
            creds,
            tx: Some(tx),
            rx: Some(rx),
        }
    }
}

#[async_trait]
impl Connector for WebSocketConnector {
    async fn connect(&mut self) -> Result<(), ConnectorError> {
        let url = Url::parse(&self.url)
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| ConnectorError::ConnectionFailed(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();
        let tx = self.tx.take().unwrap();

        // Handle authentication if credentials provided
        if let Some(ref creds) = self.creds {
            if let (Some(api_key), Some(api_secret)) =
                (creds.get("KALSHI_API_KEY"), creds.get("KALSHI_API_SECRET"))
            {
                // Send auth message (Kalshi-specific format)
                let auth_msg = serde_json::json!({
                    "type": "auth",
                    "api_key": api_key,
                    "api_secret": api_secret
                });
                write
                    .send(WsMessage::Text(auth_msg.to_string()))
                    .await
                    .map_err(|e| ConnectorError::AuthFailed(e.to_string()))?;
            }
        }

        // Spawn reader task
        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(WsMessage::Text(text)) => {
                        if tx.send((now_tsc(), text.into_bytes())).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsMessage::Binary(data)) => {
                        if tx.send((now_tsc(), data)).await.is_err() {
                            break;
                        }
                    }
                    Ok(WsMessage::Close(_)) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        Ok(())
    }

    fn messages(&mut self) -> mpsc::Receiver<TimestampedMsg> {
        self.rx.take().expect("messages() called twice")
    }

    async fn close(&mut self) -> Result<(), ConnectorError> {
        // Drop sender to signal reader task to stop
        self.tx = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_connector() {
        let creds = HashMap::from([
            ("KALSHI_API_KEY".to_string(), "test-key".to_string()),
            ("KALSHI_API_SECRET".to_string(), "test-secret".to_string()),
        ]);

        let connector = WebSocketConnector::new("wss://example.com/ws", Some(creds));
        assert_eq!(connector.url, "wss://example.com/ws");
    }

    #[test]
    fn test_messages_channel() {
        let mut connector = WebSocketConnector::new("wss://example.com/ws", None);
        let _rx = connector.messages();
        // Channel should be returned successfully
    }
}
