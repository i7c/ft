## Context

Two changes bundled here. The first — retag — is the user-facing feature; the
second — the config namespace move — is a small but breaking rename that is
cheap to land alongside it (both touch `Config::Tasks`).

**Retag, current state.** The Tasks SearchView (`ft/src/tui/tabs/tasks/search.rs`)
already has the full mutation seam: `with_selected_task` + `ops::update_task_line`
with the expected-`Task` guard, an anchor-based cursor restore, and a
graph-refresh request. It also has a working modal→view channel: the
`TaskPresetPickerModal` posts `AppRequest::Tasks(TasksRequest::ApplyPreset(dsl))`
and `TasksTab::handle_tasks_request` routes it to `SearchView::apply_preset`.
The retag flow mirrors this exactly with a different payload (a tag, not a DSL
string) and a different write (retag, not query-replace).

**The tag model constraint — load-bearing.** `EmojiFormat::serialize_line`
(`ft-core/src/task/emoji.rs:150`) emits `task.description` **verbatim** and
never re-emits `task.tags`. `task.tags` is a *derived index* rebuilt by
`extract_tags(&description)` on parse. The existing
`merge_tags_into_description` helper (`ft/src/tui/tabs/tasks/edit_popup.rs:292`)
proves the pattern: to persist a tag change you rewrite the description's
inline `#tag` words. But that helper strips *all* inline tags, which is the
wrong semantics for retag (it must preserve tags outside the configured list).
So retag needs a surgical variant, and it belongs in `ft-core::task::ops`
(not the TUI) so the CLI can reuse it later.

**Config namespace, current state.** `Config` has `presets: HashMap<String,
String>` at top level (`ft-core/src/config.rs:38`) while `GraphCfg` has
`presets` under `[graph.presets]`. `Config` is `#[serde(deny_unknown_fields)]`,
so the rename is a hard break — a leftover top-level `[presets]` is a startup
error, not a silent ignore. That is the desired behavior (the user asked to
*move* it, not alias it), but it must be loud.

## Goals / Non-Goals

**Goals:**
- One-chord retag: a configured short tag list, a picker, Enter swaps the tag.
- Swap semantics: replace only tags drawn from the configured list; leave all
  other inline tags (and the description text) untouched.
- Reuse the existing mutation + modal→view seams; no new architectural
  patterns, no new `Option<...>` field on `SearchView`.
- Move task presets to `[tasks.presets]`, matching `[graph.presets]`.

**Non-Goals:**
- No CLI surface for retag (no `ft tasks retag`). The op is in ft-core so a
  future CLI command is cheap, but this change wires the TUI only.
- No multi-select retag, no tag autocomplete beyond the configured list.
- No aliasing the old top-level `[presets]` location. The break is
  intentional and one-time.
- No change to how `Task.tags` is parsed or stored — the derived-index model
  stays; retag rewrites the description.

## Decisions

### D1: Retag lives in `ft-core::task::ops`, rewrites the description

A new `retag_task(target_path, line, format, expected, list: &[String], tag: &str)`
op. It wraps `update_task_line` with a mutate closure that:

1. Builds the new description by keeping every whitespace-delimited word that
   is **not** a `#tag` whose bare form is in `list`, then appending `#tag` if
   `tag` is non-empty.
2. Sets `task.description` to the result (the derived `task.tags` index is
   not touched directly — it is rebuilt on the next parse).

The "strip only list members, preserve others" rewrite is the surgical
counterpart to `merge_tags_into_description` (which strips all). The `is_tag_word`
predicate already exists in `edit_popup.rs`; a `ft-core` copy is the right home
since the op must not depend on the TUI crate.

**Why not mutate `task.tags` directly and let the serializer fix the
description?** Because the serializer *doesn't* fix the description — it emits
`description` verbatim. Mutating `task.tags` alone is a no-op on disk. This is
documented in the comment above `merge_tags_into_description` and is the
reason every existing tag-mutation path rewrites the description.

**Alternative considered:** make the serializer the source of truth for tags
(strip inline `#tag` words and re-append from `task.tags`). Rejected — it
would reorder/dedupe the user's description text on every unrelated edit and
breaks the round-trip property tests in `emoji.rs`.

