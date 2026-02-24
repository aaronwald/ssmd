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
        OrderRequest {
            client_order_id: Uuid::new_v4(),
            ticker: "KXTEST-123".to_string(),
            side: Side::Yes,
            action: Action::Buy,
            quantity,
            price_cents,
            time_in_force: TimeInForce::Gtc,
        }
    }

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

    #[test]
    fn test_cumulative_risk_check() {
        let state = RiskState {
            open_notional: Decimal::new(95, 0), // $95 already open
        };
        let limits = RiskLimits::default(); // $100

        // $4 should pass
        let small_order = make_order(8, 50); // $4.00
        assert!(state.check_order(&small_order, &limits).is_ok());

        // $6 should fail
        let big_order = make_order(12, 50); // $6.00
        assert!(state.check_order(&big_order, &limits).is_err());
    }

    #[test]
    fn test_zero_notional_order() {
        let state = RiskState::default();
        let limits = RiskLimits::default();
        let order = make_order(0, 50); // 0 contracts = $0
        assert!(state.check_order(&order, &limits).is_ok());
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
}
