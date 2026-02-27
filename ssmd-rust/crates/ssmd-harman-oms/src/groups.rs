use harman::db;
use harman::error::EnqueueError;
use harman::state::OrderState;
use harman::types::{
    GroupState, GroupType, LegRole, Order, OrderGroup, OrderRequest,
};
use tracing::{debug, info, warn};

use crate::Oms;

impl Oms {
    /// Create a bracket order group: entry + take_profit + stop_loss.
    ///
    /// Entry is queued immediately (Pending). TP and SL are Staged until
    /// entry fills, at which point they are activated by trigger evaluation.
    pub async fn create_bracket(
        &self,
        session_id: i64,
        entry: OrderRequest,
        take_profit: OrderRequest,
        stop_loss: OrderRequest,
    ) -> Result<(OrderGroup, Vec<Order>), EnqueueError> {
        let legs = vec![
            (entry, LegRole::Entry, OrderState::Pending),
            (take_profit, LegRole::TakeProfit, OrderState::Staged),
            (stop_loss, LegRole::StopLoss, OrderState::Staged),
        ];

        db::create_order_group(
            &self.pool,
            session_id,
            GroupType::Bracket,
            &legs,
            &self.ems.risk_limits,
        )
        .await
    }

    /// Create an OCO (one-cancels-other) order group.
    ///
    /// Both legs are queued immediately (Pending). When one fills,
    /// the other is cancelled by trigger evaluation.
    pub async fn create_oco(
        &self,
        session_id: i64,
        leg1: OrderRequest,
        leg2: OrderRequest,
    ) -> Result<(OrderGroup, Vec<Order>), EnqueueError> {
        let legs = vec![
            (leg1, LegRole::OcoLeg, OrderState::Pending),
            (leg2, LegRole::OcoLeg, OrderState::Pending),
        ];

        db::create_order_group(
            &self.pool,
            session_id,
            GroupType::Oco,
            &legs,
            &self.ems.risk_limits,
        )
        .await
    }

    /// Evaluate group triggers after a pump cycle.
    ///
    /// Returns the number of orders activated. Caller should re-pump if > 0.
    pub async fn evaluate_triggers(&self, session_id: i64) -> Result<u32, String> {
        let groups = db::get_groups_needing_evaluation(&self.pool, session_id).await?;
        let mut activated = 0u32;

        for (group, orders) in groups {
            match group.group_type {
                GroupType::Bracket => {
                    activated += self.evaluate_bracket(&group, &orders, session_id).await?;
                }
                GroupType::Oco => {
                    activated += self.evaluate_oco(&group, &orders, session_id).await?;
                }
            }
        }

        Ok(activated)
    }

    /// Cancel all legs of a group.
    ///
    /// Staged legs are cancelled directly (no exchange call needed).
    /// Open legs are cancelled via the EMS cancel flow.
    pub async fn cancel_group(&self, group_id: i64, session_id: i64) -> Result<(), String> {
        let orders = db::get_group_orders(&self.pool, group_id, session_id).await?;

        for order in &orders {
            if order.state == OrderState::Staged {
                // Staged → Cancelled directly (no exchange involvement)
                db::update_order_state(
                    &self.pool,
                    order.id,
                    session_id,
                    OrderState::Cancelled,
                    None,
                    None,
                    Some(&harman::types::CancelReason::UserRequested),
                    "group_cancel",
                )
                .await?;
            } else if order.state.is_open() {
                // Open on exchange → enqueue cancel
                let _ = self
                    .ems
                    .enqueue_cancel(
                        order.id,
                        session_id,
                        &harman::types::CancelReason::UserRequested,
                    )
                    .await;
            }
            // Terminal orders are left as-is
        }

        db::update_group_state(&self.pool, group_id, GroupState::Cancelled).await?;
        info!(group_id, "group cancelled");
        Ok(())
    }

