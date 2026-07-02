# Architecture review — 2026-07-02

Scope: whole workspace (`ft-core` + `ft`) at commit `40194af`, ~89k lines
of Rust. Focus: what hinders future development, maintenance, or
usefulness. Overall the codebase is well above average — the plan/apply
mutation split, the `write_atomic` chokepoint, the command/keymap
registry, and the no-async event loop are sound and documented. The
findings below are mostly places where the *stated* architecture and the
*actual* code have diverged, or where decisions that were fine at 20k
lines are starting to bite at 89k.

The consistent theme: the documented seams (`TaskFormat`, background
workers, `refresh_note`, `NodeKey`) are the right ones — the code
stopped using them under feature pressure. Most of the work is
re-aligning the implementation with the architecture already written
down.

## 1. The TUI has no shared data layer — every tab rebuilds the world, on the UI thread

The biggest structural problem. There are ~20 `Graph::build` call sites
in the binary. Each tab owns a private `Graph`; the Journal tab says it
outright:

> `// Build a fresh graph; the App-level graph belongs to the Graph tab
> and isn't easily reachable from here.` (`ft/src/tui/tabs/journal.rs:372`)

`ctx.vault.scan()` + `Graph::build()` run **synchronously inside key
handlers** (e.g. `tabs/graph.rs:2669`, `tabs/journal.rs:374`,
`tabs/tasks/search.rs:218`, `modal.rs:1162`). The architecture doc's
concurrency section describes the background-worker pattern, but only
git sync/commit use it — every scan, graph build, and git-blame-heavy
journal build blocks the render loop.

Consequences:

- **Latency scales with vault size in the worst place: keystroke
  handlers.** A move on the Graph tab re-reads and re-parses the entire
  vault before the toast clears.
- **Tabs drift out of sync.** A mutation on one tab doesn't invalidate
  the graphs held by the other five; each tab has ad-hoc reload logic.
- **Every new feature re-answers "where do I get a graph?"** by pasting
  another `scan(); Graph::build()` block — which is how it reached ~20
  sites.

`Graph::refresh_note` (`graph/mod.rs:498`) is a real incremental seam,
but the TUI barely uses it; tabs rebuild from scratch anyway.

**Fix direction:** one App-owned `Arc<Graph>` snapshot with a generation
counter, rebuilt by a background worker posting a `BgEvent::GraphReady`,
tabs holding `NodeKey`s (already designed to survive rebuilds). Should
land before more tabs are added — every new tab currently clones the
anti-pattern.

## 2. Full vault re-read per command — twice

`Vault::scan()` reads every file's content to parse tasks;
`Graph::build()` then **re-reads every file** to extract
links/paragraphs/headings (`graph/mod.rs:391-406`). Any command needing
both — all task queries now, since the DSL unification routes through
the graph — does two complete vault I/O passes.

Beyond that, there is no persistent index; the blame cache
(`blame_cache.rs`) is the only thing that survives a process. For a CLI
that invites shell scripting (`ft tasks list --format json | jq …` in a
loop), cold-start cost per invocation is the usefulness ceiling.

**Fix direction:** merge task parsing into the graph parse pass (one
read per file), or cache `ParsedFile` keyed on mtime. Fixing the double
read is cheap; a persistent index can come later if vault size demands
it.

## 3. The `Tab` trait is turning into a god interface; request routing is quadruplicated

`tab.rs` carries 20+ `graph_*` default no-op methods
(`tab.rs:633-758`) — per-tab actions leaking into the shared trait. The
documented recipe for adding one modal action touches an `AppRequest`
variant, a trait hook, a tab override, and **four servicing sites**:
`service_request`, `service_pending_for_test`, `service_request_for_test`,
and `drain_simple_requests` (architecture.md §"App ↔ Tab routing"). Two
of those are test-only re-implementations of production routing — drift
waiting to happen.

Also:

- Routing looks up the target tab **by `title()` string**.
- Picker modal newtypes live inside `tabs/graph.rs` "so they can reach
  graph-internal types" — the supposedly tab-agnostic modal layer is
  coupled to one tab's internals.
- `AppRequest` is a grab-bag enum whose variants' semantics live in
  comments.

**Fix direction:** a downcast to the concrete tab (or per-tab request
sub-enums) collapses the four-site ritual; delete the test-only routing
clones in favor of driving the real path.

## 4. The pluggable-format story is aspirational

