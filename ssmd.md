
# Everything I know about market data

1. You suck is as good as it gets
1. All trading needs it in some form

## Data
1. Feeds generally into different buckets
1. Stateless (no recovery)
1. Orderbook - sharded by some schema (symbol, underlying etc)
1. All events for a session for a sequenced stream which you can play back to build the correct state at a given time.

## Sources of data
1. Direct exchange (generally multicast) with binary protocols
1. Exchanges offer different tiers of data even within the binary space (gig vs wan shaped etc.)
1. Different feeds have different levels of data (l1, l2 levels, l3 orderbook)
1. You can get data from vendors with different qualities of normalization (idc, reuters)
1. You can get raw capture from different vendors (pico, maystreet/lseg)
1. Most crypto/prediction is in json via websocket
1. Very long tail of feeds now that we have crypto, tokenization, prediction markets.

## Latencies
1. Lowest l1 switching with fpga - never leave the card (or switch)
1. Software - Generally C but Rust out there. Can do highly optimized C++ if you dont need all the data.
1. Software with kernel bypass - lots of linux tunings can be done (huge tables, nohz, no interrupts, hard to measure sometiems)
1. When doing software need to invest in fastest cpu and memory. Understand numa architectures etc...
1. Lots of lock free algos exist
1. Goes up the latency spectrum from there depending on type of trading or forecasting you want to do.
1. Very latency sensitive trading is impacted by queue position in matching engine and always a quest to eliminate market data latency (some places do predictive trading off bits and not even full packets)
1. Signals and forecasting can be just as valuable as low latency trading (HFT)

## Distribution
1. Once data is ingested and normalized it needs to be internally distributed.
1. Need some middleware - guaranteed or not (multicast) - can even do TCP
1. This layer itself can be accelerated
1. Lots of options here for using things like aeron for reliable multicast (used to be lbm/informatica)
1. Need to understand requirements. Some people want guaranteed orderbooks (then never lose a msg, have to pay a penalty)
1. Tickerplant architecture forms hub and spoke (lets you redistribute data internally for a latency penalty)
1. Shared memory generally a common approach to single host solutions. Lots of cores but then lots of memory contention.

## Recording
1. Should record everyhting as close to the edge as you can
1. Can buy pcap recording devices.
1. Need to have very good clocks for best understanding your latency
1. Can do hardware timestamping.

## Correctness
1. Market data foramts should be stable over time.
1. You need to understand the version/code used on any day to have repeatable backtesting (if you always use latest libraries you'll have issues)
1. Generally want to version changes unless major regeneration.

## Playback
1. Nice to have the ability to playback for backtesting.

## Artifacts
1. All recorded data should be turned into artifacts you can digest
1. Can look at time series databases here (kdb, onetick, etc...)
1. Can look at cloud solutions for event searching
1. Want to be able to get data into formats like arrow now where we can leverage cloud tech (big table/big query etc...)
1. Want to be able to understand you data quality and iterate to fix this rapidly

## SDLC
1. Generally when we want to share market data libraries we have to think carefully about the SDLC processes for consistency.
1. Expensive builds penalize everyone.

## Observability
1. Want to be able to observe health of system at lots of levels to prevent trading loss or data loss
1. Highest rick on busiest days - most trading opp
1. Any latency generates a queue somewhere - general poorly understood outside of the systems space.