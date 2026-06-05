## ADDED Requirements

### Requirement: User-editable `[keymap]` table in `config.toml`

The user's layered `config.toml` SHALL accept a top-level `[keymap]` table. Inside it, per-scope sub-tables `[keymap.<scope>]` SHALL map chord strings to command names, where `<scope>` matches the canonical `CommandScope::as_str()` form (`global`, `tab/<name>`, or `modal/<name>`). A top-level `keymap.unbind` array SHALL list `{ scope, chord }` entries that drop default bindings without replacing them. A boolean `keymap.strict` SHALL control whether validation errors at startup are warnings (`false`, default) or fatal (`true`).

#### Scenario: Add a brand-new chord binding
- **WHEN** the user's `config.toml` contains `[keymap.global]` with `"Ctrl+s" = "app.sync-git"` and the chord is not in the default `APP_KEYMAP`
- **THEN** the chord becomes bound to `app.sync-git` at App startup and pressing `Ctrl+s` from any tab dispatches the command

#### Scenario: Replace a default binding
- **WHEN** the user's `config.toml` contains `[keymap."tab/graph"]` with `"R" = "graph.refresh"` and `[[keymap.unbind]]` listing `{ scope = "tab/graph", chord = "Ctrl+r" }`
- **THEN** the default `Ctrl+r` binding for `graph.refresh` is removed from the effective Graph-tab keymap and `Shift+R` becomes the only chord bound to `graph.refresh`

#### Scenario: Unbind without replacement
- **WHEN** the user's `config.toml` contains `[[keymap.unbind]]` with `{ scope = "global", chord = "g" }` and no override re-binds `g`
- **THEN** pressing `g` from any tab with no modal active is unbound (no command fires) and the App's default git-leader chord is disabled

### Requirement: Overlay validation surfaces every error with a stable shape

`KeymapOverlay::from_raw(raw, &CommandRegistry, scope)` SHALL validate every chord string and every command name and SHALL return a `Vec<KeymapOverlayError>` describing every problem in the supplied raw table (not just the first one). Error variants SHALL cover: unparseable chord, unknown command name, an unbind entry whose chord is not in the base keymap, and overlay-internal collision (two override entries normalising to the same chord).

#### Scenario: Unknown command name flagged
- **WHEN** `[keymap.global]` contains `"Ctrl+s" = "app.no-such-command"` and `app.no-such-command` is not in the registry
- **THEN** validation returns `KeymapOverlayError::UnknownCommand { name: "app.no-such-command", scope: CommandScope::Global }`

#### Scenario: Unparseable chord flagged
- **WHEN** `[keymap.global]` contains `"Frobnicate+x" = "app.quit"`
- **THEN** validation returns `KeymapOverlayError::InvalidChord { raw: "Frobnicate+x", … }` and `app.quit` is not added to the overlay

#### Scenario: Unbind-on-missing-chord flagged
- **WHEN** `[[keymap.unbind]]` lists `{ scope = "tab/graph", chord = "Ctrl+z" }` and the default Graph keymap does not bind `Ctrl+z`
- **THEN** validation returns `KeymapOverlayError::UnbindMissing { chord, scope: CommandScope::Tab("graph") }`

#### Scenario: All errors reported, not just the first
- **WHEN** `[keymap.global]` contains two invalid entries (one unknown command, one bad chord)
- **THEN** validation returns a Vec with both errors so the user fixes them in one editing round

### Requirement: `keymap.strict` controls startup error handling

When `keymap.strict = false` (default), validation errors SHALL be logged to `tracing::warn!` and the App SHALL start with the partial overlay (valid entries applied, invalid entries skipped). When `keymap.strict = true`, any validation error SHALL cause `App::new` to return an error and `ft tui` SHALL exit with a non-zero code.

#### Scenario: Default strict=false starts the App
- **WHEN** the user's `config.toml` has one invalid `[keymap.global]` entry and `keymap.strict` is unset
- **THEN** `ft tui` launches successfully with the valid keymap entries applied and the invalid entry logged as a warning

#### Scenario: strict=true blocks startup
- **WHEN** the user's `config.toml` sets `keymap.strict = true` and has one invalid entry
- **THEN** `ft tui` exits with code 1 and stderr describes the offending entry

### Requirement: `ft commands check-keymap` lints the user's `[keymap]` table

A new `ft commands check-keymap` subcommand SHALL run the same validator the App uses, report every error to stderr (text or JSON per `--format`), and exit 0 on a clean keymap or 2 on any validation error. The subcommand SHALL NOT instantiate an App or render the TUI.

#### Scenario: Clean keymap exits zero
- **WHEN** the user's `[keymap]` table parses cleanly against the registry
- **THEN** `ft commands check-keymap` writes nothing to stderr and exits with code 0

#### Scenario: Dirty keymap exits two
- **WHEN** the user's `[keymap]` table contains at least one invalid entry
- **THEN** `ft commands check-keymap` writes a human-readable error report (or `--format json` array) to stderr and exits with code 2

#### Scenario: JSON format for scripts
- **WHEN** the user runs `ft commands check-keymap --format json` against a dirty keymap
- **THEN** stderr contains a JSON array of `{ "scope": "...", "chord": "...", "error": "...", "raw": "..." }` objects

### Requirement: Layered config: vault `[keymap]` replaces user `[keymap]` whole

`Config::load(user, vault)` SHALL treat the `[keymap]` table atomically — if both files define `[keymap]`, the vault's table replaces the user's table entirely (no per-entry merging). If only one defines it, that one is used. Documented behaviour so users know how to scope per-vault rebindings.

#### Scenario: Vault config wins on conflict
- **WHEN** the user-level `config.toml` defines `[keymap.global]` with `"Ctrl+s" = "app.sync-git"` and the vault-level config defines its own `[keymap.global]` with `"Ctrl+f" = "graph.search"`
- **THEN** only the vault-level table is applied; the user-level `Ctrl+s` binding is not present in the effective global keymap

#### Scenario: Vault inherits user keymap when vault has no `[keymap]`
- **WHEN** the user-level `config.toml` defines `[keymap]` and the vault-level config has no `[keymap]` section
- **THEN** the user-level table is applied verbatim at App startup
