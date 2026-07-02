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
│   ├── src/output/             # table/json/markdown/ndjson (Format variants) + links.rs, graph.rs (command renderers)
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
        │   ├── query.rs        # unified graph DSL (parse_with / select / expand / walk)
        │   ├── preset.rs       # built-in graph presets
        │   ├── rename.rs       # plan_rename / apply_rename_plan
        │   ├── delete.rs       # plan_delete / apply_delete_plan
        │   └── tests.rs
        ├── markdown.rs         # heading extractor (used by search)
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
            ├── mod.rs          # re-exports; presets + sort helpers
            ├── preset.rs       # built-in task presets (parsed under Profile::Tasks)
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
and writes via `crate::fs::write_atomic`. Every entry point takes a
`format: &dyn TaskFormat` — callers with a `Vault` pass
`vault.task_format()` (the config-driven detection seam; always
`EmojiFormat` today) rather than naming a concrete format.

Line-addressed mutations also take an **expected-task guard**
(`expected: Option<&Task>`, or `MoveSource::expected`): the task the
caller saw at that line when it scanned. Both sides are canonicalized
through `format.serialize_line` and compared before writing; a mismatch
fails with `*Error::LineChanged` instead of silently mutating whatever
shifted into the line (Obsidian edit, git pull, a recurring completion
inserting its next instance above). Pass `None` only when no scanned
`Task` is available (e.g. graph-tab sites that hold lossy `TaskData`).

- `create_task(path, format, input, opts)` — insert a new task at append /
  under-heading / at-line position; refuses duplicates unless `--force`.
  When no explicit position is given, `auto_position(path, default)`
  resolves the target section: the note's `ft-tasks-section` frontmatter
  wins, then `[tasks] default_section` from config, else plain append.
  The CLI `--append` flag forces append, overriding any default section.
- `complete_task(path, line, format, expected, opts)` — mark a task done;
  if recurring, insert the next instance above the now-completed line
- `update_task_line(path, line, format, expected, mutate)` /
  `cancel_task(path, line, format, expected, on)` — quick-key edits
- `plan_move(sources, target, format)` — pure: produce a `MovePlan` of
  per-file before/after edits without writing
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

Daily-note resolution has two seams. `Vault::resolve_target` resolves
only (no I/O) and is for read-only/display sites that must never create
files. `Vault::ensure_target` is the **write-path chokepoint**: it
resolves and, when the default daily note is missing, renders its
`[periodic_notes.daily].template` via `create_or_get_periodic_path`
before returning — so a brand-new day's note matches `ft notes today`
instead of a bare file. Every creator goes through it (`ft tasks create`,
the TUI task popup/quickline, `ft timeblocks add`, the timeblocks tab's
add path); explicit `--file` paths are resolved but never auto-templated.
This composes with task-section resolution: the template renders the note
(with its `## Tasks` heading + `ft-tasks-section` frontmatter) first, then
`ops::auto_position` reads that file to place the task.

### Graph query DSL (`graph::query`)

The graph model itself — node kinds (Note, Heading, Paragraph, Task,
Ghost, Directory), the two edge families (exclusive containment vs
duplicated reference links), identity, build/refresh invariants, and
anchor resolution — is documented canonically in
[`docs/graph-semantics.md`](graph-semantics.md). The DSL `edge.kind`
value-set migration from `link`/`embed` to the unified link kinds is
documented in [`docs/graph-dsl-migration.md`](graph-dsl-migration.md).

`ft-core::graph::query::parse(src)` returns a `GraphQuery { initial,
expansion }` (it's `parse_with(src, Profile::Default, today)` defaulted;
`parse_with` is the real entry point). The DSL describes a navigation
*policy*, not a result subgraph: `GraphQuery::select(&graph)` returns
the initial set of `NoteId`s; `GraphQuery::expand(&graph, parent)`
returns the children for one hop; `GraphQuery::walk(&graph,
&WalkOptions)` materializes the full reachable subtree as
`Vec<WalkNode>` with depth + cycle bounds (Stop emits a cycle marker
and halts that branch; Allow needs `max_depth`). Consumers compose
these to taste: the TUI graph tab (`ft/src/tui/tabs/graph.rs`) drives
`select` + `expand` one hop per keystroke; the CLI subcommand `ft
graph query` (`ft/src/cmd/graph.rs` + `ft/src/output/graph.rs`) calls
`walk` once and renders the result in tree/json/ndjson/edges/markdown.
The parser is hand-rolled recursive-descent and rejects op/value type
mismatches and scope errors at parse time. See
`docs/graph-query-dsl.md` for the grammar, attribute compatibility
matrix, worked examples, and error catalog.

