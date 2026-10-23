## REMOVED Requirements

### Requirement: build_history core feed
**Reason**: `ft_core::recent` (`build_recent` / `RecentEntry` /
`RecentOptions`) is deleted; the feed had no consumer after the Recent tab
and `ft notes recent` were removed.
**Migration**: Use `ft notes search --sort date` for a recency-ordered
paragraph feed.

### Requirement: Edit-window inclusion filter
**Reason**: The git-window selection (which reused the deleted pulse
added-lines map) is deleted with the engine.
**Migration**: Express the period as a date predicate in the search query.

### Requirement: Default window of 7 days
**Reason**: There is no default window without the command.
**Migration**: None.

### Requirement: Recency ordering
**Reason**: Blame-based recency ordering lives on in
`ft notes search --sort date`, which has its own spec.
**Migration**: `ft notes search --sort date`.

### Requirement: Synth notes excluded by default
**Reason**: The feed's synth-note exclusion was feed-specific; `ft notes
search` keeps its own exclusion behaviour via `[synth] exclude_prefixes`.
**Migration**: Configure `[synth] exclude_prefixes` if desired.

### Requirement: File-prefilter performance contract
**Reason**: The prefilter existed to bound blame cost per window; there is no
window.
**Migration**: None.

### Requirement: ft notes recent subcommand
**Reason**: The subcommand is deleted (clap exits 2 with an unknown
subcommand).
**Migration**: `ft notes search --sort date`.

### Requirement: History output formats
**Reason**: The history table/JSON renderers go with the command.
**Migration**: `ft notes search --json`.

### Requirement: Cited badge in history text output
**Reason**: `CitationIndex` is deleted with the feed; no remaining surface
renders `cited:` badges.
**Migration**: None.

### Requirement: cited_in in history JSON
**Reason**: The `cited_in` field of history JSON output is removed with the
command.
**Migration**: None.

### Requirement: --uncited filter on history
**Reason**: The flag goes with the command.
**Migration**: None.
