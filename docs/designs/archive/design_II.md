# stupid simple market data

## Context
Expert system based on ssmd.md
Market data so simple you can install on homelab
So simple you can admin via tui
So simple claude can talk to it easily. Easy to define new skills.

## Notes
Are we building tickerplant? No - we are intentionally diferring order book state to the edge (after filtering and sharding)
Are we building a shared library? No (We would want client libraries)
Are we building a cloud first service? Yes
Are we building a platform createing quality data? Yes

## Features
GitOps to deploy, test, scale.
Capture raw market data to block storage/archive off to s3 object storage
We want to stream data via websocket
We want to stream data via nats and jetstream
We want to integrate with systems like chronicle and aeron (https://aeron.io/)
We want to build a system that agents can easily access and reason about

## Features - Mark II
Need to have meta data support built first. A key feature to simplicity will be to remove the chance of error for the operator.
We need to have a simple cli tool that we can script to create and modify the trading environmeents definition.

## Instructions
Create a proposal for a stupid simple market data system we can build on our homelab.


## Technical descisions
Rust/Zig/Go
Go for command line tools
ArgoCD for gitops
Postgres for all databases
Temporal for jobs with schedules

## Dev 6 Additional ideas
We will never have a QA team. We need to be able to spin up environments to compare versions in an automated fashion.
We dont want a large support team. We want to feed customer issues from something like https://linear.app/features back into our development roadmap.
Customers may think about data differently so we will need to support different transforms on raw data.
How will agents work with our system? If they find a data quality issue, how will that feed back into us?
What is zig support for something like io_uring? Maybe C++ or Rust is a better decision.
We need to be able to shard the connectors and collectors using our meta data.
Avoid configuration with side effects. We want simple declarative solutions.

Fail fast model on configuration.
Clocks
audit/compliance
security model - authn/authz
