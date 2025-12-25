---
name: explore-data
description: How to discover and understand available market data
---

# Exploring Data

When you need to understand what data is available:

1. Use `list_datasets` to see available feeds and dates
2. Use `sample_data` to look at actual records
3. Use `get_schema` to understand field types
4. Use `list_builders` to see what state can be derived

## Key Patterns

- Kalshi uses prediction market format: yes_bid, yes_ask
- Spread = yes_ask - yes_bid
- All timestamps are Unix milliseconds (UTC)

## Watch Out For

- Gaps in data (check has_gaps in dataset info)
- Low volume tickers (noisy)
- Market hours (Kalshi has weekend gaps)
