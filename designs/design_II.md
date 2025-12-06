# stupid simple market data

## Context
Expert system based on ssmd.md
Market data so simple you can install on homelab
So simple you can admin via tui
So simple claude can talk to it easily. Easy to define new skills.

## Notes
Are we building tickerplant? No
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

## Instructions
Create a proposal for a stupid simple market data system we can build on our homelab.
