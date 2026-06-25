# ft — Refactor Review

*2026-06-12*

> **As-of snapshot.** This review describes the codebase at the date
> above and has not been updated since; some of the duplications it
> flags may already be addressed. Treat it as a historical artifact,
> not a current to-do list. For the current architecture see
> `docs/architecture.md`.

Scope: API and internal data-model duplication, dirty implementations,
shaky premises. Looks for low-effort high-impact refactors that keep ft
clean, simple and reliable.

The codebase is in good shape overall. Most of what's listed is
*seam tidying* — none of it is structural rot. The plan/apply pattern,
`TaskFormat` trait, single-threaded TUI concurrency model, and the
unified graph DSL are all load-bearing and worth keeping intact.

## Top duplications worth extracting

### 1. `FT_TODAY` resolution — repeated 16+ times in three shapes

CLAUDE.md claims "everything that resolves 'today' reads through one
seam." It doesn't. The same five-line snippet appears in:

- `ft-core/src/link_review.rs:264`
- `ft/src/cmd/{tasks.rs ×4, notes.rs, do.rs, graph.rs, synth.rs, timeblocks.rs ×2}`
- `ft/src/tui/{app.rs:1153, tabs/timeblocks/mod.rs:1287, notes_actions/{periodic.rs, capture.rs, create.rs}}`

There's even a `today_now_from_env` helper in `cmd/notes.rs:959` whose
comment in `cmd/timeblocks.rs:638` literally reads *"Mirrors the helper
in cmd/notes.rs — kept local rather than shared because the two CLI
modules don't otherwise depend on each other."* That's an admission,
not an architectural decision.

**Fix.** Add `ft_core::dates::{today(), now_pair()}` (both honor
`FT_TODAY`). Delete every duplicate.

### 2. Markdown line-buffer primitives, duplicated three ways

`ft-core/src/task/ops.rs` ships local `split_lines` / `join_lines` /
`find_heading` / `parse_heading` / `section_end` / `read_or_empty`.
`ft-core/src/notes.rs` ships its own byte-offset cousins
(`line_byte_offsets`, `section_end_line`, `section_end_offset_byte`).
The graph parser does its own `split_inclusive('\n')` walk.

Worse: `read_or_empty` (task/ops.rs:157) and `read_or_empty_move`
(task/ops.rs:794) have *identical bodies* — the only difference is
which error enum they wrap into.

**Fix.** Promote a tiny `ft_core::markdown::lines` module with
`split_keep`, `join_with_newline`, `find_heading_text`, `section_end`.
Migrate task::ops and timeblock to use it.

### 3. Per-operation `*Error` enums in `task::ops`

`CreateError`, `CompleteError`, `UpdateError`, `MoveError`,
`CancelError` (the last is a thin wrapper around `UpdateError` plus one
variant) each independently define `Read{path,source}` / `LineMissing`
/ `NotATask` / `Write` variants. Five enums × four near-identical
variants = boilerplate that compounds every time a new op is added.

**Fix.** Shared `FileEditError` (Read / Write / LineMissing / NotATask)
+ per-op `Result<_, FileEditError>` plus operation-specific tail
variants where needed.

### 4. `Filter` runs in parallel with the Tasks-profile graph DSL

`ft_core::query::Filter` was the v1 task filter API. Then
`graph::query::Profile::Tasks` was added and the DSL ate every
predicate Filter expresses. Today `ft/src/cmd/tasks.rs:184` does *both*:
builds a `Filter` from CLI flags **and** AND-composes it with the graph
queries, then iterates `scan.tasks` checking both gates.

Two parallel filter systems with identical semantics is a maintenance
trap — a new field (e.g. `cancelled_after`) must be added in both
places to behave consistently.

**Fix.** Compile `Filter` flags into a `GraphQuery` once at CLI-arg
time and feed only the DSL path. Or keep `Filter` as the DSL's *output*
once parsed. One source of truth for "what predicates exist."

*This item needs a design decision (kill Filter vs. document the
seam). Out of scope for the first pass of refactors.*

### 5. AppRequest dispatch in `App::service_*` paths

`ft/src/tui/app.rs:1735–1900` has ~15 nearly identical blocks:

```rust
AppRequest::GraphX { ... } => {
    if let Some(idx) = self.tabs.iter().position(|t| t.title() == "Graph") {
        let ctx = TabCtx { vault: &self.vault, recents: &self.recents, today: self.today,
            last_refresh: &self.last_refresh, pending_request: &self.pending_request,
            active_modal_name: self.active_modal_name(), host_popup_open: false };
        self.tabs[idx].graph_x(&ctx, ...);
    }
}
```

This is in three places (real `service_request`,
`service_pending_for_test`, `service_request_for_test`, plus
`drain_simple_requests`). Same idiom repeated also in `tab.rs` for the
19 `graph_*` Tab-trait hooks.

**Fix.** An `App::with_graph_tab(|tab, ctx| ...)` helper that
constructs the ctx once and looks up the tab. Removes the "did I
update all three dispatch paths?" footgun.

### 6. CLI vault-discovery context line, 77 sites

`Vault::discover(vault_flag).context("could not locate an Obsidian
vault")?` appears 77 times across `ft/src/cmd/*.rs`. Same for
`Graph::build(&vault, &scan).context("...")` (~39 sites).