### Profiles and the unified DSL

There is one query engine: `graph::query`. Task queries are not a
separate DSL — `ft tasks list <query>` and the TUI Tasks tab query bar
parse the same graph DSL through `parse_with(src, Profile::Tasks,
today)`. `Profile::Tasks` prepends an implicit `node where kind =
Task and …` prelude so users can keep typing the short form
(`priority = High`, `due < today`) while `Profile::Default` is the
verbose graph syntax with an explicit `node` block. There is no
separate `Filter` type — `query::mod.rs` is explicit about this; CLI
flag filters (`--status`, `--tag`, `--due-before`, …) lower to DSL
fragments and AND into the parsed expression. Sort and limit are CLI
flags (`--sort`, `--limit`), not DSL clauses. See
`docs/graph-query-dsl.md` for the grammar and
`docs/migrating-task-queries.md` for the predicate translation table
from the removed standalone task DSL.

### Synthesis ritual (`link_review` + `synth`)

The synthesis ritual is the "post-connecting" workflow for the
quick-capture style note-taking the user-guide philosophy chapter
describes: review recently-mentioned `[[wikilinks]]`, aggregate
cross-vault context for a chosen subset, and produce synth notes whose
quoted excerpts are pinned to verifiable git provenance. It runs
through three composable layers:

```rust
// Engine 2: git log diff scan + paragraph-frequency dedup.
pub fn ft_core::link_review::compute_link_review(
    graph: &Graph, vault: &Vault, repo: &Path,
    window: &WindowRange, cfg: &Synth,
) -> Result<LinkReview>;

// Engine 1 (generalized): one journal feed across many targets.
// targets.len() == 1 preserves the original Related-aliases +
// self-exclusion semantics; multi-target skips both.
pub fn ft_core::journal::build_journal(
    graph: &Graph, targets: &[NoteId],
    vault: &Vault, repo: &Path, cache: &mut BlameCache,
) -> Result<JournalReport>;

// Plan/apply for synth-note scaffolding.
pub fn ft_core::synth::scaffold::plan_synth_scaffold(...)
    -> Result<SynthScaffoldPlan>;          // pure: no I/O writes
pub fn ft_core::synth::scaffold::apply_synth_scaffold(...)
    -> Result<PathBuf>;                    // writes via fs::write_atomic

// Per-section verification against the pinned git blob.
pub fn ft_core::synth::verify::verify_synth_note(...)
    -> Vec<VerificationResult>;
pub fn ft_core::synth::verify::verify_all(...)
    -> Vec<(PathBuf, Vec<VerificationResult>)>;
```

A **protected section** is an Obsidian-style callout written verbatim
into the synth note's markdown:

```
> [!ft-source] "notes/foo.md" L42-44 @abc1234 #7f3a91
> The original paragraph text
> spanning two lines.
```

The header tokens are, in order: vault-relative source path, inclusive
line range, short (7-hex) commit SHA, and a 6-hex blake3 content-hash
prefix. `synth::callout::{serialize, parse, compute_section_hash}`
round-trip cleanly; `verify_synth_note` strips the `> ` prefix from the
body and compares against the git blob slice at the pinned commit,
reporting `Ok` / `Drifted` / `SourceMissing` / `Malformed` per section.

A synth note is identified by an `ft-synth: true` frontmatter marker.
This lets the link-review skip wikilinks quoted inside `[!ft-source]`
callouts of a synth note (recycled material doesn't double-count on
the next ritual) while still counting links the user wrote in their
own prose between callouts.

CLI surface lives in `ft/src/cmd/{review.rs, synth.rs}` and the
extended `ft/src/cmd/notes.rs::run_journal` (which now accepts repeated
`--link "[[X]]"` flags in addition to the positional note argument).
The TUI exposes the flow through a new `Review` tab
(`ft/src/tui/tabs/review.rs`) that hands selected links off to the
Journal tab via `AppRequest::JournalForMulti` carrying a
`MultiTargetRequest`. The Journal tab gained: multi-target rendering
with a `matched: X, Y` badge when an entry's paragraph hits more than
one selected target, an in-window-only toggle (`w`), entry multi-select
(`Space`), and a `s` chord that opens an inline send-to-synth prompt
running the plan/apply scaffold and triggering the editor handoff.

