## Why

The `commands-and-keymaps` change made every TUI action a registered `Command` with a stable `<context>.<verb>` name and the `KeyMap` data shape to bind chords to it — but bindings are still compiled-in. A user who wants `Ctrl+s` to mean `app.sync-git` or who wants vim-style `gg/G` on the graph tab has to fork the binary. The `?` overlay, `docs/keybindings.md`, `ft commands list`, and `ft do` all already read from the same registry; the chord side is the one piece the user can't override. This change adds a `[keymap]` table to the existing `config.toml` so users can rebind any registered command without touching source.

## What Changes

- Extend `Config` (in `ft-core/src/config.rs`) with a new `[keymap]` table. Two sub-tables: `[keymap.<scope>]` for chord-to-command entries (where `<scope>` is `global`, `tab/<name>`, or `modal/<name>`), and a top-level `keymap.unbind` array for explicitly removing default bindings.
- Add `KeyMap::with_overlay(base: &KeyMap, overlay: &[KeymapOverride], registry: &CommandRegistry) -> Result<KeyMap, KeymapOverlayError>` in `ft/src/tui/keymap.rs`. Applies the overlay on top of a base map, validating every chord parses and every command name exists in the registry. Overrides replace existing chord bindings; unbinds remove them; new chord-to-command pairs are appended.
- Wire `App::new` to load the user's `[keymap]` table once at startup, build an effective `KeyMap` per scope by overlaying the static `APP_KEYMAP` / `<TAB>_KEYMAP` / `<MODAL>_KEYMAP`, and store the effective maps on the `App` / per-tab / per-modal slots (replacing direct returns of the static `LazyLock<KeyMap>`).
- `Tab::keymap(&self) -> &KeyMap` and `Modal::keymap(&self) -> &KeyMap` continue to return a borrow; the underlying storage moves from a `LazyLock<KeyMap>` to a field on the tab/modal/App so per-instance overlay results are reachable. Default trait impls keep returning `empty_keymap()`.
- New CLI subcommand `ft commands check-keymap` (under the existing `ft commands` group) parses the user's `[keymap]` table against the live registry and reports every error (unknown chord, unknown command, collision within a scope, attempt to override a chord that doesn't exist when paired with `unbind`). Exit 0 on a clean keymap, exit 2 on any error. Useful as a pre-commit hook and as a CI sanity check.
- The `?` overlay, `docs/keybindings.md` generator, and `ft commands list` keep working unchanged — they read whatever `keymap()` returns, so the user's overrides flow through automatically. The generator writes whatever the *defaults* are (the static maps); user overrides only affect runtime.
- New docs section in `docs/config.md` documenting the `[keymap]` schema with examples; cross-link from `docs/commands.md`.

## Capabilities

### New Capabilities

- `keymap-config`: User-editable `[keymap]` table in `config.toml`. Defines the schema (`[keymap.<scope>]` sub-tables + `keymap.unbind` array), the overlay semantics (override / append / unbind), the validation contract (`ft commands check-keymap`), and the merge order against the static defaults.

### Modified Capabilities

- `tui-keymaps`: The "Key bindings live in scoped `KeyMap` data" requirement gains a sub-requirement covering runtime overlays from `[keymap]`. Effective `KeyMap`s (the ones tabs/modals/App return from `keymap()`) become the static defaults overlaid with the user's `[keymap]` entries. Collision detection and chord-to-command resolution stay byte-identical; the data source widens.

## Impact

- **Modified**: `ft-core/src/config.rs` (new `[keymap]` schema + parse), `ft/src/tui/keymap.rs` (overlay constructor + error enum), `ft/src/tui/app.rs` (load + apply overlays at startup, store effective maps), `ft/src/tui/tab.rs` + every `Tab` impl (per-instance keymap storage instead of `LazyLock` return), `ft/src/tui/modal.rs` + every `Modal` impl (same), `ft/src/cmd/commands.rs` (new `check-keymap` subcommand).
- **Tests**: overlay unit tests (override / append / unbind / unknown-command / unknown-chord / collision), config parser tests for the new schema, an integration test that loads a fixture vault with a `config.toml` containing a `[keymap]` override and asserts the chord fires the override command via the registry, a `check-keymap` subcommand test (clean + dirty fixtures), and a TUI snapshot of the `?` overlay rendering an overridden binding.
- **Docs**: new `[keymap]` section in `docs/config.md`; cross-link from `docs/commands.md`. No new docs file — the existing two cover the topic well.
- **Build invariants**: All four (`build --release`, `test --workspace`, `clippy --workspace --tests -- -D warnings`, `fmt --check`) stay green.
- **Non-goals (explicit)**: No vim-style multi-chord sequences in this change (still out of scope, per `commands-and-keymaps` design.md). No GUI keymap editor. No per-vault keymap distinct from the user/global config — overrides come from the same layered `Config` `commands-and-keymaps` already merges. No new commands (rebinding only; new commands ship via `commands-and-keymaps`-style PRs).
