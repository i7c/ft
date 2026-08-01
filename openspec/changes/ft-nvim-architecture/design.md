# ft-nvim-architecture — design

## Context

`ft.nvim` (a sibling repo, `git@github.com:i7c/ft.nvim`) is a Neovim plugin
that currently does follow-wikilinks, `[[`-completion (blink.cmp +
omnifunc), and inline `![[embeds]]` rendering by shelling out to the `ft`
CLI through one ad-hoc helper (`vault.ft_run` → `vim.fn.system`). There is
no documented contract with the CLI, no unified state model, and no
concurrency story. The next features on the roadmap — in-place task and
subtask creation, a Gather flow, synth notes — are text-heavy and
git-blame-heavy (seconds of work), which would expose every gap.

Two external constraints shape everything:

1. **The plugin is a client of a CLI.** The `ft` binary is already a
   required dependency (`executable('ft')`), so an "external binary on
   PATH" model is the plugin's native shape, not a compromise.
2. **nvim is usually launched from inside the ft TUI.** The TUI's editor
   handoff (all strategies — suspend, tmux popup/window/split) parks the
   TUI process for the whole editor session: it blocks waiting for the
   editor to exit and then *unconditionally force-refreshes its graph
   snapshot* (`dispatch_open_in_editor`: "Whatever the editor did, force a
   refresh so the active tab reflects on-disk state"). The editor child
   inherits `FT_VAULT`.

## Goals / Non-Goals

**Goals:**
- All plugin↔ft communication flows through one transport seam
  (`ft.rpc`); nothing else touches the `ft` process.
- Zero domain logic in Lua: no task-line parsing/serialization, no
  query evaluation, no callout serialization, no date math. Lua only
  wires ft's outputs to the editor.
- A freshness model with the same shape as the TUI's shared-graph
  snapshot: caches are derived, invalidated on events, generation-tagged;
  nothing authoritative lives in the plugin.
- The TUI-handoff concurrency contract stated in code and docs: the
  plugin never requires a live ft process to respond, because in the
  suspend strategy the TUI *cannot* respond while nvim is open.
- A picker seam (`ft.picker`) so the backend choice is deferred and
  reversible per feature.
- Embed rendering removed entirely (**BREAKING**).
- `ARCHITECTURE.md` in ft.nvim recording all of the above plus the ft
  CLI protocol contract the plugin depends on.

**Non-Goals:**
- No changes to `ft`/`ft-core` in this change. The deferred ft-side
  surfaces (line selectors on `tasks complete`/`cancel`, a `synth
  --print` render-only mode, an `FT_TUI` env marker) are added when the
  features that need them are built.
- No ft server / daemon mode. CLI-per-op is the model until it is
  measured to be the bottleneck.
- No new features this pass (tasks-in-place, Gather, synth flows land as
  separate changes on top of this architecture).
- No telescope/fzf-lua dependency this pass; `ft.picker` defaults to
  `vim.ui.select` (which delegates to the user's own `vim.ui.select`
  override when they have one).
- No plugin test harness this pass (flagged as a follow-up; the design
  keeps the seams in place so a headless harness can be added later).

## Decisions

### D1. Domain logic lives in ft; the plugin is editor glue + transport

Every operation that produces or transforms domain data (task lines,
queries, callouts, dates) is performed by a fresh `ft` process; Lua
inserts/splices/selects what ft returned.

**Alternatives considered:**
- Reimplement the emoji task format and query DSL in Lua: rejected. ft's
  serializer is canonical (field order, `✅ YYYY-MM-DD` dates, priority
  emoji), the ops layer carries an expected-task guard that fails with
  `LineChanged` on any mismatch, and `resolve_hierarchy` owns parent/child
  structure. Hand-written task lines in Lua would be latent round-trip
  failures — the exact drift this architecture exists to prevent.
- Partial: only "simple" ops (toggle a checkbox) in Lua: rejected. The
  line between simple and subtle is exactly where bugs live (recurrence
  insertion, completion dates, emoji canonicalization). One rule, no
  judgement calls.

### D2. CLI-per-op through a swappable transport seam (`ft.rpc`); no server

`ft.rpc.call(args)` today = spawn `ft` (or `FT_BIN`), capture stdout,
return `(stdout, exit_code)`. Every feature module imports `rpc`, never
`vim.fn.system` directly. A future stdio JSON-RPC mode would implement the
same signature; nothing else changes.

**Alternatives considered:**
- Long-running `ft server` spawned by the plugin at setup: rejected for
  now. Process spawn is 1–2 ms; the real cost (vault scan) is already
  amortized by the note cache. A server's in-memory graph would also go
  stale exactly like the TUI's does — it solves latency, not freshness —
  so it only pays off when per-op latency is measured to matter. The rpc
  seam keeps that option open without paying for it today.
- Talking to the *running* TUI process: rejected as a dead end — in the
  suspend strategy the TUI is blocked in `Command::status()` and cannot
  answer IPC until nvim exits; any design requiring a live ft response
  during an editor session deadlocks.

### D3. Two transport tiers inside `ft.rpc`: sync `call` + async `job`

- `rpc.call(args)` — synchronous, for single-digit-ms ops (follow
  resolution, task create/complete at cursor, find, note-index rebuild is
  async). Blocks nvim briefly; acceptable.
- `rpc.job(args, on_done)` — `vim.fn.jobstart`, for second-scale ops
  (gather, pulse, full index rebuilds). Results posted back via a `User
  ft:rpc-done` autocmd; **single-flight per job kind** (a `busy` slot per
  kind, mirroring the TUI's `App.jobs` and `GraphJob { in_flight, dirty }`
  coalescing) so two Gathers can't stack two git blames.

**Rationale:** mirrors the TUI's "main loop + worker threads posting one
completion event" model; `vim.fn.system` blocking for seconds would freeze
nvim, which is not acceptable for the roadmap features.

### D4. Freshness: derived caches, event-driven invalidation, generation counter

- The note index (`ft.cache`) and the vault root (`ft.vault`) are the only
  caches, and both are *derived* — disk + git are the source of truth.
- Invalidation is event-driven, not timer-driven: `BufWritePost` /
  `BufDelete` on `.md` files inside the vault, plus the plugin's own
  mutations, mark the index dirty. A debounced, single-flight async
  rebuild happens on first use after dirty.
- A monotonic **generation counter** tags each cache build (same idea as
  `GraphSnapshot.generation`); anything that captured an older snapshot
  (an open picker, a stale completion session) detects the mismatch and
  re-derives at use time.

**Alternatives considered:**
- Always re-run ft at use time (no cache): rejected — completion would
  scan the vault per keystroke.
- Timer-based refresh: rejected — stale windows and redundant scans
  without any guarantee of correctness at the moment that matters (the
  mutation the plugin itself just made).

### D5. Picker seam (`ft.picker`): `vim.ui.select` default, backends feature-detected

`ft.picker.select(items, opts)` → `vim.ui.select` by default; if telescope
or fzf-lua is installed and enabled by the user, the plugin uses that
backend instead. `ft.picker.multi(items, opts)` is *declared but not
implemented* until a feature requires multi-select (Gather's
send-selected-entries-to-synth will).

**Rationale:** `vim.ui.select` is a delegation point — most picker setups
override it globally (telescope, dressing, fzf-lua all ship overrides), so
the zero-dep default automatically becomes "whatever picker the user
configured." The API is single-choice and preview-less, which is fine for
the current features (follow target picks, task picks); Gather's
paragraph preview + multi-select will force the backend decision then,
and the seam keeps that decision local and reversible.

**Alternatives considered:**
- Pick telescope now: heaviest dep, most verbose code, API churn; nothing
  current needs its previewers.
- Pick fzf-lua now: excellent fuzzy feel, but another external binary
  (`fzf`) and nothing current needs it either.
- Write a custom mini-picker: full control but ~300–600 LOC of
  permanently-ours code — violates D1's spirit (don't reimplement what a
  tool already does).

