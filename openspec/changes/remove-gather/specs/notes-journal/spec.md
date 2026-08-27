# notes-journal

## REMOVED Requirements

### Requirement: ft notes gather subcommand
**Reason**: The deprecated `ft notes gather` subcommand (and its `journal` alias) is deleted; search is the sourcing front-end.
**Migration**: Use `ft notes search <query>` (link-target searches via `[[Link]]`; any-mode via `--any`).

### Requirement: Journal alias resolution via Related section
**Reason**: `build_gather`'s single-target Related-alias resolution is deleted with the engine.
**Migration**: Use `ft notes search "[[Note]] [[Alias]]" --any` to cover the note and its aliases; Related-alias resolution survives only for `ft notes related` scoring.

### Requirement: Journal source coverage
**Reason**: The engine is deleted.
**Migration**: Use `ft notes search` (scan-derived paragraph index, whole vault) or `ft notes recent` for the time-shaped whole-vault feed.

### Requirement: Journal matching via ParagraphLink edges
**Reason**: The engine is deleted.
**Migration**: Link-target queries via `ft notes search "[[X]]"`.

### Requirement: Journal entries sorted reverse-chronologically
**Reason**: The engine is deleted.
**Migration**: `ft notes search --sort date` (newest first) or `ft notes recent` (recency-ordered by blame date).

### Requirement: Journal default (table) output
**Reason**: The command is deleted.
**Migration**: `ft notes search` / `ft notes recent` table output.

### Requirement: Journal JSON output
**Reason**: The command is deleted.
**Migration**: `ft notes search --json` / `ft notes recent --json` (stable shapes documented in their own specs).

### Requirement: Multi-link invocation mode
**Reason**: The command is deleted.
**Migration**: `ft notes search "[[Foo]] [[Bar]]" --any` preserves the old multi-target OR semantics.

### Requirement: In-window filter flag
**Reason**: The command is deleted; search has no window concept.
**Migration**: Use `--sort date` on search, or `ft notes recent --since/--range` for windowed feeds.

### Requirement: Matched-targets field per entry
**Reason**: The command is deleted.
**Migration**: `ft notes search --json` emits `matched` (matched clause labels) instead of matched note IDs.

### Requirement: Cited badge in journal text output
**Reason**: The command is deleted.
**Migration**: `ft notes recent` keeps the same citation-badge grammar.

### Requirement: cited_in in journal JSON
**Reason**: The command is deleted.
**Migration**: `ft notes recent --json` keeps the `cited_in` field (see notes-history).

### Requirement: --uncited filter on journal
**Reason**: The command is deleted.
**Migration**: `ft notes recent --uncited` retains the filter.

### Requirement: Recent remains unaffected
**Reason**: The reason this requirement existed (gather's deprecation coexisting with recent) is gone with the command; recent's behavior is unchanged.
**Migration**: No action — `ft notes recent` is untouched by this change.
