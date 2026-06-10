## 1. Foundations: config, dependencies, journal signature

- [x] 1.1 Add `blake3` to `ft-core/Cargo.toml`.
- [x] 1.2 Add `synth: Synth` sub-struct to `Config` in `ft-core/src/config.rs` with `folder: String` (default `"Synthesis/"`) and `exclude_prefixes: Vec<String>` (default derived from periodic-notes folder). `#[serde(deny_unknown_fields)]` on `Synth`. Unit test for default + override + unknown-key rejection.
- [x] 1.3 Generalize `ft_core::journal::build_journal` signature from `note_id: NoteId` to `targets: &[NoteId]`. Add `matched: Vec<NoteId>` field to `JournalEntry`. Preserve single-target semantics when `targets.len() == 1` (Related-aliases resolution + self-exclusion). Skip both in multi-target mode.
- [x] 1.4 Sweep all callers of `build_journal` and convert to `&[id]`. Known callers: `ft/src/cmd/notes.rs` (CLI), `ft/src/tui/tabs/journal.rs` (TUI), `ft-core/src/journal.rs` tests. `cargo build --release` clean.
- [x] 1.5 Update existing journal tests to assert `matched` field populated correctly in single-target case (one-element vec).

## 2. Link-review engine (Engine 2)

- [x] 2.1 Create `ft-core/src/link_review.rs` with `WindowRange` enum (`Since(Duration)` | `Range(String, String)`) and `LinkReviewRow { count: usize, target: String, is_ghost: bool, source_paths: Vec<PathBuf> }`.
- [x] 2.2 Implement `compute_link_review(graph, vault, repo, window, cfg) -> Result<Vec<LinkReviewRow>>`: invokes `git log -p` over the window, parses unified diff, extracts `[[wikilinks]]` from added lines using existing markdown parser (skip fenced code blocks via `LineSkipState`).
- [x] 2.3 Implement HEAD-relative paragraph mapping: for each `(commit, path, added_line, link)` tuple, look up the containing paragraph in the HEAD paragraph index via `graph.paragraph_by_loc` or fresh `extract_paragraphs` over the HEAD file; fall back to synthetic `(path, added_line)` key when no current paragraph contains the line.
- [x] 2.4 Implement two-layer exclusion: (a) skip any added-line whose post-commit path starts with any prefix in `cfg.synth.exclude_prefixes`; (b) skip any `[[wikilink]]` whose position in the post-commit file falls inside a `> [!ft-source]` callout in a note with `ft-synth: true` frontmatter.
- [x] 2.5 Dedup `(link, paragraph_key)` pairs; sort rows by `count` desc, `target` asc; tag `is_ghost` via graph lookup.
- [x] 2.6 Resolve dates: `--since 7d` → compute date threshold via `FT_TODAY`-respecting clock and find the commit at-or-before; `--range X..Y` → use git refs verbatim.
- [x] 2.7 Unit tests covering: same link twice in one paragraph counts once; same link in two paragraphs of one note counts twice; removed-only link does not count; fenced-code-block link ignored; excluded-prefix dropped; synth-callout link skipped; synth-prose link counted; ghost marking; sort + tiebreak; empty window.

## 3. Synth-notes core: callout grammar, plan/apply, verify