### D6. Embeds are removed (**BREAKING**)

`lua/ft/embed.lua` (viewport-tracked inline rendering of `![[notes]]` with
gutter + indent), the `embeds` / `max_lines` config keys, and the README
claims are deleted. The README states the feature is no longer supported.

**Rationale:** embeds were the one feature that made the plugin a Markdown
*renderer* — a role ft explicitly disclaims ("Not a Markdown renderer").
It was also the highest-maintenance surface (per-buffer viewport tracking,
gutter layout, per-note file reads), duplicated Obsidian behavior that
Obsidian itself does better, and added nothing to the domain logic in ft
that the plugin's other features build on. Removing it shrinks the
maintenance surface and makes the plugin's philosophy uniform.

### D7. TUI-handoff concurrency contract

Documented and encoded: (a) nvim launched from the ft TUI inherits
`FT_VAULT` — discovery is free, and the plugin's existing precedence
(env first) already handles it; (b) the TUI is parked while the editor is
open and force-refreshes its snapshot on exit, so the plugin never
notifies the TUI of changes — there is no channel and must not be one;
(c) fresh ft child processes spawned by nvim are safe — the worst case is
two processes touching the vault concurrently, which is the normal
"external editor + CLI" situation ft already handles via the
expected-task guard; the plugin adds no locking.

