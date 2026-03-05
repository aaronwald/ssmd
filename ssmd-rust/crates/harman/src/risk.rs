use rust_decimal::Decimal;

use crate::error::RiskCheckError;
use crate::types::OrderRequest;

/// Risk limits configuration
#[derive(Debug, Clone)]
pub struct RiskLimits {
    /// Maximum total notional exposure in dollars
    pub max_notional: Decimal,
    /// Maximum notional for a single order (fat-finger protection)
    pub max_order_notional: Decimal,
    /// Maximum daily realized loss in dollars (positive number, e.g., 50 = -$50 threshold)
    pub daily_loss_limit: Decimal,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_notional: Decimal::new(100, 0),       // $100 default
            max_order_notional: Decimal::new(25, 0),   // $25 default
            daily_loss_limit: Decimal::new(50, 0),     // $50 default
        }
    }
}

/// Current risk state computed from open orders
#[derive(Debug, Clone)]
pub struct RiskState {
    /// Sum of notional for all open orders
    pub open_notional: Decimal,
}

impl Default for RiskState {
    fn default() -> Self {
        Self {
            open_notional: Decimal::ZERO,
        }
    }
}

impl RiskState {
    /// Check whether a new order passes risk limits
    pub fn check_order(
        &self,
        order: &OrderRequest,
        limits: &RiskLimits,
    ) -> Result<(), RiskCheckError> {
        let requested = order.notional();

        // Fat-finger: single order notional cap
        if requested > limits.max_order_notional {
            return Err(RiskCheckError::MaxOrderNotionalExceeded {
                order_notional: requested,
                limit: limits.max_order_notional,
            });
        }

        // Aggregate: total open notional cap
        let total = self.open_notional + requested;
        if total > limits.max_notional {
            return Err(RiskCheckError::MaxNotionalExceeded {
                current: self.open_notional,
                requested,
                limit: limits.max_notional,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, Side, TimeInForce};
    use uuid::Uuid;

    fn make_order(quantity: Decimal, price_dollars: Decimal) -> OrderRequest {
        make_order_with_side_action(quantity, price_dollars, Side::Yes, Action::Buy)
    }

    fn make_order_with_side_action(
        quantity: Decimal,
        price_dollars: Decimal,
        side: Side,
        action: Action,
    ) -> OrderRequest {
        OrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: "KXTEST-123".to_string(),
            side,
            action,
            quantity,
            price_dollars,
            time_in_force: TimeInForce::Gtc,
        }
    }

    /// Limits with high max_order_notional — for testing aggregate checks only
    fn aggregate_limits() -> RiskLimits {
        RiskLimits {
            max_order_notional: Decimal::new(10000, 0), // $10k — effectively no fat-finger
            ..RiskLimits::default()
        }
    }

    // ======================================================================
    // Basic pass/fail
    // ======================================================================

    #[test]
    fn test_order_passes_risk_check() {
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(10), Decimal::new(50, 2)); // $5.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_order_exactly_at_limit() {
        let state = RiskState::default();
        let limits = aggregate_limits();
        let order = make_order(Decimal::from(100), Decimal::new(100, 2)); // $100.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_order_exceeds_limit() {
        let state = RiskState::default();
        let limits = aggregate_limits();
        let order = make_order(Decimal::from(101), Decimal::new(100, 2)); // $101.00
        let err = state.check_order(&order, &limits).unwrap_err();
        assert!(matches!(err, RiskCheckError::MaxNotionalExceeded { .. }));
    }

    // ======================================================================
    // Boundary / edge cases
    // ======================================================================

    #[test]
    fn test_minimum_notional_one_contract_one_cent() {
        // Smallest possible order: 1 contract at 1 cent = $0.01
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(1), Decimal::new(1, 2));
        assert!(state.check_order(&order, &limits).is_ok());
        assert_eq!(order.notional(), Decimal::new(1, 2)); // $0.01
    }

