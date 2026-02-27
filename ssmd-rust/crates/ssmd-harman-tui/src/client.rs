use anyhow::{Context, Result};
use reqwest::Client;

use crate::types::{
    CreateOrderRequest, HealthResponse, MassCancelResult, Order, OrdersResponse, PositionInfo,
    PositionsResponse, PumpResult, ReconcileResult, RiskInfo, Snapshot, SnapResponse,
};

/// HTTP client for the harman order management API.
pub struct HarmanClient {
    client: Client,
    base_url: String,
    token: String,
}

impl HarmanClient {
    pub fn new(base_url: String, token: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    /// GET /v1/orders?state=...
    pub async fn list_orders(&self, state: Option<&str>) -> Result<Vec<Order>> {
        let mut url = format!("{}/v1/orders", self.base_url);
        if let Some(s) = state {
            url.push_str(&format!("?state={}", s));
        }
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("list_orders request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("list_orders: {} {}", status, body);
        }
        let body: OrdersResponse = resp.json().await.context("list_orders: parse response")?;
        Ok(body.orders)
    }

    /// POST /v1/orders
    pub async fn create_order(&self, req: &CreateOrderRequest) -> Result<serde_json::Value> {
        let url = format!("{}/v1/orders", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(req)
            .send()
            .await
            .context("create_order request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("create_order: {} {}", status, body);
        }
        resp.json().await.context("create_order: parse response")
    }

    /// GET /v1/orders/:id
    pub async fn get_order(&self, id: i64) -> Result<Order> {
        let url = format!("{}/v1/orders/{}", self.base_url, id);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("get_order request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("get_order: {} {}", status, body);
        }
        resp.json().await.context("get_order: parse response")
    }

    /// DELETE /v1/orders/:id
    pub async fn cancel_order(&self, id: i64) -> Result<()> {
        let url = format!("{}/v1/orders/{}", self.base_url, id);
        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("cancel_order request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("cancel_order: {} {}", status, body);
        }
        Ok(())
    }

    /// POST /v1/orders/:id/amend
    pub async fn amend_order(
        &self,
        id: i64,
        new_price: Option<&str>,
        new_qty: Option<&str>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/v1/orders/{}/amend", self.base_url, id);
        let body = crate::types::AmendOrderRequest {
            new_price_dollars: new_price.map(|s| s.to_string()),
            new_quantity: new_qty.map(|s| s.to_string()),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("amend_order request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("amend_order: {} {}", status, body);
        }
        resp.json().await.context("amend_order: parse response")
    }

    /// POST /v1/orders/:id/decrease
    pub async fn decrease_order(&self, id: i64, reduce_by: &str) -> Result<serde_json::Value> {
        let url = format!("{}/v1/orders/{}/decrease", self.base_url, id);
        let body = crate::types::DecreaseOrderRequest {
            reduce_by: reduce_by.to_string(),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("decrease_order request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("decrease_order: {} {}", status, body);
        }
        resp.json()
            .await
            .context("decrease_order: parse response")
    }

    /// POST /v1/orders/mass-cancel with {"confirm": true}
    pub async fn mass_cancel(&self) -> Result<MassCancelResult> {
        let url = format!("{}/v1/orders/mass-cancel", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&serde_json::json!({"confirm": true}))
            .send()
            .await
            .context("mass_cancel request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("mass_cancel: {} {}", status, body);
        }
        resp.json().await.context("mass_cancel: parse response")
    }

    /// POST /v1/admin/pump
    pub async fn pump(&self) -> Result<PumpResult> {
        let url = format!("{}/v1/admin/pump", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("pump request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("pump: {} {}", status, body);
        }
        resp.json().await.context("pump: parse response")
    }

    /// POST /v1/admin/reconcile
    pub async fn reconcile(&self) -> Result<ReconcileResult> {
        let url = format!("{}/v1/admin/reconcile", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("reconcile request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("reconcile: {} {}", status, body);
        }
        resp.json().await.context("reconcile: parse response")
    }

    /// GET /v1/admin/positions
    pub async fn list_positions(&self) -> Result<Vec<PositionInfo>> {
        let url = format!("{}/v1/admin/positions", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("list_positions request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("list_positions: {} {}", status, body);
        }
        let body: PositionsResponse = resp.json().await.context("list_positions: parse response")?;
        Ok(body.exchange)
    }

    /// GET /v1/admin/risk
    pub async fn get_risk(&self) -> Result<RiskInfo> {
        let url = format!("{}/v1/admin/risk", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .context("get_risk request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("get_risk: {} {}", status, body);
        }
        resp.json().await.context("get_risk: parse response")
    }

    /// GET /health
    pub async fn health(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("health request failed")?;
        if !resp.status().is_success() {
            return Ok(false);
        }
        let body: HealthResponse = resp.json().await.context("health: parse response")?;
        Ok(body.status == "healthy")
    }
}

/// HTTP client for the data-ts API (market data snapshots).
pub struct DataClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl DataClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    /// GET /v1/data/snap?feed={feed}[&tickers=t1,t2,...]
    /// Returns normalized Snapshot structs parsed per-feed.
    pub async fn snap(&self, feed: &str, tickers: Option<&[&str]>) -> Result<Vec<Snapshot>> {
        let mut url = format!("{}/v1/data/snap?feed={}", self.base_url, feed);
        if let Some(ts) = tickers {
            if !ts.is_empty() {
                url.push_str(&format!("&tickers={}", ts.join(",")));
            }
        }
        let resp = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("snap request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("snap: {} {}", status, body);
        }
        let raw: SnapResponse = resp.json().await.context("snap: parse response")?;
        let parser = match feed {
            "kalshi" => Snapshot::from_kalshi,
            "kraken-futures" => Snapshot::from_kraken,
            _ => Snapshot::from_polymarket,
        };
        Ok(raw.snapshots.iter().filter_map(parser).collect())
    }
}