**Fix.** `crate::cmd::common::{discover_vault, build_graph}` helpers.

### 7. `path.strip_prefix(&vault.path).unwrap_or(...)` — 10+ sites

Every CLI command relativizes paths before display the same way
(`tasks.rs:532, 542, 641, 916, 926`; `notes.rs`; `timeblocks.rs:663`
with its own `fn rel`). Add `Vault::relativize(&self, p: &Path) ->
&Path`.

### 8. `write_atomic` only handles `&str`; `BlameCache::save` reimplements it

`ft-core/src/blame_cache.rs:72-104` does the *same dance*
(`NamedTempFile` in same dir, `sync_all`, persist) as
`fs::write_atomic` but for `Vec<u8>`.

**Fix.** Extract `write_atomic_bytes(path, &[u8])` and have
`write_atomic` call it.

### 9. `.map_err(|e| anyhow!("{e}"))?` — ~20 sites doing nothing

`ft_core::error::Error` derives `thiserror::Error`, so `anyhow` already
auto-converts it via the `From<E: Error + Send + Sync + 'static>`
blanket impl. Every `.map_err(|e| anyhow!("{e}"))?` could just be `?`.
The flavored variants (`.map_err(|e| anyhow!("--{label}: {e}"))?`) are
fine; the bare-passthroughs are noise.

## Smaller dirt / shaky premises

### 10. `Task` does not implement `Default`

CLAUDE.md says model structs should derive Default. `Task` doesn't.
The `selector::tests::task()` helper and several other test sites
hand-construct 18-field structs verbatim. `Default` is a one-line fix
that pays out forever in test cleanup.

### 11. `TaskData` is a stringly-typed mirror of `Task`

In `graph/mod.rs:160`, `TaskData` stores `status: String`
("Open"/"Done"), `priority: Option<String>`, `due: Option<String>`
(YYYY-MM-DD), etc. The graph DSL evaluator then stringifies-compares.
Three of four roundtrip conversions go via `.to_string()`/`.parse()`
per query.

This is a real performance and correctness hazard — date comparisons
become lexicographic string compares (works for ISO dates by luck, but
a future TZ-aware format would silently misorder). The DSL evaluator
could be taught to read `Status` / `Priority` / `NaiveDate` directly;
the textual surface only needs to exist at parse-time.

**Effort.** Medium. **Impact.** Worth doing before TaskData grows more
fields.

### 12. `NodeKey` ↔ `NodeKind` ↔ `Graph::stable_key` is implicitly coupled

Each new `NodeKind` variant requires manual updates to `NodeKey`,
`Graph::stable_key`, and `Graph::id_for_key` (graph/mod.rs:488–514).
Compiler doesn't catch missing entries — `NodeKey` is a separate enum
so omissions in `id_for_key` only show as runtime `None`s. A
doc note + maybe a debug assertion linking the two would help, or
merge them into one enum with payload methods.

### 13. Two `Selector` enums with the same shape

`ft_core::selector::Selector` (Id / FileLine / Fuzzy) for tasks,
`ft_core::timeblock::ops::Selector` (Line / Time / Fuzzy) for blocks,
both with their own `resolve` returning either a `Vec` or a custom
`SelectorResult`. Different domains, same pattern, no shared code.

### 14. Single broad `Error` with `String` payload variants

`ft-core/src/error.rs` has `Error::Notes(String)`, `Error::Periodic(String)`,
`Error::Git(String)`, `Error::Timeblock(String)`. The type tag carries
no real semantic — most callers immediately wrap in `anyhow!("{e}")`.
Either commit fully to per-domain typed errors (drop the strings) or
rip out the per-op enums in favor of one `Error::TaskOp` variant.
Don't keep both.

### 15. `ft/src/tui/tests.rs` is 9,108 lines

One file, hundreds of tests. Splitting along the tab boundary
(`tests/tabs/graph.rs`, etc.) would help anyone trying to find a
regression test. No risk; cargo doesn't care.

### 16. Raw `Color::Rgb(50, 38, 30)` inlined four places

`palette.rs` exists to prevent this. `widgets/picker.rs:329`,
`tabs/notes/view.rs:1205, 1350`, `tabs/tasks/search.rs:1899` all reach
for the same RGB tuple. Add `palette::ROW_HIGHLIGHT_BG`; one-time fix.

## Recommended order of attack

Low effort, high impact first:

1. `ft_core::dates::today()` / `now_pair()` — kill 16 FT_TODAY
   duplicates.
2. `write_atomic_bytes` + `cmd/common::{discover_vault, build_graph}`
   + `Vault::relativize`.
3. Strip the `.map_err(|e| anyhow!("{e}"))` no-ops.
4. Derive `Default` on `Task` and other model structs; collapse test
   helpers.
5. Shared markdown line-buffer module; delete `read_or_empty_move`;
   collapse `find_heading` / `section_end`.
6. Decide on `Filter` vs DSL: either delete `Filter` or document why
   both exist. *(needs design decision — defer)*
7. Factor `App::with_graph_tab` to collapse `service_*` dispatch.

Then the meatier ones — typed `TaskData`, `Task` / `Graph::stable_key`
consolidation, splitting `tests.rs` — when there's appetite.
