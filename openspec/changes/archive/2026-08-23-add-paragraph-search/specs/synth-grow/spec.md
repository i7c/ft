# synth-grow

## REMOVED Requirements

### Requirement: Last-synth watermark
**Reason**: The `grow --new-only` scope and its git-topology watermark computation are removed together with the `grow` subcommand. Search-based scaffold re-runs with append-dedup make scoped accretion unnecessary.
**Migration**: Re-run `ft notes synth scaffold <target> --search "<query>"` — append-dedup skips already-pinned sections; use `--sort date` for newest-first ordering.

### Requirement: Missing-entry filter
**Reason**: The dedup invariant survives, but its home is the scaffold planner, not a grow-specific helper; the exposed helper is removed with the capability.
**Migration**: Append-dedup behavior is unchanged — `plan_synth_scaffold` drops entries whose `(source_path, body)` is already pinned; see the synth-notes "ft notes synth scaffold command" requirement.

### Requirement: ft notes synth grow command
**Reason**: `grow` duplicated scaffold's append path with extra selection machinery (`--new-only`, `--limit`, frontmatter-target reading). With search-based sourcing there is no separate accretion command; scaffold is the single entry point.
**Migration**: Use `ft notes synth scaffold <target.md> --search "<query>"` (idempotent via append-dedup). `--limit` is approximated with search result ordering plus `ft notes search --limit N`; there is no `--new-only` equivalent.
