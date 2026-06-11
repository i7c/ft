## Context

The Journal tab currently maintains its loaded-targets state across three near-parallel paths:

1. `target: Option<JournalTarget>` set by `load_for(target)` (single-source `/` picker, or `Shift+J` from Graph).
2. `multi_targets: Vec<JournalTarget>` plus `window: Option<JournalWindow>` set by `load_for_multi(request)` (Review-tab `Enter` handoff).
3. A tab-resident `picker: Option<FuzzyPicker<VaultFilePickerSource>>` field that captures keys directly (bypassing the global modal driver).

The TUI uses the App-level `ActiveModal` slot (extract-modal-driver) for every other modal flow; the Journal tab's `picker` field is the odd one out. The send-to-synth flow already routes its own pickers through tab-resident state, so the precedent of "managing UI from inside the tab" exists, but it's also been called out as something to migrate.

Cross-tab affordances today: `Shift+J` from Graph → `AppRequest::JournalFor(target)` (single); Review `Enter` → `AppRequest::JournalForMulti(request)` (replace-all). There is no append affordance and no way to view or edit the source set after the load.

The graph tab's `multi_selected: HashSet<NodeKey>` is already maintained for the rename-multi-move flow; this change reuses it without modifying its lifecycle.

`Graph::nodes()` already exposes both `NodeKind::Note` and `NodeKind::Ghost`; building a fresh graph at the moment a picker opens is cheap relative to the rest of the journal pipeline (`load_for_multi` already does this).

## Goals / Non-Goals

**Goals:**
- Make the active source set always visible on the Journal tab — never inferred from a title bar.
- Provide a single coherent place (the Sources Manager modal) to view, clear, add, and remove sources.
- Add ghost-aware picking (current `VaultFilePickerSource` only surfaces real files).
- Provide a Graph → Journal "append these to the journal" flow that reuses the existing `multi_selected` state.
- Collapse the two parallel load paths (`load_for` / `load_for_multi`) into one source-set-backed rebuild, removing the structural drift between single-target and multi-target journal state.
- Migrate the Journal tab's stray `picker` field onto the App-level modal driver (consistent with the rest of the TUI).

**Non-Goals:**
- Modifying `ft-core` (journal builder, blame cache, link review).
- Changing the Review-tab → Journal handoff semantics. Review still issues a "replace all sources" intent via `JournalForMulti`; the new Append/Replace prompt is only raised for the new `JournalAddSources` request.
- Persisting the source set across TUI sessions (out of scope; session-level state).
- Background-thread journal rebuilds (still synchronous, same as today).
- Changing the journal-entry list rendering, send-to-synth flow, in-window filter, or matched-targets badge.
- Reworking the Graph-tab `Shift+J` single-target behavior. It stays as-is; `Shift+A` is additive.

## Decisions

### 1. Single `sources: Vec<JournalTarget>` slot replaces `target` + `multi_targets`

**Decision**: Replace the Journal tab's `target: Option<JournalTarget>` and `multi_targets: Vec<JournalTarget>` fields with one `sources: Vec<JournalTarget>` (and the existing `window: Option<JournalWindow>` field retained). Empty = empty state. Any rebuild reads from this slot.

**Why**: The current two-path arrangement encodes "how I was loaded" as state, which is exactly what the new model removes. With one slot, `rebuild_journal(ctx)` becomes the single mutation seam; `JournalFor` (graph Shift+J), `JournalForMulti` (review handoff), and the new `JournalAddSources` (graph Shift+A) all funnel through "mutate sources, then rebuild". Tests assert on `sources.iter()`, not on a sometimes-set sometimes-not pair.

**Alternative considered**: Keep `target` for single-source mode and only flip to `multi_targets` past `len==1`. Rejected because the rendering branch on `multi_targets.len() > 1` is exactly the divergence the strip is meant to erase — the strip should look the same conceptually for 1, 2, or N sources.

### 2. Sources Manager modal lives on the App `ActiveModal` slot (not tab-resident)

