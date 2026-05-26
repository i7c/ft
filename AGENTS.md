# CLAUDE.md

`ft` is a Rust CLI + TUI over an Obsidian vault, focused on the Tasks-plugin
emoji format. This file is the quick map; deeper detail lives in
[docs/architecture.md](docs/architecture.md) and [README.md](README.md).

## Workspace shape

Two crates, one workspace:

- `ft-core/` — the brain. Vault discovery, config, task model + emoji
  format, scan, query DSL, mutation ops, link graph, timeblocks, dates,
  atomic writes, periodic-note resolution.
- `ft/` — thin binary. Clap parsing, output rendering, TTY concerns,
  editor handoff, and the TUI (`ft/src/tui/`).

The TUI depends only on `ft-core`. If the TUI needs something new, add it
to `ft-core` first so the CLI benefits too.

## Load-bearing patterns

- **Plan/apply split for mutations.** Pure planners produce a `*Plan`
  struct of file edits (`task::ops::plan_move`, `graph::rename::plan_rename`,
  …); a separate `apply_*` step writes each file via
  `ft_core::fs::write_atomic` (same-dir tempfile + rename, preserves mode).
  Never write directly — always go through `write_atomic`. Same-file edits
  apply in descending byte order so earlier ranges stay valid.
- **Format trait seam.** `task::format::TaskFormat` is the only thing
  consumers see; `EmojiFormat` is the v1 impl. A new wire format plugs in
  here without touching the scanner, ops layer, or query engine.
- **`FT_TODAY=YYYY-MM-DD`** overrides "today" everywhere — date parsing,
  query DSL keywords, the TUI clock. Use it in tests and reproducible
  scripts; everything that resolves "today" reads through one seam.
- **`FT_VAULT`** + `--vault` + walk-up-for-`.obsidian/` + user config
  default is the discovery precedence; surface every rung tried on
  failure.
- **Vault-relative paths in user-facing output and errors** (not absolute).
- **TUI concurrency** is single-threaded except for two producer threads
  (crossterm input + background workers) feeding one `mpsc::Receiver`.
  No async runtime, no `Mutex<AppState>`. Workers own their inputs, post a
  `BgEvent::*` back, and exit. In-flight state lives on `App` as a typed
  `RefCell<Option<...>>` slot. The git-sync worker is the reference impl.
- **`.devplans/`** holds per-feature plans (id, status, sessions log).
  New features get a plan there; ongoing work updates the matching
  Sessions entry. Use the `devplan` skill.

## Build invariants

Every change must keep all four clean:

```sh
cargo build --release
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
cargo fmt --check
```

## Testing strategy

- Unit tests live with their module (`#[cfg(test)] mod tests`).
- Integration tests under `ft/tests/` use `assert_cmd` + `assert_fs`
  against fixture vaults built in temp dirs.
- Fixture vaults under `tests/fixtures/`: `tiny/`, `realistic/`,
  `pathological/`.
- `insta` snapshot tests for every stable output format and for TUI
  frames via `ratatui::backend::TestBackend`.
- `proptest` round-trips for the emoji parser/serializer.
- Real-vault tests (`ft-core/tests/real_vault.rs`,
  `ft/tests/real_vault_cli.rs`) gated on `FT_REAL_VAULT_TESTS=1` —
  never on by default; they touch `/Users/cmw/git/fortytwo`.
- Perf tests gated on `FT_PERF_TESTS=1`.

## Where to add things

- **New subcommand:** add `ft/src/cmd/<name>.rs`, register the module,
  add the variant to `Commands` in `ft/src/main.rs`, dispatch. Use
  `Vault::discover(vault_flag)?` + `vault.scan()` for vault data.
- **New TUI tab:** implement `Tab` (in `ft/src/tui/tab.rs`), push it into
  the `tabs` vec in `App::new`, override `help_sections()` so the `?`
  overlay shows your keymap, add a `TestBackend` snapshot in
  `ft/src/tui/tests.rs`.
- **New task format:** new module under `ft-core/src/task/`, implement
  `TaskFormat`, wire format detection in `vault::parse_file`, round-trip
  property test (`serialize(parse(line)) == line`).
- **New output format:** new module under `ft/src/output/`, variant on
  `output::Format`, wire it into `ft/src/cmd/tasks.rs::run_list`.

## Conventions to keep

- `thiserror` enums in `ft-core`; `anyhow::Context` in the binary.
- Color via `owo-colors`, auto-off when stdout isn't a TTY, when
  `NO_COLOR` is set, or with `--no-color`.
- `--json-errors` at the top level for scriptable error output.
- Don't add backwards-compat shims, dead `_var` renames, or
  `// removed X` markers — just delete unused code.
- Comments only when the *why* is non-obvious. Don't narrate the *what*.
