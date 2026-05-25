# Architecture

`ft` is split into a thin binary (`ft/`) and a library crate
(`ft-core/`). Everything reusable lives in the library; the binary owns
clap parsing, terminal/TTY concerns, the editor handoff, and the
interactive picker.

## Workspace layout

```
ft/
├── Cargo.toml                  # workspace manifest
├── rust-toolchain.toml         # MSRV pin
├── ft/                         # binary crate (thin)
│   ├── Cargo.toml
│   ├── src/main.rs             # clap dispatch only
│   ├── src/cmd/                # one file per subcommand
│   │   ├── tasks.rs            # list / create / complete / move
│   │   ├── vault.rs            # vault info
│   │   ├── git.rs              # `ft git sync`
│   │   ├── graph.rs            # `ft graph query`
│   │   ├── completions.rs      # `ft completions <shell>`
│   │   └── man.rs              # `ft man [--out DIR]`
│   ├── src/output/             # table.rs, json.rs, markdown.rs, ndjson.rs, links.rs, graph.rs
│   └── tests/                  # integration tests with assert_cmd
└── ft-core/                    # library crate (the brain)
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── vault.rs            # discovery + scan
        ├── config.rs           # layered config (user + vault)
        ├── periodic.rs         # periodic-note path + template resolution
        ├── git.rs              # discover_repo + status + upstream + sync
        ├── graph/
        │   ├── mod.rs          # Graph + NodeKind/EdgeKind/NoteId/LinkEdge
        │   ├── parser.rs       # extract_links: wikilinks, md links, embeds
        │   ├── resolve.rs      # Obsidian shortest-path resolution rules
        │   ├── query.rs        # graph query DSL (parse / select / expand)
        │   └── rename.rs       # plan_rename / apply_rename_plan
        ├── markdown.rs         # heading extractor + shared LineSkipState
        ├── dates.rs            # ISO / keyword / relative / NL parsing
        ├── fs.rs               # write_atomic
        ├── selector.rs         # id / file:line / fuzzy
        ├── error.rs
        ├── task/
        │   ├── mod.rs          # Task struct, Status / Priority enums
        │   ├── format.rs       # TaskFormat trait
        │   ├── emoji.rs        # emoji format impl
        │   ├── hierarchy.rs    # parent-pointer resolution
        │   ├── ops.rs          # create_task / complete_task / plan_move
        │   └── recurrence.rs   # rule parser + next-instance engine
        └── query/
            ├── mod.rs
            ├── filter.rs       # programmatic typed filters
            ├── expr.rs         # AST: Expr / Atom
            ├── dsl.rs          # tokenizer + recursive-descent parser
            ├── preset.rs       # built-in named queries
            └── sort.rs         # sort_by_keys + SortKey / SortOrder
```

## Key traits and seams

### `TaskFormat`

`ft-core::task::format::TaskFormat` is the seam between the in-memory
`Task` model and a wire format. Implementors provide:

```rust
fn parse_line(&self, line: &str, ctx: ParseContext) -> Option<Task>;
fn serialize_line(&self, task: &Task) -> String;
```

The v1 implementation is `task::emoji::EmojiFormat`, matching the
Obsidian Tasks plugin v7.22 canonical output. A dataview implementation
is a future plug-in here — every consumer (scanner, ops layer, query
engine) holds a `Task`, not a format-specific representation, so a new
format only needs to plug into this trait.

### Operations API (`task::ops`)

Mutation primitives. Each one reads a file, computes the new content,
and writes via `crate::fs::write_atomic`:

- `create_task(path, input, opts)` — insert a new task at append /
  under-heading / at-line position; refuses duplicates unless `--force`
- `complete_task(path, line, opts)` — mark a task done; if recurring,
  insert the next instance above the now-completed line
- `plan_move(sources, target)` — pure: produce a `MovePlan` of per-file
  before/after edits without writing
- `apply_move_plan(plan)` — write each non-no-op edit atomically

The CLI binary calls these directly. The TUI (plan 002) will compose
them at finer granularity.

### Timeblocks (`timeblock`)

`ft-core::timeblock` is the day-planner block layer. It mirrors the
task module's library / CLI / TUI seam pattern: a parser + serializer +
ops layer in `ft-core`, a thin clap shell in `ft::cmd::timeblocks`,
and a TUI tab in `ft::tui::tabs::timeblocks`.

