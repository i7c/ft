## 1. Config schema (`ft-core`)

- [x] 1.1 Add `KeymapConfig` and `KeymapUnbindEntry` structs in `ft-core/src/config.rs` with serde derives. `KeymapConfig` holds `strict: bool` (default false), `unbind: Vec<KeymapUnbindEntry>` (default empty), and `scopes: HashMap<String, HashMap<String, String>>` (the `[keymap.<scope>]` sub-tables). `KeymapUnbindEntry` holds `scope: String, chord: String`. Both `pub`-exported.
- [x] 1.2 Add `keymap: Option<KeymapConfig>` field to `Config`. Layered merge: vault `[keymap]` replaces user `[keymap]` whole (no per-entry merge — documented in design §"Risks").
- [x] 1.3 Config-parser unit tests: empty TOML (None), `[keymap]` only (strict defaults to false), full table with overrides + unbinds + strict=true, vault-overrides-user replace semantics.

## 2. Overlay validator + applier (`ft/src/tui/keymap.rs`)

- [x] 2.1 New types: `KeymapOverlay { overrides: Vec<KeymapBinding>, unbinds: Vec<KeyChord> }`, `KeymapBinding { chord: KeyChord, command: Command }`, and `KeymapOverlayError` enum (`InvalidChord { raw, source }`, `UnknownCommand { name, scope }`, `UnbindMissing { chord, scope }`, `OverlayCollision { chord, first, second, scope }`). `thiserror::Error` so error messages are uniform.
- [x] 2.2 `KeymapOverlay::from_raw(raw_scope_table, raw_unbinds, &CommandRegistry, scope, base: &KeyMap) -> Result<KeymapOverlay, Vec<KeymapOverlayError>>`. Pure function — no I/O, no App, returns *all* errors rather than the first. Parses every chord via `chord_from_str`, looks up every command via `registry.lookup`, checks every unbind chord against `base`, and detects overlay-internal collisions via post-normalization equality.
- [x] 2.3 `KeyMap::with_overlay(&self, overlay: &KeymapOverlay) -> KeyMap`. Apply order: unbinds first (drop from base by chord), then overrides (replace if chord still exists post-unbind, append otherwise). Infallible — every error already caught at validate time.
- [x] 2.4 Unit tests: 12 cases — empty overlay round-trip, new chord append, replace existing chord, unbind without override, unbind-then-rebind to different command, unknown command name, invalid chord string, unbind-on-missing-chord, overlay-internal collision (two overrides on same chord), normalization collision (`Shift+c` and `C`), all-errors-not-just-first, vault-vs-user replacement semantics validated via `Config::load` integration.

## 3. Wire overlays into App / tabs / modals

- [x] 3.1 `App::new(...)` loads `Config::keymap` (defaulting to empty when None), builds one `KeymapOverlay` per scope using `KeymapOverlay::from_raw(...)`, and either propagates errors as `anyhow::Error` (when `strict=true`) or logs each via `tracing::warn!` + continues (when `strict=false`).
- [x] 3.2 `App` gains `effective_global_keymap: KeyMap` field; `App::global_keymap(&self) -> &KeyMap` returns `&self.effective_global_keymap` instead of `&APP_KEYMAP`. The static `APP_KEYMAP` stays as the default source for the overlay.
- [x] 3.3 Every `Tab` impl gains a `keymap: KeyMap` field, populated at construction time from `<TAB>_KEYMAP.with_overlay(&overlay_for_tab)`. The constructor signature changes from `Tab::new(...)` to `Tab::new(..., overlay: &KeymapOverlay)` (or a default-empty-overlay shim for tests). `Tab::keymap(&self) -> &KeyMap` returns `&self.keymap`.
- [x] 3.4 Every `Modal` impl gains a `keymap: KeyMap` field, populated when the modal is constructed (each `ActiveModal` variant gets the overlay handed in via the modal-open path — App passes the relevant per-modal overlay when servicing `AppRequest::OpenModal`). `Modal::keymap(&self) -> &KeyMap` returns `&self.keymap`.
- [x] 3.5 `App` stores `per_modal_overlays: HashMap<&'static str, KeymapOverlay>` (keyed by `Modal::name()`) so modal opens can grab the right overlay without re-validating. Built once at `App::new`.

## 4. CLI: `ft commands check-keymap`

- [x] 4.1 New `CheckKeymap(CheckKeymapArgs)` variant on `CommandsCommand` in `ft/src/cmd/commands.rs`. Args: `--format text|json` (default text). Honours top-level `--json-errors` automatically via `main.rs`'s error path.
- [x] 4.2 `run_check_keymap(args, vault_flag)` loads `Config` (same path App uses), runs `KeymapOverlay::from_raw` per scope, collects all errors, and reports per `--format`. Exit 0 on no errors; exit 2 on any error.
- [x] 4.3 Update `ft commands docs` registry walk to include the new `check-keymap` subcommand in the generated `docs/keybindings.md` CLI reference (mechanical).
- [x] 4.4 Unit tests: clean keymap → exit 0; dirty keymap → exit 2 + every error reported (text + json shapes).

## 5. CLI: `ft commands list --effective`

- [x] 5.1 Add `--effective` flag to `CommandsCommand::List`. When set, the lister composes per-scope effective keymaps (defaults + user overlay) and emits chord-to-command rows; default behaviour (unset) keeps emitting the registry-derived view as today.
- [x] 5.2 Unit test: list against a fixture vault whose `config.toml` overrides one binding; `--effective` shows the override, plain `list` does not.

## 6. Tests: end-to-end

- [x] 6.1 Integration test under `ft/tests/`: write a temp vault with `config.toml` containing `[keymap."tab/graph"]` `"R" = "graph.refresh"`; launch the App via the existing TUI test harness; assert the Graph tab's effective `keymap().lookup` for the R chord returns `graph.refresh`.
- [x] 6.2 Integration test: unbind a default chord via `[[keymap.unbind]]`; assert the chord lookup returns `None` in the effective map.
- [x] 6.3 TUI snapshot test: `?` overlay with a user override applied shows the new chord in the relevant section.
- [x] 6.4 Integration test: `keymap.strict = true` with a bad entry causes `App::new` to fail; default strict=false allows startup with a logged warning.

## 7. Docs

- [x] 7.1 New `[keymap]` section in `docs/config.md` documenting schema (per-scope sub-tables, unbind array, strict), examples (rebind, unbind, alias), and the "vault replaces user whole" merge rule. Includes the canonical list of valid scope strings (drawn from `CommandScope::as_str()`).
- [x] 7.2 Cross-link from `docs/commands.md` "Adding commands and keymaps" section: existing commands are user-rebindable via `[keymap]`; defaults still live in source.
- [x] 7.3 README.md `## Interactive TUI`: one-line note that bindings are user-configurable via `[keymap]` in `config.toml`, with a pointer to `docs/config.md`.

## 8. Build validation

- [x] 8.1 `cargo build --release` — clean
- [x] 8.2 `cargo test --workspace` — all tests pass; only deliberate snapshot re-blesses (none expected — overlays don't affect default-keymap snapshots).
- [x] 8.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 8.4 `cargo fmt --check` — clean
- [x] 8.5 `ft commands docs --check` — clean (the new subcommand should appear; regenerate first)
- [x] 8.6 `ft commands check-keymap` — exits 0 against the repo's own (empty) keymap; exits 2 against a fixture vault with a deliberately-broken `[keymap]`.
