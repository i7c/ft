# Proposal: drift-exclude

## Why

Vaults with linked attachments (`![[diagram-v1.png]]`,
`[[report-2025.pdf]]`) grow ghost concepts with systematically similar
names; they pass the drift name gate and pollute `ft notes drift` with
pairs that are files, not concepts. The user needs a way to exclude
them from consideration.

## What Changes

- New `[drift]` config table with `exclude: Vec<String>` — glob
  patterns (`*`/`?`, case-insensitive) matched against each concept's
  target (ghost raw string; note stem and vault-relative path).
  Matching concepts never enter the drift candidate universe.
- Documented in docs/config.md and the drift section of
  docs/guide/notes.md.

## Capabilities

### New Capabilities

<!-- none -->

### Modified Capabilities

- `drift-detection`: ADDED requirement — config-driven exclusion of
  concepts from the candidate universe.

## Impact

- `ft-core`: `config.rs` (new table, unknown-keys rejected),
  `graph::drift` (filter + small dependency-free glob matcher).
- Tests: core unit + CLI integration with a configured vault.
- No CLI flag changes; no other behavior changes.
