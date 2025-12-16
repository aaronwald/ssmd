# stupid simple market data (ssmd)

## Context
Expert system based on ssmd.md
Market data so simple you can install on homelab
So simple you can admin via tui
So simple claude can talk to it easily. Easy to define new skills.
All scripts and jobs should run on a fixed memory profile and support batching and recovery.

## Notes
Are we building tickerplant? No - we are intentionally diferring order book state to the edge (after filtering and sharding)
Are we building a shared library? No (We would want client libraries)
Are we building a cloud first service? Yes
Are we building a platform createing quality data? Yes

## Features
GitOps to deploy, test, scale.
At the end of each day we tear down the system. At the start of each day we start of the system. A day can be defined by an arbitrary schedule.
When we deploy we decide which markets to deploy.
Capture raw market data to block storage/archive off to s3 object storage.
We want to stream data via websocket
We want to stream data via nats and jetstream
We want to integrate with systems like chronicle and aeron (https://aeron.io/)
We want to build a system that agents can easily access and reason about
Need to have meta data support built first. A key feature to simplicity will be to remove the chance of error for the operator.
We need to have a simple cli tool that we can script to create and modify the trading environmeents definition.
Customers may think about data differently so we will need to support different transforms on raw data.
Key all keys in store so we can support rapid interation with tear down/build up
Support re-runnuing normalization on historic files.
All metadata is a timeseries. Can evolve overtime. When we rerun jobs we should always take a day (utc) as input.
Record provenance for data.

## Technical descisions
Rust/Go (Zig is an option but we need to figure that out)
Go for command line tools
ArgoCD for gitops
Postgres for all databases
Temporal for jobs with schedules
Use nats for journaling
Use mqtt for message oriented middleware
Use redis for aching only (dont use pub/sub redis)
Use debezium to publish secmaster changes to a journal (turn out secmaster into a stream of upd ates)
Abstract out the interface for middleware, journaling, caching so that we can later choose different implementations. When we define a deployment we will choose the implementation to use.
We need to be able to shard the connectors and collectors using our meta data.
Avoid configuration with side effects. We want simple declarative solutions.
Support the use of libechidna which is in C++ for at least the kraken market (prove out C++ integration)

# Implementation Details
We will use the Brooklyn offsite to test s3 storage.
We will never have a QA team. We need to be able to spin up environments to compare versions in an automated fashion.
We dont want a large support team. We want to feed customer issues from something like https://linear.app/features back into our development roadmap.
Fail fast model on configuration.
Support a non-realtime clock for back testing.
We should support security features from the beginning.
Want to support autoscale with temporal and jobs that run in background (these could run on separate k8s)

## Open Quwestions
How will agents work with our system? If they find a data quality issue, how will that feed back into us?
What compliance/audit needs are there?
How will we support authn/authz?

## Instructions
Create a detailed designed proposal for a stupid simple market data system we can build on our homelab.
Create a technical architecture diagram or diagrams.
Create a high level phased implementation plan.

## Goals
Port kalshi from tradfiportal
Add polymarket support
Add kraken suppoort via libechidna.