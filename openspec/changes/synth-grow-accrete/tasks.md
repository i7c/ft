## 1. ft-core: frontmatter targets + upsert helper

- [ ] 1.1 Add `parse_synth_targets(content: &str) -> Option<Vec<String>>` to `ft-core/src/synth/callout.rs`: scan the YAML frontmatter for a `ft-synth-targets:` key, parse its value as a YAML flow sequence of strings (lenient: accept quoted `"[[Foo]]"` or bare `Foo`, strip surrounding `[[ ]]` normalization deferred to resolution time). Return `None` when the key is absent. Unit tests: quoted, bare, mixed, absent, non-sequence value ignored.
- [ ] 1.2 Add `upsert_synth_frontmatter(content: &str, targets: Option<&[String]>) -> String` to `ft-core/src/synth/callout.rs`: pure transform that idempotently ensures `ft-synth: true` is present and, when `targets` is `Some`, sets `ft-synth-targets: ["[[a]]", ...]` (flow sequence); preserves all other frontmatter keys. Refactor the existing `mark_note_as_synth`/`upsert_ft_synth_marker` in `ft/src/tui/tabs/journal.rs` to delegate to this core helper (move the pure transform to ft-core, keep the thin I/O wrapper in the TUI or move it too). Unit tests: no-frontmatter, frontmatter-without-targets, frontmatter-with-stale-targets-replaced, marker-preserved-when-adding-targets, unrelated-keys-preserved.
- [ ] 1.3 `cargo test -p ft-core` clean.

## 2. ft-core: accrete primitives

- [ ] 2.1 Create `ft-core/src/synth/accrete.rs` and declare it in `ft-core/src/synth/mod.rs`.
- [ ] 2.2 Implement `filter_missing(existing: &[ParsedCallout], entries: Vec<JournalEntry>) -> Vec<JournalEntry>`: drop entries whose `(source_path, body)` matches an existing callout. Use the 6-hex `content_hash` as a fast pre-filter into a `HashMap<&str, Vec<&str>>` (hash → bodies) then exact body compare. Preserve input order. Pure, no I/O.
- [ ] 2.3 Unit tests for `filter_missing`: unchanged-paragraph-dropped, updated-paragraph-kept, brand-new-kept, order-preserved, hash-collision-falls-back-to-body-compare (synthesize two distinct bodies with colliding 6-hex prefixes is impractical; instead assert exact body compare is the source of truth by testing two bodies sharing a prefix via short identical leading text).
- [ ] 2.4 Implement `last_synth_watermark(repo_root: &Path, existing: &[ParsedCallout]) -> Result<Option<(git2::Oid, NaiveDate)>>`: collect distinct short SHAs, resolve each via `git cat-file -e` (skip unreachable), run `git rev-list --max-count=1 <reachable...>` for the tip, then `git log -1 --format=%cI <tip>` for the committer date. Return `Ok(None)` when no callouts or all unreachable. Surface ambiguous short SHAs via `Error::SynthWatermark`.
- [ ] 2.5 Add `Error::SynthWatermark { sha: String, detail: String }` variant to the relevant `thiserror` enum in `ft-core/src/error.rs`.
- [ ] 2.6 Unit tests for `last_synth_watermark` using temp git repos: descendant-tip-among-two, brand-new-note-None, unreachable-skipped, all-unreachable-None, ambiguous-sha-err (construct by shortening to a prefix that matches two early commits — best-effort; skip if impractical in CI, cover via the unreachable path).
- [ ] 2.7 `cargo test -p ft-core` clean; `cargo clippy -p ft-core --tests -- -D warnings` clean.

## 3. ft-core: dedup-on-append in the planner

- [ ] 3.1 In `plan_synth_scaffold` (`ft-core/src/synth/scaffold.rs`), when `create == false` (append), read the existing note, `parse_callouts`, and run `accrete::filter_missing(parsed, entries)` before constructing `ProtectedSection`s. The create branch is unchanged. Add a doc note that append is now idempotent.
- [ ] 3.2 Add `dedup_skipped: usize` (or similar) to `SynthScaffoldPlan` so callers can report "N already pinned, skipped" — surface in CLI/TUI output. Update `PartialEq`/`Eq` derives accordingly (or impl them).
- [ ] 3.3 Update/extend scaffold tests: assert that appending an entry already present yields `dedup_skipped == 1` and zero new sections; assert re-running scaffold on an unchanged source is a no-op write. Confirm no existing test relies on duplicate appends (audit `scaffold.rs`, `verify.rs` tests).
- [ ] 3.4 `cargo test -p ft-core` clean.

## 4. ft binary: `ft synth grow` CLI

