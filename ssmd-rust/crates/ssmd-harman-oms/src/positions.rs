use serde::Serialize;

use harman::db;
use harman::db::LocalPosition;

use crate::Oms;

#[derive(Debug, Serialize)]
pub struct PositionsView {
    pub positions: Vec<LocalPosition>,
}

/// Compute positions for a session from fills in the DB.
pub async fn positions(oms: &Oms, session_id: i64) -> Result<PositionsView, String> {
    let positions = db::compute_local_positions(&oms.pool, session_id).await?;
    Ok(PositionsView { positions })
}

/// Per-session positions breakdown for admin view
#[derive(Debug, Serialize)]
pub struct SessionPositions {
    pub session_id: i64,
    pub positions: Vec<LocalPosition>,
}

/// All-sessions positions view: aggregate + per-session breakdown
#[derive(Debug, Serialize)]
pub struct AllPositionsView {
    pub aggregate: Vec<LocalPosition>,
    pub sessions: Vec<SessionPositions>,
}

/// Compute positions for all active sessions from fills in the DB.
pub async fn all_positions(
    oms: &Oms,
    exchange_type: &str,
    environment: &str,
) -> Result<AllPositionsView, String> {
    let aggregate =
        db::compute_all_local_positions(&oms.pool, exchange_type, environment).await?;

    let session_ids =
        db::list_session_ids(&oms.pool, exchange_type, environment).await?;

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
        aggregate,
        sessions,
    })
}
