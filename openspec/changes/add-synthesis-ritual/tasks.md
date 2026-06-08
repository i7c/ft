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

- [ ] 4.1 Add `ft review` subcommand: new `ft/src/cmd/review.rs`, variant on `Commands` in `ft/src/main.rs`, dispatch. Args: `--since <duration>` OR `--range <X>..<Y>` (mutually exclusive via clap groups), `--json`. Calls `link_review::compute_link_review`.
- [ ] 4.2 Add review output renderer in `ft/src/output/review.rs`: default table `(count) [[target]]` with `?` for ghosts, sorted as Engine 2 produces; `--json` emits the documented schema.
- [ ] 4.3 Add `ft notes journal --link <link>` (repeatable, mutually exclusive with positional `<note>`), `--since`, `--range`, `--in-window` to existing `ft/src/cmd/notes.rs`. Dispatch through `build_journal` with a slice of resolved targets. Multi-target: skip Related-aliases. In-window: apply post-pass filter using Engine 2's added-lines map for the same window.
- [ ] 4.4 Update `ft/src/output/journal.rs` (or equivalent) to render the `matched: X, Y` indicator when `matched.len() > 1`, and to include `matched` in JSON.
- [ ] 4.5 Add `ft synth` subcommand with two sub-subcommands: `ft synth <target.md> ...` (scaffold; default action when first positional is a path) and `ft synth verify [<note.md> | --all] [--json]`. New `ft/src/cmd/synth.rs`. Args for scaffold: `--link` (repeatable), `--from <path>:<line>` (repeatable), `--since`/`--range`, `--all`/`--in-window`, `--no-edit`.
- [ ] 4.6 Add synth verify output renderer: default text `<status> | <header info>`; `--json` emits the documented schema. Exit code 0 if all `Ok`, else 1.
- [ ] 4.7 Integration tests in `ft/tests/`: `review_cli.rs`, `journal_multi_link_cli.rs`, `synth_scaffold_cli.rs`, `synth_verify_cli.rs`. Use `assert_cmd` + `assert_fs` against new fixture vault `tests/fixtures/synth/`.

## 5. TUI: Review tab

- [ ] 5.1 Create `ft/src/tui/tabs/review.rs` implementing the `Tab` trait. State: window range, computed rows, selection set, in-flight worker handle.
- [ ] 5.2 Implement background worker for Engine 2 following the existing single-threaded + mpsc pattern (reference: git-sync worker). Worker posts `BgEvent::LinkReviewComputed(Vec<LinkReviewRow>)` on completion. In-flight state in typed `RefCell<Option<...>>` slot on `App`.
- [ ] 5.3 Render: header showing `<start> .. <end>` window, body listing one row per link with selection visual and `?` suffix for ghosts.
- [ ] 5.4 Keys: `j`/`k` cursor, `<space>` toggle selection, `<enter>` handoff (queues `MultiTargetRequest { targets, window }` to App's new slot and switches focus to Journal tab), window-adjust keys (`<` / `>` or `[` / `]` — pick during impl, document in `help_sections`).
- [ ] 5.5 Register tab in `App::new` after the Journal tab. Override `help_sections()` so `?` overlay shows the keymap.
- [ ] 5.6 Snapshot tests via `ratatui::backend::TestBackend` in `ft/src/tui/tests.rs`: empty state, populated list, with selection, with ghost rows, header with window range.

## 6. TUI: Journal tab additions

- [ ] 6.1 Add `queued_targets: RefCell<Option<MultiTargetRequest>>` slot on `App`. Define `MultiTargetRequest { targets: Vec<NoteId>, window: Option<WindowRange> }` in `ft-core`.
- [ ] 6.2 In `on_focus`, consume `queued_targets` if set; otherwise fall through to existing `queued_journal_for_path` logic. Both-set precedence: multi-target wins; single-note slot cleared.
- [ ] 6.3 Update renderer to display `matched: X, Y` badge after the date line when an entry's `matched.len() > 1`. Use display titles (not raw `[[]]`).
- [ ] 6.4 Add in-window-only toggle key (`w`): toggles a tab-local flag; when active AND in multi-target mode AND `queued_targets.window.is_some()`, filter entries via Engine 2's added-lines map for that window. Header reflects current state.
- [ ] 6.5 Add entry multi-select via `<space>` with persistent selection across cursor movement.
- [ ] 6.6 Add send-to-synth key (`s`): opens an inline prompt with a fuzzy picker over existing `ft-synth: true` notes + a "new note" option. On confirm, call `plan_synth_scaffold` with selected entries (or all displayed if no selection), `apply_synth_scaffold`, trigger existing editor-handoff at the bottom of the target file.
- [ ] 6.7 Update `Tab::help_sections()` to include `space`, `s`, `w` bindings.
- [ ] 6.8 Snapshot tests: multi-target rendering, badge present/absent, in-window toggle on/off, send-to-synth prompt opened.

## 7. Fixtures, integration, polish

- [ ] 7.1 Add `tests/fixtures/synth/` vault: a few notes with various `[[wikilink]]` patterns, a synth note with valid + drifted + malformed sections, periodic-notes folder for exclude testing, a small git history with commits in known time windows.
- [ ] 7.2 Real-vault tests in `ft-core/tests/real_vault.rs` and `ft/tests/real_vault_cli.rs` gated on `FT_REAL_VAULT_TESTS=1` exercising `ft review`, multi-target `ft notes journal --link`, `ft synth verify --all`. Off by default.
- [ ] 7.3 Update `README.md` and `docs/architecture.md` with the new ritual flow (link-review → multi-source journal → synth notes), the callout grammar, and the new config keys.
- [ ] 7.4 Run all four build invariants and ensure clean: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`.
- [ ] 7.5 Manual smoke test in the real vault: full ritual end-to-end (open Review tab, select 2-3 links, hand off, multi-select entries, send to a new synth note, verify the resulting file in Obsidian preview, run `ft synth verify` on it).
