## Context

The TUI already hosts four tabs (`Graph`, `Tasks`, `Notes`, `Timeblocks`) that all implement a single `Tab` trait. Each tab owns its state and renders via `render(&mut self, frame, area, ctx)`. The App owns the event loop, tab vector, and shared resources (`Arc<Vault>`, `Arc<RecentsLog>`, today's date, `pending_request`).

`ft_core::journal::build_journal` is already in place from `related-notes-journal`, as is `BlameCache` (msgpack at `.ft/cache/blame.msgpack`). The CLI `ft notes journal <note>` exercises the same data path the tab will reuse.

The existing Graph tab demonstrates the patterns this change relies on:
- A fuzzy picker overlay via `FuzzyPicker<VaultFilePickerSource>` (used for move-target).
- Modal overlays with centered layout (`centered_rect` + `Clear`).
- Editor-open requests raised through `pending_request` (e.g. `o` to open a Note row).
- A `queue_*_for(path)` trait hook (`queue_related_modal`) used by the App to forward a startup `InitialAction` to a specific tab.

## Goals / Non-Goals

**Goals:**
- A Journal tab the user can pick a note in (via the same fuzzy picker the graph tab uses) and scroll through its reverse-chronological feed.
- `Enter` on an entry opens the source note in `$EDITOR` at the paragraph's first line, using the existing editor-open request path.
- `R` reloads the current target; `c` clears back to the picker prompt.
- A graph-tab `J` keybinding that, when a Note row is selected, queues that note on the Journal tab and switches focus.
- Loaded entries persist across tab switches; no work is repeated unless the user asks.
- `?` help overlay surfaces every binding for the new tab.

**Non-Goals:**
- Background-worker loading (sync first; promote later if profiling shows lag).
- Filter or within-feed search (out of scope for v1).
- JSON export from the TUI (the CLI's `--json` already covers it).
- Configurable date formatting; `NaiveDate::Display` is fine.
- A "live" feed that updates when files change (graph rebuild only on `R`).

## Decisions

### D1: New top-level tab, not a modal on an existing tab

**Decision**: Add `JournalTab` as a peer of the four existing tabs.

**Rationale**: The journal is its own information space — a scrollable, mutable list of dated entries — not a one-shot action on a graph node. The Related-updater is a modal because it's a brief picker action; the journal is a destination the user lives in for minutes.

**Alternative considered**: A modal overlay on the graph tab (like the Related modal). Rejected — modals don't scroll comfortably and don't compose well with editor handoffs.

### D2: Reuse `FuzzyPicker<VaultFilePickerSource>` for note selection

**Decision**: The picker overlay used by the Journal tab is the same one the graph tab opens for move-target selection.

**Rationale**: One picker codepath, one set of bindings the user already knows. Recents come along automatically because the source pulls from `Arc<RecentsLog>` already.

**Alternative considered**: A heading-aware fuzzy search via `ft_core::search::fuzzy_find`. Rejected — the Journal tab targets whole notes, not headings; the file-only picker matches that semantics.

### D3: Synchronous `build_journal` on selection (no background worker)

**Decision**: When the picker yields a note, the tab calls `build_journal` inline, blocking the event loop briefly.

**Rationale**: Cold-cache cost is ~1s for a vault with a few dozen matching files; warm cache is sub-100ms. This matches the trade-off the Related modal already made for `score_related`. Promoting to a background worker means plumbing a new `BgEvent::JournalLoaded` variant, a `RefCell<Option<JobHandle>>` slot, and a progress indicator — not worth it for v1.

**Alternative considered**: Background worker following the git-sync pattern. Defer — easy to add later without changing public surface.

### D4: Persist `target_path` + `entries` across tab switches

**Decision**: The tab keeps its loaded state when the user switches away and back. `on_focus` does NOT re-run `build_journal` unless `queued_journal_for_path` has been set (cross-tab jump) or `R` is pressed.

**Rationale**: Matches user expectation from Tasks/Graph tabs (they don't re-scan on every focus). Re-running blame on every tab switch would be wasteful.

**Alternative considered**: Always-fresh on focus. Rejected — slow and surprising.

### D5: Cross-tab jump via existing `pending_request` mechanism

**Decision**: The graph tab's `J` keybinding raises a new `AppRequest::JournalForNote { path }` variant. The App services it by calling `JournalTab::queue_journal_for(&path)` and `switch_tab(journal_idx)`. The Journal tab consumes the queued path on its next `on_focus`.

**Rationale**: `pending_request` is the established channel for tab → App messages; adding a variant is additive and parallels existing requests (`OpenEditor`, `ToggleSync`).

**Alternative considered**: Direct tab-to-tab call through `App`. Rejected — would require mutable cross-references the trait dispatch can't express cleanly.

### D6: Per-tab BlameCache lifecycle

**Decision**: The Journal tab holds a `BlameCache` inside its state (lazy-initialized on first `build_journal` call). Save best-effort after each load.

**Rationale**: Sharing one cache across the CLI and the TUI session is desirable but cross-cutting (would need App-level ownership). For v1 the tab owning its cache is simpler and still benefits from the on-disk file: a fresh tab session does a cold load, populates from disk, and saves back.

**Alternative considered**: Cache lives on `App`. Defer until a second consumer needs it (e.g. a stats tab).

### D7: `Enter` opens the source note at the paragraph's first line

**Decision**: Pressing `Enter` on a selected entry raises an editor-open request with the paragraph's `line_start` as the jump target. Reuses the same editor invocation path the graph tab's `o` key uses.

**Rationale**: The most common follow-on action ("see this paragraph in context") deserves the most prominent key. `Enter`-to-act matches every other tab.

## Risks / Trade-offs

**[Risk] Cold first load on a large vault feels sluggish**
→ Mitigation: Synchronous path is fast enough for typical (<500 notes) vaults; users see picker close immediately, then a short blank state before render. If lag becomes a complaint, swap in a background worker — the data API doesn't change.

**[Risk] Stale entries after editing the source note from inside the tab**
→ Mitigation: `R` reloads. We don't auto-rebuild on focus because that would surprise users who alt-tab between the journal and another tool. Document this in the help overlay (`R: reload`).

**[Risk] `J` collides with an existing graph-tab binding**
→ Mitigation: `J` (uppercase, Shift+j) is not currently bound on the graph tab — lowercase `j` is "next row", which is unchanged. Add the `Shift+J` binding pattern explicitly so the keymap matches the help text.

**[Risk] BlameCache duplication between CLI and TUI**
→ Mitigation: Both write to the same `.ft/cache/blame.msgpack` file. On-disk format is shared (msgpack via `rmp-serde`); the last writer wins. Cache invalidation is keyed on HEAD hash, so concurrent writes converge.

## Migration Plan

No persistent state migration. The Journal tab appears at startup; existing keybindings on other tabs are unchanged. Users who never touch the new tab pay nothing (the tab does no work until focused).

## Open Questions

- None at design time. Background-worker promotion remains a documented follow-up.
