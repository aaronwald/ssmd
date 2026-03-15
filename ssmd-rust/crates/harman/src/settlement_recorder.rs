use tracing::info;

use deadpool_postgres::Pool;

use crate::audit::AuditSender;
use crate::db;
use crate::types::ExchangeSettlement;

/// Record settlements from the exchange into the database.
///
/// Each settlement is recorded idempotently via `ON CONFLICT DO NOTHING`.
/// Returns the count of newly inserted settlement records.
pub async fn record_settlements(
    pool: &Pool,
    session_id: i64,
    settlements: &[ExchangeSettlement],
    actor: &str,
    audit: Option<&AuditSender>,
) -> Result<u64, String> {
    let mut count = 0u64;

    for settlement in settlements {
        let inserted = db::record_settlement(pool, session_id, settlement).await?;
        if inserted {
            info!(
                ticker = %settlement.ticker,
                event_ticker = %settlement.event_ticker,
                market_result = %settlement.market_result,
                revenue_cents = settlement.revenue_cents,
                settled_time = %settlement.settled_time,
                actor,
                "recorded settlement"
            );
            if let Some(audit) = audit {
                audit.ws_event(
                    Some(session_id),
                    None,
                    "settlement_recorded",
                    Some(serde_json::json!({
                        "ticker": settlement.ticker,
                        "market_result": format!("{}", settlement.market_result),
                        "revenue_cents": settlement.revenue_cents,
                        "settled_time": settlement.settled_time.to_rfc3339(),
                    })),
                    Some(serde_json::json!({"actor": actor})),
                );
            }
            count += 1;
        }
    }

    if count > 0 {
        info!(count, actor, "discovered new settlements");
    }

    Ok(count)
}