    /// Evaluate a bracket group's triggers.
    async fn evaluate_bracket(
        &self,
        group: &OrderGroup,
        orders: &[Order],
        session_id: i64,
    ) -> Result<u32, String> {
        let entry = orders.iter().find(|o| o.leg_role == Some(LegRole::Entry));
        let exits: Vec<&Order> = orders
            .iter()
            .filter(|o| {
                o.leg_role == Some(LegRole::TakeProfit)
                    || o.leg_role == Some(LegRole::StopLoss)
            })
            .collect();

        let entry = match entry {
            Some(e) => e,
            None => {
                warn!(group_id = group.id, "bracket group missing entry leg");
                return Ok(0);
            }
        };

        let mut activated = 0u32;

        if entry.state == OrderState::Filled {
            // Entry filled → activate staged exit legs
            for exit in &exits {
                if exit.state == OrderState::Staged {
                    db::activate_staged_order(&self.pool, exit.id, session_id).await?;
                    activated += 1;
                    debug!(order_id = exit.id, group_id = group.id, "exit leg activated");
                }
            }

            // Check if an exit leg has already filled
            let exit_filled = exits.iter().any(|o| o.state == OrderState::Filled);
            if exit_filled {
                // Cancel the other exit leg(s) and complete the group
                for exit in &exits {
                    if exit.state == OrderState::Staged {
                        db::update_order_state(
                            &self.pool,
                            exit.id,
                            session_id,
                            OrderState::Cancelled,
                            None,
                            None,
                            Some(&harman::types::CancelReason::UserRequested),
                            "trigger",
                        )
                        .await?;
                    } else if exit.state.is_open() {
                        let _ = self
                            .ems
                            .enqueue_cancel(
                                exit.id,
                                session_id,
                                &harman::types::CancelReason::UserRequested,
                            )
                            .await;
                    }
                }
                db::update_group_state(&self.pool, group.id, GroupState::Completed).await?;
                info!(group_id = group.id, "bracket group completed (exit filled)");
            }
        } else if entry.state.is_terminal() {
            // Entry rejected/cancelled/expired → cancel staged exit legs, mark group cancelled
            for exit in &exits {
                if exit.state == OrderState::Staged {
                    db::update_order_state(
                        &self.pool,
                        exit.id,
                        session_id,
                        OrderState::Cancelled,
                        None,
                        None,
                        Some(&harman::types::CancelReason::UserRequested),
                        "trigger",
                    )
                    .await?;
                }
            }
            db::update_group_state(&self.pool, group.id, GroupState::Cancelled).await?;
            info!(group_id = group.id, "bracket group cancelled (entry terminal)");
        }

        // Check if all legs are terminal → finalize
        self.maybe_finalize_group(group, orders).await?;

        Ok(activated)
    }

    /// Evaluate an OCO group's triggers.
    async fn evaluate_oco(
        &self,
        group: &OrderGroup,
        orders: &[Order],
        session_id: i64,
    ) -> Result<u32, String> {
        let filled_leg = orders.iter().find(|o| o.state == OrderState::Filled);

        if let Some(_filled) = filled_leg {
            // One leg filled → cancel all other non-terminal legs
            for order in orders {
                if !order.state.is_terminal() && order.state.is_open() {
                    let _ = self
                        .ems
                        .enqueue_cancel(
                            order.id,
                            session_id,
                            &harman::types::CancelReason::UserRequested,
                        )
                        .await;
                }
            }
            db::update_group_state(&self.pool, group.id, GroupState::Completed).await?;
            info!(group_id = group.id, "OCO group completed (leg filled)");
        }

        // Check if all legs are terminal → finalize
        self.maybe_finalize_group(group, orders).await?;

        Ok(0) // OCO never activates staged orders
    }

    /// If all legs are terminal, finalize the group state.
    async fn maybe_finalize_group(
        &self,
        group: &OrderGroup,
        orders: &[Order],
    ) -> Result<(), String> {
        if group.state != GroupState::Active {
            return Ok(());
        }

        let all_terminal = orders.iter().all(|o| o.state.is_terminal());
        if !all_terminal {
            return Ok(());
        }

        // Determine final state: completed if any filled, cancelled otherwise
        let any_filled = orders.iter().any(|o| o.state == OrderState::Filled);
        let final_state = if any_filled {
            GroupState::Completed
        } else {
            GroupState::Cancelled
        };

        db::update_group_state(&self.pool, group.id, final_state).await?;
        debug!(group_id = group.id, state = %final_state, "group finalized");
        Ok(())
    }
}
