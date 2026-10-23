## Context

The synthesis entry point is `ft notes search` and the Search tab: a
single-pass index over the vault's paragraphs, a query grammar with
`--sort date`, and the shared send-to-synth flow. Pulse and Recent predate
it and answer a strictly weaker version of the same question:

- **Pulse** (`ft_core::pulse`) walks `git diff` between two refs, maps added
  lines back to HEAD paragraphs, and frequency-ranks the `[[links]]` found
  in them. Consumer count: one CLI command, one TUI tab.
- **Recent** (`ft_core::recent`) reuses Pulse's added-lines map to select
  paragraphs, scores them by `git blame`, and emits a feed. Consumer count:
  one CLI command, one TUI tab.

They form a closed island: `recent` is `pulse`'s only remaining consumer, so
deleting Recent makes Pulse dead, and deleting both strands a second tier of
support code whose only purpose was feeding those two tabs —
`synth::citations` (`CitationIndex`, built into every TUI graph snapshot),
`output::feed` (the CLI feed renderer), the pulse-only git diff helpers, and
`From<&RecentEntry> for SynthSource`.

Constraints:

- `ft.nvim` is a thin client over the CLI; it calls neither `pulse` nor
  `recent` (verified: no Lua references), so this is not an editor-protocol
  break.
- `ft-core/src/recents.rs` is a *different* feature (per-vault
  recently-*opened* log behind the pickers) and must survive untouched.
- The default tab lineup is a documented, keybound surface (`1`–`5`), so its
  renumbering is user-visible and needs the docs regenerated, not just code.
- `openspec/changes/feed-split-layout` is an active proposal that targets the
  Pulse, Recent, and (already removed) Gather tabs; it becomes unbuildable.

## Goals / Non-Goals

**Goals:**

- Delete both features end to end: engines, CLI subcommands, TUI tabs,
  commands, keybindings, capability specs, tests, docs.
- Delete every module that becomes consumer-less as a consequence, so the
  tree has no orphaned engine kept alive only by tests.
- Leave the surviving sweep path (`search → scaffold/verify/repair/reslice`)
  behaviourally identical.
- Keep the shared machinery both features used *and* search still needs:
  `widgets::{feed_split,feed_text}`, `synth_send`, `blame_cache`,
  `[synth] exclude_prefixes`, `AppRequest::SearchWithQuery`.

**Non-Goals:**

- No replacement "recent"/"pulse" surface, no `ft notes since` alias, no
  deprecation period or hidden subcommands.
- No change to `ft notes search`, the Search tab, blame-cache semantics, or
  the query DSL. Removing Pulse's `Enter` handoff does not remove
  `AppRequest::SearchWithQuery` — the Graph tab's `J` / `Ctrl+J` use it.
- No rework of `recents.rs`, the pickers, or editor handoff.
- Not fixing the stale Pulse/Recent mentions inside the already-implemented
  `hierarchical-ft-frontmatter` change (verified: its spec deltas do not
  touch any capability this change removes, so archiving stays clean).

## Decisions

**1. Delete, don't deprecate.** Gather was deprecated for a release before
removal; Pulse and Recent get removed outright. Rationale: the deprecation
flag is exactly the kind of compatibility shim the project conventions
forbid, and a hidden-but-live Pulse keeps `pulse.rs`, the diff helpers, and
the tab's command registry alive — the opposite of the goal. Cost: hard
break for scripts and `[keymap]` overrides, called out as BREAKING in the
changelog and release notes.

**2. Delete `CitationIndex` with the feed.** It is derivative data (rebuilt
per snapshot, never persisted) and its only two readers were the Recent
tab's badge line and `ft notes recent --json`'s `cited_in`. Keeping it
"for later" would mean shipping a 378-line module plus a per-rebuild build
cost that no surface exercises. Alternative rejected: leave it in place
in case a future surface wants it — reintroducing it is a self-contained
change (the spec text is recoverable from this archive).

