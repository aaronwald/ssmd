//! ssmd-schema: Cap'n Proto generated types for market data
//!
//! This crate contains the generated Rust types from Cap'n Proto schemas.

#[allow(dead_code)]
mod trade_capnp {
    include!(concat!(env!("OUT_DIR"), "/schemas/trade_capnp.rs"));
}

pub use trade_capnp::*;

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::message::Builder;

    #[test]
    fn test_build_trade() {
        let mut message = Builder::new_default();
        {
            let mut trade = message.init_root::<trade::Builder>();
            trade.set_timestamp(1703318400000000000); // nanos
            trade.set_ticker("BTCUSD");
            trade.set_price(100.50);
            trade.set_size(10);
            trade.set_side(Side::Buy);
            trade.set_trade_id("trade-001");
        }

        let reader = message.get_root_as_reader::<trade::Reader>().unwrap();
        assert_eq!(reader.get_timestamp(), 1703318400000000000);
        assert_eq!(reader.get_ticker().unwrap(), "BTCUSD");
        assert_eq!(reader.get_price(), 100.50);
        assert_eq!(reader.get_size(), 10);
        assert!(matches!(reader.get_side().unwrap(), Side::Buy));
    }

    #[test]
    fn test_build_order_book_update() {
        let mut message = Builder::new_default();
        {
            let mut update = message.init_root::<order_book_update::Builder>();
            update.set_timestamp(1703318400000000000);
            update.set_ticker("BTCUSD");

            {
                let mut bids = update.reborrow().init_bids(2);
                {
                    let mut bid0 = bids.reborrow().get(0);
                    bid0.set_price(100.0);
                    bid0.set_size(50);
                }
                {
                    let mut bid1 = bids.reborrow().get(1);
                    bid1.set_price(99.0);
                    bid1.set_size(100);
                }
            }

            {
                let asks = update.reborrow().init_asks(1);
                let mut ask0 = asks.get(0);
                ask0.set_price(101.0);
                ask0.set_size(25);
            }
        }

        let reader = message
            .get_root_as_reader::<order_book_update::Reader>()
            .unwrap();
        assert_eq!(reader.get_ticker().unwrap(), "BTCUSD");

        let bids = reader.get_bids().unwrap();
        assert_eq!(bids.len(), 2);
        assert_eq!(bids.get(0).get_price(), 100.0);
    }
}
