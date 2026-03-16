use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::types::{ExchangeSettlement, MarketResult};

/// Per-fill summary row aggregated from DB (grouped by side + action).
#[derive(Debug, Clone)]
pub struct FillSummary {
    pub side: String,
    pub action: String,
    pub total_quantity: Decimal,
    pub total_cost: Decimal,
    pub taker_quantity: Decimal,
}

/// Taker fee per contract on Kalshi ($0.07).
pub fn taker_fee_per_contract() -> Decimal {
    Decimal::new(7, 2)
}

/// Extract the event ticker from a market ticker.
///
/// Convention: first two dash-separated segments.
/// e.g., "KXOSCARACTO-26-WAG" -> "KXOSCARACTO-26"
/// Two segments: "KXBTCD-26MAR0211" -> "KXBTCD-26MAR0211"
/// Single segment: "SOMETICKER" -> "SOMETICKER"
pub fn derive_event_ticker(ticker: &str) -> String {
    ticker.split('-').take(2).collect::<Vec<_>>().join("-")
}

/// Convert a Decimal representing cents to i64.
///
/// Returns Err if the value cannot be represented as i64 (overflow or NaN-like).
/// The input should already be in cents (integer-valued Decimal).
fn decimal_cents_to_i64(cents: Decimal) -> Result<i64, String> {
    cents
        .trunc()
        .to_i64()
        .ok_or_else(|| format!("settlement value {} out of i64 range", cents))
}