- [ ] 4.1 Add `Grow` variant + `GrowArgs` to `ft/src/cmd/synth.rs`: `target: PathBuf`, `link: Vec<String>`, `from: Vec<String>`, `new_only: bool`, `since: Option<String>`, `range: Option<String>`, `in_window: bool`, `limit: Option<usize>`, `no_edit: bool`. Add `Grow(_)` to `SynthCommand` and dispatch.
- [ ] 4.2 Implement `run_grow`: require target exists (else error pointing to `scaffold`); resolve targets from `--link`/`--from` or, when both absent, from `parse_synth_targets` on the note's frontmatter (error clearly if absent). Build journal via `build_journal` (reuse existing helpers). Apply `--since`/`--range`/`--in-window` filter (reuse `resolve_window`/`entry_overlaps_window`). When `--new-only`, compute `last_synth_watermark` and retain entries with `date > watermark.date`; on `None` print a warning and keep all. Dedup happens in the planner (step 3.1) — `dedup_skipped` is reported. Apply `--limit` (newest-first) after dedup. Ensure frontmatter targets are upserted when `--link` was supplied and the key is absent. `plan_synth_scaffold` (append) → `apply_synth_scaffold` → editor handoff (unless `--no-edit`).
- [ ] 4.3 Wire `Grow` into `SynthCommand` and the `Commands` dispatch in `ft/src/main.rs` if needed (it's a sub-subcommand of `synth`, so only `synth.rs` dispatch changes). Reuse `normalize_md_target`, `resolve_link_to_id`, `pick_paragraph`, `parse_from_spec`, `dedup_entries` (note: `dedup_entries` dedups within-run by `(path, line_start)`; the cross-note dedup is now the planner's job — keep both, document the layering).
- [ ] 4.4 Print output: `appended N section(s) to <rel> (M already pinned, skipped)` when `dedup_skipped > 0`, mirroring scaffold's wording.
- [ ] 4.5 Integration tests in `ft/tests/synth_cli.rs` (extend existing) or a new `ft/tests/synth_grow_cli.rs`: grow-appends-only-missing, grow-new-only-scopes-by-watermark (two-commit fixture), grow-new-only-brand-new-falls-back-with-warning, grow-reads-frontmatter-targets, grow-no-targets-errors, grow-nonexistent-target-errors, grow-limit-caps, grow-no-edit, scaffold-writes-targets-on-create, grow-appends-targets-when-absent. Use `assert_cmd` + `assert_fs`.
- [ ] 4.6 `cargo test -p ft` clean; `cargo clippy -p ft --tests -- -D warnings` clean.

## 5. TUI: dedup-on-append + new-only command

- [ ] 5.1 Add `CommandDef`s `journal.send-to-synth-new-only` to `JOURNAL_COMMANDS` in `ft/src/tui/tabs/journal.rs` (group "Synth", `opens_modal: true`). Bind `n` in `JOURNAL_KEYMAP`. Add a `journal.send-to-synth-new-only` arm to `dispatch_command`.
- [ ] 5.2 Extend `SynthSendState` with a new-only path: after `PickExisting` resolves a note in the new-only flow, parse its callouts, compute `last_synth_watermark`, and store `(path, watermark)` so `commit_send` can filter `entries_to_send()` to `date > watermark.date` before planning. Reuse `on_existing_picked` with a new-only flag, or add `on_existing_picked_new_only`. When watermark is `None`, post an info toast and proceed with all missing entries.
- [ ] 5.3 `commit_send` gains a `new_only: bool` param (or a pre-filtered entries vec): when set, filter `entries_to_send()` by the watermark date before `plan_synth_scaffold`. The dedup is handled by the planner (step 3.1) so no TUI-side dedup logic is needed — `commit_send` just sources the right entries.
- [ ] 5.4 Update `help_sections()` "Synth" group to list `n` → "send only entries newer than the picked note's last synth".
- [ ] 5.5 Regenerate `docs/keybindings.md`: `cargo run --release -q -- commands docs > docs/keybindings.md`, then `cargo run --release -q -- commands docs --check`.
- [ ] 5.6 TUI frame-assertion tests in `ft/src/tui/tests.rs`: (a) `s` on a note that already pins an entry appends only the missing one; (b) `n` picks a note and appends only newer-than-watermark entries; (c) `n` on a note with no callouts falls back with a toast. Use `TestBackend` + fixture vaults.
- [ ] 5.7 `cargo test -p ft` clean; `cargo clippy -p ft --tests -- -D warnings` clean.

## 6. Build invariants + docs

- [ ] 6.1 `cargo build --release` clean.
- [ ] 6.2 `cargo test --workspace` clean.
- [ ] 6.3 `cargo clippy --workspace --tests -- -D warnings` clean.
- [ ] 6.4 `cargo fmt --check` clean.
- [ ] 6.5 `cargo run --release -q -- commands docs --check` clean.
- [ ] 6.6 Update `docs/architecture.md` §"Synthesis ritual" to mention `grow`, the dedup-on-append invariant, the watermark primitive, and `ft-synth-targets`. Keep it brief — point to `openspec/changes/synth-grow-accrete/` for detail.
- [ ] 6.7 Run the openspec archive flow (`.pi/skills/openspec-archive-change`) once all tasks are complete and invariants are green.