    #[test]
    fn test_maximum_price_99_cents() {
        // Kalshi max: 99 cents per contract
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(1), Decimal::new(99, 2)); // $0.99
        assert!(state.check_order(&order, &limits).is_ok());
        assert_eq!(order.notional(), Decimal::new(99, 2));
    }

    #[test]
    fn test_one_cent_over_limit() {
        // $99.99 existing + $0.02 new = $100.01 → reject
        let state = RiskState {
            open_notional: Decimal::new(9999, 2), // $99.99
        };
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(1), Decimal::new(2, 2)); // $0.02
        assert!(state.check_order(&order, &limits).is_err());
    }

    #[test]
    fn test_one_cent_under_limit() {
        // $99.99 existing + $0.01 new = $100.00 → pass
        let state = RiskState {
            open_notional: Decimal::new(9999, 2), // $99.99
        };
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(1), Decimal::new(1, 2)); // $0.01
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_accumulation_to_exact_limit() {
        // Simulate multiple orders accumulating to exactly $100
        let limits = aggregate_limits();

        // Start at $0
        let state = RiskState::default();
        let order1 = make_order(Decimal::from(50), Decimal::new(50, 2)); // $25.00
        assert!(state.check_order(&order1, &limits).is_ok());

        // Now at $25
        let state = RiskState {
            open_notional: Decimal::new(25, 0),
        };
        let order2 = make_order(Decimal::from(50), Decimal::new(50, 2)); // $25.00
        assert!(state.check_order(&order2, &limits).is_ok());

        // Now at $50
        let state = RiskState {
            open_notional: Decimal::new(50, 0),
        };
        let order3 = make_order(Decimal::from(100), Decimal::new(50, 2)); // $50.00 → total $100 = limit
        assert!(state.check_order(&order3, &limits).is_ok());

        // Now at $100 — any additional should fail
        let state = RiskState {
            open_notional: Decimal::new(100, 0),
        };
        let order4 = make_order(Decimal::from(1), Decimal::new(1, 2)); // $0.01
        assert!(state.check_order(&order4, &limits).is_err());
    }

    #[test]
    fn test_different_combos_same_notional() {
        // Different price/quantity combos yielding the same $10 notional
        let state = RiskState::default();
        let limits = RiskLimits::default();

        let a = make_order(Decimal::from(10), Decimal::new(100, 2)); // 10 contracts * $1.00 = $10
        let b = make_order(Decimal::from(100), Decimal::new(10, 2)); // 100 contracts * $0.10 = $10
        let c = make_order(Decimal::from(20), Decimal::new(50, 2)); // 20 contracts * $0.50 = $10

        assert_eq!(a.notional(), b.notional());
        assert_eq!(b.notional(), c.notional());
        assert!(state.check_order(&a, &limits).is_ok());
        assert!(state.check_order(&b, &limits).is_ok());
        assert!(state.check_order(&c, &limits).is_ok());
    }

    #[test]
    fn test_zero_quantity_order() {
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(0), Decimal::new(50, 2)); // 0 contracts = $0
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_zero_limit_rejects_everything() {
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::ZERO,
            ..RiskLimits::default()
        };
        // Even the smallest order should fail
        let order = make_order(Decimal::from(1), Decimal::new(1, 2)); // $0.01
        assert!(state.check_order(&order, &limits).is_err());
    }

    #[test]
    fn test_zero_limit_allows_zero_notional() {
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::ZERO,
            ..RiskLimits::default()
        };
        let order = make_order(Decimal::from(0), Decimal::new(50, 2)); // $0.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    // ======================================================================
    // Side / action variations (risk applies equally)
    // ======================================================================

    #[test]
    fn test_no_side_order_risk() {
        // No-side orders contribute same notional as yes-side
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order_with_side_action(Decimal::from(10), Decimal::new(50, 2), Side::No, Action::Buy);
        assert_eq!(order.notional(), Decimal::new(500, 2)); // $5.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_sell_action_order_risk() {
        // Sell orders also consume risk
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order_with_side_action(Decimal::from(10), Decimal::new(50, 2), Side::Yes, Action::Sell);
        assert_eq!(order.notional(), Decimal::new(500, 2)); // $5.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_no_side_sell_exceeds_limit() {
        let state = RiskState {
            open_notional: Decimal::new(96, 0), // $96
        };
        let limits = RiskLimits::default();
        let order = make_order_with_side_action(Decimal::from(10), Decimal::new(50, 2), Side::No, Action::Sell); // $5
        assert!(state.check_order(&order, &limits).is_err());
    }

    // ======================================================================
    // Cumulative / existing exposure
    // ======================================================================

    #[test]
    fn test_cumulative_risk_check() {
        let state = RiskState {
            open_notional: Decimal::new(95, 0), // $95 already open
        };
        let limits = RiskLimits::default(); // $100

        // $4 should pass (95 + 4 = 99)
        let small_order = make_order(Decimal::from(8), Decimal::new(50, 2)); // $4.00
        assert!(state.check_order(&small_order, &limits).is_ok());

        // $6 should fail (95 + 6 = 101)
        let big_order = make_order(Decimal::from(12), Decimal::new(50, 2)); // $6.00
        assert!(state.check_order(&big_order, &limits).is_err());
    }

    #[test]
    fn test_custom_risk_limit() {
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::new(50, 0), // $50
            max_order_notional: Decimal::new(10000, 0),
            ..RiskLimits::default()
        };
        let order = make_order(Decimal::from(100), Decimal::new(51, 2)); // $51.00
        assert!(state.check_order(&order, &limits).is_err());
    }

    #[test]
    fn test_risk_at_boundary_with_existing() {
        let state = RiskState {
            open_notional: Decimal::new(99, 0), // $99
        };
        let limits = RiskLimits::default(); // $100

        // $1.00 should pass (99 + 1 = 100)
        let order = make_order(Decimal::from(1), Decimal::new(100, 2)); // $1.00
        assert!(state.check_order(&order, &limits).is_ok());

        // $1.01 should fail (99 + 1.01 = 100.01)
        let order2 = make_order(Decimal::from(1), Decimal::new(101, 2)); // $1.01
        assert!(state.check_order(&order2, &limits).is_err());
    }

    #[test]
    fn test_large_quantity_small_price() {
        // 1000 contracts at 1 cent = $10
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(Decimal::from(1000), Decimal::new(1, 2));
        assert_eq!(order.notional(), Decimal::new(10, 0)); // $10
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_large_quantity_small_price_exceeds_limit() {
        // 10001 contracts at 1 cent = $100.01 → exceeds $100 limit
        let state = RiskState::default();
        let limits = aggregate_limits();
        let order = make_order(Decimal::from(10001), Decimal::new(1, 2));
        assert!(state.check_order(&order, &limits).is_err());
    }

    // ======================================================================
    // Error message validation
    // ======================================================================

    #[test]
    fn test_error_includes_correct_values() {
        let state = RiskState {
            open_notional: Decimal::new(80, 0), // $80
        };
        let limits = RiskLimits {
            max_notional: Decimal::new(90, 0), // $90
            max_order_notional: Decimal::new(10000, 0),
            ..RiskLimits::default()
        };
        let order = make_order(Decimal::from(20), Decimal::new(60, 2)); // $12.00 → $80 + $12 = $92 > $90

        let err = state.check_order(&order, &limits).unwrap_err();
        assert!(matches!(err, RiskCheckError::MaxNotionalExceeded { .. }));
    }

    // ======================================================================
    // Fat-finger (max order notional) checks
    // ======================================================================

    #[test]
    fn test_fat_finger_under_limit() {
        let state = RiskState::default();
        let limits = RiskLimits::default(); // $25 max_order_notional
        let order = make_order(Decimal::from(40), Decimal::new(50, 2)); // $20.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_fat_finger_at_limit() {
        let state = RiskState::default();
        let limits = RiskLimits::default(); // $25 max_order_notional
        let order = make_order(Decimal::from(50), Decimal::new(50, 2)); // $25.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_fat_finger_over_limit() {
        let state = RiskState::default();
        let limits = RiskLimits::default(); // $25 max_order_notional
        let order = make_order(Decimal::from(100), Decimal::new(50, 2)); // $50.00
        let err = state.check_order(&order, &limits).unwrap_err();
        assert!(matches!(err, RiskCheckError::MaxOrderNotionalExceeded { .. }));
    }

    #[test]
    fn test_fat_finger_checked_before_aggregate() {
        // Even with zero existing notional, fat-finger rejects first
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::new(1000, 0), // high aggregate
            max_order_notional: Decimal::new(10, 0), // low per-order
            ..RiskLimits::default()
        };
        let order = make_order(Decimal::from(20), Decimal::new(60, 2)); // $12.00 > $10
        let err = state.check_order(&order, &limits).unwrap_err();
        assert!(matches!(err, RiskCheckError::MaxOrderNotionalExceeded { .. }));
    }
}