Config: a new `[synth]` table with `folder` (default `"Synthesis/"`)
and `exclude_prefixes` (default empty; users typically add their
periodic-notes folder).

## Adding things

### A new subcommand

1. Create `ft/src/cmd/<name>.rs` with an `Args` struct and `run` fn.
2. Add `pub mod <name>;` to `ft/src/cmd/mod.rs`.
3. Add the variant to `Commands` in `ft/src/main.rs` and dispatch it.
4. If it needs vault data, call `Vault::discover(vault_flag)?` and
   `vault.scan()` — same pattern as the existing subcommands.

### A new TUI tab

The TUI ships six tabs today: Graph, Tasks, Notes, Timeblocks,
Journal, and Review (the last drives the synthesis ritual's
link-pick → Journal handoff; see §"Synthesis ritual").

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

## Modal driver (TUI)

The TUI's overlay/popup pattern (pickers, multi-step flows,
confirmation modals, the query-bar input mode) is unified behind a
single `Modal` trait and an App-level slot. Before this pattern,
every tab held an `Option<...>` field per modal kind and a long
`is_some()` dispatch chain prioritised them by ordering. The driver
collapses that to one `Option<ActiveModal>` on `App` and a uniform
dispatch precedence: **modal first, tab second, App-global third**.

### Trait + enum

`ft/src/tui/modal.rs` defines:

```rust
pub trait Modal {
    fn handle_event(&mut self, ev: Event, ctx: &TabCtx) -> ModalOutcome;
    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &TabCtx);
    fn keymap_help(&self) -> HelpSection;
    fn name(&self) -> &'static str;
    // Registry integration (defaults to empty / the shared empty keymap):
    fn commands(&self) -> &[CommandDef] { &[] }
    fn keymap(&self) -> &KeyMap { &EMPTY_KEYMAP }
}

pub enum ModalOutcome {
    Consumed,                       // modal handled the key, stays open
    Closed,                         // drop the slot
    OpenSibling(Box<ActiveModal>),  // swap the slot for a new modal
    NotHandled,                     // fall through to the tab
}

pub enum ActiveModal {
    Create(CreateState), Append(AppendState),
    CapturePicker(CapturePickerModal), CaptureVar(CaptureVarPromptState),
    SectionMove(SectionMoveState), MoveOuter(GraphMoveOuter),
    Rename(GraphRenameState), PresetPicker(PresetPickerModal),
    Related(RelatedModal), Search(SearchPickerModal),
    ConfirmDelete(ConfirmDeleteState), CreateSubdir(CreateSubdirState),
    JournalSources(JournalSourcesModal),
    JournalAppendOrReplace(JournalAppendOrReplaceModal),
    PeriodicLeader, QueryBar { view_id: usize },
}
```

(`commands`/`keymap` are the hook into the Command/Keymap registry
[docs/commands.md](commands.md) describes; the variant set is current
as of this writing — see `ft/src/tui/modal.rs` for the source of
truth.)

`App` holds `active_modal: RefCell<Option<ActiveModal>>` and exposes
`active_modal_name() -> Option<&'static str>` for the status-bar
indicator and tests.

### Three patterns by modal shape

- **Flow modals with free-function handlers**
  (`CreateState`, `AppendState`, `SectionMoveState`,
  `CaptureVarPromptState`): handler lives in
  `notes_actions/*::handle_key`; render lives in
  `notes_view::render_*_overlay`. `Modal::handle_event` wraps the
  handler; `Modal::render` calls the renderer. Modal impls live in
  `modal.rs`.
- **Tab-resident state machines outside the `Modal` slot.** The
  synth-section reslice flow (`ResliceState` in
  `notes_actions/reslice.rs`) is driven as a `NotesState::Reslicing`
  variant on the Notes tab via its own `handle_reslice_key`, not as an
  `ActiveModal`. It's the one remaining multi-step flow that predates
  the modal driver; new multi-step flows should go through `Modal`
  instead.
- **Pickers** (`SearchPickerModal`, `PresetPickerModal`,
  `CapturePickerModal`): each is a newtype wrapping
  `FuzzyPicker<S>` plus modal-specific metadata. On
  `PickerOutcome::Selected(item)`, the modal posts a tab-specific
  `AppRequest::Graph*` (e.g. `GraphJumpToNodes`,
  `GraphApplyPreset`, `RunCapturePreset`) with the typed payload.
  The newtypes live in `tabs/graph.rs` so they can reach
  graph-internal types.