### D8. Repository structure: two repos, openspec in ft, protocol as contract

ft.nvim stays a standalone repo (it is installed by repo URL / `dir=` via
lazy.nvim — a hard distribution constraint). The openspec change in ft is
the single coordination artifact; task lists mark where each task lands
(`[ft.nvim]`). The durable docs live in ft.nvim's `ARCHITECTURE.md`.
Coupling mechanisms, in place of a submodule: (a) a **protocol contract
table** in ft.nvim listing every ft command + flags + output format the
plugin depends on; (b) a **`min_ft_version` soft check** at setup (reads
`ft --version`, warns on too-old binaries — plugin-side, no ft change);
(c) **`FT_BIN` env var** honored by `rpc` so development points at a
freshly built `ft/target/release/ft` without `cargo install`.

**Alternatives considered:**
- Submodule ft.nvim inside ft: rejected — a submodule is a second copy of
  a repo whose canonical artifact must remain standalone for
  distribution; it couples source layout when the real contract is the
  CLI protocol at a version; every plugin commit becomes status noise in
  ft.
- Monorepo: rejected — breaks the lazy.nvim install URL.
- openspec in both repos: rejected as overkill — coordination belongs in
  one place; can be revisited if ft.nvim outgrows its consumer role.

## Risks / Trade-offs

- [Lua hand-editing task lines drifts from ft's canonical serializer →
  the expected-task guard (`LineChanged`) trips on the next op] → All
  task-producing/transforming mutations go through `ft` CLI commands;
  Lua's text edits are structural only (cursor, indent, splice). The
  guard is a safety net, not a workflow.
- [`vim.fn.system` blocks nvim for seconds on gather/pulse] → The async
  tier is not optional for those features; single-flight prevents stack
  blames.
- [Per-op process spawn overhead accumulates in tight loops (completion
  keystrokes)] → The note index cache makes completion one ft call per
  session, not per keystroke; revisit a server only if measured to be the
  bottleneck.
- [Two ft processes touching the vault concurrently (TUI parked, plugin
  job running, user editing in a tmux pane)] → All plugin reads are
  read-only (queries, gather); task mutations carry the guard; no locks
  added.
- [Embeds removal is user-visible] → README states it explicitly; the
  config key is dropped (ignored) rather than erroring.
- [`min_ft_version` warning annoys users on older binaries] → Soft warning
  only, never a hard failure; the protocol contract table makes the
  requirement legible.
- [Two-repo dance (commit ft.nvim, record sha in ft openspec) is
  discipline, not tooling] → Archive step records the paired sha; the
  openspec change is the shared ledger.

## Migration Plan

All steps in the `ft.nvim` repo; this openspec change tracks them. Order:

1. **Add `lua/ft/rpc.lua`** — bin resolution (`FT_BIN` → `ft`), `call()`
   (sync), `job()` (async, single-flight slots, `User ft:rpc-done`
   delivery), exit-code→notification error normalization, `--vault`
   injection.
2. **Migrate existing modules onto `rpc`** — `vault.lua` (drop
   `ft_run`/`ft_cmd` in favor of `rpc`; keep discovery), `cache.lua`
   (rebuild via `rpc.job`), `complete.lua` + `blink.lua` (query via
   `rpc`), `follow.lua` (via `rpc.call`).
3. **Add `lua/ft/picker.lua`** — `select()` over `vim.ui.select` with
   telescope/fzf-lua feature detection; `multi()` declared, raising a
   clear "not yet implemented" error.
4. **Remove embeds** — delete `embed.lua`, strip setup wiring + config
   (`embeds`, `max_lines`), update README (features, config, explicit
   "embeds no longer supported").
5. **Add `min_ft_version` check** in `setup()`.
6. **Write `ARCHITECTURE.md`** — the pillars, protocol contract table,
   decision log, "when to add an ft command vs Lua".
7. **Smoke test** — run nvim against the fixture vault (a temp
   `.obsidian/` vault + a couple of notes), exercising follow,
   completion, and the config-path with `embeds` removed; verify no
   `vim.fn.system` calls remain except inside `rpc` (grep guard).

**Rollback:** the change is additive except the embeds removal; reverting
the ft.nvim commit restores prior behavior. No data migration — nothing in
the vault changes.

## Open Questions

- Whether `ft.picker.multi()` should exist as a stub now or be introduced
  with the first multi-select feature (currently: stub, per D5).
- Naming/scope of the `FT_BIN` dev override (env var only, or also a
  setup option) — env var only for now.
- The headless test harness is out of scope this pass but the seams keep
  it possible; confirm it is tracked as a follow-up change.
