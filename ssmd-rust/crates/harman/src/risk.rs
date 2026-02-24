use rust_decimal::Decimal;

use crate::error::RiskCheckError;
use crate::types::OrderRequest;

/// Risk limits configuration
#[derive(Debug, Clone)]
pub struct RiskLimits {
    /// Maximum total notional exposure in dollars
    pub max_notional: Decimal,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_notional: Decimal::new(100, 0), // $100 default
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

    fn make_order(quantity: i32, price_cents: i32) -> OrderRequest {
        make_order_with_side_action(quantity, price_cents, Side::Yes, Action::Buy)
    }

    fn make_order_with_side_action(
        quantity: i32,
        price_cents: i32,
        side: Side,
        action: Action,
    ) -> OrderRequest {
        OrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: "KXTEST-123".to_string(),
            side,
            action,
            quantity,
            price_cents,
            time_in_force: TimeInForce::Gtc,
        }
    }

    // ======================================================================
    // Basic pass/fail
    // ======================================================================

    #[test]
    fn test_order_passes_risk_check() {
        let state = RiskState::default();
        let limits = RiskLimits::default(); // $100
        let order = make_order(10, 50); // $5.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_order_exactly_at_limit() {
        let state = RiskState::default();
        let limits = RiskLimits::default(); // $100
        let order = make_order(100, 100); // $100.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_order_exceeds_limit() {
        let state = RiskState::default();
        let limits = RiskLimits::default(); // $100
        let order = make_order(101, 100); // $101.00
        let err = state.check_order(&order, &limits).unwrap_err();
        match err {
            RiskCheckError::MaxNotionalExceeded {
                current,
                requested,
                limit,
            } => {
                assert_eq!(current, Decimal::ZERO);
                assert_eq!(requested, Decimal::new(10100, 2));
                assert_eq!(limit, Decimal::new(100, 0));
            }
        }
    }

    // ======================================================================
    // Boundary / edge cases
    // ======================================================================

    #[test]
    fn test_minimum_notional_one_contract_one_cent() {
        // Smallest possible order: 1 contract at 1 cent = $0.01
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(1, 1);
        assert!(state.check_order(&order, &limits).is_ok());
        assert_eq!(order.notional(), Decimal::new(1, 2)); // $0.01
    }

    #[test]
    fn test_maximum_price_99_cents() {
        // Kalshi max: 99 cents per contract
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(1, 99); // $0.99
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
        let order = make_order(1, 2); // $0.02
        assert!(state.check_order(&order, &limits).is_err());
    }

    #[test]
    fn test_one_cent_under_limit() {
        // $99.99 existing + $0.01 new = $100.00 → pass
        let state = RiskState {
            open_notional: Decimal::new(9999, 2), // $99.99
        };
        let limits = RiskLimits::default();
        let order = make_order(1, 1); // $0.01
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_accumulation_to_exact_limit() {
        // Simulate multiple orders accumulating to exactly $100
        let limits = RiskLimits::default();

        // Start at $0
        let state = RiskState::default();
        let order1 = make_order(50, 50); // $25.00
        assert!(state.check_order(&order1, &limits).is_ok());

        // Now at $25
        let state = RiskState {
            open_notional: Decimal::new(25, 0),
        };
        let order2 = make_order(50, 50); // $25.00
        assert!(state.check_order(&order2, &limits).is_ok());

        // Now at $50
        let state = RiskState {
            open_notional: Decimal::new(50, 0),
        };
        let order3 = make_order(100, 50); // $50.00 → total $100 = limit
        assert!(state.check_order(&order3, &limits).is_ok());

        // Now at $100 — any additional should fail
        let state = RiskState {
            open_notional: Decimal::new(100, 0),
        };
        let order4 = make_order(1, 1); // $0.01
        assert!(state.check_order(&order4, &limits).is_err());
    }

    #[test]
    fn test_different_combos_same_notional() {
        // Different price/quantity combos yielding the same $10 notional
        let state = RiskState::default();
        let limits = RiskLimits::default();

        let a = make_order(10, 100); // 10 contracts * $1.00 = $10
        let b = make_order(100, 10); // 100 contracts * $0.10 = $10
        let c = make_order(20, 50); // 20 contracts * $0.50 = $10

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
        let order = make_order(0, 50); // 0 contracts = $0
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_zero_limit_rejects_everything() {
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::ZERO,
        };
        // Even the smallest order should fail
        let order = make_order(1, 1); // $0.01
        assert!(state.check_order(&order, &limits).is_err());
    }

    #[test]
    fn test_zero_limit_allows_zero_notional() {
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::ZERO,
        };
        let order = make_order(0, 50); // $0.00
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
        let order = make_order_with_side_action(10, 50, Side::No, Action::Buy);
        assert_eq!(order.notional(), Decimal::new(500, 2)); // $5.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_sell_action_order_risk() {
        // Sell orders also consume risk
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order_with_side_action(10, 50, Side::Yes, Action::Sell);
        assert_eq!(order.notional(), Decimal::new(500, 2)); // $5.00
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_no_side_sell_exceeds_limit() {
        let state = RiskState {
            open_notional: Decimal::new(96, 0), // $96
        };
        let limits = RiskLimits::default();
        let order = make_order_with_side_action(10, 50, Side::No, Action::Sell); // $5
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
        let small_order = make_order(8, 50); // $4.00
        assert!(state.check_order(&small_order, &limits).is_ok());

        // $6 should fail (95 + 6 = 101)
        let big_order = make_order(12, 50); // $6.00
        assert!(state.check_order(&big_order, &limits).is_err());
    }

    #[test]
    fn test_custom_risk_limit() {
        let state = RiskState::default();
        let limits = RiskLimits {
            max_notional: Decimal::new(50, 0), // $50
        };
        let order = make_order(100, 51); // $51.00
        assert!(state.check_order(&order, &limits).is_err());
    }

    #[test]
    fn test_risk_at_boundary_with_existing() {
        let state = RiskState {
            open_notional: Decimal::new(99, 0), // $99
        };
        let limits = RiskLimits::default(); // $100

        // $1.00 should pass (99 + 1 = 100)
        let order = make_order(1, 100); // $1.00
        assert!(state.check_order(&order, &limits).is_ok());

        // $1.01 should fail (99 + 1.01 = 100.01)
        let order2 = make_order(1, 101); // $1.01
        assert!(state.check_order(&order2, &limits).is_err());
    }

    #[test]
    fn test_large_quantity_small_price() {
        // 1000 contracts at 1 cent = $10
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(1000, 1);
        assert_eq!(order.notional(), Decimal::new(10, 0)); // $10
        assert!(state.check_order(&order, &limits).is_ok());
    }

    #[test]
    fn test_large_quantity_small_price_exceeds_limit() {
        // 10001 contracts at 1 cent = $100.01 → exceeds $100 limit
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(10001, 1);
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
        };
        let order = make_order(20, 60); // $12.00 → $80 + $12 = $92 > $90

        let err = state.check_order(&order, &limits).unwrap_err();
        match err {
            RiskCheckError::MaxNotionalExceeded {
                current,
                requested,
                limit,
            } => {
                assert_eq!(current, Decimal::new(80, 0));
                assert_eq!(requested, Decimal::new(1200, 2)); // $12.00
                assert_eq!(limit, Decimal::new(90, 0));
            }
        }
    }
}
