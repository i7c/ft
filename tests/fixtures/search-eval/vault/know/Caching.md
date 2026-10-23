# Caching

A cache trades memory for latency: keep recently used results close to where they are needed.

Invalidation is the hard part; prefer time-based expiry when the source of truth is remote.
