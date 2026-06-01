## 1. Paragraph Extraction (ft-core)

- [x] 1.1 Add `extract_paragraphs(content: &str) -> Vec<Paragraph>` to `ft-core/src/markdown.rs`, splitting on blank lines, heading lines, and `---`/`--` separators, skipping frontmatter and fenced code blocks via `LineSkipState`
- [x] 1.2 Write unit tests for `extract_paragraphs` covering: blank-line boundary, heading boundary, `--` boundary, frontmatter skip, fenced code skip, empty input, single paragraph, trailing blank lines
- [x] 1.3 Add `proptest` round-trip: `extract_paragraphs` on arbitrary content produces non-overlapping line ranges that cover all non-skipped lines

## 2. Graph: New Node and Edge Kinds

- [x] 2.1 Add `NodeKind::Paragraph(ParagraphData)` and `ParagraphData { source_file, line_start, line_end, text }` to `ft-core/src/graph/mod.rs`
- [x] 2.2 Add `EdgeKind::OwnsParagraph` and `EdgeKind::ParagraphLink` variants to `EdgeKind` in `ft-core/src/graph/mod.rs`; update all exhaustive match arms across the codebase (`cargo build` will surface them)
- [x] 2.3 Add `paragraph_index: HashMap<(PathBuf, u32), NoteId>` to the `Graph` struct; add `paragraph_by_loc(&self, path: &Path, line_start: u32) -> Option<NoteId>` lookup method

## 3. Graph: Build and Refresh

- [x] 3.1 In `Graph::build`, extend the parallel parse phase to also call `extract_paragraphs(&content)` alongside `extract_links`, collecting `(rel_path, paragraphs)` pairs
- [x] 3.2 In the serial phase of `Graph::build`, insert paragraph nodes via `insert_paragraph_nodes` and `OwnsParagraph` edges, populating `paragraph_index`
- [x] 3.3 In the serial phase of `Graph::build`, insert `ParagraphLink` edges from each paragraph node by resolving its extracted wiki links using existing `resolve_wiki` / `resolve_md` logic
- [x] 3.4 Add `remove_paragraph_nodes(src: NoteId)` to remove all `OwnsParagraph`-connected paragraph nodes and their `paragraph_index` entries when refreshing a note
- [x] 3.5 Call `remove_paragraph_nodes` and then re-insert paragraph nodes/edges in `Graph::refresh_note`, mirroring the existing edge removal and re-insertion pattern
- [x] 3.6 Write graph unit tests: note with two paragraphs yields correct `OwnsParagraph` count; wiki link in paragraph yields `ParagraphLink` to resolved note; `refresh_note` after adding a paragraph increases paragraph count; `paragraph_by_loc` returns correct id

## 4. Graph Query DSL Extension

- [x] 4.1 Add `kind:paragraph` node-kind filter to `graph::query` DSL, selecting only `NodeKind::Paragraph` nodes; ensure existing `kind:note`, `kind:directory`, `kind:task`, `kind:ghost` are also formally handled
- [x] 4.2 Add `owns-paragraph` and `paragraph-link` as valid edge-kind specifiers in DSL expansion steps
- [x] 4.3 Write DSL parse and evaluation tests: `kind:paragraph` selects paragraphs; `owns-paragraph` expansion from a note yields its paragraph nodes; `paragraph-link` expansion from a paragraph yields target notes

## 5. Git Blame and BlameCache (ft-core)

- [x] 5.1 Add `LineBlame { line: u32, commit_hash: String, timestamp: i64 }` and `blame_file(repo: &Path, rel_path: &Path) -> Result<Vec<LineBlame>>` to `ft-core/src/git.rs`, shelling out to `git blame --porcelain`
- [x] 5.2 Write unit tests for `blame_file` using a temp git repo fixture: single-commit file, two-commit file, file not tracked returns error
- [x] 5.3 Create `ft-core/src/blame_cache.rs`: `BlameCache` struct with `load(vault_root: &Path) -> Result<BlameCache>`, `save(&self, vault_root: &Path) -> Result<()>`, `get(&self, path: &str, head: &str) -> Option<&Vec<LineBlame>>`, `insert(&mut self, path: String, head: String, entries: Vec<LineBlame>)`, backed by `rmp-serde` serialization to `.ft/cache/blame.msgpack`
- [x] 5.4 Add `rmp-serde` to `ft-core/Cargo.toml` dependencies
- [x] 5.5 Write `BlameCache` unit tests: round-trip through msgpack, missing file loads empty cache, stale HEAD returns None, fresh HEAD returns cached data
- [x] 5.6 Add helper `paragraph_date(blame: &[LineBlame], line_start: u32, line_end: u32) -> Option<NaiveDate>` that returns `max(timestamp)` in range converted to UTC `NaiveDate`

## 6. Journal Module (ft-core)

- [x] 6.1 Create `ft-core/src/journal.rs` with `JournalEntry { source_title: String, source_path: PathBuf, section_text: String, date: NaiveDate }` and `build_journal(graph: &Graph, note_id: NoteId, vault: &Vault, repo: &Path, cache: &mut BlameCache) -> Result<Vec<JournalEntry>>`
- [x] 6.2 Implement alias resolution in `build_journal`: load note N's content, find `## Related` heading line range via `extract_headings`, filter `graph.outgoing(note_id, EdgeKind::Link)` by line range, collect alias `NoteId`s
- [x] 6.3 Implement graph traversal in `build_journal`: for N and each alias, collect all `incoming(id)` edges of kind `ParagraphLink`; deduplicate paragraph nodes; exclude paragraphs whose `source_file` is N's own path
- [x] 6.4 Implement date lookup in `build_journal`: for each matched paragraph's `source_file`, call `blame_file` (or use cache), derive date via `paragraph_date`; sort entries by date descending, then source title ascending
- [x] 6.5 Write unit tests for `build_journal` against a fixture vault with git history: journal for a note includes expected paragraphs; N's own file is excluded; Related aliases are included; reverse-chronological ordering is correct
- [x] 6.6 Export `journal` module from `ft-core/src/lib.rs`

