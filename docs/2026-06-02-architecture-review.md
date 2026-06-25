# ft — Architecture & UX Review

*2026-06-02*

> **As-of snapshot.** This review describes the codebase at the date
> above and has not been updated since. Counts (commands, tabs, LoC,
> commits) and the DSL landscape have drifted — notably the standalone
> task DSL it references has since been removed in favour of the
> unified graph DSL (see `docs/architecture.md` and
> `docs/migrating-task-queries.md`). Treat this as a historical
> artifact, not a current map.

## 1. Architecture recap

**Shape.** Two-crate workspace: `ft-core/` holds the engine (vault
discovery, config, task/timeblock/graph models, two query DSLs, mutation
ops, atomic writes); `ft/` is a thin binary that owns clap parsing,
output rendering, editor handoff, and the ratatui TUI. The TUI consumes
only `ft-core` — no shortcut path from TUI to filesystem.

**Load-bearing patterns.**

- **Plan/apply for every mutation.** Pure planners (`plan_move`,
  `plan_rename`, `plan_multi_rename`, timeblock ops) produce a typed
  `*Plan`; a separate `apply_*` writes through `fs::write_atomic`
  (same-dir tempfile + rename, same-file edits sorted by descending byte
  offset). This is the single best decision in the codebase.
- **`TaskFormat` trait.** Scanner/ops/query all hold a `Task`, not lines.
  `EmojiFormat` is v1; a Dataview impl is genuinely a plug-in.
- **Heterogeneous graph from day one.** `NodeKind`
  (Note/Ghost/Directory/Task/Paragraph) and `EdgeKind`
  (Link/Embed/Contains/HasTask/OwnsParagraph/ParagraphLink/LinksInto)
  over a `StableDiGraph`. Adding `Directory`, `Task`, `Paragraph`
  post-launch didn't require rewriting — the additive payoff is visible
  in the git log.
- **Single-threaded TUI.** One `mpsc::Receiver`, two producer threads
  (crossterm + background workers), no async, no `Mutex<AppState>`.
  In-flight state is `RefCell<Option<JobHandle>>` slots. Cancellation is
  cooperative `Arc<AtomicBool>`. Workers post a `BgEvent::*` and exit.
- **Dual-DSL preset pattern.** Task DSL and graph DSL each have a
  `builtin()` + `builtin_names()` table; user presets shadow built-ins;
  CLI `--preset` and TUI quick-pick resolve identically.
- **Env seams.** `FT_TODAY`, `FT_VAULT`, `FT_REAL_VAULT_TESTS`,
  `FT_PERF_TESTS` give reproducible tests and one place to override
  "today" / "vault" everywhere.

**Surface area.** 9 top-level commands (`vault`, `tasks`, `timeblocks`,
`find`, `notes`, `graph`, `git`, `tui`, `completions`, `man`). `ft notes`
alone has 13 subcommands. TUI has 5 tabs (Graph, Tasks, Notes,
Timeblocks, Journal). ~55k LoC of Rust, 146 commits, an openspec
workflow that has archived 12 changes.

**Evolution shape (from `git log` and the openspec archive).** Built in
declared sessions: tasks CLI first (8 sessions), then the TUI scaffold
(5 sessions), then `notes`, then the graph (v1 DSL → v2 DSL with task
nodes, paragraph nodes, directory nodes), then timeblocks, then rename →
multi-rename → cross-dir mv, then journal + related-updater + paragraph
graph. Each capability landed with a spec, a design, and a tasks file.
This is the strongest signal that the project is engineered, not just
hacked.

---

## 2. Critiques, ranked by impact

### Critique 1 — `tabs/graph.rs` has become a god-file (HIGH, blocking)

`ft/src/tui/tabs/graph.rs` is **4,276 lines**. It owns: the tree viewer,
the query input, the preset picker, the in-tree search picker, the
create-note flow, the append-template flow, the quick-capture flow, the
periodic-leader chord, the section-move outer state, the rename modal,
and the Related-section modal. The struct has ~15 modal slots
(`create_state`, `append_state`, `capture_picker`, `capture_var_state`,
`periodic_leader`, `move_outer`, `rename_state`, `preset_picker`,
`related_modal`, `search_picker`, …) and an `input_mode` flag to
arbitrate.

