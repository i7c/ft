## 1. Unified `SynthSource` input type (ft-core)

- [x] 1.1 Create `ft-core/src/synth/source.rs` defining `SynthSource { source_path, line_start, line_end, body }` (the honest 4-field input). Add `impl From<&GatherEntry> for SynthSource` and `impl From<&RecentEntry> for SynthSource` (copy the four pinning fields, drop feed-specific ones). Re-export from `ft-core/src/synth/mod.rs`.
- [x] 1.2 Migrate `plan_synth_scaffold` signature in `ft-core/src/synth/scaffold.rs` from `&[GatherEntry]` to `&[SynthSource]`; update its body to read `source.source_path` / `.line_start` / `.line_end` / `.body`. Keep the dirty-source refusal, HEAD pinning, dedup-on-append, and create-vs-append behavior identical.
- [x] 1.3 Migrate `accrete::filter_missing` in `ft-core/src/synth/accrete.rs` from `Vec<GatherEntry>` to `Vec<SynthSource>`; keep the `(source_path, body)` dedup key and `commit_sha` exclusion unchanged.
- [x] 1.4 Update the scaffold/accrete unit tests (`scaffold.rs`, `accrete.rs`) to construct `SynthSource` directly (or via `(&entry).into()` where a feed entry fixture exists). Confirm the existing dirty-source / untracked-source / idempotent-replan / append-dedup tests still pass with their assertions unchanged.
- [x] 1.5 Update `ft-core/src/synth/{verify,repair,reslice,citations}.rs` test fixtures that build `GatherEntry` to feed the planner to build `SynthSource` instead (these modules' production code keys on callout fields and is unchanged, but their tests construct planner inputs).

## 2. Lower feed callers to `SynthSource` (ft-core + ft binary)

- [x] 2.1 In `ft-core/src/gather.rs`/`recent.rs` (or their TUI commit paths in `ft/src/tui/tabs/{gather,recent}.rs`), lower feed entries to `SynthSource` at the `plan_synth_scaffold` call boundary via `.into_iter().map(Into::into).collect()`. Keep `GatherEntry`/`RecentEntry` carrying `date`/`matched`.
- [x] 2.2 Update `ft/src/cmd/synth.rs`: keep `pick_paragraph` returning `GatherEntry` (the CLI orders by blame `date`, so the date is genuinely used, not fabricated), and lower the whole `Vec<GatherEntry>` to `Vec<SynthSource>` via `.iter().map(Into::into).collect()` at both `plan_synth_scaffold` call sites (scaffold + grow).
- [x] 2.3 Update `ft/src/cmd/notes.rs` gather-renderer helpers that reference `&[GatherEntry]` for display only (unchanged shape) to confirm they still compile; the display path keeps `GatherEntry`, only the scaffold handoff changes.
- [x] 2.4 Run `cargo test --workspace` and fix any remaining `GatherEntry`-to-planner call sites surfaced by the compiler.

## 3. Paragraph-synth flow module (ft TUI)

- [x] 3.1 Create `ft/src/tui/notes_actions/paragraph_synth.rs`. Define `ParagraphSynthState` (a step enum: `SourcePicking` (Notes-tab entry), `ParagraphMultiSelect`, `TargetPicking`, `Committing`/done) and a `SynthStep` outcome enum (`Stay`/`Transition`/`Finished`/`NotHandled`) mirroring `section_move.rs`'s `MoveStep`.
- [x] 3.2 Implement the `ParagraphMultiSelect` step: read source content, `extract_paragraphs`, hold `selected: BTreeSet<usize>`, `focus: usize`, and a per-pick `adjust: BTreeMap<usize, Adjust { top_trim, bot_trim }>`. Key handler: `j/k` move, `Space` toggle, `[`/`]`/`r` shrink/reset on the focused pick, `Enter` → target pick (error toast if none selected), `Esc` → previous step.
- [x] 3.3 Implement `begin_for_source(ctx, source_rel) -> Option<ParagraphSynthState>` (the clean-tree-guarded, tree-seeded entry that skips the source picker — used by the Graph tab), mirroring `section_move::begin_for_source` but reading paragraphs and guarding on `git::status(repo.root()).is_clean()`.
- [x] 3.4 Implement the `SourcePicking` step (Notes-tab entry): a `FuzzyPicker<VaultFilePickerSource>`; on select, run the same clean-tree guard then transition to `ParagraphMultiSelect` seeded to the picked note.
- [x] 3.5 Implement the `TargetPicking` step: reuse gather's split — `s` opens an existing-note `FuzzyPicker<VaultFilePickerSource>`, `S` opens the create-new (folder → title → template + vars) sub-flow. Reject source-equals-target inline (footer error). Carry multi-select + adjust state so `Esc` returns to `ParagraphMultiSelect` intact.
- [x] 3.6 Implement `commit`: build a `SynthSource` per selected paragraph using its *effective* range (`line_start + top_trim ..= line_end - bot_trim`, floor 1) and re-sliced body from the source content; call `plan_synth_scaffold` + `apply_synth_scaffold`; `mark_note_as_synth` on append to non-synth; `$EDITOR` handoff; toast dedup count. On `SynthDirtySources`, toast + return to `ParagraphMultiSelect` (state preserved).

## 4. Modal driver + command/keymap wiring (ft TUI)

- [x] 4.1 Add `ActiveModal::ParagraphSynth(ParagraphSynthState)` to `ft/src/tui/modal.rs`; delegate `handle_event`/`render`/`keymap_help`/`name` (pick a stable `name` string, e.g. `"paragraph-synth"`).
- [x] 4.2 Register the modal's commands + keymap in the Command/Keymap registry (a `PARAGRAPH_SYNTH_COMMANDS` static slice + `PARAGRAPH_SYNTH_KEYMAP`), covering the multi-select keys (`j/k/Space/Enter/Esc/[ /]/r`) and the target-pick keys (`s/S/Ctrl+N/Esc`).
- [x] 4.3 Graph tab: add `graph.synth-from-note` `CommandDef` + bind `y` in `ft/src/tui/tabs/graph/commands.rs`/`keymap`; dispatch arm posts `OpenModal(ActiveModal::ParagraphSynth(begin_for_source(ctx, node_path)))` after the clean-tree guard. No-op on non-Note nodes.
- [x] 4.4 Notes tab: add `notes.synth-from-note` `CommandDef` + bind `y` in `ft/src/tui/tabs/notes/mod.rs`; dispatch arm opens `ActiveModal::ParagraphSynth(SourcePicking { picker })` after the clean-tree guard.
- [x] 4.5 Override `help_sections()` on both tabs so the `?` overlay lists the new `y` binding; add the modal's `keymap_help()` section.
- [x] 4.6 Regenerate the committed reference: `cargo run --release -q -- commands docs > docs/keybindings.md`; run `cargo run --release -q -- commands docs --check` to confirm sync.

## 5. Range-adjust preview rendering (ft TUI)

- [x] 5.1 Render the `ParagraphMultiSelect` step via `render_feed_split`: top list rows = `{marker} {line-range} {short preview}` per paragraph; bottom preview = the focused paragraph's context.
- [x] 5.2 Add the effective-range highlight in the preview body: lines within `(line_start + top_trim) ..= (line_end - bot_trim)` highlighted, lines trimmed away dimmed. Preview header shows `L<orig> (adj: L<effective>)` (omit the adj clause when unadjusted). Decide the helper shape (a `render_feed_split` variant or a body-line styler) per the design open question; the spec requires the effective range be visible.
- [x] 5.3 Ensure the list/preview handle long paragraphs and small terminals (scrollbar + cursor-follow come from `render_feed_split`/`render_scroll_list`; verify no panic on 1-paragraph or 0-paragraph-after-filter notes).

## 6. Tests

- [x] 6.1 Add a `TestBackend` snapshot under `ft/src/tui/tests/` covering the modal's steps: entry → paragraph list → a shrink (`[`) → toggle → target pick (existing) → commit. Use `insta` and `FT_TODAY` for determinism.
- [x] 6.2 Add a unit test for the effective-range computation + body re-slicing (floor-of-1 clamping on both `[` and `]`; reset; adjusted body hash differs from full-paragraph hash).
- [x] 6.3 Add a unit test for the clean-tree entry guard: dirty tree → no modal opened + toast (both Graph and Notes entry). Add a test that a dirty source detected at commit returns the modal to `ParagraphMultiSelect`.
- [x] 6.4 Add an integration test (`ft/tests/`) asserting the end-to-end flow writes a `[!ft-source]` callout with the *adjusted* range and body, pinned to HEAD, and that re-running the same pick into the same note is a dedup no-op + toast.
- [x] 6.5 Add a test asserting source-equals-target is rejected inline at the target-pick step.

## 7. Build invariants

- [x] 7.1 `cargo build --release`
- [x] 7.2 `cargo test --workspace`
- [x] 7.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 7.4 `cargo fmt --check`
- [x] 7.5 `cargo run --release -q -- commands docs --check`