## 7. Related Module (ft-core)

- [x] 7.1 Create `ft-core/src/related.rs` with `RelatedScore { note_id: NoteId, title: String, score: u32, already_in_related: bool }` and `score_related(graph: &Graph, note_id: NoteId) -> Vec<RelatedScore>`
- [x] 7.2 Implement same-paragraph scoring (+3): traverse all paragraph nodes reachable via `incoming(note_id, ParagraphLink)`, collect all other `ParagraphLink` targets from those paragraphs, add 3 per paragraph per co-occurring concept
- [x] 7.3 Implement same-file cross-paragraph scoring (+1): group matched paragraphs by `source_file`; for each file, collect all `ParagraphLink` targets from non-matching paragraphs in the same file; add 1 per file per co-occurring concept not already scored at +3 from a same-paragraph hit in that file
- [x] 7.4 Populate `already_in_related` using the same alias resolution logic as `build_journal`; exclude N and all aliases from results; omit zero-score concepts
- [x] 7.5 Implement `plan_related_update(content: &str, new_concepts: &[String]) -> RelatedUpdatePlan` and `apply_related_update(plan: &RelatedUpdatePlan, path: &Path) -> Result<()>` using `write_atomic`; handle both existing Related section (append) and missing (create at end of file)
- [x] 7.6 Write unit tests for `score_related`: same-paragraph co-occurrence scores 3; same-file cross-paragraph scores 1; N excluded; already_in_related flag correct; zero-score omitted
- [x] 7.7 Write unit tests for `plan_related_update` / `apply_related_update`: append to existing section; create section when absent; empty selection is no-op
- [x] 7.8 Export `related` module from `ft-core/src/lib.rs`

## 8. CLI: ft notes journal

- [x] 8.1 Add `NotesCommand::Journal(JournalArgs)` to `ft/src/cmd/notes.rs`; `JournalArgs` has `note: String` and `--json` flag
- [x] 8.2 Implement `run_journal`: resolve note via fuzzy search, build graph, load/save `BlameCache`, call `build_journal`, print results
- [x] 8.3 Implement table output renderer: date line (`YYYY-MM-DD  <Title>`), separator, paragraph text, blank line between entries; respect `--no-color` / `NO_COLOR` / non-TTY
- [x] 8.4 Implement `--json` output: emit JSON array with `date`, `source_title`, `source_path`, `section` fields per entry
- [x] 8.5 Wire `NotesCommand::Journal` into the `run` dispatch in `ft/src/cmd/notes.rs` and update `ft/src/main.rs` if needed
- [x] 8.6 Write integration tests using `assert_cmd` + a temp vault fixture with a small git history: journal for a note returns expected entries in correct order; `--json` produces valid JSON; unknown note exits non-zero; N's own file excluded

## 9. CLI: ft notes update-related

- [x] 9.1 Add `NotesCommand::UpdateRelated(UpdateRelatedArgs)` to `ft/src/cmd/notes.rs`; `UpdateRelatedArgs` has `note: String`
- [x] 9.2 Implement `run_update_related`: resolve note, build graph, call `score_related`, launch TUI with graph tab and Related updater modal pre-populated; exit with error if non-TTY
- [x] 9.3 Wire `NotesCommand::UpdateRelated` into dispatch

## 10. TUI: Related Updater Modal on Graph Tab

- [x] 10.1 Define `RelatedModal` state struct on the graph tab: holds `Vec<RelatedScore>`, a `HashSet<NoteId>` of checked entries, scroll offset, and the target `NoteId`
- [x] 10.2 Add `R` keybinding on the graph tab when a `NodeKind::Note` node is selected: triggers `score_related` (via background worker following the git-sync pattern), stores result in a `RefCell<Option<RelatedModal>>` slot on the graph tab
- [x] 10.3 Implement modal rendering as a centered overlay: header with note title, scrollable list with already-in-related entries (marked, non-interactive) at top, then candidates sorted by score descending with checkboxes
- [x] 10.4 Implement modal input handling: Space toggles checkbox on candidate entries, Enter confirms and calls `apply_related_update` for checked entries then closes modal, Escape/`q` cancels without writing
- [x] 10.5 Add `RelatedModal` keybindings (`R` open, `Space` toggle, `Enter` confirm, `Esc` cancel) to the graph tab's `help_sections()` return value
- [x] 10.6 Write `TestBackend` snapshot tests: graph tab with Related modal open (entries listed); help overlay on graph tab shows `R` binding; modal confirm path applies changes and closes

## 11. Build Invariants and Cleanup

- [x] 11.1 Run `cargo build --release` and fix any exhaustiveness warnings from new `NodeKind` / `EdgeKind` variants in existing match arms
- [x] 11.2 Run `cargo test --workspace` and fix any failing tests
- [x] 11.3 Run `cargo clippy --workspace --tests -- -D warnings` and fix all warnings
- [x] 11.4 Run `cargo fmt --check` and apply `cargo fmt` if needed
- [ ] 11.5 Verify real-vault tests still pass: `FT_REAL_VAULT_TESTS=1 cargo test --workspace` (gated, run manually)