- **Tab-resident state** (`GraphRenameState`, `RelatedModal`,
  `GraphMoveOuter`): state types stay in `tabs/graph.rs`; their
  `Modal` impls live there too. Commits post tab-specific
  `AppRequest::Graph*` variants (e.g. `GraphCommitRename`,
  `GraphConfirmRelated`) so the host can plan/apply against
  in-memory graph state. On recoverable error, the host re-posts
  `OpenModal` with the modal's last-typed state preserved.

### App ↔ Tab routing

Tab-specific actions raised by modals route through `AppRequest`
variants. There is exactly one routing table: `App::service_simple`.
It services every variant that doesn't need the terminal or the event
stream, looking the target tab up by its typed `Tab::kind()`
(`TabKind::Graph`, `TabKind::Journal`, …) and calling a typed
`Tab::graph_*` hook — same shape as the `Tab::queue_journal_for`
precedent (typed hooks per action, default no-op, host overrides).
The four terminal-touching variants (`OpenInEditor`, `OpenInObsidian`,
`SyncGit`, `CommitGit`) are deferred back to `App::service_request`,
which the real event loop calls; `App::service_pending_requests`
drains through the same table for the focus/switch paths and tests,
leaving deferred variants in the slot so tests can assert on them.
The recipe for adding a new modal action:

1. Add `AppRequest::Graph<Action> { … }`.
2. Add `Tab::graph_<action>(&mut self, …)` default no-op.
3. Override on `GraphTab` (or wherever the modal is hosted).
4. Add one arm to `App::service_simple`.

### TabCtx exposes modal state for render cues

`TabCtx::active_modal_name: Option<&'static str>` lets a tab's
`render` decide whether a modal is up without owning a parallel
flag. Used by `GraphTab::render` to style the query prompt yellow
and position the cursor when `Some("query-bar")`.

### Completion popup dispatch precedence

When a modal forwards keys to an `EditBuffer` that has an open
[`CompletionPopup`], the popup gets the first crack at every event.
The buffer's `handle_event`:

1. If `completion.popup` is `Some`, dispatches the key to the popup
   first. `Accepted(item)` applies the chosen item and closes the
   popup; `Dismissed` closes the popup (consuming `Esc`); `Consumed`
   absorbs navigation chords; `NotHandled` falls through to step 2.
2. Looks the chord up in `EDIT_KEYMAP` and dispatches the matched
   `edit.*` command, or inserts a printable char.
3. After an input mutation, if a provider is attached and its
   `TriggerSet` matches, queries the provider and opens / refreshes
   / closes the popup.

The modal host needs to know whether a popup is open so it can decide
whether `Esc` should reach the buffer (and dismiss the popup) or be
handled at the modal layer (closing the modal). `Tab::host_popup_open`
returns this state; the App pre-fills `TabCtx::host_popup_open` from
the active tab's value before invoking `ActiveModal::handle_event`.
Currently only `GraphTab` overrides `host_popup_open` (for its query
bar buffer) and only `QueryBar` reads `ctx.host_popup_open` (to
forward all keys — including `Esc` and `Enter` — to the buffer when
the popup is up).

Outside the popup-open path, `Esc` and `Enter` still close / apply
the modal directly. This means the modal layer's semantics are
unchanged for buffers without a provider attached — most mount sites.

[`CompletionPopup`]: ../ft/src/tui/widgets/completion.rs

### Status-bar modal indicator

When a modal is active, the status bar's right cell renders
`modal: <name>` in magenta instead of `mode: <label>` in yellow.
The in-flight sync indicator still takes priority over the modal
indicator.

### Adding a new modal

1. Define the state type (newtype if wrapping a picker, struct if
   tab-resident, or use an existing notes_actions flow type).
2. Implement `Modal` for it. Pickers post `AppRequest::Graph*` on
   selection. Tab-resident commit modals post a typed request and
   close.
3. Add a variant to `ActiveModal`.
4. Wire `ActiveModal::handle_event`/`render`/`keymap_help`/`name` to
   delegate to the new variant.
5. The launch site posts `OpenModal(Box::new(ActiveModal::<X>(state)))`
   via `ctx.pending_request`.

The plan-extract-modal-driver work is the reference implementation;
the migration is complete (every modal, `GraphMoveOuter` included,
now goes through `impl Modal`).

## Commands and keymaps (TUI)

Every TUI action has a stable `<context>.<verb>` name (`Command`) with
metadata (`CommandDef`); every key binding is a row in a `KeyMap` that
maps a chord to a command. The `?` overlay, `docs/keybindings.md`,
`ft commands list`, and `ft do` all read from the same registry — one
source of truth for what exists, what it does, and how to trigger it.

