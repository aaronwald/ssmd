//! EMS-level risk checks.
//!
//! Currently, execution risk (max_notional per order, fat finger) is handled
//! inside `harman::db::enqueue_order`. This module is a placeholder for
//! EMS-specific risk logic that may be added later (e.g., rate limit
//! awareness, exchange-specific order size limits).
