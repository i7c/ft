## Why

`ft notes` has accumulated 13 subcommands and now overlaps confusingly with both `ft find` and `ft graph`:

- `ft find` and `ft notes open` accept identical fuzzy syntax; one prints, the other opens.
- `ft notes backlinks` / `ft notes links` are read-only graph edge queries.
- `ft notes journal` is a graph-backed temporal feed.
- `ft notes today` exists alongside `ft tasks list today` (preset) and `ft timeblocks list` (today-default) — three "today" verbs.

The user's mental model has to do unnecessary work to predict which namespace owns which operation. New users learn it twice; scripters debug it later.

This change splits along a clean axis: **notes = operate on one note**; **graph = read across notes**. The split is hard-break: no aliases, no deprecation period — only one syntax exists after this lands. The vault and the CLAUDE.md are the only "users" today, and both will be updated in the same change.

This change sequences last (after the DSL unification) so the moved subcommands speak the unified DSL.

## What Changes

### Move to `ft graph`

- `ft notes backlinks <note>` → `ft graph backlinks <note>`
- `ft notes links <note>` → `ft graph links <note>`
- `ft notes journal <note>` → `ft graph journal <note>`

These are read-only graph queries by their nature.

### Move to `ft notes`

- `ft find <query>` → `ft notes find <query>` (the only fold-in — `ft find` is removed as a top-level command)

### Stay under `ft notes`

`ft notes` retains: `open`, `move-section`, `create`, `today`, `periodic`, `rename`, `mv`, `update-related`, `append`, plus the new `find`. These all operate on a single note.

### Stay under `ft graph`

`ft graph` retains: `query`. Plus the three moved subcommands. The graph namespace becomes the cross-note-read namespace.

### Hard break

- No aliases. `ft find foo` exits with code 2 and a message instructing the user to use `ft notes find foo`.
- No deprecation period.
- CHANGELOG documents every renamed path.
- Man pages and shell completions regenerated.

### Test updates

Every CLI integration test referencing the moved paths is rewritten. The TUI tests are unaffected (TUI surfaces don't go through CLI paths).

## Capabilities

### New Capabilities

- `cli-namespace-graph`: The `ft graph` subcommand exposes `backlinks`, `links`, `journal` in addition to the existing `query`. These are read-only operations over the link / paragraph graph.

### Modified Capabilities

- `cli-namespace-notes`: The `ft notes` namespace is constrained to single-note operations. Cross-note read operations (`backlinks`, `links`, `journal`) move to `ft graph`. `ft find` folds in as `ft notes find`.

### Removed Capabilities

- `cli-find`: The top-level `ft find` subcommand is removed. Its surface is preserved as `ft notes find` with identical args.

## Impact

- **Modified**: `ft/src/main.rs` — remove `Find` variant, add subcommand routing for `notes find`, move `backlinks`/`links`/`journal` dispatch from notes.rs to graph.rs.
- **Modified**: `ft/src/cmd/notes.rs` (~1871 lines today) — remove `Backlinks`/`Links`/`Journal` variants and their `run_*` functions; add `Find` variant that wraps the existing `ft/src/cmd/find.rs` logic.
- **Modified**: `ft/src/cmd/graph.rs` — add `Backlinks`/`Links`/`Journal` variants with the run functions lifted from `notes.rs`.
- **Removed**: `ft/src/cmd/find.rs` (top-level entry point) — its `run` becomes an internal helper that `notes::Find` calls.
- **Modified**: `docs/architecture.md`, `README.md`, `docs/timeblocks.md` cross-references.
- **Modified**: `CLAUDE.md` examples updated.
- **Generated**: man pages, completion scripts, `docs/commands.md` registry section.
- **Tests**: every `assert_cmd::Command::cargo_bin("ft").arg("notes").arg("backlinks")` becomes `arg("graph").arg("backlinks")`; ditto for `find` → `notes find`.
- All four build invariants stay green.