**Why this is the top issue.** Every new note-touching action lands
here. The shared `notes_actions/` module was the right refactor on
paper, but the *orchestrator* still concentrates in one tab. Modal slot
count grows linearly with features; the chord-vs-input arbitration has
to be re-derived for each new flow; the test count for this tab alone is
43. The next feature (e.g. tag-graph operations, bulk-rename UI) will
push past 5k lines and the arbitration logic will become genuinely hard
to reason about.

**Recommendation.** Pull the modal-arbitration logic into a generic
`ActiveModal` enum on `App` (or on a sub-controller above `GraphTab`) so
flows compose without each tab owning a private modal stack. The shared
flows are already in `notes_actions/`; what's missing is a shared
*driver*.

### Critique 2 — Two parallel DSLs with no shared parser plumbing (HIGH)

`ft-core/src/query/dsl.rs` (812 lines) and `ft-core/src/graph/query.rs`
(3,320 lines) are independent hand-rolled tokenizer + recursive-descent
parsers with very similar surface (`attr op value`, `and`, `where`, set
literals, error catalogs). They share *zero* infrastructure. The graph
DSL's v2 jump already had to re-implement the tokenizer; a third DSL
(timeblock filter, tag query, etc.) would copy-paste a third time.

**Why it matters now, not later.** The recently-added task attributes
(`status`, `priority`, `due`, `description`, `tags`) on the graph DSL
are essentially the task DSL atoms living in a second house. The two
predicate vocabularies are already drifting. A shared lexer + a typed
`AttrSchema` (subject → attr → op compatibility matrix) would unify
error catalogs and let task/graph queries reuse each other.

**Recommendation.** Extract a `ft-core/src/dsl/` lexer (a `Lexer`,
`Span`, `ParseError` with location), and a typed `AttrSchema` trait. The
two parsers stay separate; the lower halves merge. Net win is realised
as soon as a third DSL appears, but unification of error formatting is
immediate.

### Critique 3 — `ft notes` is becoming a kitchen-sink namespace (MEDIUM, UX)

13 subcommands under `notes`: `open`, `move-section`, `create`, `today`,
`periodic`, `backlinks`, `links`, `rename`, `mv`, `journal`,
`update-related`, `append`. Meanwhile the top level also has `find` (a
near-twin of `notes open` minus the editor handoff) and `graph` (which a
user reasonably thinks of as a "notes" operation). The mental model
breaks for both new users and scripters:

- `ft find` prints; `ft notes open` opens — same fuzzy syntax.
- `ft notes backlinks` / `ft notes links` are read-only queries over the
  graph; conceptually they're `ft graph` operations.
- `ft notes today` exists; `ft tasks list today` exists; `ft timeblocks
  list` defaults to today. Three different "today" verbs.
- `ft notes journal <note>` is a graph-backed feed but lives under
  `notes`.

**Recommendation.** Either (a) lean into `notes` as the user-facing
namespace and move backlinks/links/rename/mv/journal there permanently
(already underway), folding `find` into `notes find`; or (b) collapse to
two namespaces: `notes` (manipulate one note: open/create/append/rename/
mv) and `graph` (read across notes: query/backlinks/links/journal). The
current state is half-way. I'd pick (b) — `journal` and `update-related`
are obviously graph-backed, and `backlinks/links` are literally graph
edges.

### Critique 4 — Test gravity: `ft/src/tui/tests.rs` at 8,062 lines (MEDIUM)

One file, 293 tests, drives every tab through a `TestBackend`. It's been
load-bearing for catching regressions, but adding a test means either
finding the right neighborhood in 8k lines or accidentally creating a
9k-line file. Cross-tab tests (e.g. Graph→Journal jump) end up there by
default because there's nowhere else.

**Recommendation.** Split per-tab: `tests/graph.rs`, `tests/tasks.rs`,
etc., with a `tests/common/` for the snapshot harness. This is
mechanical and unblocks more aggressive test-driven additions to the
Graph tab.

### Critique 5 — No persistent cache; every invocation walks the vault (MEDIUM, latency-shaped)