```rust
pub fn parse_line(s: &str) -> Result<Timeblock, ParseError>;
pub fn serialize_line(b: &Timeblock) -> String;
pub fn parse_tags(desc: &str) -> Vec<Tag>;            // lenient inline
pub fn parse_tag_string(s: &str) -> Result<Tag, ParseError>;  // strict

doc::Document::read(path, heading)  -> Result<Document>;
doc::Document::write(&self)         -> Result<()>;   // atomic, section-replace

ops::add_block(path, heading, new, AddOptions)            -> Result<Document>;
ops::edit_block(path, heading, &Selector, EditMutation)   -> Result<Document>;
ops::delete_block(path, heading, &Selector)               -> Result<Document>;

report::time_per_tag(&[Timeblock])    -> Vec<TagTime>;
report::total_minutes(&[Timeblock])   -> u32;          // excludes @break
```

`Selector` mirrors the task one: `Line(N)` (1-indexed display order,
unique per block), `Time(NaiveTime)` (matches by start; can be
ambiguous when two blocks share a start — the TUI uses `Line` to
avoid that), or `Fuzzy(String)` (substring match on desc, the CLI
default).

Tag grammar: max 3 levels, each `[A-Za-z0-9_-]+`. The inline parser
(`parse_tags`) is lenient — malformed `@…` tokens stay in `desc` but
don't surface as tags, so legacy blockary notes still read. The
strict variant is for `--add-tag X` / `--tag X` CLI flags.

Daily-note resolution piggybacks on `periodic_notes::resolve_periodic_path`.
When a write targets a daily note whose file is missing on disk, both
the CLI's `add` and the TUI's `c` / `a` chords run
`create_or_get_periodic_path` first so the configured
`[periodic_notes.daily].template` is rendered before the section gets
spliced in.

### Graph query DSL (`graph::query`)

`ft-core::graph::query::parse(src)` returns a `GraphQuery { initial,
expansion }`. The DSL describes a navigation *policy*, not a result
subgraph: `GraphQuery::select(&graph)` returns the initial set of
`NoteId`s; `GraphQuery::expand(&graph, parent)` returns the children
for one hop; `GraphQuery::walk(&graph, &WalkOptions)` materializes
the full reachable subtree as `Vec<WalkNode>` with depth + cycle
bounds (Stop emits a cycle marker and halts that branch; Allow
needs `max_depth`). Consumers compose these to taste: the TUI
graph tab (`ft/src/tui/tabs/graph.rs`) drives `select` + `expand`
one hop per keystroke; the CLI subcommand `ft graph query`
(`ft/src/cmd/graph.rs` + `ft/src/output/graph.rs`) calls `walk`
once and renders the result in tree/json/ndjson/edges/markdown.
The parser is hand-rolled recursive-descent, mirroring `query::dsl`,
and rejects op/value type mismatches and scope errors at parse time.
See `docs/graph-query-dsl.md` for the grammar, attribute compatibility
matrix, worked examples, and error catalog.

### Query language (`query::dsl`)

`dsl::parse(src, today)` returns a `Query { expr, sort_keys, limit }`.
The expression AST (`query::expr`) is `Expr` (And/Or/Not over `Atom`s)
where atoms are predicates like `Status(Open)`, `DueBefore(date)`, etc.
`Expr::matches(&Task)` evaluates against a task. The CLI composes the
DSL with flag filters by AND-ing the parsed expression with a typed
`Filter`. See `docs/query-dsl.md` for the supported subset.

## Adding things

### A new subcommand

1. Create `ft/src/cmd/<name>.rs` with an `Args` struct and `run` fn.
2. Add `pub mod <name>;` to `ft/src/cmd/mod.rs`.
3. Add the variant to `Commands` in `ft/src/main.rs` and dispatch it.
4. If it needs vault data, call `Vault::discover(vault_flag)?` and
   `vault.scan()` — same pattern as the existing subcommands.

### A new TUI tab

