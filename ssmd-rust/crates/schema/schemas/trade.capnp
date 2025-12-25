@0xa1b2c3d4e5f60001;

enum Side {
    buy @0;
    sell @1;
}

struct Trade {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    price @2 :Float64;
    size @3 :UInt32;
    side @4 :Side;
    tradeId @5 :Text;
}

struct Ticker {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    bidPrice @2 :Float64;
    askPrice @3 :Float64;
    lastPrice @4 :Float64;
    volume @5 :UInt64;
    openInterest @6 :UInt64;
}

struct Level {
    price @0 :Float64;
    size @1 :UInt32;
}

struct OrderBookUpdate {
    timestamp @0 :UInt64;        # Unix nanos
    ticker @1 :Text;
    bids @2 :List(Level);
    asks @3 :List(Level);
}

enum MarketStatus {
    open @0;
    closed @1;
    halted @2;
}

struct MarketStatusUpdate {
    timestamp @0 :UInt64;
    ticker @1 :Text;
    status @2 :MarketStatus;
}