**Decision**: Add `ActiveModal::JournalSources(JournalSourcesModal)` to the existing modal enum. The Journal tab's `/` keymap entry raises `AppRequest::OpenModal(...)`; on Enter the modal commits via `AppRequest::JournalCommitSources { sources, mode }` (where `mode ∈ {Replace}` — the manager always replaces, since it edits the live set directly). The legacy `picker: Option<FuzzyPicker<...>>` field on `JournalTab` is removed in the same change.

**Why**: Aligns with the modal-driver pattern every other tab now uses (extract-modal-driver). Avoids the bespoke `handle_picker_key` early-return in `JournalTab::handle_event`. Snapshot tests already exercise modal rendering via a single path.

**Alternative considered**: Keep the picker tab-resident and just bolt the list + actions onto a new field. Rejected because it perpetuates the very inconsistency CLAUDE-flagged in the existing code, and because the modal carries its own keymap which is easier to test in isolation.

### 3. Append/Replace prompt is a separate small modal raised on `JournalAddSources`

**Decision**: When the Journal tab receives `AppRequest::JournalAddSources { targets, default_mode }`, it raises `AppRequest::OpenModal(JournalAppendOrReplace { incoming_targets, default_mode })`. The modal renders a 5-line inline prompt (mirroring the existing `NonSynthPrompt` shape) with `[a] append` / `[r] replace` / `[c] cancel`. Enter commits the focused choice; the modal then either replaces or unions sources and triggers a rebuild.

**Why**: The decision is binary, the prompt is fleeting, and the existing `NonSynthPrompt` flow is the closest precedent — a small inline prompt at the bottom of the tab area. A full modal page would be visual overkill.

**Alternative considered**: Auto-append silently (no prompt) and surface a toast. Rejected: the user might be deep in a 12-source review and accidentally adding 30 graph rows; an undo via toast is brittle.

**Alternative considered**: Always replace (current Review-handoff behavior) and let the user reconstruct the set via the manager. Rejected: the user explicitly asked for an append affordance and "replace by accident" is the most painful single regression a tab can have.

### 4. Ghost-aware picker source

**Decision**: Add a new `JournalSourcePickerSource` to `ft/src/tui/widgets/picker.rs` that owns an `Arc<Vault>` *and* takes a fresh `Graph` snapshot at construction time. Its `query()` calls `fuzzy_find` on the vault for real-note hits and then matches the input against `graph.nodes()`-derived ghost raw strings, merging both into one ranked result list. Items carry a new `JournalSourceHit` enum `{ Note(PathBuf), Ghost(String) }`. The legacy `VaultFilePickerSource` is unchanged so the rest of the TUI keeps working.