### D2: Picker rides the `TaskPresetPickerModal` pattern exactly

New `TaskRetagPickerModal` + `TaskRetagPickerSource` in
`ft/src/tui/tabs/tasks/modals.rs`, parallel to the existing preset picker.
On Enter it posts
`AppRequest::Tasks(TasksRequest::RetagSelected(tag))`; `TasksTab::handle_tasks_request`
gains an arm routing to a new `SearchView::apply_retag(tag)`.

`apply_retag` mirrors `apply_preset`'s shape but writes instead of queries: it
calls `with_selected_task` (which already does the expected-`Task` guard,
the anchor restore, and `ctx.request_graph_refresh()`), invoking
`ops::retag_task` inside the closure.

**Why a `TasksRequest` variant and not a direct write from the modal?** The
modal has no handle to the `SearchView`'s selected task or its `tasks` slice
— it only has `&TabCtx`. The `AppRequest::Tasks` channel is the designed path
for modal-raised, Tasks-tab-serviced mutations (see `tui-tab-request-routing`
spec and the comment above `TasksRequest`).

### D3: Empty list → no-op open, not an error

If `config.tasks.retag_tags` is empty (the default), `tasks.retag` shows a
toast ("no retag tags configured — add [tasks.retag_tags]") rather than
opening an empty picker. Mirrors the preset picker's empty-list guard in
`tasks.preset-pick` (which silently no-ops; here a toast is clearer since the
feature is opt-in via config).

### D4: `retag_tags` stores bare names, no leading `#`

Consistent with `Task.tags` (bare strings, the `#` is decoration) and with
how `parse_tags_field` / `merge_tags_into_description` normalize. The picker
labels render with a leading `#` for readability; the stored value and the
`TasksRequest::RetagSelected` payload are bare.

### D5: Config rename is a hard move, no aliasing

`Config::presets` field is deleted; `Tasks` gains `presets`. Because
`Config` is `deny_unknown_fields`, an old `[presets]` section fails config
load with a clear "unknown field `presets`" error. We do **not** add a
`#[serde(alias = "presets")]` fallback — the user asked for the move, and a
silent fallback would leave two sources of truth forever. The error message
plus the changelog entry is the migration path.

All call sites updated atomically with the field move: `cmd/tasks.rs`
(`resolve_preset`), `cmd/vault.rs` (config dump), and `tabs/tasks/modals.rs`
(`TaskPresetPickerSource::new`).

## Risks / Trade-offs

- **[BREAKING config rename]** → Mitigated by a loud `deny_unknown_fields`
  error at startup naming the field, plus documenting the rename in the
  change. No data loss — the user moves the section. Low blast radius
  (single-user tool, config is version-controlled alongside the vault).
- **[Retag is a description rewrite; exotic description shapes could
  surprise]** A description with a `#tag` glued to punctuation
  (`fix#computer-thing`) — `is_tag_word` requires the word to be *exactly*
  `#<alphanum/_-/chars>`, so glued tags aren't matched and are preserved.
  This matches the parser's own `extract_tags` behavior, so what retag sees
  as "a tag" == what the system sees as "a tag". No surprise.
- **[No CLI retag in v1]** → The op is in ft-core so a future
  `ft tasks retag --tag X --id <id>` is a thin wrapper. Not blocking.
- **[Picker offers only configured tags; a tag already on the task that
  isn't in the list stays]** → This is the requested behavior. The list is
  the "switchable set"; everything else is structural. If the user wants
  freeform tagging they use the edit popup (`e`).

## Migration Plan

1. Land the code: config field move + `retag_task` op + modal + command +
   keymap, regenerate `docs/keybindings.md`.
2. User renames `[presets]` → `[tasks.presets]` in their `config.toml`
   (one `sed` or manual edit). The startup error names the stray field if
   they forget.
3. User optionally adds `[tasks]\nretag_tags = ["wait", "computer", "physical"]`.
4. Rollback: revert the commit; the old top-level `[presets]` works again
   (config is not persisted in a migrated form — it's user-authored TOML).
