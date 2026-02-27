use harman::db;
use harman::error::EnqueueError;
use harman::types::{CancelReason, Order, OrderRequest};
use rust_decimal::Decimal;

use crate::Ems;

impl Ems {
    /// Enqueue a new order (risk check + atomic DB insert + queue item).
    pub async fn enqueue(
        &self,
        session_id: i64,
        request: &OrderRequest,
    ) -> Result<Order, EnqueueError> {
        db::enqueue_order(&self.pool, request, session_id, &self.risk_limits).await
    }

    /// Enqueue a cancel action for an existing order.
    pub async fn enqueue_cancel(
        &self,
        order_id: i64,
        session_id: i64,
        cancel_reason: &CancelReason,
    ) -> Result<(), String> {
        db::atomic_cancel_order(&self.pool, order_id, session_id, cancel_reason).await
    }

    /// Enqueue an amend action for an existing order.
    pub async fn enqueue_amend(
        &self,
        order_id: i64,
        session_id: i64,
        new_price_dollars: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> Result<(), String> {
        db::atomic_amend_order(&self.pool, order_id, session_id, new_price_dollars, new_quantity)
            .await
    }

    /// Enqueue a decrease action for an existing order.
    pub async fn enqueue_decrease(
        &self,
        order_id: i64,
        session_id: i64,
        reduce_by: Decimal,
    ) -> Result<(), String> {
        db::atomic_decrease_order(&self.pool, order_id, session_id, reduce_by).await
    }
}
