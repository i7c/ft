---
aliases: [memo cache]
---
# Memoization

Memoization stores the result of a pure function call so a repeated call with identical arguments returns instantly.

Repeated eigen decompositions of one matrix are a classic place to apply this; key the cache by a hash of the matrix.

See also [[Caching]] and [[Dynamic Programming]].
