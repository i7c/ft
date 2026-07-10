## 1. Shared frontmatter reader

- [x] 1.1 Create `ft-core/src/frontmatter.rs` with an indentation-aware nested-map reader: extract the leading `---\n…\n---` block (reuse `notes::append::extract_frontmatter_block`), then walk lines tracking indentation to read `ft:` → `tasks`/`append`/`synth` → `section`/`enabled`/`targets` paths.
- [x] 1.2 Add `ft_tasks_section(content) -> Option<String>`, `ft_append_section(content) -> Option<String>`, `ft_synth_enabled(content) -> Option<bool>`, `ft_synth_targets(content) -> Option<Vec<String>>` — each reads the nested key only (no legacy flat fallback).
- [x] 1.3 Unit-test the reader: nested form, key absent (None), quoted/bare values, flow- and block-sequence targets, hand-authored quirks (mixed indentation, CRLF). Add explicit cases that legacy flat keys return None (ignored).
- [x] 1.4 Re-export from `ft-core` lib root and delete `notes::append::{frontmatter_value, frontmatter_append_section, frontmatter_tasks_section}` once all callers move (no shims — repo convention).

## 2. Switch synth readers/writers

- [x] 2.1 Rewrite `synth::callout::is_synth_note` to delegate to `frontmatter::ft_synth_enabled` (nested `ft.synth.enabled` only; legacy `ft-synth:` ignored).
- [x] 2.2 Rewrite `synth::callout::parse_synth_targets` to delegate to `frontmatter::ft_synth_targets` (nested `ft.synth.targets` only; legacy `ft-synth-targets:` ignored). Reuse the existing flow-/block-sequence parsing logic.
- [x] 2.3 Rewrite `synth::callout::upsert_synth_frontmatter` to emit the nested `ft:` form only (`ft.synth.enabled: true` and `ft.synth.targets`). When the note carries legacy `ft-synth:` / `ft-synth-targets:` lines, remove them (orphan cleanup). Preserve all unrelated frontmatter. Update existing unit tests (`upsert_*`).
- [x] 2.4 Update `synth::scaffold::SYNTH_FRONTMATTER` constant to emit the nested form for fresh notes.
- [x] 2.5 Confirm `synth::verify`, `synth::repair`, `synth::citations`, `recent`, `pulse` need no changes (they all go through `is_synth_note`); spot-check each.

## 3. Switch append/task-section readers

- [x] 3.1 Update `task::ops::auto_position` to read `frontmatter::ft_tasks_section` instead of `notes::append::frontmatter_tasks_section`. Precedence unchanged: explicit flag → frontmatter → `[tasks] default_section` → `Append`.
- [x] 3.2 Update the TUI append flow (`ft/src/tui/notes_actions/append.rs`) and CLI `ft notes append` (`ft/src/cmd/notes.rs`) to read `frontmatter::ft_append_section`.
- [x] 3.3 Update the gather-tab `o` context-note picker and `ft synth grow` (`ft/src/cmd/synth.rs`) to read targets via `frontmatter::ft_synth_targets`.

## 4. Tests & fixtures

- [x] 4.1 Update existing fixture/test notes that hard-code flat keys to the nested form. Add cases asserting legacy flat keys are ignored (return None / not-a-synth-note).
- [x] 4.2 Update `ft/tests/synth_cli.rs`, `ft/tests/notes_append.rs`, `ft/tests/tasks_create.rs`, `ft/tests/notes_history.rs` to the nested form.
- [x] 4.3 Update `ft-core/src/synth/callout.rs`, `scaffold.rs`, `recent.rs`, `pulse.rs` unit tests to nested frontmatter.
- [x] 4.4 Update TUI synthesis/gather snapshot tests (`ft/src/tui/tests/synthesis.rs`, `ft/src/tui/tests/graph.rs`, `ft/src/tui/tests/tasks.rs`, `ft/src/tui/tests/history.rs`) to nested frontmatter; regenerate affected insta snapshots.

## 5. Docs

- [x] 5.1 Rewrite README demo and frontmatter examples to the nested form; add a "breaking change" callout noting legacy flat keys are no longer recognized.
- [x] 5.2 Update `docs/guide/tasks.md`, `docs/guide/capture-and-templates.md`, `docs/guide/synthesis.md`, `docs/guide/notes.md` to the nested form (flat form removed entirely).
- [x] 5.3 Update `docs/append-and-capture.md`, `docs/config.md`, `docs/architecture.md` (synth marker + targets sections, the append-section frontmatter reference).
- [x] 5.4 Add a short "Frontmatter" section to the guide documenting the `ft:` map shape and the nested-only policy.

## 6. Build invariants

- [x] 6.1 `cargo build --release`
- [x] 6.2 `cargo test --workspace`
- [x] 6.3 `cargo clippy --workspace --tests -- -D warnings`
- [x] 6.4 `cargo fmt --check`
- [x] 6.5 `cargo run --release -q -- commands docs --check` (no keymap change expected; confirm still passes)
