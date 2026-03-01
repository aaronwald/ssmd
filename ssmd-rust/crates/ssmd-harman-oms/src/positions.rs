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

/// Per-session positions breakdown for admin view
#[derive(Debug, Serialize)]
pub struct SessionPositions {
    pub session_id: i64,
    pub positions: Vec<LocalPosition>,
}

/// All-sessions positions view: exchange-wide + per-session breakdown + aggregate local
#[derive(Debug, Serialize)]
pub struct AllPositionsView {
    pub exchange: Vec<Position>,
    pub aggregate_local: Vec<LocalPosition>,
    pub sessions: Vec<SessionPositions>,
}

/// Fetch exchange positions + per-session breakdown for all active sessions.
pub async fn all_positions(
    oms: &Oms,
    exchange_type: &str,
    environment: &str,
) -> Result<AllPositionsView, String> {
    let exchange = oms
        .exchange
        .get_positions()
        .await
        .map_err(|e| format!("get positions: {}", e))?;

    let aggregate_local =
        db::compute_all_local_positions(&oms.pool, exchange_type, environment).await?;

    let session_ids =
        db::list_active_session_ids(&oms.pool, exchange_type, environment).await?;

    let mut sessions = Vec::with_capacity(session_ids.len());
    for sid in session_ids {
        let local = db::compute_local_positions(&oms.pool, sid).await?;
        if !local.is_empty() {
            sessions.push(SessionPositions {
                session_id: sid,
                positions: local,
            });
        }
    }

    Ok(AllPositionsView {
        exchange,
        aggregate_local,
        sessions,
    })
}
