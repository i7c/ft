## Context

The synthesis ritual's scaffold step (`plan_synth_scaffold` / `apply_synth_scaffold`) creates or appends protected sections to a synth note. Today the append path reads the existing note only to *join* new sections after the old content — it never looks at what's already pinned. Two consequences: re-running scaffold with the same target duplicates sections, and there is no notion of "when did I last synth" to scope "only the new ones." The user wants (1) a catch-up loop that accretes only entries written since the last synth, and (2) a CLI "complete this note with all missing entries for a target" — a persisted-journal note that grows toward completeness.

The existing plumbing is almost sufficient: `build_journal` produces the candidate entries; `plan_synth_scaffold` constructs sections; `apply_synth_scaffold` writes atomically. What's missing is (a) a selection step that drops already-pinned entries and (b) a watermark that scopes "new since last synth." Both are pure functions over the note's existing callouts plus the candidate `JournalEntry` list, so they compose as a pre-filter before the unchanged planner.

A note's callouts each carry a `commit_sha` = HEAD-at-scaffold-time. All sections from one scaffold share a SHA; a later append pins a newer (descendant) SHA. So the **newest pinned SHA** is a faithful last-synth watermark, and the **set of `(source_path, body)` pairs** is a faithful "already captured" set. These two observations drive the whole design.

## Goals / Non-Goals

**Goals:**
- Make scaffold's append path idempotent across CLI and TUI (never duplicate an already-pinned paragraph).
- Provide a `--new-only` selection that scopes entries to those created/updated after the last synth, derived from the note itself (no external state).
- Let a synth note self-describe its journal target(s) so `grow` can run with no `--link` — the persisted-journal UX.
- Mirror both behaviors in the TUI Journal tab's send-to-synth flow.

**Non-Goals:**
- Changing the dedup key away from exact body match (reformats that preserve content but change whitespace/line-wrap are a separate concern; repair/reslice already handle drifted bodies).
- Auto-refreshing drifted pins (that is `ft synth repair` / `reslice`).
- Background-worker-ization of the watermark computation (it is one git call over a handful of SHAs).
- A TUI folder/target picker for the new-only variant beyond the existing note picker.
- Migrating existing hand-authored synth notes to carry `ft-synth-targets` (opt-in, written going forward).

## Decisions

### D1. Dedup key: `(source_path, body)` exact string match

**Choice.** An entry is "already pinned" iff some existing callout has the same `source_path` AND byte-identical `body`. The 6-hex `content_hash` is used only as a fast pre-filter (cheap integer-ish compare before the string compare); `commit_sha` is deliberately **not** part of the key.

**Why not `(path, line_range)`.** An existing section pins `(path, L10-12, sha_T1, body@T1)`; a fresh journal entry is `(path, L10-12, body_at_HEAD)` at a *different* commit coordinate. Line ranges shift across commits (insertions above), so `(path, line_range)` collides on the wrong paragraph or misses a shifted one. Body is the stable identity of *what was said*.

**Why `commit_sha` is excluded.** Same body at a newer commit means the paragraph's content is unchanged; there is no reason to re-pin it (refreshing a stale pin is `repair`/`reslice`, a different flow). Including SHA would re-capture unchanged paragraphs on every grow, defeating the dedup.

**Alternatives considered.**
- *content_hash only:* collision-resistant enough at 6 hex for a single note, but two genuinely different paragraphs could share a 6-hex prefix by astronomically bad luck; the body compare makes it exact. Negligible cost (bodies are small).
- *`(path, line_range, body)`:* line_range adds no discriminating power over body alone for the "is this the same paragraph" question, and it would falsely miss a paragraph whose body is identical but whose line range shifted. Dropping it is simpler and more correct.

### D2. Watermark = newest pinned commit SHA; scoped via commit date, not diff

