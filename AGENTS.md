# CLAUDE.md

`ft` is a Rust CLI + TUI over an Obsidian vault, focused on the Tasks-plugin
emoji format. This file is the quick map; deeper detail lives in
[docs/architecture.md](docs/architecture.md) and [README.md](README.md).

## Workspace shape

Two crates, one workspace:

- `ft-core/` — the brain. Vault discovery, config, task model + emoji
  format, scan, unified query DSL (`graph::query`), mutation ops, link
  graph, timeblocks, dates, atomic writes, periodic-note resolution.
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
  consumers see; `EmojiFormat` is the v1 impl. The ops layer is
  format-parametric (every `task::ops` entry point takes
  `format: &dyn TaskFormat`); callers with a `Vault` pass
  `vault.task_format()` — the future config-driven detection seam —
  rather than naming a concrete format. Line-addressed mutations also
  take an expected-`Task` guard and fail with `LineChanged` when the
  line shifted on disk; pass the scanned task, `None` only when no
  faithful `Task` is at hand.
- **`FT_TODAY=YYYY-MM-DD`** overrides "today" for date parsing, query
  DSL keywords, and the TUI clock. Use it in tests and reproducible
  scripts. The intended seam is `ft_core::dates::today()` (and
  `now_pair()` when a time is also needed); most call sites read through
  it. A few places read `FT_TODAY` directly instead of going through
  `dates::today()` — before adding a new "today" reader, `rg FT_TODAY`
  and reuse `dates::today()` rather than another inline `std::env::var`.
- **`FT_VAULT`** + `--vault` + walk-up-for-`.obsidian/` + user config
  default is the discovery precedence; surface every rung tried on
  failure.
- **Vault-relative paths in user-facing output and errors** (not absolute).
- **TUI concurrency** is single-threaded except for two producer threads
  (crossterm input + background workers) feeding one `mpsc::Receiver`.
  No async runtime, no `Mutex<AppState>`. Workers own their inputs, post a
  `BgEvent::*` back, and exit. In-flight state lives on `App` as a typed
  `RefCell<Option<...>>` slot. The git-sync worker is the reference impl.
- **Shared graph snapshot (TUI).** One App-owned `Arc<GraphSnapshot>`
  (graph + scan + generation) is the only graph in the TUI; tabs read it
  via `TabCtx::snapshot` and **never** run `vault.scan()`/`Graph::build`
  themselves. After a mutation, raise `ctx.request_graph_refresh()` (and
  store a pending cursor anchor if needed); a background worker rebuilds
  and tabs re-derive on generation change (`on_graph_ready` / `on_focus`).
  Tests pump with `App::pump_graph_rebuild_for_test()`. See
  `docs/architecture.md` §"Shared graph snapshot".
- **Unified query DSL with profiles.** One parser (`graph::query::parse_with`)
  drives both task and graph queries. `Profile::Tasks` prepends an
  implicit `node where kind = Task and …` block so users can keep
  typing `priority = High and due < today`. `Profile::Default` is the
  verbose graph syntax. Same preset pattern as before: a
  `builtin(name) -> Option<&str>` + `builtin_names()` table per DSL
  consumer, user presets shadow built-ins, CLI `--preset <name>`
  resolution (user config → built-in → exit 2 on unknown). Task
  presets live in `Config::presets`; graph presets in
  `GraphCfg::presets`; quick-capture presets in `Config::capture_presets`
  (`CapturePreset`, drives the TUI `Q` flow) — three separate maps
  because the three consumers default to different profiles/shapes.
- **Synthesis ritual (`link_review` + `journal` + `synth`).** The
  post-connecting workflow: `ft_core::link_review::compute_link_review`
  (git-log wikilink scan), `ft_core::journal::build_journal` (multi-source
  git-blame feed), and `ft_core::synth::{scaffold,verify,repair,reslice,callout}`
  (plan/apply synth notes with `[!ft-source]` callouts pinned to git
  provenance). Synth notes carry an `ft-synth: true` frontmatter marker;
  callouts round-trip via `synth::callout::{serialize,parse,compute_section_hash}`.
  CLI: `ft review`, `ft synth`, `ft notes journal --link`. See
  `docs/architecture.md` §"Synthesis ritual".
- **Signature changes on core APIs.** A new param to a widely-called
  function (e.g. `Graph::build`) ripples through every test file. Before
  making such a change, grep for callers and consider a compatibility
  helper or struct-params pattern. Flag the test-ripple cost in the
  design phase so the effort is budgeted.
