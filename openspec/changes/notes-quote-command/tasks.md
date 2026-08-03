## 1. Core refactor: line-range slice

- [ ] 1.1 Add `ft-core/src/synth/slice.rs` with `slice_lines(content: &str, line_start: u32, line_end: u32) -> Option<String>`: split on `\n`, drop a trailing empty element when `content` ends with `\n` (trailing newline is not a line), validate `line_start >= 1 && line_start <= line_end <= line_count`, rejoin with `\n` (no trailing newline). Register the module in `ft-core/src/synth/mod.rs`.
- [ ] 1.2 Unit tests for `slice_lines`: multi-line slice, single-line slice, full-file slice, trailing-newline file (`"a\nb\n"` = 2 lines, `L1-2` → `"a\nb"`), no-trailing-newline file, empty file, `L1-1` on empty file → `None`, `start == 0` → `None`, `start > end` → `None`, `end > line_count` → `None`, embedded blank line inside range preserved as `\n\n` in the join.
- [ ] 1.3 Refactor `ft-core/src/synth/verify.rs::verify_one` to use `slice_lines` for the blob slice; preserve the `SourceMissing` "line range outside file (file has N lines)" detail. Existing verify tests must stay green.

## 2. Core refactor: shared pin-building primitives

- [ ] 2.1 In `ft-core/src/git.rs`, add `head_short_sha(repo: &Path) -> Result<String>` wrapping `head_hash` + truncation to `SHORT_SHA_LEN` (or a `min(len)` guard), with a unit test.
- [ ] 2.2 In `ft-core/src/synth/scaffold.rs`, extract `find_dirty_sources(repo: &git::RepoMap, paths: &[PathBuf]) -> Result<Vec<PathBuf>>` from `plan_synth_scaffold`'s batch check (one `git status`, sorted + deduped offenders, same dirty-set semantics: modified/deleted/conflicted/untracked). Unit test: dirty set, untracked, clean, unrelated-dirty-file-not-included.
- [ ] 2.3 In `ft-core/src/synth/scaffold.rs`, extract `build_pinned_section(short_sha: &str, entry: &SynthSource) -> ProtectedSection` (the pure loop body: hash + struct) and make `plan_synth_scaffold`'s loop call it. Refactor `plan_synth_scaffold` to use `find_dirty_sources` + `head_short_sha`. All existing scaffold tests stay green unchanged (error shape for `SynthDirtySources` preserved).
- [ ] 2.4 Opportunistic: if `reslice.rs`/`repair.rs` blob slices are trivially replaceable by `slice_lines` without behavior change, adopt it there too; otherwise leave them (note in the task list why).

## 3. CLI: `ft notes quote`

- [ ] 3.1 Add `ft/src/cmd/quote.rs` with `QuoteArgs` (`<file: PathBuf>` positional, `--lines A-B` required long flag parsed like `synth reslice --lines`; reject `A < 1`, `A > B`, non-numeric) and `run_quote(args, vault_flag) -> Result<ExitCode>`: discover vault → discover repo (error if none) → read file (missing/unreadable → error naming file) → `find_dirty_sources` on the single path (dirty → error naming file, "committed and unmodified" message) → `slice_lines` (out of bounds → error naming file + actual line count) → `build_pinned_section` with `head_short_sha` → `callout::serialize` → print to stdout with one trailing newline. Read-only: no writes, no editor. Use `vault.relativize` for absolute inputs; do not auto-append `.md`.
- [ ] 3.2 Register the module in `ft/src/cmd/mod.rs`; add `NotesCommand::Quote(QuoteArgs)` variant in `ft/src/cmd/notes.rs` with a help string and dispatch arm in `run`.
- [ ] 3.3 Integration tests in `ft/tests/` (assert_cmd + assert_fs fixture vault): success output is exactly the canonical callout (path/range/@sha7/#hash6 + `> `-prefixed body, single trailing newline); missing file; unreadable/non-UTF8; dirty source (modified, untracked, staged); unrelated dirty file still succeeds; out-of-range (error includes actual line count); trailing-newline file `L1-2` accepted with body `a\nb`; absolute path relativized in header; no `.md` auto-append; exit codes 0/1; no vault / no git repo errors.
- [ ] 3.4 Round-trip test: emit a callout via `ft notes quote`, append it to a synth note in the fixture vault, run `ft notes synth verify` → section reported `ok`.

## 4. Docs

- [ ] 4.1 Document `ft notes quote` in `docs/guide/synthesis.md` (plumbing surface: prerequisites, output contract, read-only) and add a line to the CLI surface list in `docs/architecture.md` §"Synthesis".
- [ ] 4.2 Update README command list if it enumerates `ft notes` subcommands.

## 5. Verification

- [ ] 5.1 Run the five build invariants: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`, `cargo run --release -q -- commands docs --check`.
- [ ] 5.2 Commit the implementation as its own commit (spec commit already landed separately).

## 6. [ft.nvim] editor-side consumer (sibling repo)

- [ ] 6.1 In `ft.nvim`, add a pin-selection action that calls `ft notes quote <file> --lines A-B` (via the `ft.rpc` transport seam), inserts the returned callout at the cursor or into a target note, and surfaces errors (dirty source, range out of bounds) as notifications. Update `ft.nvim`'s `ARCHITECTURE.md` with the new CLI contract it depends on. Record the paired commit SHA in the archive note for this change.
