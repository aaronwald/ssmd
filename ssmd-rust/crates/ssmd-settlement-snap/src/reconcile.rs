//! Startup reconciliation backfill (Task 10).
//!
//! Placeholder wired into the run loop by Task 8; the Postgres-backed backfill
//! is implemented in Task 10.

use std::sync::Arc;

use anyhow::Result;
use deadpool_postgres::Pool;

use crate::gcs::GcsWriter;

/// Backfill missed settlements from the secmaster `markets` table. Returns the
/// number of records written. Implemented in Task 10.
pub async fn run(_pool: &Pool, _gcs: &Arc<GcsWriter>) -> Result<u64> {
    Ok(0)
}
