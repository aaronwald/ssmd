// ssmd-harman/src/pump.rs -- now delegates to EMS crate
pub use ssmd_harman_ems::pump::PumpResult;

use crate::AppState;

pub async fn pump(state: &AppState, session_id: i64) -> PumpResult {
    state.ems.pump(session_id).await
}