- **Model structs implement `Default`.** Structs with many optional
  fields (e.g. `Task`, `LinkEdge`) should derive or impl `Default`
  so tests can construct them with `..Default::default()`. This keeps
  test code resilient when new fields are added.
- **Command/Keymap registry (TUI).** Every TUI action is a stable
  `<context>.<verb>` `Command` with `CommandDef` metadata; every key
  binding is a row in a `KeyMap`. `ft/src/tui/command.rs` (registry) +
  `keymap.rs` (chords) are the single source of truth — the `?` overlay,
  `docs/keybindings.md`, `ft commands list`, and `ft do` all read it. Keys
  resolve modal → tab → global. See `docs/commands.md`.
- **Modal driver (TUI).** One `Modal` trait (`ft/src/tui/modal.rs`) and a
  single `RefCell<Option<ActiveModal>>` slot on `App` back every
  overlay/popup/multi-step flow. Dispatch precedence is **modal first,
  tab second, App-global third**; `ModalOutcome`
  (Consumed/Closed/OpenSibling/NotHandled) controls flow. Before adding a
  multi-step TUI flow, read `docs/architecture.md` §"Modal driver" and
  prefer a new `ActiveModal` variant over a per-tab `Option<...>` field.

## Build invariants

Every change must keep all five clean:

```sh
cargo build --release
cargo test --workspace
cargo clippy --workspace --tests -- -D warnings
cargo fmt --check
cargo run --release -q -- commands docs --check   # keybindings.md in sync with the registry
```

After touching any `CommandDef` slice or keymap, regenerate the
committed reference: `cargo run --release -q -- commands docs > docs/keybindings.md`.

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
- **New TUI tab:** implement `Tab` (in `ft/src/tui/tab.rs`) including
  the required `kind() -> TabKind` routing key; read graph/task data
  from `ctx.snapshot` (never `vault.scan()`/`Graph::build`) and raise
  `ctx.request_graph_refresh()` after mutations. Declare
  `<TAB>_COMMANDS` + `<TAB>_KEYMAP` static slices next to it, push it
  into `build_tabs_with_overlays` in `ft/src/tui/app.rs` wrapped as
  `Box::new(<Tab>::new().with_keymap_overlay(&<tab>_overlay))` — the
  overlay line is mandatory or user `[keymap]` overrides / `ft commands
  check-keymap` silently miss your tab. Override `help_sections()` so the
  `?` overlay shows your keymap, add a `dispatch_command` arm, and add
  a `TestBackend` snapshot under `ft/src/tui/tests/`. Re-run
  `ft commands docs > docs/keybindings.md`.
- **New task format:** new module under `ft-core/src/task/`, implement
  `TaskFormat`, wire format detection in `vault::parse_file` and
  `Vault::task_format()`, round-trip property test
  (`serialize(parse(line)) == line`). The ops layer needs no changes —
  it already takes `&dyn TaskFormat`. (Today `parse_file` hard-codes
  `EmojiFormat` with no priority loop; a config-driven detection order
  is the future plug-in shape, not the current code.)
- **New output format:** new module under `ft/src/output/`, variant on
  `output::Format`, wire it into `ft/src/cmd/tasks.rs::run_list`.
- **New graph preset:** add entry to `ft_core::graph::preset::builtin()`
  and `builtin_names()`, add a round-trip parse test. CLI `--preset`
  and TUI quick-pick pick it up automatically. Same pattern applies
  to task presets (`ft_core::query::preset`).

## Conventions to keep

- `thiserror` enums in `ft-core`; `anyhow::Context` in the binary.
- Color via `owo-colors`, auto-off when stdout isn't a TTY, when
  `NO_COLOR` is set, or with `--no-color`.
- `--json-errors` at the top level for scriptable error output.
- Don't add backwards-compat shims, dead `_var` renames, or
  `// removed X` markers — just delete unused code.
- Comments only when the *why* is non-obvious. Don't narrate the *what*.

## Change workflow (openspec)

Non-trivial changes are managed through [openspec](openspec/) — a
propose → apply → archive workflow with specs, designs, and task
lists under `openspec/changes/` (active) and `openspec/changes/archive/`
(done). `openspec/specs/` holds the capability specs. Before a large
change, check `openspec/changes/` for prior context and consider
running the proposal flow (`.pi/skills/openspec-propose`) rather than
ad-hoc editing. Skills: `openspec-propose`, `openspec-apply-change`,
`openspec-archive-change`, `openspec-explore`.
