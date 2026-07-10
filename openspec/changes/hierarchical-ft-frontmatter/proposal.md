## Why

ft's four frontmatter keys (`ft-tasks-section`, `ft-append-section`,
`ft-synth`, `ft-synth-targets`) are flat and uncoordinated — a
sprawl of top-level `ft-*` keys with no shared namespace. Grouping
them under one `ft:` map makes the set self-describing, keeps
unrelated frontmatter clean, and gives future ft keys a home with a
clear contract instead of inventing a new top-level `ft-foo` for
each feature.

## What Changes

- **New hierarchical `ft:` namespace** in YAML frontmatter. The four
  existing flat keys map to nested keys under one `ft:` map, e.g.:

  ```yaml
  ft:
    tasks:
      section: Tasks            # was ft-tasks-section
    append:
      section: Sessions         # was ft-append-section
    synth:
      enabled: true            # was ft-synth: true
      targets: ["[[Foo]]"]     # was ft-synth-targets
  ```

  Exact sub-key names are a design.md decision; the contract is "all
  ft-owned frontmatter lives under one `ft:` map."

- **Move fully to the nested form — BREAKING for existing vaults
  (explicit exception).** Readers recognize **only** the nested `ft:`
  keys; the legacy flat keys (`ft-tasks-section`, `ft-append-section`,
  `ft-synth`, `ft-synth-targets`) are no longer read and are treated
  as ordinary unknown frontmatter. This is a deliberate, one-time
  departure from the project's "don't break vault data" rule (the
  precedent in `note-flow-renames` was "breaking scripts is fine;
  breaking vaults is not" — this change waives the vault side as an
  exception). Concrete consequence: existing notes carrying flat keys
  silently stop being recognized — a note with `ft-synth: true` will
  no longer be treated as a synth note (so `ft notes synth verify
  --all` stops sweeping it and its `[!ft-source]` callouts stop being
  protected by link-review), and `ft-tasks-section`/`ft-append-section`
  defaults revert to file-end. No automated migration is provided.

- **Writers emit nested only and clean up orphans.** Every writer
  (`upsert_synth_frontmatter`, the synth scaffold) emits only the
  nested form. When a writer rewrites the frontmatter of a note that
  still carries legacy flat `ft-*` keys, it removes them — so any
  note ft *touches* is left in the canonical nested form (a
  migration-by-touch property, even without a dedicated command).

- **Frontmatter reader upgrade.** The current single-key string-level
  extractor (`notes/append.rs::frontmatter_value`) cannot follow
  nested maps. The readers move to a small indentation-aware nested
  extractor (design.md decides; preference for no new heavyweight
  dependency — nested maps for ~4 known keys don't justify
  `serde_yaml`). `ft-synth` / `ft-synth-targets` detection in
  `synth/callout.rs` moves to the same shared reader.

- **Docs updated** to the nested form everywhere
  (`docs/guide/*.md`, `docs/append-and-capture.md`,
  `docs/config.md`, README, `docs/architecture.md`); the flat form is
  removed entirely (not shown as a legacy fallback, since it is no
  longer read).

## Capabilities

### New Capabilities
- `ft-frontmatter-namespace`: the `ft:` hierarchical namespace
  contract — the key map shape, the old→new key mapping, the
  nested-only (no read-both) policy, and the writers-emit-nested +
  orphan-cleanup rule.

### Modified Capabilities
- `append-template`: the `ft-append-section` requirement becomes the
  nested `ft.append.section` key (nested-only, no legacy read).
- `quick-capture`: same `ft-append-section` read as append-template.
- `synth-notes`: the "Synth note frontmatter marker" requirement
  (`ft-synth: true`) and "Self-describing synth note targets"
  requirement (`ft-synth-targets`) move to the nested
  `ft.synth.enabled` / `ft.synth.targets` keys (nested-only).
- `synth-grow`: the targets source-set requirement reads the nested
  `ft.synth.targets` key (nested-only).

Mechanical scenario-example updates in specs that *mention* a flat
key without owning a requirement (`note-flow-naming`,
`journal-tui-tab`, `citation-index`, `link-review`, `notes-history`,
`ghost-promotion`) are a docs-pass task, not per-capability deltas.

## Impact

- **Readers/writers (centralized):** `ft-core/src/notes/append.rs`
  (`frontmatter_value`, `frontmatter_append_section`,
  `frontmatter_tasks_section`); `ft-core/src/synth/callout.rs`
  (`is_synth_note`, `parse_synth_targets`, `upsert_synth_frontmatter`);
  `ft-core/src/synth/scaffold.rs` (`SYNTH_FRONTMATTER`).
- **Callers of the synth marker** (scan-every-file paths):
  `synth/verify.rs`, `synth/repair.rs`, `synth/citations.rs`,
  `recent.rs`, `pulse.rs` — go through `is_synth_note`, so the
  nested-only read lives in one place.
- **CLI/TUI:** the gather tab's `o` context-note picker and
  `ft synth grow` read the nested targets. No new subcommand.
- **New dependency decision:** whether to add `serde_yaml` (or keep
  the string-level approach with a nested extractor) — design.md.
- **Tests:** every fixture and snapshot that hard-codes a flat key
  (~12 test files across `ft-core`/`ft`) updates to the nested form.
  No read-both coverage (flat form is unsupported).
- **Docs:** README demo, all guide pages, `docs/append-and-capture.md`,
  `docs/config.md`, `docs/architecture.md`.
- **Build invariants:** `cargo test --workspace`, clippy, fmt, and
  `ft commands docs --check` (no keymap change, so no regeneration).
- **On-disk breakage (by design, accepted exception):** existing
  flat-keyed vaults stop being recognized until hand-updated to the
  nested form. Notes ft touches are auto-cleaned; all others need
  manual frontmatter edits. No migration tool is provided.