1. Implement [`Tab`](#) on your struct and add it to the `tabs` vec in
   `App::new`. The default `help_sections()` returns nothing, which would
   leave the `?` overlay showing only the shared global section — so:
2. Override `Tab::help_sections(&self) -> Vec<HelpSection>` returning one
   or more named [`HelpSection`]s (see `ft/src/tui/help.rs`) for your
   keymap. Group bindings the way users will look them up
   ("Navigation", "Mutations", "Modals"); keep key strings short
   (≤ 18 chars) so they fit alongside descriptions in the 80-col popup.
3. Add a snapshot test in `ft/src/tui/tests.rs` that switches to the new
   tab, calls `app.enter_help()`, and renders to a `TestBackend` so the
   help inventory is locked against drift.

### A new task format (e.g. dataview)

1. Create `ft-core/src/task/<name>.rs` implementing `TaskFormat`.
2. Add format detection in `vault::parse_file` (try formats in priority
   order configured by `.ft/config.toml`).
3. Round-trip property tests: `serialize(parse(line)) == line` and
   `parse(serialize(task)) == task` (proptest, snapshot, real-vault).

### A new output format

1. Add a module to `ft/src/output/`.
2. Add a variant to `output::Format`.
3. Wire it into the match in `ft/src/cmd/tasks.rs::run_list`.

## Concurrency model

The codebase is deliberately single-threaded everywhere except for two
producer threads that feed the TUI event channel. There is no async
runtime — no tokio, no `async fn`. The main event loop blocks on
`mpsc::Receiver::recv()`, processes one event, redraws, and loops.

### Producers

1. **Crossterm input thread** (`ft/src/tui/event.rs::crossterm_loop`)
   reads stdin and sends `Event::{Key,Mouse,Resize,Tick}` onto a shared
   `mpsc::channel`. Owns no app state.
2. **Background workers** (`ft/src/tui/app.rs::run_sync_job` and any
   future siblings) own their inputs (`Arc<Vault>` clones, `PathBuf`,
   options) moved into a `move ||` closure, do synchronous work, and
   send exactly one `Event::Background(BgEvent)` back into the same
   channel before exiting.

The main loop is single-receiver: there is no `select!`, no second
channel, no polling. Background completions are just another event
variant the main loop matches on.

### Shared state

- **`Arc<T>` for read-only sharing** — `Arc<Vault>`, `Arc<RecentsLog>`.
  These are *not* `Mutex`-protected; they're cloned read handles.
- **`RefCell<Option<T>>` for single-threaded "slot" state** —
  `pending_request`, `toast`, `sync_conflict`, `jobs`. Single-threaded
  so no lock contention.
- **No `Arc<Mutex<AppState>>` anywhere.** Background workers do not
  reach into App state — they post a message and let the main loop
  apply it.

### Pattern for adding off-thread work

When a future feature needs to run something off the main loop (file
watching, fuzzy-index rebuilds, schedule-driven autosync, HTTP fetch):

1. Worker thread owns its inputs (move them into the closure).
2. Result goes back via `EventStream::sender()` as a new
   `BgEvent::*` variant.
3. In-flight state lives on `App` as a typed slot
   (`RefCell<Option<JobHandle>>` for v1; promote to `HashMap<JobId,
   JobHandle>` once concurrent jobs of different kinds are needed).
4. Cancellation is cooperative — share an `Arc<AtomicBool>` flag the
   worker checks between phases. Never kill the thread.
5. Quit doesn't `join()`. The OS reaps orphaned workers; their
   `send()` calls return `Err` after the receiver drops.

The plan-014 git-sync background worker is the reference
implementation.

## Testing strategy

- **Unit tests** live with the modules (`#[cfg(test)] mod tests`)
- **Integration tests** under `ft/tests/` use `assert_cmd` + `assert_fs`
  against fixture vaults built per-test in temp directories
- **Fixture vaults** under `tests/fixtures/`: `tiny/` (a few tasks),
  `realistic/` (~25 tasks across PARA + journal + inbox), `pathological/`
  (deep subtasks, every emoji, weird unicode, malformed lines)
- **Snapshot tests** with `insta` for stable output formats
- **Proptest** round-trips for the parser
- **Real-vault tests** (`ft-core/tests/real_vault.rs` and
  `ft/tests/real_vault_cli.rs`) gated on `FT_REAL_VAULT_TESTS=1` so CI
  never depends on a local vault

## Build invariants

- `cargo build --release` produces a single `ft` binary
- `cargo test --workspace` runs everything
- `cargo clippy --workspace --tests -- -D warnings` is clean
- `cargo fmt --check` is clean
