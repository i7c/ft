## Why

`ft.nvim` grew organically — follow, completion, and embeds each shell out to
the `ft` CLI ad hoc, with no documented contract between the plugin and the
CLI, no unified state/freshness model, and no concurrency story for the fact
that nvim is most often launched *from inside the ft TUI* (where the TUI is
parked for the whole editor session and force-refreshes on return). The
plugin is about to grow text-heavy, performance-sensitive features
(in-place tasks/subtasks, Gather, synth notes) that would expose every one
of those gaps. Before adding features, the plugin needs a deliberate
architecture: **all domain logic stays in ft; the plugin is editor glue
plus a transport.** This change establishes that architecture, migrates
existing functionality onto it, removes the one feature that doesn't fit
(embed rendering), and documents the decisions in the ft.nvim repository.

## What Changes

All work lands in the `ft.nvim` repository (sibling of this one; the
openspec change lives here as the single coordination artifact). No
changes to `ft`/`ft-core` in this change — new ft-side surfaces are
deferred until the features that need them are built.

- **New transport seam `ft.rpc`** — the one way the plugin talks to ft.
  Synchronous `call()` for fast ops (task create/complete, follow,
  find), asynchronous `job()` for expensive ones (gather, pulse, full
  index rebuilds) via `jobstart`, with single-flight slots per job kind
  (mirrors the TUI's `GraphJob`/`jobs` pattern). Bin resolution honors
  `FT_BIN` (dev builds) before `ft` on PATH. Every existing shell-out
  migrates onto it; nothing else calls `vim.fn.system` for ft.
- **New `ft.picker` seam** — `select(items, opts)` / `multi(items, opts)`
  with `vim.ui.select` as the default backend and telescope/fzf-lua
  feature-detected when installed. `multi` is declared but not
  implemented until a feature requires it (Gather multi-send will).
- **State & freshness model** — caches are derived, never authoritative;
  invalidation is event-driven (`BufWritePost`/`BufDelete`/own mutations
  mark dirty), a generation counter makes stale snapshots detectable,
  and no cache survives a mutation without a rebuild. Mirrors the TUI's
  shared-graph-snapshot invariant (tabs consume, never build).
- **TUI-handoff concurrency contract** — documented and encoded: nvim
  launched from the ft TUI inherits `FT_VAULT`; the TUI is parked while
  the editor is open and force-refreshes on exit, so the plugin never
  needs (and must never attempt) a live-ft-process channel. Fresh ft
  child processes spawned by nvim are safe; the expected-task guard is
  the consistency mechanism, not locking.
- **BREAKING — embeds removed.** `![[Other Note]]` inline rendering
  (`lua/ft/embed.lua`, the `embeds`/`max_lines` config keys, README
  claims) is deleted entirely. The feature is no longer supported; the
  README states this explicitly.
- **`ARCHITECTURE.md` in ft.nvim** — the durable record: the four
  pillars above, the protocol contract (every ft command + flags +
  output format the plugin depends on), the decision log (why
  CLI-per-op, why no server, why embeds removed, why the picker seam),
  and "when to add an ft command vs Lua" guidance.
- **`min_ft_version` soft check** — plugin-side, at setup: reads
  `ft --version`, warns (does not fail) when the installed binary
  predates a version the protocol contract requires.

## Capabilities

### New Capabilities
- `nvim-plugin`: the ft.nvim plugin's architecture as a spec —
  transport seam (`ft.rpc`), picker seam (`ft.picker`), derived-cache
  freshness model, the TUI-handoff concurrency contract, the ft CLI
  protocol contract, and the no-embeds invariant.

### Modified Capabilities
- None — no `ft`/`ft-core` spec-level behavior changes in this change.

## Impact

- `ft.nvim` repository only: `lua/ft/` (new `rpc.lua`, `picker.lua`;
  migrated `vault.lua`, `cache.lua`, `complete.lua`, `blink.lua`,
  `follow.lua`; deleted `embed.lua`), `init.lua` (setup wiring, embeds
  removal), `README.md` (feature list, install/config docs),
  new `ARCHITECTURE.md`.
- No changes to `ft` or `ft-core`; no `docs/keybindings.md` regeneration.
- External contract: users with `embeds = true` in their config lose the
  feature (config key is ignored/removed); users with a too-old `ft`
  binary get a setup warning.
