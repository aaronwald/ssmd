//! Active price monitoring for stop-loss orders.
//!
//! Subscribes to PriceFeed, evaluates trigger conditions on each tick,
//! and activates IOC orders when conditions are met. TP sibling is
//! cancelled before SL fires to prevent double-position.

use std::collections::HashMap;

use deadpool_postgres::Pool;
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use harman::db;
use harman::audit::AuditSender;
use harman::types::{Action, CancelReason, LegRole, Side};

use crate::price_feed::{PriceFeed, PriceTick};
use crate::runner::PumpTrigger;

/// An armed trigger being monitored.
#[derive(Debug, Clone)]
pub struct Trigger {
    pub order_id: i64,
    pub session_id: i64,
    pub group_id: i64,
    pub ticker: String,
    pub side: Side,
    pub action: Action,
    pub trigger_price: Decimal,
    pub submit_price: Decimal,
    pub quantity: Decimal,
}

/// Commands sent to PriceMonitor from other components.
#[derive(Debug)]
pub enum PriceMonitorCommand {
    /// Arm a new trigger (order entered Monitoring state).
    Arm(Trigger),
    /// Disarm a trigger (order cancelled or group completed).
    Disarm { order_id: i64 },
}

/// Cheap cloneable handle for sending commands to PriceMonitor.
#[derive(Clone)]
pub struct PriceMonitorHandle {
    tx: mpsc::UnboundedSender<PriceMonitorCommand>,
}

impl PriceMonitorHandle {
    /// Arm a trigger for a Monitoring order.
    pub fn arm(&self, trigger: Trigger) {
        let _ = self.tx.send(PriceMonitorCommand::Arm(trigger));
    }

    /// Disarm a trigger (e.g., group cancelled, sibling filled).
    pub fn disarm(&self, order_id: i64) {
        let _ = self.tx.send(PriceMonitorCommand::Disarm { order_id });
    }
}

/// Evaluate whether a trigger condition is met.
///
/// For SL on long-Yes (action=Sell, side=Yes): trigger when yes_bid <= trigger_price
/// For SL on short-Yes/long-No (action=Buy, side=Yes): trigger when yes_ask >= trigger_price
///
/// Uses executable prices (bid/ask), NOT last_price.
pub fn should_trigger(trigger: &Trigger, tick: &PriceTick) -> bool {
    match (trigger.action, trigger.side) {
        // Selling Yes (closing a long-Yes position) — trigger on bid drop
        (Action::Sell, Side::Yes) => tick
            .yes_bid_dollars()
            .map(|bid| bid <= trigger.trigger_price)
            .unwrap_or(false),
        // Buying Yes (closing a short-Yes / long-No position) — trigger on ask rise
        (Action::Buy, Side::Yes) => tick
            .yes_ask_dollars()
            .map(|ask| ask >= trigger.trigger_price)
            .unwrap_or(false),
        // Selling No — trigger on no_bid drop (yes_ask rise, since no_bid = 1 - yes_ask)
        (Action::Sell, Side::No) => tick
            .yes_ask_dollars()
            .map(|ask| {
                let no_bid = Decimal::ONE - ask;
                no_bid <= trigger.trigger_price
            })
            .unwrap_or(false),
        // Buying No — trigger on no_ask rise (yes_bid drop)
        (Action::Buy, Side::No) => tick
            .yes_bid_dollars()
            .map(|bid| {
                let no_ask = Decimal::ONE - bid;
                no_ask >= trigger.trigger_price
            })
            .unwrap_or(false),
    }
}

pub struct PriceMonitor {
    pool: Pool,
    pump_trigger: PumpTrigger,
    audit: AuditSender,
    price_feed: Box<dyn PriceFeed>,
    command_rx: mpsc::UnboundedReceiver<PriceMonitorCommand>,
}

impl PriceMonitor {
    pub fn new(
        pool: Pool,
        pump_trigger: PumpTrigger,
        audit: AuditSender,
        price_feed: Box<dyn PriceFeed>,
    ) -> (Self, PriceMonitorHandle) {
        let (tx, rx) = mpsc::unbounded_channel();
        let monitor = Self {
            pool,
            pump_trigger,
            audit,
            price_feed,
            command_rx: rx,
        };
        let handle = PriceMonitorHandle { tx };
        (monitor, handle)
    }