**3. Renumber tabs; keep digits derived.** The lineup becomes
`1 Graph  2 Notes  3 Search`. Digits and cycling already derive from the
built tab list, so this is a docs/test-only consequence of dropping two
`Box<dyn Tab>` entries — no keymap changes beyond deleting `tab/pulse` and
`tab/recent`.

**4. Keep the shared widgets even though one caller disappears.**
`render_feed_split`, `inline_markdown_spans`, `wrap_line`,
`feed_text`'s Markdown span helpers, and `synth_send` all remain because the
Search tab uses them. Only the two citation-badge renderers and
`pad_to_width` (Recent-only) go. This is the one place where "delete
everything dead" and "keep the shared seam" pull in opposite directions;
the tie-breaker is *actual current callers*, not provenance.

**5. Delete `output::feed` rather than repoint search at it.** Search's CLI
renderer is its own module (`output::search`-shaped rows); `output::feed`'s
`FeedRow`/`CitedIn`/`render_table`/`render_json` were shaped for the Recent
feed alone. Repointing search at it would be a refactor with no user-visible
benefit inside a deletion change.

**6. Remove `feed-split-layout` from the active change list.** Its two
remaining subjects are the Pulse and Recent tabs, so its spec deltas
(`history-tui-tab`) can never be synced — the capability is deleted by this
same change. It is marked implemented (the split layout and Pulse
scroll-follow it describes are in the tree today), so deleting it drops an
unarchived record of that work; accepted because the code it changed is
deleted here too, and git history keeps the proposal.

**7. `ft.nvim` gets a docs-only fixup in its own commit.** `ARCHITECTURE.md`
lists `ft notes pulse --json` in its CLI-surface table. No Lua code refers to
it, so the fix is one table row, landed separately in the sibling repo to
keep the two repos' commits independent.

## Risks / Trade-offs

- **[Hard break for users with `[keymap]` overrides]** → `parse_scope`/
  validation errors point at the unknown command; docs/config.md's scope list
  and a CHANGELOG `### Removed` entry name the two dropped scopes explicitly.
- **[Silent behaviour regression from over-deleting]** — the shared widgets
  and `synth_send` are used by Search; deleting a "Recent-only" helper that
  is actually shared breaks the Search tab → guard: only delete symbols with
  zero non-Recent callers (verified per symbol in the task list), and the
  Search tab's own tests plus `ft/tests/search_cli.rs` stay green.
- **[Snapshot churn masks a real TUI regression]** — deleting two tabs
  invalidates several `insta` frames → deliberately re-accept the tab-strip
  and layout snapshots and eyeball the diff for anything beyond the tab bar
  and index renumbering.
- **[Test suite loses feed coverage that search does not replicate]** — e.g.
  synth-note exclusion from a feed, citation staleness → acceptable: those
  behaviours are removed with the feature, and their specs go with them.
- **[Stale cross-references linger]** — `rg -i "pulse|notes recent|Recent
  tab|citation"` must end with only historical docs (`docs/2026-*-review.md`,
  `openspec/changes/archive/`) matching → final sweep task.

## Migration Plan

1. Land the openspec change (this directory) as its own commit.
2. Apply code removal in the order: `ft-core` engines → CLI → TUI → dead
   helpers/shared-widget trim → tests → docs/keybindings regeneration.
3. Users: delete `tab/pulse` / `tab/recent` blocks from `[keymap]`; replace
   `ft notes pulse` with `ft notes search --sort date` and `ft notes recent`
   with `ft notes search --sort date <date predicate>`. Documented in the
   CHANGELOG entry.
4. Rollback: revert the apply commit (no data migration, no on-disk format
   change, no persisted state touched — the blame cache and `recents.jsonl`
   are unchanged).

## Open Questions

None outstanding. The scope decisions confirmed before drafting were: keep
`recents.rs`; renumber the tabs; sweep all debris (engines, dead helpers,
the four capability specs, plus the now-orphaned `citation-index`
capability and the moot `feed-split-layout` change).