- **`ft/src/tui/command.rs`** — `Command`, `CommandDef`,
  `CommandScope`, `ArgSpec`, `CommandOutcome` (`Handled`/`NotHandled`),
  `CommandRegistry` (build-time union of every static command slice).
- **`ft/src/tui/keymap.rs`** — `KeyChord` (normalized so terminal
  inconsistencies don't matter), `chord_from_str`/`chord_to_str`
  round-trip, `KeyMap` with a fluent `.bind(...)` builder that panics
  on duplicate chords at construction time.
- **`ft/src/tui/app_commands.rs`** — `APP_COMMANDS` + `APP_KEYMAP`
  (global bindings: quit, tab cycling, help, git-leader).
- **`ft/src/tui/modal_commands.rs`** — per-modal `<MODAL>_COMMANDS`
  and `<MODAL>_KEYMAP` plus `confirm_def`/`cancel_def`/`nav_def`
  helpers.
- **Per-tab declarations** live next to each tab
  (`ft/src/tui/tabs/<tab>/` — `<TAB>_COMMANDS`, `<TAB>_KEYMAP`,
  `dispatch_command`).

Input resolves modal → tab → global. Cross-scope side effects flow
through `ctx.pending_request` as `AppRequest` variants so
`CommandOutcome` stays small.

Status bar: when a modal is active, the center cell renders up to
three `chord:label` pairs picked from the modal's keymap by
`CommandDef.is_primary = true`; the right cell shows
`modal: <name>` (from `extract-modal-driver`). Toasts override the
hint cell.

Headless dispatch: `ft do <command>` looks up the command in the
registry, validates args against `args_schema`, and calls a shared
headless handler (in `ft/src/cmd/do.rs`). Commands with
`opens_modal = true` are rejected with exit 2; commands with no
headless handler yet exit 3. Atomic ops with explicit selectors
(`tasks.complete-by-id --arg id=…`) are factored as the underlying
`ft-core` ops become callable without TUI ambient state.

Full write-up: [docs/commands.md](commands.md). Generated reference of
every registered command (re-run `ft commands docs > docs/keybindings.md`
after touching a `CommandDef` slice): [docs/keybindings.md](keybindings.md).

## Concurrency model

The codebase is deliberately single-threaded everywhere except for two
producer threads that feed the TUI event channel. There is no async
runtime — no tokio, no `async fn`. The main event loop blocks on
`mpsc::Receiver::recv()`, processes one event, redraws, and loops.

### Producers

1. **Crossterm input thread** (`ft/src/tui/event.rs::crossterm_loop`)
   reads stdin and sends `Event::{Key,Mouse,Resize,Tick}` onto a shared
   `mpsc::channel`. Owns no app state.
2. **Background workers** (`ft/src/tui/app.rs::run_sync_job`,
   `run_commit_job`, `run_graph_job`, and any future siblings) own
   their inputs (`Arc<Vault>` clones, `PathBuf`, options) moved into a
   `move ||` closure, do synchronous work, and send exactly one
   `Event::Background(BgEvent)` back into the same channel before
   exiting.

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

### Shared graph snapshot

The TUI holds exactly one graph: an App-owned
`Option<Arc<GraphSnapshot>>` (`ft/src/tui/snapshot.rs` — the built
`Graph`, the `Scan` it came from, and a monotonic generation), handed
to every tab and modal via `TabCtx::snapshot`. **Tabs never call
`vault.scan()` or `Graph::build`.** Rebuilds run on `run_graph_job`
(a background worker like git sync) and are requested by raising
`TabCtx::request_graph_refresh()` after any vault mutation — a
`Cell<bool>` the App consumes at its drain points, single-flight with
a dirty flag so bursts coalesce into at most one follow-up build.
On `BgEvent::GraphReady` the App installs the snapshot and calls the
active tab's `on_graph_ready`; background tabs catch up by comparing
generations in `on_focus`/`handle_event`. Cross-rebuild UI state
(expansion, selection, cursor anchors) keys off `NodeKey`; mutations
that need read-after-write cursor placement store a pending anchor
resolved on the next adoption. Until the first snapshot lands, tabs
render a loading line. Tests drive the lifecycle deterministically via
`App::pump_graph_rebuild_for_test()` (the `for_test` constructors
install an eager first snapshot). See
`openspec/changes/shared-graph-snapshot/`.

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