`Vault::scan()` rayon-walks every `.md` file on every CLI call;
`Graph::build` rebuilds the entire graph every TUI focus event that
triggers a refresh. The blame cache (`.ft/cache/blame.msgpack`) is the
only persisted artifact and it's narrow (per-file, HEAD-keyed). At ~1k
notes this is invisible. At 10k it will be noticeable. At 50k it will
dominate the wall-clock of CLI calls.

This is fine *today*. But the recent direction — paragraph graphs,
related-updater scoring, journal feed — pushes per-call work up. Two
specific signals: `graph::build` is no longer just edge extraction (it
now does paragraph extraction and task extraction in the same pass),
and the Journal tab does git-blame on first focus.

**Recommendation.** Don't pre-emptively build a cache layer, but plan
one seam: a `VaultSnapshot` (mtime + hash per file) and an incremental
`Graph::refresh(path)`. The graph already has `refresh_note`; promote it
to the top-level interface and add a per-file mtime check at scan time.
Then the CLI can short-circuit when nothing changed.

### Critique 6 — Tab trait is growing tab-specific hooks (LOW–MEDIUM)

`Tab::queue_related_modal`, `Tab::queue_journal_for`,
`Tab::selected_is_note_for_test` are all default-empty methods
overridden by exactly one tab. They're the trait equivalent of
`if active_tab == "graph"` checks. Each new cross-tab interaction adds
another hook.

**Recommendation.** Replace these with a typed `AppRequest::*` variant
the App routes to a tab-id-keyed handler, or with a small per-tab
`Capabilities` struct returned by `Tab::capabilities()`. Either
eliminates the no-op overrides.

### Critique 7 — `serde(deny_unknown_fields)` everywhere will bite config evolution (LOW)

Every config struct uses `#[serde(deny_unknown_fields)]`. Good for
typo-catching today; risky later — a user's config pinned to ft v0.2
will hard-error on v0.3 the moment any new key is added in a sub-table.
There's no migration path because there's no version field.

**Recommendation.** Either accept the risk (it's a personal-tooling
project — fine), or add `[meta] config_version = N` and a warn-on-unknown
mode. No immediate action needed; just don't carry `deny_unknown_fields`
into the first user-facing release.

### Critique 8 — TUI keychord landscape is dense (LOW, UX)

From `tabs/graph.rs` alone: `c`/`C` create, `A` append, `Q`
quick-capture, `R` related, `f` search, `m`/`t` section-move, `p`
periodic-leader, `Ctrl+N`/`Ctrl+P` preset, `r` rename, `z` re-root, plus
arrows, plus digit jumps, plus `?`. The per-tab `?` overlay helps, but
the same keystroke means different things in different modal states —
e.g. `t` is "today" in the periodic leader and "fuzzy picker" in the
move flow.

**Recommendation.** This is the kind of thing that only really breaks at
the next user. Worth a one-pass audit: are any chords identical between
top-level binding and modal-state binding? The `?` overlay should make
modal context explicit (you already show per-tab; consider per-modal).

---

## 3. Fitness for further evolution — net assessment

**Strong foundations.** The plan/apply split, the format trait, the
heterogeneous graph, the single-threaded TUI with typed slots, the env
seams, and the openspec/devplan rhythm are all genuinely good and they
will compound. The fact that paragraph nodes slotted into the existing
graph in one change is the proof.

**Where the foundations are fraying.**

1. The Graph tab has slowly become the universal action surface for
   notes; the modal-arbitration in one struct is hitting its scaling
   limit.
2. The two query DSLs are duplicating work that wants to converge.
3. The CLI namespace has drifted; users will start asking "where does X
   live" and get inconsistent answers.

**Net read.** ft is fit for another 2–3 capabilities at the current
shape. After that, two of (1)/(2)/(3) above will need to be addressed
before the *next* feature, not after. The cheapest high-leverage move
right now is fixing critique 1 — pull the modal driver out of
`GraphTab` — because every upcoming feature touches it. Critique 2 (DSL
infrastructure) is the second cheapest, because it's mechanical and
bounded. Critiques 3 and 4 are quality-of-life and can wait until the
first external user appears.

**One unequivocal recommendation.** Before adding the next openspec
change that touches the Graph tab, spend a session extracting the modal
driver. It's the highest-leverage refactor in the codebase right now and
the modal slot count is already past the point where the next addition
will get genuinely hard.
