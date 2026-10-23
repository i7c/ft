---
tags: [proj/busao]
---
# Route Planning

- **Stop memory**: approved synthetic stops become known stops with usage counts
  and rank ahead of fresh candidates when snapping.
- **Corridor memory**: count how often each road segment appears in approved routes
  and apply a bounded discount to those edges in future solves.
- **Parameter memory**: per-plant defaults where the planners consistently override.
- Snap candidates to the road network before solving.
- Penalize left turns across the busy avenues.
- Respect the maximum ride time per passenger.
- Keep the vehicle capacity constraint hard.
- Re-run the solver nightly with the latest roster.
- Export the routes as GeoJSON for the planners.
- Log every manual override for later analysis.
- Cache the distance matrix between solves.
- Compare against the routes of last week before publishing.