/// Compute a full ExchangeSettlement from fill summaries and WS settlement data.
///
/// Returns Ok(None) if fill_summaries is empty (no position to settle).
/// Returns Err if the computed values overflow i64 (indicates upstream data corruption).
pub fn compute_settlement(
    ticker: &str,
    market_result: MarketResult,
    settled_time: DateTime<Utc>,
    fill_summaries: &[FillSummary],
) -> Result<Option<ExchangeSettlement>, String> {
    if fill_summaries.is_empty() {
        return Ok(None);
    }

    let mut yes_bought = Decimal::ZERO;
    let mut yes_sold = Decimal::ZERO;
    let mut no_bought = Decimal::ZERO;
    let mut no_sold = Decimal::ZERO;
    let mut buy_cost = Decimal::ZERO;
    let mut sell_proceeds = Decimal::ZERO;
    let mut total_taker_contracts = Decimal::ZERO;

    for fs in fill_summaries {
        match (fs.side.as_str(), fs.action.as_str()) {
            ("yes", "buy") => {
                buy_cost += fs.total_cost;
                yes_bought += fs.total_quantity;
            }
            ("yes", "sell") => {
                sell_proceeds += fs.total_cost;
                yes_sold += fs.total_quantity;
            }
            ("no", "buy") => {
                buy_cost += fs.total_cost;
                no_bought += fs.total_quantity;
            }
            ("no", "sell") => {
                sell_proceeds += fs.total_cost;
                no_sold += fs.total_quantity;
            }
            _ => {
                return Err(format!(
                    "unexpected fill side/action: {}/{}",
                    fs.side, fs.action
                ));
            }
        }
        total_taker_contracts += fs.taker_quantity;
    }

    // Net cost = money spent on buys minus money received from sells
    let cost_basis = buy_cost - sell_proceeds;

    let yes_count = yes_bought - yes_sold;
    let no_count = no_bought - no_sold;

    // each contract settles at $1.00
    // Positive count = long (receives payout), negative = short (owes payout)
    let payout = match market_result {
        MarketResult::Yes => yes_count,
        MarketResult::No => no_count,
        MarketResult::Void => cost_basis, // refund at cost basis -> revenue = 0
        MarketResult::Scalar | MarketResult::Unknown => {
            return Err(format!(
                "unsupported market result {:?} for ticker {}",
                market_result, ticker
            ));
        }
    };

    let revenue = payout - cost_basis;
    let hundred = Decimal::new(100, 0);
    let revenue_cents = decimal_cents_to_i64(revenue * hundred)?;

    let fee_cost_dollars = total_taker_contracts * taker_fee_per_contract();

    // value_cents = absolute notional value at settlement (direction captured in revenue)
    let value_cents = match market_result {
        MarketResult::Yes => Some(decimal_cents_to_i64(yes_count.abs() * hundred)?),
        MarketResult::No => Some(decimal_cents_to_i64(no_count.abs() * hundred)?),
        MarketResult::Void => None,
        MarketResult::Scalar | MarketResult::Unknown => {
            return Err(format!(
                "unsupported market result {:?} for ticker {}",
                market_result, ticker
            ));
        }
    };

    let event_ticker = derive_event_ticker(ticker);

    Ok(Some(ExchangeSettlement {
        ticker: ticker.to_string(),
        event_ticker,
        market_result,
        yes_count,
        no_count,
        revenue_cents,
        settled_time,
        fee_cost_dollars,
        value_cents,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_derive_event_ticker_multi_segment() {
        assert_eq!(derive_event_ticker("KXOSCARACTO-26-WAG"), "KXOSCARACTO-26");
    }

    #[test]
    fn test_derive_event_ticker_two_segment() {
        assert_eq!(derive_event_ticker("KXBTCD-26MAR0211"), "KXBTCD-26MAR0211");
    }

    #[test]
    fn test_derive_event_ticker_single() {
        assert_eq!(derive_event_ticker("SOMETICKER"), "SOMETICKER");
    }

    #[test]
    fn test_compute_settlement_yes_wins() {
        // Bought 10 Yes at $0.40 each -> cost_basis = $4.00
        // Result = Yes -> payout = 10 * $1.00 = $10.00
        // Revenue = $10.00 - $4.00 = $6.00 = 600 cents
        let fills = vec![FillSummary {
            side: "yes".to_string(),
            action: "buy".to_string(),
            total_quantity: Decimal::new(10, 0),
            total_cost: Decimal::new(4, 0), // 10 * $0.40
            taker_quantity: Decimal::new(10, 0),
        }];

        let result = compute_settlement(
            "KXBTCD-26MAR0211",
            MarketResult::Yes,
            Utc::now(),
            &fills,
        )
        .expect("should not error");

        let s = result.expect("should produce settlement");
        assert_eq!(s.revenue_cents, 600);
        assert_eq!(s.yes_count, Decimal::new(10, 0));
        assert_eq!(s.no_count, Decimal::ZERO);
        assert_eq!(s.fee_cost_dollars, Decimal::new(70, 2)); // 10 * $0.07
        assert_eq!(s.value_cents, Some(1000)); // 10 * 100
        assert_eq!(s.event_ticker, "KXBTCD-26MAR0211");
        assert_eq!(s.market_result, MarketResult::Yes);
    }

    #[test]
    fn test_compute_settlement_no_wins() {
        // Bought 5 No at $0.30 each -> cost = $1.50
        // Result = No -> payout = 5 * $1.00 = $5.00
        // Revenue = $5.00 - $1.50 = $3.50 = 350 cents
        let fills = vec![FillSummary {
            side: "no".to_string(),
            action: "buy".to_string(),
            total_quantity: Decimal::new(5, 0),
            total_cost: Decimal::new(150, 2), // $1.50
            taker_quantity: Decimal::new(3, 0),
        }];

        let result = compute_settlement(
            "KXBTCD-26MAR0211",
            MarketResult::No,
            Utc::now(),
            &fills,
        )
        .expect("should not error");

        let s = result.expect("should produce settlement");
        assert_eq!(s.revenue_cents, 350);
        assert_eq!(s.no_count, Decimal::new(5, 0));
        assert_eq!(s.fee_cost_dollars, Decimal::new(21, 2)); // 3 * $0.07
        assert_eq!(s.value_cents, Some(500)); // 5 * 100
    }

    #[test]
    fn test_compute_settlement_loser() {
        // Bought 10 Yes at $0.60 each -> cost = $6.00
        // Result = No -> payout = 0 (yes_count=10 but result is No, no_count=0)
        // Revenue = $0.00 - $6.00 = -$6.00 = -600 cents
        let fills = vec![FillSummary {
            side: "yes".to_string(),
            action: "buy".to_string(),
            total_quantity: Decimal::new(10, 0),
            total_cost: Decimal::new(6, 0), // $6.00
            taker_quantity: Decimal::new(10, 0),
        }];

        let result = compute_settlement(
            "KXBTCD-26MAR0211",
            MarketResult::No,
            Utc::now(),
            &fills,
        )
        .expect("should not error");

        let s = result.expect("should produce settlement");
        assert_eq!(s.revenue_cents, -600);
        assert_eq!(s.yes_count, Decimal::new(10, 0));
        assert_eq!(s.no_count, Decimal::ZERO);
        assert_eq!(s.value_cents, Some(0)); // no_count = 0
    }

    #[test]
    fn test_compute_settlement_mixed_sides() {
        // Bought 10 Yes at $0.40 ($4.00), sold 3 Yes at $0.60 ($1.80)
        // yes_count = 10 - 3 = 7
        // cost_basis = buy_cost - sell_proceeds = $4.00 - $1.80 = $2.20
        // Result = Yes -> payout = 7 * $1.00 = $7.00
        // Revenue = $7.00 - $2.20 = $4.80 = 480 cents
        let fills = vec![
            FillSummary {
                side: "yes".to_string(),
                action: "buy".to_string(),
                total_quantity: Decimal::new(10, 0),
                total_cost: Decimal::new(4, 0), // $4.00
                taker_quantity: Decimal::new(10, 0),
            },
            FillSummary {
                side: "yes".to_string(),
                action: "sell".to_string(),
                total_quantity: Decimal::new(3, 0),
                total_cost: Decimal::new(180, 2), // $1.80
                taker_quantity: Decimal::new(0, 0),
            },
        ];

        let result = compute_settlement(
            "KXOSCARACTO-26-WAG",
            MarketResult::Yes,
            Utc::now(),
            &fills,
        )
        .expect("should not error");

        let s = result.expect("should produce settlement");
        assert_eq!(s.yes_count, Decimal::new(7, 0));
        assert_eq!(s.revenue_cents, 480);
        assert_eq!(s.event_ticker, "KXOSCARACTO-26");
    }

    #[test]
    fn test_compute_settlement_empty_fills() {
        let result = compute_settlement(
            "KXBTCD-26MAR0211",
            MarketResult::Yes,
            Utc::now(),
            &[],
        )
        .expect("should not error");
        assert!(result.is_none());
    }

    #[test]
    fn test_compute_settlement_void() {
        // Void: refund at cost basis -> payout = cost_basis -> revenue = 0
        let fills = vec![FillSummary {
            side: "yes".to_string(),
            action: "buy".to_string(),
            total_quantity: Decimal::new(5, 0),
            total_cost: Decimal::new(250, 2), // $2.50
            taker_quantity: Decimal::new(5, 0),
        }];

        let result = compute_settlement(
            "KXBTCD-26MAR0211",
            MarketResult::Void,
            Utc::now(),
            &fills,
        )
        .expect("should not error");

        let s = result.expect("should produce settlement");
        assert_eq!(s.revenue_cents, 0);
        assert_eq!(s.market_result, MarketResult::Void);
        assert_eq!(s.value_cents, None);
    }

    #[test]
    fn test_compute_settlement_short_yes_loses() {
        // Sold 10 Yes at $0.60 (short Yes) -> proceeds $6.00, cost_basis = -$6.00
        // Result: Yes -> short owes $10.00 at settlement
        // Payout = -10 * $1.00 = -$10.00
        // Revenue = -$10.00 - (-$6.00) = -$4.00 = -400 cents
        let fills = vec![FillSummary {
            side: "yes".to_string(),
            action: "sell".to_string(),
            total_quantity: Decimal::from(10),
            total_cost: Decimal::new(600, 2), // $6.00
            taker_quantity: Decimal::from(10),
        }];

        let s = compute_settlement(
            "KXBTCD-26-T100K",
            MarketResult::Yes,
            Utc::now(),
            &fills,
        )
        .unwrap()
        .unwrap();

        assert_eq!(s.yes_count, Decimal::from(-10));
        assert_eq!(s.revenue_cents, -400);
        assert_eq!(s.value_cents, Some(1000)); // abs(10) * 100
    }
}