- [x] 3.1 Create `ft-core/src/synth/mod.rs`, `ft-core/src/synth/callout.rs`, `ft-core/src/synth/scaffold.rs`, `ft-core/src/synth/verify.rs`.
- [x] 3.2 Implement callout grammar in `callout.rs`: `serialize(section: &ProtectedSection) -> String` and `parse(text: &str) -> Vec<ParsedCallout>` (extracts every `> [!ft-source] ...` block from a markdown body). Use a regex for the header line as specified in design D4. `ParsedCallout` carries the header tokens (path, line_start, line_end, sha7, hash6), the body text (with `> ` stripped), and the byte range in the source for diagnostics.
- [x] 3.3 Implement `compute_section_hash(text: &str) -> String` using blake3, returning 6 hex chars.
- [x] 3.4 Proptest round-trip: `parse(serialize(s)) == s` for arbitrary `ProtectedSection` values (path with subdirs, multi-line body, etc.).
- [x] 3.5 Implement `plan_synth_scaffold(graph, vault, repo, target_rel_path, entries: &[JournalEntry]) -> Result<SynthScaffoldPlan>`: pure function, no I/O writes. Distinguishes create vs append. When creating: include `ft-synth: true` frontmatter. For each entry, build a `ProtectedSection` (look up commit hash for the source paragraph via blame at its lines — first commit is fine; line range = paragraph line_start..=line_end; hash = blake3 of paragraph text).
- [x] 3.6 Implement `apply_synth_scaffold(vault, plan) -> Result<PathBuf>`: writes via `ft_core::fs::write_atomic`; returns the absolute path that should be opened (for editor handoff). When extending, append `\n\n` + sections to existing content.
- [x] 3.7 Implement `verify_synth_note(repo, vault, note_path) -> Vec<VerificationResult>`: parse every callout, fetch git blob at `(sha, path)`, slice lines, strip `> ` prefix from body, compare for byte-equality, also re-compute blake3 to check hash6. Return per-section results with status `Ok`/`Drifted`/`SourceMissing`/`Malformed` and diagnostic.
- [x] 3.8 Implement `verify_all(vault) -> Vec<(PathBuf, Vec<VerificationResult>)>`: walk every `.md` file, identify those with `ft-synth: true` frontmatter, verify each.
- [x] 3.9 Unit tests: scaffold create + append; verify ok; verify drifted (body edited); verify source-missing (path renamed or commit unreachable); verify malformed (header missing token); plan does no I/O (assert via temp-dir snapshot before+after); apply uses write_atomic (no temp files left behind).

## 4. CLI surfaces

