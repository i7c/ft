## Context

The Graph tab ships a `Ctrl+P` "load preset into active view" flow: a
fuzzy picker (`PresetPickerModal` + `PresetPickerSource`, both in
`tabs/graph/modals.rs`) reads `Config::graph.presets` +
`ft_core::graph::preset::builtin`, and on Enter posts
`AppRequest::Graph(GraphRequest::ApplyPreset(dsl))`, which the App routes
to `GraphTab::handle_graph_request` (the one typed cross-tab channel
described by the `tui-tab-request-routing` spec). The Graph tab's own
view applies the DSL in-memory against the snapshot — no rebuild.

The Tasks tab has the same shape of problem (an editable DSL query bar on
`SearchView` driving a filtered task list) but none of the machinery:
no preset command, no picker, and no cross-tab request channel back to
itself. Task presets already exist in `ft-core`
(`query::preset::builtin` → `today`/`overdue`/`upcoming`/`done-today`/
`not-done`, parsed under `Profile::Tasks`; user presets in
`Config::presets` — a separate map from `Config::graph.presets`) and the
CLI resolves them via `ft/src/cmd/tasks.rs::resolve_preset`. The Tasks
TUI tab currently only ever posts `AppRequest::Toast` and
`AppRequest::OpenInEditor`; it has no way to receive a request.

Constraints carried over from the codebase:

- New multi-step / keyboard-capturing flows go through the `Modal`
  driver (one `ActiveModal` slot), not per-tab `Option<...>` fields
  (AGENTS.md "Modal driver (TUI)"; `tui-modal-driver` spec). The Tasks
  tab's existing inline overlays (`popup`, `quickline`, `edit_state`)
  predate the migration and are explicitly *not* the model to copy.
- Cross-tab request routing is per-tab typed enums + one `Tab` hook,
  not a generic active-tab abstraction (`tui-tab-request-routing` spec;
  confirmed in design disambiguation Q1).