**Why**: Ghosts are first-class in the journal builder (they have NoteIds in the graph); the picker should reflect that. A separate source keeps the existing `VaultFilePickerSource` untouched (it's used by the send-to-synth and notes-create flows where ghosts would be wrong).

**Alternative considered**: Extend `VaultFilePickerSource` with a constructor flag. Rejected because every existing call site would pay the cost of either reading the flag or holding a graph snapshot they don't need.

**Alternative considered**: Two separate pickers in the manager — one for notes, one for ghosts. Rejected because users think of "this concept" not "real or imaginary", and `[[Phantom]]` should rank near `Phantasm.md` in one list.

### 5. Graph → Journal append uses existing `multi_selected` + cursor fallback

**Decision**: New command `graph.add-to-journal-sources` (default chord `Shift+A`). It first reads `active_view().multi_selected`; if empty, falls back to the cursor row. For each entry it resolves the `NodeKey` → `NodeKind` and emits `JournalTarget::Note(path)` or `JournalTarget::Ghost(raw)`; non-Note/non-Ghost rows (Directory, Task, Paragraph) are silently skipped. If zero targets resolve, the command toasts an error and is a no-op. The final non-empty list is shipped as `AppRequest::JournalAddSources { targets, default_mode: AppendOrReplaceMode::Append }`.

**Why**: Mirrors the existing graph→journal jump (`graph.journal`) at line ~2942 — same node-kind mapping, ghost-aware. Reuses `multi_selected` so users don't learn a new selection model.

**Alternative considered**: Use a different selection mechanism (e.g. a one-shot pick mode). Rejected because the user already has muscle memory for `Space` to multi-select on the graph, and adding a second mode is unnecessary surface area.

### 6. Sources strip rendering layout

**Decision**: Reserve 2 lines at the top of the Journal tab's inner area (inside the existing border):
- Line 1: `Sources (N) [window: since 7d] [filter: in-window]` (the `[window: ...]` and `[filter: ...]` chunks only appear when applicable).
- Line 2: comma-separated source labels. Each label is `<path>` or `<raw> (ghost)`. If the joined string exceeds inner width, truncate to `…, +K more` where K is the number of sources not shown. Empty state renders `no sources loaded — press / to manage sources`.

The existing border title (`Journal — ...`) keeps the source count for terminal-width compatibility but is no longer the sole signal.

**Why**: Two lines is the minimum that comfortably surfaces both the count + window AND the labels themselves. Single line forces too much truncation. Three lines wastes vertical space on the common 1–3 sources case.

**Alternative considered**: Conditional 1- or 2-line height. Rejected because variable layout breaks the entry-list scroll math (`entry_starts` indexing into a known `view_height`); a fixed 2-line reservation keeps the math stable.

### 7. Removed-source side effects

**Decision**: Removing or clearing sources in the manager rebuilds the journal as soon as the modal commits (Enter or Esc). The selection cursor on entries resets to 0. The `in_window_only` filter state is preserved iff `sources.len() >= 2 && window.is_some()`; otherwise it's cleared. Selected entries (`entry_selected`) are cleared on any source-set mutation.

**Why**: A source-set change invalidates the cursor's position relative to entries that may have moved or disappeared. Preserving `in_window_only` only when meaningful avoids a stale toggle that does nothing.

### 8. Keymap & help

**Decision**:
- Journal tab: `/` now opens the manager (lands on add-source picker), `c` still clears (now: via the manager's clear action), `R` reloads (unchanged). New: `+` opens manager landed on add-source (alias for `/`'s subbehavior). `Esc` from anywhere closes the manager.
- Graph tab: new chord `Shift+A` → `graph.add-to-journal-sources`. Help section: existing "Cross-tab" group, new row `Shift+A: append selected to Journal sources`.
- Help overlay for Journal tab: replace the "Source" section with "Sources" section listing `/`, `+`, `c` (via manager), `R`, and `Esc` semantics.

**Why**: `/` semantics shift but are documented; `+` is a discoverable additive entry. `Shift+A` mirrors `Shift+J` in spirit (cross-tab affordance, capital chord).

## Risks / Trade-offs

- **`/` muscle memory breakage** → The legacy `/` was "open picker, pick one note, replace". The new `/` opens the manager. We mitigate by: (a) defaulting the manager to add-source picker mode so muscle memory of "press `/`, type, Enter" still finds + adds a source — the difference is it appends rather than replaces; (b) calling the breakage out in the proposal and help overlay; (c) `c` from the manager landing state can still clear-then-add, achieving the old behavior in two keystrokes.
- **Always-on 2-line strip costs vertical space** → On a 24-row terminal that's 8% of the journal feed. Mitigation: strip's text is dim by default; for power users on tiny terminals we can later add a `Z` toggle to hide it. Out of scope for this change.
- **Ghost picker requires a fresh graph snapshot on open** → For very large vaults this could be a perceptible delay. Mitigation: `Graph::build` for the existing journal pipeline already has the same cost characteristics; users who can tolerate `Shift+J` will tolerate `/` here. If this becomes a problem we can lazy-load ghost rows after the first keystroke.
- **Tests over modal flows are verbose** → Snapshot tests for the strip + manager + prompt + cross-tab append flow will be 4–6 new snapshots. Mitigation: reuse `TestBackend` + `assert_cmd` patterns already established for the Review/Journal tabs.
- **Replacing two load paths with one risks regressing edge cases** → e.g. window state, error banner timing. Mitigation: the new `rebuild_journal(ctx)` is the only call site; each existing test path (`/` load, Shift+J, Review Enter) gets at least one integration test in this change.
- **`JournalAddSources` is a new AppRequest variant — Tab trait widens by one method** → Pattern is already established for `JournalFor`, `JournalForMulti`. Default no-op on every other tab keeps existing tabs untouched.
