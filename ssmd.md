
# Everything I know about market data

1. You suck is as good as it gets
1. All trading needs it in some form

## Goals

1. Big design decision is what you want service you offer to your PMs. Leads to efficiencies and problems.
1. Shared platform? (Operations)
1. Libraries?
1. Data?
1. Tickerplant?

## Network design matters
1. Where the multicast or firewall rules exist can impact where and how fast you can deplot new kit
1. Understadning the shape of data is important. Can you reshard to different plans. Are you publishing more or less data then the exchange.
1. Do you have tools like netflow to understand what your switches are doing.
1. Is your networking kit observable?

## Job Scheduling
1. Assume exists.

## Data
1. Feeds generally into different buckets
1. Stateless (no recovery)
1. Orderbook - sharded by some schema (symbol, underlying etc)
1. All events for a session for a sequenced stream which you can play back to build the correct state at a given time.

## Schema
1. A common data schema is needed.
1. Managing the schema across groups is tricky. Too rigid and new users may suffer. Too flexible then we can easily introduce regression.

## MetaData - Market Data Model
1. Need exchange calendar (with as-of date support)
1. Need description of each exchange as a time series so we know protocols and sources of data available on what dates.
1. Need to track what dates we have data for and when we have failures.

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
1. Do you monitor latency with something like corvil?
1. Do you track latency regression in software?

## Distribution
1. Once data is ingested and normalized it needs to be internally distributed.
1. Need some middleware - guaranteed or not (multicast) - can even do TCP
1. This layer itself can be accelerated
1. Lots of options here for using things like aeron for reliable multicast (used to be lbm/informatica)
1. Need to understand requirements. Some people want guaranteed orderbooks (then never lose a msg, have to pay a penalty)
1. Tickerplant architecture forms hub and spoke (lets you redistribute data internally for a latency penalty)
1. Shared memory generally a common approach to single host solutions. Lots of cores but then lots of memory contention.
1. Think about how you shard - Fan in/fan out. Fast but flexible.

## Recording
1. Should record everyhting as close to the edge as you can
1. Can buy pcap recording devices.
1. Need to have very good clocks for best understanding your latency
1. Can do hardware timestamping.
1. Keep raw logs. Keep pcap when possible.

## Correctness
1. Market data foramts should be stable over time.
1. You need to understand the version/code used on any day to have repeatable backtesting (if you always use latest libraries you'll have issues)
1. Generally want to version changes unless major regeneration.

## Playback
1. Nice to have the ability to playback for backtesting.
1. Can you select streams or instruments quickly.

## Artifacts
1. All recorded data should be turned into artifacts you can digest
1. Can look at time series databases here (kdb, onetick, etc...)
1. Can look at cloud solutions for event searching
1. Want to be able to get data into formats like arrow now where we can leverage cloud tech (big table/big query etc...)
1. Want to be able to understand you data quality and iterate to fix this rapidly
1. Open formats have probably caught up to proprietary
1. Understand time series query vs event query. Outside of order book state a lot can can be done on query for improving data.
1. How do you shard artifacts for reserach - How do you trade? Portfolio? Single stock? Related instruments.

## Transport
1. Reliability if needed
1. Slow consumers
1. Scaling and sharding

## SDLC
1. Generally when we want to share market data libraries we have to think carefully about the SDLC processes for consistency.
1. Expensive builds penalize everyone.

## DevOps
1. Great devops can allow you to redeploy and reconfigure quick
1. Great git ops is important
1. Infrastructure as Code (terraform/opentofu) solutions are key to improving data quality over time. Kit should be commodity for data at scale.

## Data Quality
1. Do not want manual QA. Want everything in code so we can iterate (and now AI)

## Observability
1. Want to be able to observe health of system at lots of levels to prevent trading loss or data loss
1. Highest rick on busiest days - most trading opp
1. Any latency generates a queue somewhere - general poorly understood outside of the systems space.
1. Incident management improves trust.

## Entitlements
1. How do you manage entitlements?
1. How do you report usage?
1. Do you do display vs non-display?
1. How do you respond to audits?

## Market Data as a Service
1. Cloud native offerings.
1. Requires entitlements
1. Conflation needs to be done to take some load off (may be changing with more modern rust/zig impls in cloud - more clever designs that dont need multicast)

## Team Structure
1. How close are dev and production?