**Choice.** `last_synth_watermark(repo_root, existing_callouts)` returns `Option<(Oid, NaiveDate)>`: the topological tip among the callouts' `commit_sha` values, plus that commit's committer date. Computed by `git rev-list --max-count=1 <sha1> <sha2> …` (the union's descendant = the newest reachable), then `git log -1 --format=%cI <tip>` for the date. `--new-only` keeps an entry iff `entry.date > watermark.date`.

**Why date filter over reusing `compute_link_review`'s added-lines filter.** `entry.date` (most-recent commit touching the paragraph) is exactly "when was this paragraph last created/updated," and the journal already computes it. Reusing `--range <watermark>..HEAD --in-window` would tie selection to wikilink-bearing *lines* (a paragraph updated without changing its link line would be missed) and pull in the link-review engine as a dependency for a question it doesn't answer. The date filter is simpler, more faithful to "created/updated since," and composes cleanly with body-dedup (a paragraph reformatted since last synth → new body → dedup keeps it; a paragraph touched but body-identical → dedup drops it — both correct).

**Why `git rev-list` over max-by-date.** Max-by-date picks the most recent *date*, but two SHAs could have equal dates across branches; `rev-list` returns the topological descendant, which is the meaningful "last commit that is an ancestor of or equal to the others." For a linear-history synth note all SHAs are descendants of the first, so this collapses to the last-appended SHA — but the descendant logic is robust to non-linear histories where the user merged/rebased between synths.

**Brand-new note (no callouts).** Watermark is `None` → `--new-only` cannot scope → degrades to "all missing" with a warning. This is the only sensible behavior (there is no "last time" on a note that has never been synhted) and avoids inventing a fallback date.

### D3. Unreachable SHAs: skip, don't fail

**Choice.** A pinned SHA may be unreachable in the local history (shallow clone, branch switch, rebase that dropped the commit). `last_synth_watermark` resolves each SHA via `git cat-file -e <sha>` first; unreachable SHAs are skipped. If *all* are unreachable, the watermark is `None` and `--new-only` degrades to "all missing" with a diagnostic naming the unreachable commits.

**Why not hard-fail.** The note's existing sections still verify (they carry their own SHA; verify fetches the blob independently and would already report `source-missing` if truly gone). Losing the watermark should not block the catch-up loop; it should just broaden the scope and tell the user why.

**7-hex ambiguity.** Callouts store short (7-hex) SHAs. In a large repo `git rev-list abc1234` could be ambiguous. Let git error and surface it via `Error::SynthWatermark` (the short SHA is what's on disk; widening the stored prefix is a separate format change, explicitly out of scope).

### D4. Dedup lives in the planner, watermark filter at the call site

**Choice.** `filter_missing(existing_callouts, entries) -> Vec<JournalEntry>` is a pure function in a new `ft-core/src/synth/accrete.rs`, and `plan_synth_scaffold`'s append branch calls it unconditionally before constructing sections. The watermark computation and date filter stay at the CLI/TUI call site (they need a git call against the note's SHAs, which is a read of the note + repo — appropriate for the caller, not the pure planner).

**Why push dedup into the planner.** It is an invariant ("append never duplicates") that should hold for every caller — CLI `scaffold`/`grow`, TUI `commit_send`, any future caller. Mirrors the project's "invariant always holds" style (cf. `dedup_entries` is caller-side today and only dedups within one run). The planner already reads the note for the append branch, so the cost is one `parse_callouts` it effectively already owes.

**Why keep the watermark at the call site.** The planner is pure (no git, no repo). The watermark needs `git rev-list`/`git log`, which is I/O. Keeping it at the call site preserves the plan/apply purity contract and makes the "new-only" intent explicit at the point of use.

### D5. Self-describing frontmatter: `ft-synth-targets`

**Choice.** A new optional YAML key `ft-synth-targets: ["[[Foo]]", "[[Bar]]"]` (a YAML sequence of `[[wikilink]]` strings). Scaffold/grow write it when `--link` is supplied and the note is being created (or appended and the key is absent). `grow` reads it when `--link`/`--from` are absent to source the journal. Verify/repair/reslice ignore it.

**Format details.** Parsed leniently (like `is_synth_note`): accept quoted or bare, `"[[Foo]]"` or `"Foo"`. Serialized canonically as a flow sequence `["[[Foo]]", "[[Bar]]"]` on its own frontmatter line. Stored as `Vec<String>` raw wikilink text (matching `--link`'s CLI shape), resolved to `NoteId`s at use time via the same `resolve_link_to_id` path.

**Why frontmatter over a sidecar.** A synth note already carries its provenance in-band (the callouts); the target belongs with it. A sidecar would be a second source of truth that can drift from the note. Frontmatter keeps the note self-contained and portable.

**Why opt-in / backward compatible.** Existing synth notes have no targets key and must keep working (scaffold append, verify, etc.). `grow` with an explicit `--link` overrides frontmatter targets; `grow` with neither `--link`/`--from` and no frontmatter key errors clearly ("no targets: pass --link or add ft-synth-targets frontmatter").

### D6. TUI: dedup default-on, new-only as a sibling command

**Choice.** `s` (send-to-existing) dedups by default (free, via the planner invariant). New command `journal.send-to-synth-new-only` bound to `n`: after picking the existing note, compute its watermark, filter `entries_to_send()` to `date > watermark`, then plan+apply. Both reuse `commit_send`.

**Flow reorder.** `s` today loads entries → picks note → scaffolds. For `n` the note must be picked *first* (to get the watermark), then entries filtered. So `n`'s path: open note picker → on pick, parse callouts + compute watermark → filter `entries_to_send()` → `commit_send`. This is a small reorder confined to a new branch in `handle_synth_send_key`/`on_existing_picked`; the existing `s` path is untouched.

**Why not a second picker prompt.** The new-only variant differs from `s` only in *which* entries it ships, not in *where* they go. Reusing the existing note picker avoids a second modal and keeps the `SynthSendState` machine from growing a parallel folder/title flow.

## Risks / Trade-offs

- **[Date-filter false negatives]** A paragraph updated *after* the last synth but whose blame date predates the watermark (rare: a `git rebase` that rewrites author dates, or a commit cherry-picked with an older date) would be missed by `--new-only`. → Mitigation: body-dedup runs *after* the date filter in the same pipeline, so a missed-by-date paragraph that is genuinely new (not pinned) is still captured *only if* it passes the date filter. This is a real gap; documented as a known limitation. The alternative (reusing added-lines) has worse false negatives (misses link-less updates). Accept the date-filter gap; `--since`/`--range` remain available for manual line-level scope.
- **[Case-2 volume]** "All missing" over a long-lived target can be hundreds of entries. → Mitigation: `--limit N` caps the count (newest-first, preserving journal order). Document that users wanting bounded catch-up should pair `--new-only` with `--limit`.
- **[Watermark across branch switches]** If the user synhted on branch A, switched to branch B, and runs `--new-only`, the watermark SHA from A may not be an ancestor of B's HEAD. `git rev-list <shaA>` still resolves (the commit exists), and `entry.date > watermark.date` still filters sensibly — it just compares against A's last-synth date, which may over- or under-capture relative to B's history. → Mitigation: acceptable; the watermark is "last time I synhted this note," not "last commit on this branch." Documented.
- **[Frontmatter key on hand-edited notes]** A user who hand-writes `ft-synth-targets` with a typo or a non-resolving link gets a clear error at `grow` time ("target `[[Fop]]` did not resolve"). → Mitigation: `grow`'s target resolution reuses `resolve_link_to_id`, which returns `None` on miss; the caller errors with the unresolved list.
- **[Dedup changes scaffold test expectations]** Existing scaffold tests append *different* content, so dedup is a no-op there. But any test that asserts "appending the same entry twice yields two callouts" would break. → Mitigation: audited existing tests (`scaffold.rs`, `verify.rs`) — none rely on duplicate appends. The dedup is safe to make unconditional.
- **[Frontmatter round-trip with `mark_note_as_synth`]** The existing `upsert_ft_synth_marker` and the new `ft-synth-targets` writer both edit frontmatter; they must compose (marker writer must not clobber targets, targets writer must not drop the marker). → Mitigation: a single `upsert_synth_frontmatter(content, targets: Option<&[String]>)` pure transform handles both keys idempotently; `mark_note_as_synth` becomes a thin wrapper.

## Migration Plan

No data migration. Existing synth notes keep working: no `ft-synth-targets` key means `grow` requires `--link` (same as today's scaffold); dedup-on-append is a behavior improvement that only affects re-runs (which previously duplicated). Users who want the persisted-journal UX for an existing note add the frontmatter key once (manually or by running `grow --link ...` on a freshly-created note). Rollback is trivial: revert the code; existing notes are unchanged on disk.

## Open Questions

- Should `ft synth verify` warn (not fail) when a note has `ft-synth-targets` but the journal for those targets has entries newer than the watermark — i.e. "this note is behind"? Out of scope for v1 (verify is about provenance, not completeness), but worth a follow-up `ft synth status` command. No decision needed now.
- Should `--limit` apply before or after dedup? Current design: dedup first (drop already-pinned), then `--limit` the remainder. This means `--limit 10` always yields ≤10 *new* sections. Confirm during implementation against the integration tests.