    pub async fn run(mut self) {
        let mut triggers: HashMap<i64, Trigger> = HashMap::new();
        let mut ticker_rx = self.price_feed.subscribe();

        info!("PriceMonitor started (0 triggers armed)");

        loop {
            tokio::select! {
                tick_result = ticker_rx.recv() => {
                    match tick_result {
                        Ok(tick) => {
                            self.evaluate_tick(&tick, &mut triggers).await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(skipped = n, "PriceMonitor lagged on price feed");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            error!("Price feed closed — PriceMonitor stopping. CRASH: armed SL triggers are now dead.");
                            std::process::exit(1);
                        }
                    }
                }
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(PriceMonitorCommand::Arm(trigger)) => {
                            info!(
                                order_id = trigger.order_id,
                                ticker = %trigger.ticker,
                                trigger_price = %trigger.trigger_price,
                                side = %trigger.side,
                                action = %trigger.action,
                                "PriceMonitor: trigger armed"
                            );
                            triggers.insert(trigger.order_id, trigger);
                        }
                        Some(PriceMonitorCommand::Disarm { order_id }) => {
                            if triggers.remove(&order_id).is_some() {
                                info!(order_id, "PriceMonitor: trigger disarmed");
                            }
                        }
                        None => {
                            info!("PriceMonitor command channel closed, stopping");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn evaluate_tick(&self, tick: &PriceTick, triggers: &mut HashMap<i64, Trigger>) {
        // Find triggers matching this ticker that fire
        let fired: Vec<i64> = triggers
            .iter()
            .filter(|(_, t)| t.ticker == tick.ticker && should_trigger(t, tick))
            .map(|(id, _)| *id)
            .collect();

        for order_id in fired {
            let trigger = match triggers.remove(&order_id) {
                Some(t) => t,
                None => continue,
            };

            let market_price = match (trigger.action, trigger.side) {
                (Action::Sell, Side::Yes) => tick.yes_bid_dollars(),
                (Action::Buy, Side::Yes) => tick.yes_ask_dollars(),
                (Action::Sell, Side::No) => {
                    tick.yes_ask_dollars().map(|a| Decimal::ONE - a)
                }
                (Action::Buy, Side::No) => {
                    tick.yes_bid_dollars().map(|b| Decimal::ONE - b)
                }
            };

            info!(
                order_id = trigger.order_id,
                ticker = %trigger.ticker,
                trigger_price = %trigger.trigger_price,
                market_price = ?market_price,
                "PriceMonitor: SL TRIGGERED — activating order"
            );

            self.audit.ws_event(
                Some(trigger.session_id),
                Some(trigger.order_id),
                "price_trigger",
                Some(serde_json::json!({
                    "trigger_price": trigger.trigger_price.to_string(),
                    "market_price": market_price.map(|p| p.to_string()),
                    "ticker": trigger.ticker,
                    "action": trigger.action.to_string(),
                    "side": trigger.side.to_string(),
                })),
                None,
            );

            // Cancel resting TP sibling first (prevent double-position)
            self.cancel_tp_sibling(trigger.group_id, trigger.session_id, trigger.order_id)
                .await;

            // Activate via existing path: Monitoring → Pending + enqueue for pump
            match db::activate_staged_order(&self.pool, trigger.order_id, trigger.session_id).await
            {
                Ok(()) => {
                    self.pump_trigger.notify(trigger.session_id);
                    info!(order_id = trigger.order_id, "SL order enqueued for pump");
                }
                Err(e) => {
                    error!(
                        order_id = trigger.order_id,
                        error = %e,
                        "failed to activate monitored order"
                    );
                }
            }
        }
    }

    /// Cancel the resting TP sibling in the same group before SL fires.
    async fn cancel_tp_sibling(&self, group_id: i64, session_id: i64, sl_order_id: i64) {
        match db::get_group_orders(&self.pool, group_id, session_id).await {
            Ok(orders) => {
                for order in orders {
                    if order.id != sl_order_id
                        && order.leg_role == Some(LegRole::TakeProfit)
                        && order.state.is_open()
                    {
                        if let Err(e) = db::atomic_cancel_order(
                            &self.pool,
                            order.id,
                            session_id,
                            &CancelReason::UserRequested,
                        )
                        .await
                        {
                            warn!(
                                order_id = order.id,
                                error = %e,
                                "failed to cancel TP sibling"
                            );
                        } else {
                            self.pump_trigger.notify(session_id);
                            info!(order_id = order.id, "TP sibling cancel enqueued");
                        }
                    }
                }
            }
            Err(e) => {
                error!(group_id, error = %e, "failed to get group orders for TP cancel");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_trigger(action: Action, side: Side, trigger_price: f64) -> Trigger {
        Trigger {
            order_id: 1,
            session_id: 1,
            group_id: 1,
            ticker: "KXTEST".into(),
            side,
            action,
            trigger_price: Decimal::try_from(trigger_price).unwrap(),
            submit_price: Decimal::try_from(trigger_price - 0.05).unwrap(),
            quantity: Decimal::new(10, 0),
        }
    }

    fn make_tick(yes_bid: Option<i64>, yes_ask: Option<i64>) -> PriceTick {
        PriceTick {
            ticker: "KXTEST".into(),
            yes_bid,
            yes_ask,
            last_price: None,
            ts: Utc::now(),
        }
    }

    // --- Sell Yes (long-Yes SL): trigger when yes_bid <= trigger_price ---

    #[test]
    fn test_sell_yes_triggers_on_bid_at_trigger() {
        let trigger = make_trigger(Action::Sell, Side::Yes, 0.40);
        let tick = make_tick(Some(40), Some(42));
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_sell_yes_triggers_on_bid_below_trigger() {
        let trigger = make_trigger(Action::Sell, Side::Yes, 0.40);
        let tick = make_tick(Some(35), Some(37));
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_sell_yes_does_not_trigger_on_bid_above() {
        let trigger = make_trigger(Action::Sell, Side::Yes, 0.40);
        let tick = make_tick(Some(45), Some(47));
        assert!(!should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_sell_yes_no_trigger_when_bid_missing() {
        let trigger = make_trigger(Action::Sell, Side::Yes, 0.40);
        let tick = make_tick(None, Some(42));
        assert!(!should_trigger(&trigger, &tick));
    }

    // --- Buy Yes (short-Yes SL): trigger when yes_ask >= trigger_price ---

    #[test]
    fn test_buy_yes_triggers_on_ask_at_trigger() {
        let trigger = make_trigger(Action::Buy, Side::Yes, 0.60);
        let tick = make_tick(Some(58), Some(60));
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_buy_yes_triggers_on_ask_above_trigger() {
        let trigger = make_trigger(Action::Buy, Side::Yes, 0.60);
        let tick = make_tick(Some(63), Some(65));
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_buy_yes_does_not_trigger_below() {
        let trigger = make_trigger(Action::Buy, Side::Yes, 0.60);
        let tick = make_tick(Some(53), Some(55));
        assert!(!should_trigger(&trigger, &tick));
    }

    // --- Sell No: trigger when no_bid <= trigger_price (i.e., yes_ask >= 1 - trigger_price) ---

    #[test]
    fn test_sell_no_triggers_when_no_bid_drops() {
        // trigger_price = 0.40 for No side, no_bid = 1 - yes_ask
        // If yes_ask = 62, no_bid = 0.38 <= 0.40 → trigger
        let trigger = make_trigger(Action::Sell, Side::No, 0.40);
        let tick = make_tick(Some(58), Some(62));
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_sell_no_does_not_trigger_above() {
        // If yes_ask = 55, no_bid = 0.45 > 0.40 → no trigger
        let trigger = make_trigger(Action::Sell, Side::No, 0.40);
        let tick = make_tick(Some(53), Some(55));
        assert!(!should_trigger(&trigger, &tick));
    }

    // --- Buy No: trigger when no_ask >= trigger_price (i.e., yes_bid <= 1 - trigger_price) ---

    #[test]
    fn test_buy_no_triggers_when_no_ask_rises() {
        // trigger_price = 0.60, no_ask = 1 - yes_bid
        // If yes_bid = 38, no_ask = 0.62 >= 0.60 → trigger
        let trigger = make_trigger(Action::Buy, Side::No, 0.60);
        let tick = make_tick(Some(38), Some(42));
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_buy_no_does_not_trigger_below() {
        // If yes_bid = 45, no_ask = 0.55 < 0.60 → no trigger
        let trigger = make_trigger(Action::Buy, Side::No, 0.60);
        let tick = make_tick(Some(45), Some(47));
        assert!(!should_trigger(&trigger, &tick));
    }

    // --- Edge cases ---

    #[test]
    fn test_wrong_ticker_does_not_trigger() {
        let trigger = make_trigger(Action::Sell, Side::Yes, 0.40);
        let tick = PriceTick {
            ticker: "OTHER-TICKER".into(),
            yes_bid: Some(30),
            yes_ask: Some(32),
            last_price: None,
            ts: Utc::now(),
        };
        // should_trigger doesn't check ticker — caller filters by ticker first
        // This test verifies the pure price logic still works regardless of ticker
        assert!(should_trigger(&trigger, &tick));
    }

    #[test]
    fn test_boundary_exact_price() {
        // Trigger at exactly 0.50 with bid at exactly 50 cents
        let trigger = make_trigger(Action::Sell, Side::Yes, 0.50);
        let tick = make_tick(Some(50), Some(52));
        assert!(should_trigger(&trigger, &tick)); // <= is inclusive
    }
}