- Every TUI action is a registered `Command` + `CommandDef`; every
  binding a `KeyMap` row; `?` overlay, `docs/keybindings.md`, and
  `ft commands list` all read the registry (AGENTS.md "Command/Keymap
  registry").

## Goals / Non-Goals

**Goals:**
- `Ctrl+P` on the Tasks tab opens a fuzzy picker over task presets
  (user `Config::presets` + `query::preset::builtin`, user shadows
  built-in).
- Selecting a preset replaces the active `SearchView`'s query text and
  recomputes matches against the current snapshot, in normal mode, with
  no graph rebuild.
- Full registry + docs integration: new `tasks.preset-pick` command,
  new `modal/task-preset-picker` scope + commands/keymap, `?` overlay
  row, regenerated `docs/keybindings.md`.
- A Tasks-targeted cross-tab request channel (`AppRequest::Tasks` +
  `Tab::handle_tasks_request`) so the modal can commit back to the
  owning tab — the one structural addition the Tasks tab lacks.

**Non-Goals:**
- No multi-view / "new view with preset" flow on the Tasks tab (the
  Graph tab's `Ctrl+N` path). The Tasks tab has a single `SearchView`
  and no view strip; `Ctrl+P` applies to that one view only.
- No generalization of `PresetPickerModal`/`PresetPickerSource` to serve
  both tabs. A parallel, decoupled `TaskPresetPicker*` is added instead
  (confirmed Q2) — the two tabs read different preset maps and post to
  different request channels, and the registry already has a parallel
  `preset-picker` modal to mirror.
- No migration of the Tasks tab's existing inline `popup`/`quickline`/
  `edit_state` overlays to `ActiveModal`. Out of scope; this change
  only adds one new modal.
- No CLI change. `ft tasks --preset` already resolves the same
  presets; the TUI reuses `ft_core::query::preset` as the source of
  truth.
- No new config keys.

## Decisions

### D1: Per-tab typed request channel, mirroring Graph (not a generic active-tab route)

The modal needs to hand a `TasksRequest::ApplyPreset(dsl)` back to the
owning `SearchView`, but it only has `&TabCtx` (view state lives on the
App). The Graph tab solves this with `AppRequest::Graph(GraphRequest)`
+ `with_tab(TabKind::Graph, |tab, ctx| tab.handle_graph_request(req, ctx))`.

**Decision:** mirror that pattern exactly — add `AppRequest::Tasks(TasksRequest)`
+ `Tab::handle_tasks_request` + one `service_simple` arm doing
`with_tab(TabKind::Tasks, …)`. `TasksRequest` starts with a single
`ApplyPreset(String)` variant.

**Why over the alternative (generic `AppRequest::ApplyPresetToActiveTab`):**
the codebase's stated philosophy is one typed channel per owning tab
(`tui-tab-request-routing` spec: "replaces what used to be sixteen
dedicated `AppRequest::Graph*` variants"). A generic active-tab route
would couple the modal to the assumption that the tab that opened it is
the active tab (true today, but a weaker invariant) and would introduce
an abstraction with no second consumer. The per-tab enum is
boilerplate-heavy but matches every existing precedent (Gather has
four dedicated hooks; Graph has its enum) and keeps each tab's request
surface self-documenting. Confirmed in disambiguation Q1.

**Why not extend `GraphRequest` with a tasks variant:** that would route
a Tasks-targeted request through the Graph tab's lookup arm — wrong
owning tab, and would violate the "one channel per tab" invariant.

### D2: Parallel `TaskPresetPicker*` types + `ActiveModal::TaskPresetPicker`, not a generalized picker

The existing `PresetPickerModal`/`PresetPickerSource` (in
`tabs/graph/modals.rs`) are graph-coupled: the source reads
`Config::graph.presets` + `graph::preset::builtin`, and the modal's
commit hardcodes `AppRequest::Graph(GraphRequest::ApplyPreset)`.

**Decision:** add a parallel, decoupled pair:
- `TaskPresetPickerSource` — reads `Config::presets` +
  `query::preset::builtin` (the *task* preset maps), dedups with
  user-shadows-built-in. A near-copy of `PresetPickerSource::new` body
  against different maps; lives in a new `tabs/tasks/modals.rs`.
- `TaskPresetPickerModal` — `FuzzyPicker<TaskPresetPickerSource>` +
  modal chrome; on Enter resolves the name → DSL (user map first, then
  `query::preset::builtin`) and posts
  `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`.
- `ActiveModal::TaskPresetPicker(TaskPresetPickerModal)` — one new
  enum variant with the standard five `Modal`-trait dispatch arms in
  `modal.rs` (handle_event / render / keymap_help / name / commands /
  keymap).

**Why not generalize `PresetPickerModal` over a commit strategy** (e.g.
a closure or trait object param): the fuzzy-picker chrome and keymap
*are* shareable, but generalizing would couple the two tabs' commit
paths behind an abstraction and force `PresetPickerModal` to become
generic over both the source type and the commit target. The codebase
prefers explicit variants per concern (`TaskEdit` vs `TaskCreate`,
`CapturePicker` vs `PresetPicker`); a parallel variant is consistent
and the duplication is shallow (the source `new()` is ~15 lines, the
modal wrapper is ~60). The registry gets a parallel
`modal/task-preset-picker` scope + `TASK_PRESET_PICKER_COMMANDS`/
`KEYMAP` mirroring the existing `preset-picker` ones. Confirmed Q2.

**Shared `FuzzyPicker<S>` + `PickerSource` trait:** yes — the underlying
fuzzy-picker widget is already generic over `S: PickerSource`; both
tabs reuse it. Only the source impl and modal wrapper are duplicated.

### D3: Apply = set `query_text` + `recompile` + `recompute_matches`, stay in normal mode

`SearchView` already has the machinery: `recompile(today)` parses
`query_text` under `Profile::Tasks` into `parse_state`, and
`recompute_matches(today)` rebuilds the `matches` index against the
snapshot's graph (no `Graph::build`, no `vault.scan()` — reads
`ctx.snapshot`). `apply_edit` (the inline `/` path) does exactly this
sequence after setting `query_text = buf.text`.

**Decision:** `TasksTab::handle_tasks_request(ApplyPreset(dsl))` sets
`view.query_text = dsl`, calls `view.recompile(ctx.today)` +
`view.recompute_matches(ctx.today)`, and does *not* enter edit mode
(`edit_state` stays `None`). The view stays in normal mode, mirroring
the Graph tab's `apply_preset_to_active_view`.

**Why normal mode (not drop into edit mode):** parity with the Graph
tab, and the most common case is "pick a preset, look at the new
results." A user who wants to tweak can press `/`. Confirmed Q3. The
apply path takes `ctx.today` (already fixed for the App's lifetime) so
relative-date presets like `due < today` resolve consistently.

### D4: Routing the apply through `TasksTab`, not `SearchView` directly

`SearchView` is private to `tabs/tasks/`, but the `Tab` hook signature is
`handle_tasks_request(&mut self, req, ctx)` on `TasksTab`. The tab owns
the `views: Vec<Box<dyn View>>` + `active_view` index (kept for future
multi-view expansion even though there's one view today). The hook will
downcast the active view to `SearchView` (via the `view::View` trait not
exposing `query_text` — so either add a `View`-trait method or route
through a tasks-tab-internal method that knows the concrete type).

**Decision:** keep the concrete-type knowledge inside `tabs/tasks/`:
`TasksTab::handle_tasks_request` calls a `SearchView`-specific method
(e.g. `apply_preset(&mut self, dsl, today)`) added to `SearchView`,
reaching the concrete type via the tab's `views[active_view]` — which is
`Box<dyn View>`, so the tab will need a downcast or a dedicated trait
method. Cleanest: add `fn apply_preset(&mut self, dsl: &str, today:
NaiveDate)` to the `view::View` trait with a default no-op (only
`SearchView` overrides it), so `TasksTab` can call it through the
`Box<dyn View>` without downcasting. This mirrors how `on_graph_ready`
/ `refresh` are already `View`-trait methods with default no-ops.

### D5: Command + keymap + scope registration

Per the registry invariants (`tui-commands` spec; AGENTS.md build
invariants), every new action needs a `CommandDef` and the modal needs
its own scope:

- `tasks.preset-pick` (`opens_modal: true`, `scope: Tab("tasks")`,
  `group: "Navigation"`) added to `TASKS_COMMANDS` in
  `tabs/tasks/mod.rs`; bound `Ctrl+p` in `SEARCH_KEYMAP`
  (`tabs/tasks/search.rs`); dispatched in `SearchView::dispatch_idle_command`
  → opens the modal.
- `modal/task-preset-picker` scope added to the
  `scope_for_command_name` map in `keymap.rs`.
- `TASK_PRESET_PICKER_COMMANDS` + `TASK_PRESET_PICKER_KEYMAP` (scope
  `CommandScope::Modal("task-preset-picker")`) added to
  `modal_commands.rs`, and included in the `all_modal_command_names_unique`
  test slice list and the `(&KEYMAP, COMMANDS)` registry pair list.
- Tasks tab `help_sections()` gains a `Ctrl+P` row.
- `docs/keybindings.md` regenerated via `ft commands docs`.

### D6: Empty-presets no-op guard (unreachable, but consistent)

The Graph tab's `open_preset_picker_for_active_view` early-returns when
`src.items.is_empty()`. Tasks *always* has built-ins, so this branch is
unreachable — but mirroring the guard keeps the two flows structurally
identical and defends against a future "disable built-ins" config.
Confirmed Q5.

## Risks / Trade-offs

- **[Duplication between graph and tasks picker sources/modals]** →
  Accepted (D2). The duplication is shallow and the two are coupled to
  different preset maps + different request channels. Generalizing
  would add a generic-over-commit abstraction for one extra consumer.
  If a *third* preset-picker tab ever appears, revisit and extract a
  shared `PresetPicker<S, Commit>` then.
- **[Adding a `View`-trait method (`apply_preset`) just for one impl]**
  → Accepted (D4). The `View` trait already carries default-no-op
  methods used by a single impl (`on_graph_ready`, `refresh`); one
  more is consistent and avoids `dyn` downcasting.
- **[Two parallel routing channels (`Graph` + `Tasks`) on `AppRequest`]**
  → Accepted (D1). Each is one variant + one `service_simple` arm +
  one `Tab` hook. This is the established pattern; a generic channel
  would be premature.
- **[Snapshot test churn]** → The tasks `?`-overlay snapshot gains a
  `Ctrl+P` row; one open-picker snapshot is added. Bless deliberately
  per the modal-driver snapshot convention. `cargo run --release -q --
  commands docs --check` must pass.
- **[No behavioral change to `ft tasks --preset`]** → The CLI path is
  untouched; both read the same `ft_core::query::preset` source. No
  risk of drift in preset resolution semantics.

## Migration Plan

Additive only — no behavior is removed or renamed. No config migration,
no data format change. Implementation order (matches `tasks.md`):

1. `ft-core` needs no changes (presets + builtin already exist).
2. Add `TasksRequest` + `AppRequest::Tasks` + `Tab::handle_tasks_request`
   (tab.rs) and the `service_simple` arm (app.rs).
3. Add `TaskPresetPickerSource` + `TaskPresetPickerModal`
   (`tabs/tasks/modals.rs`) and the `ActiveModal::TaskPresetPicker`
   variant + dispatch arms (`modal.rs`).
4. Add the registry bits (`modal_commands.rs`, `keymap.rs`) and the
   `tasks.preset-pick` command + `Ctrl+p` binding + dispatch
   (`tabs/tasks/mod.rs`, `tabs/tasks/search.rs`).
5. Wire `SearchView::apply_preset` (and the `View`-trait method) +
   `TasksTab::handle_tasks_request`.
6. Tests + snapshot + docs regen; run the five build invariants.

Rollback: revert the change; no on-disk state depends on it.