`TaskFormat` is described as *the* seam, but `EmojiFormat` is hard-coded
at ~10 non-test call sites — not just `vault::parse_file` (which
CLAUDE.md admits) but the **entire ops layer**: `create_task`,
`complete_task`, and `plan_move` all parse and serialize via
`EmojiFormat` directly (`task/ops.rs:161,198,340,375,466,651`). A second
format isn't "implement the trait + wire detection"; it's surgery on
every mutation primitive. Either thread a format handle through `ops`
now (cheap while there's one impl) or drop the claim from the docs — a
seam nobody can use is worse than no seam.

Related: **the task model exists twice.** Typed `Task` for the ops
layer, string-typed `TaskData` in the graph (dates, status, priority all
as `String`, `graph/mod.rs:191-214`) for DSL evaluation. Every new task
field must be added to `Task`, the emoji parser/serializer, `TaskData`,
the denormalization, the `Attr` enum, the `value_type()` matrix, and the
docs. Heavy schema-evolution tax, and string-typed comparison invites
subtle bugs (a future priority ordering would compare lexicographically).

## 5. Identity is `(file, line)` all the way down

`NodeKey::Task(path, line)`, `Selector::FileLine`, and the Tasks tab
mapping query hits back to its cache "by (path, line)"
(`tabs/tasks/search.rs:220`). Line numbers are the identity of record
for mutations. `complete_task` re-parses the line and errors if it's not
a task — good — but if the file shifted by one line between scan and
mutation (Obsidian is typically open on the same vault), it will
**silently complete whatever valid task now sits at that line**. Classic
TOCTOU of line-addressed editing, and it also blocks features the
codebase is heading toward: file-watching, incremental refresh, undo.

**Fix direction:** a content guard on the expected line text (the synth
callout layer already uses blake3 hashes) closes the silent-wrong-task
hole cheaply; stable task IDs (Obsidian block IDs are already parsed)
are the longer-term answer.

## 6. Monolithic files

- `ft/src/tui/tests.rs` — **10,255 lines, 355 tests in one file.**
- `ft/src/tui/tabs/graph.rs` — 6,798 lines: tab logic, four modals,
  picker newtypes, and its own test module.
- `ft-core/src/graph/query.rs` — 4,624 lines: lexer, parser, type
  checker, three evaluators, and `Display` in one module.
- `cmd/notes.rs` (2,084), `tui/app.rs` (2,074).

Merge-conflict magnets, and they set the gravitational pattern — new
graph-tab code lands in `graph.rs` because that's where everything is.
The query module splits naturally along its existing internal
boundaries (lexer/parser/eval).

## 7. The git dependency is load-bearing for a third of the product

`link_review`, `journal`, and `synth` — the whole synthesis ritual —
hard-require the vault to be a git repo, shell out to the `git` binary
(subprocess per blame/show/diff), and pin provenance to commit SHAs
inside note content. Legitimate design choice, but the costs:

- Behavior varies with the user's git version/config.
- Subprocess-per-file is a perf cliff, only partially papered over by
  `blame_cache`.
- **Any history rewrite (rebase, squash-merge, aggressive gc) strands
  every `[!ft-source] @sha` pin in the vault**, degrading verify results
  en masse with no repair path. A `ft synth repair` / re-pin flow will
  be needed eventually.

## 8. Process and doc debt

- **15 active openspec changes vs 20 archived**, several active ones
  (e.g. `unify-query-dsls`, `support-ft-vault-marker`) describing work
  already merged. If the active directory doesn't reliably mean "in
  flight", it stops being useful as the prior-context entry point
  CLAUDE.md prescribes. An archive sweep is overdue.
- **architecture.md's workspace tree is stale** — it omits `timeblock/`,
  `synth/`, `journal.rs`, `link_review.rs`, `notes/`, `search.rs`,
  `markdown.rs`, `related.rs`, `recents.rs`. The prose below it is
  current, which makes the stale tree actively misleading.
- Code comments reference **plan numbers and fork labels** ("session 3",
  "plan-014", "Fork A2") that are meaningless without excavating the
  openspec archive.
- `package.json` + `node_modules` in a Rust repo solely for the openspec
  CLI is a minor onboarding wart.

## 9. Test-suite frictions

- Heavy `insta` snapshotting of full TUI frames means any visual refresh
  churns hundreds of snapshots (the pending `refresh-tui-visuals` change
  will demonstrate this). Snapshots of *structure* (help inventories,
  row text) age better than snapshots of pixels-in-cells.
- Env-var globals (`FT_TODAY`, `FT_VAULT`) as the injection mechanism
  force `ENV_LOCK` mutex serialization in tests (`vault.rs:625`) and
  make `ft-core` awkward to embed as a library. `dates::today()` is the
  right seam; finish the migration off direct `env::var` reads and let
  context carry "today" explicitly.
- Real-vault tests hardcode `/Users/cmw/git/fortytwo` — a macOS path,
  while development is now on Linux, so the gated tests are currently
  un-runnable on this machine.

## Priorities

1. **App-owned shared graph + background rebuild** (finding 1) — fixes
   the worst latency, unblocks watching/incremental refresh, stops the
   copy-paste spread.
2. **Merge scan into the graph parse pass** or cache `ParsedFile` by
   mtime (finding 2) — cheap, big CLI win.
3. **Thread the format handle through `task::ops`** while there's still
   one format (finding 4) — cost grows with every op added.
4. **Content-guard line-addressed mutations** (finding 5) — small
   change, closes a silent-data-corruption class.
5. **Routing consolidation** (finding 3) — collapse the four servicing
   sites, delete test-only routing clones.
6. **Openspec archive sweep + fix the architecture.md tree** (finding
   8) — an afternoon, and it keeps the doc-driven workflow trustworthy.
