---
tags: [proj/fluxo]
---
# Fluxo

Punch-in and punch-out events land in one Postgres table; the release cadence is every second Friday.

Rip out the in-memory database and run every integration test against a real Postgres.