- [x] 4.1 Add `ft review` subcommand: new `ft/src/cmd/review.rs`, variant on `Commands` in `ft/src/main.rs`, dispatch. Args: `--since <duration>` OR `--range <X>..<Y>` (mutually exclusive via clap groups), `--json`. Calls `link_review::compute_link_review`.
- [x] 4.2 Add review output renderer in `ft/src/output/review.rs`: default table `(count) [[target]]` with `?` for ghosts, sorted as Engine 2 produces; `--json` emits the documented schema. (Inlined in `ft/src/cmd/review.rs` — single small command, no need for a separate output module.)
- [x] 4.3 Add `ft notes journal --link <link>` (repeatable, mutually exclusive with positional `<note>`), `--since`, `--range`, `--in-window` to existing `ft/src/cmd/notes.rs`. Dispatch through `build_journal` with a slice of resolved targets. Multi-target: skip Related-aliases. In-window: apply post-pass filter using Engine 2's added-lines map for the same window.
- [x] 4.4 Update `ft/src/output/journal.rs` (or equivalent) to render the `matched: X, Y` indicator when `matched.len() > 1`, and to include `matched` in JSON.
- [x] 4.5 Add `ft synth` subcommand with two sub-subcommands: `ft synth <target.md> ...` (scaffold; default action when first positional is a path) and `ft synth verify [<note.md> | --all] [--json]`. New `ft/src/cmd/synth.rs`. Args for scaffold: `--link` (repeatable), `--from <path>:<line>` (repeatable), `--since`/`--range`, `--all`/`--in-window`, `--no-edit`. (Scaffold is an explicit `scaffold` subcommand rather than default — clap doesn't support default-subcommand cleanly and the explicit form matches `ft notes` style.)
- [x] 4.6 Add synth verify output renderer: default text `<status> | <header info>`; `--json` emits the documented schema. Exit code 0 if all `Ok`, else 1.
- [x] 4.7 Integration tests in `ft/tests/`: `review_cli.rs`, `journal_multi_link_cli.rs`, `synth_cli.rs` (combines scaffold + verify since they share fixtures). Use `assert_cmd` + `assert_fs`. 17 tests total, all passing.

## 5. TUI: Review tab

- [x] 5.1 Create `ft/src/tui/tabs/review.rs` implementing the `Tab` trait. State: window range, computed rows, selection set, in-flight worker handle. (Worker handle deferred — v1 uses synchronous compute on focus, matches the existing Journal tab's sync `build_journal` pattern.)
- [~] 5.2 Background worker for Engine 2 — **deferred**: v1 computes synchronously on focus / window-change (same pattern as existing Journal tab). The "UI remains responsive during load" spec scenario is not met; documented in `review.rs` module docs. Track for v2 if performance becomes an issue on large vaults.
- [x] 5.3 Render: header showing `<window-label> (<count> links, <selected> selected)`, body listing one row per link with `[*]` select marker and `?` suffix for ghosts.
- [x] 5.4 Keys: `j`/`k` cursor, `<space>` toggle selection, `<enter>` handoff (queues `MultiTargetRequest { targets, window }` to App and switches focus to Journal tab via `AppRequest::JournalForMulti`), `[` / `]` window-adjust (halve / double), `R` reload.
- [x] 5.5 Register tab in `App::new` (and `for_test*` constructors) after the Journal tab. Override `help_sections()` so `?` overlay shows the keymap.
- [x] 5.6 Snapshot/behavior tests in `ft/src/tui/tests.rs`: empty window friendly message; populated list with counts + ghost `?` suffix; help-section content. (Used assertion-on-frame style rather than insta snapshots since the new tab already required 54 existing snapshots to be rolled for the tab-strip addition.)

## 6. TUI: Journal tab additions

- [x] 6.1 Defined `MultiTargetRequest { targets: Vec<JournalTarget>, window: Option<JournalWindow> }` in `ft/src/tui/tab.rs` (not `ft-core` — it carries the TUI-side `JournalTarget` enum). Added `AppRequest::JournalForMulti { request }` plus `Tab::queue_journal_for_multi` hook with default no-op.
- [x] 6.2 In `on_focus`, consume `queued_multi` first; if set, the single-note `queued_for` slot is cleared without execution (matches spec precedence rule).
- [x] 6.3 Renderer shows `matched: X, Y` badge after the date line when `entry.matched.len() > 1`. Display titles resolved at load time into `entry_matched_titles` (parallel vec) so the render path doesn't need graph access.
- [x] 6.4 `w` key toggles `in_window_only`. Filter re-applies on toggle. Title reflects current state (`all-time` vs `in-window`). Toggle is no-op outside multi-target+window context.
- [x] 6.5 Entry multi-select via `<space>`. Selection visualized with `[*]` marker; persists across cursor movement; cleared on every load.
- [x] 6.6 Send-to-synth key (`s`): opens an inline typed-path prompt (rather than the full fuzzy picker — v1 simplification; documented as v2 polish). On Enter: resolves bare names under `synth.folder`, runs `plan_synth_scaffold` + `apply_synth_scaffold`, queues editor handoff, posts a success toast.
- [x] 6.7 Updated `Tab::help_sections()` with a new "Synth" section listing `Space`, `w`, `s` bindings.
- [x] 6.8 Behavior tests in `ft/src/tui/tests.rs`: multi-target rendering with `matched: Foo, Bar` badge + `2 targets` title; send-to-synth prompt opens on `s`. (Frame-assertion style; same rationale as 5.6.)

## 7. Fixtures, integration, polish

- [~] 7.1 Static `tests/fixtures/synth/` vault — **deferred**: a static fixture can't carry git history (commits + dates), which the link-review and verify flows need. Instead, fixture-builder helpers (`make_*_vault` in each integration-test file) build tempdir-backed vaults with in-process `git init` + dated commits. Matches the existing `notes_journal.rs` fixture pattern.
- [x] 7.2 Real-vault tests: added `real_vault_link_review_runs` + `real_vault_synth_verify_all_runs` to `ft-core/tests/real_vault.rs`; added `real_vault_review_since_7d_runs`, `real_vault_review_json_is_valid_json`, `real_vault_synth_verify_all_runs` to `ft/tests/real_vault_cli.rs`. All gated on `FT_REAL_VAULT_TESTS=1`. (multi-target `ft notes journal --link` skipped from real-vault smoke because it requires a known-existing link in the vault — would couple test data to vault contents.)
- [x] 7.3 Updated `docs/architecture.md` with a new "Synthesis ritual (`link_review` + `synth`)" section covering the three-layer pipeline, the callout grammar, the `ft-synth: true` marker, the TUI handoff plumbing, and the new config table. Linked from `README.md`'s reference docs list.
- [x] 7.4 All four invariants green: `cargo build --release` ✓, `cargo test --workspace` (every test passes), `cargo clippy --workspace --tests -- -D warnings` ✓, `cargo fmt --check` ✓. Also pinned `FT_TODAY=2026-05-10` in `capture_var_prompt_snapshot` to stop the daily snapshot rot that bit P5/P6 commits.
- [~] 7.5 Manual smoke test in the real vault — **out of scope for the agent**: this is a user-driven step. The CLI surface is reachable now (`ft review --since 7d`, `ft notes journal --link "[[X]]" --link "[[Y]]"`, `ft synth scaffold Synthesis/topic.md --link "[[X]]" --no-edit`, `ft synth verify --all`), and the TUI Review→Journal→send-to-synth flow is wired up. Run it whenever convenient.
