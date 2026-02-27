use serde::Serialize;

use harman::db;
use harman::db::LocalPosition;
use harman::types::Position;

use crate::Oms;

#[derive(Debug, Serialize)]
pub struct PositionsView {
    pub exchange: Vec<Position>,
    pub local: Vec<LocalPosition>,
}

/// Fetch both exchange and local positions for a session.
pub async fn positions(oms: &Oms, session_id: i64) -> Result<PositionsView, String> {
    let exchange = oms
        .exchange
        .get_positions()
        .await
        .map_err(|e| format!("get positions: {}", e))?;

    let local = db::compute_local_positions(&oms.pool, session_id).await?;

    Ok(PositionsView { exchange, local })
}
