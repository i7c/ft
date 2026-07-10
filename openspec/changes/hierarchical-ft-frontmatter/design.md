## Context

ft recognizes four flat frontmatter keys today:

| Flat key              | Type        | Read by                          |
|-----------------------|-------------|----------------------------------|
| `ft-tasks-section`    | scalar      | `notes::append::frontmatter_tasks_section` |
| `ft-append-section`   | scalar      | `notes::append::frontmatter_append_section` |
| `ft-synth`            | boolean     | `synth::callout::is_synth_note` |
| `ft-synth-targets`    | YAML seq    | `synth::callout::parse_synth_targets` |

The two scalar readers use a deliberately lightweight string-level
extractor (`frontmatter_value`) — no YAML dependency. The two synth
readers hand-parse frontmatter lines. Writers:
`upsert_synth_frontmatter` (sets the marker + targets) and
`SYNTH_FRONTMATTER` (the fresh-note constant in `scaffold.rs`).

These keys appear in ~12 test files and across the guide docs, and —
critically — in real user vaults. The project precedent
(`note-flow-renames`, archived 2026-07-06) waives script back-compat
but explicitly preserved vault data ("breaking scripts is fine;
breaking vaults is not"). **This change waives the vault-data rule as
a deliberate, one-time exception**, per the user's direction: move
fully to the new nested form, no read-both, no migration command.

## Goals / Non-Goals

**Goals:**
- One self-describing `ft:` namespace holding all ft-owned frontmatter.
- A single canonical nested form — no read-both complexity.
- A single shared frontmatter reader used by every consumer.
- Any note ft writes is left in the canonical form (orphan cleanup).

**Non-Goals:**
- Reading the legacy flat keys (dropped outright — the exception).
- A migration command or automated migration tool.
- Changing the `[!ft-source]` callout body format (that's note body,
  not frontmatter — untouched).
- Touching the `[synth]` *config* table or any other `ft.toml` key
  (config, not frontmatter).
- Renaming the CLI `ft synth` / `ft notes synth` commands.

## Decisions

### D1: Sub-key shape

```yaml
ft:
  tasks:
    section: Tasks        # was ft-tasks-section
  append:
    section: Sessions     # was ft-append-section
  synth:
    enabled: true        # was ft-synth: true
    targets: ["[[Foo]]"] # was ft-synth-targets
```

- `ft.tasks.section` / `ft.append.section`: keep the word `section`
  on both — they *are* section names, and the parallel structure
  (`tasks.section` / `append.section`) reads well. The flat originals
  already encode "section" in their names; we preserve that word.
- `ft.synth.enabled` (not `ft-synth: true`): a boolean *property of
  the note* reads better as an adjective than a noun. `enabled: true`
  is unambiguous and mirrors how config booleans are conventionally
  named.
- `ft.synth.targets` (not `ft-synth-targets`): dropping the redundant
  `synth-` prefix since the parent already says `synth`.
- The user's suggested `ft.note.synth.*` nesting is dropped — every
  frontmatter key is about *the note the frontmatter is on*, so a
  `note:` level adds a layer for nothing. `ft.synth.*` is enough.

**Alternatives considered:** `ft.synth.is: true` (too terse, `is` is a
poor key name); `ft.synth: true` as a scalar with a sibling
`ft.synth-targets` (keeps the flat-style split we're trying to cure);
a single `ft.synth: { targets: [...] }` where presence implies
"enabled" (implicit-true is a footgun — a note with `ft.synth.targets`
but a typo'd empty list would silently stop being a synth note).

### D2: Nested form only (breaking — explicit exception)

Readers recognize **only** the nested `ft:` keys. The legacy flat
keys are treated as ordinary unknown frontmatter: a note with
`ft-synth: true` and no nested map is **not** a synth note; a note
with `ft-tasks-section: Tasks` and no nested map resolves tasks to
file-end. This is the user-directed exception to the "don't break
vault data" rule.

The silent consequence is real and stronger than a command rename
(which broke scripts, not note semantics): existing synth notes lose
synth status (`verify --all` stops sweeping them, their
`[!ft-source]` callouts stop being protected by link-review); the
two section keys revert to file-end. The affected set is the
maintainer's own vault, accepted as the cost of a clean cutover.

**Mitigation by touch:** writers clean up orphans (D4), so any note
ft writes to is auto-converted. Untouched notes need manual frontmatter
edits — documented loudly in the guide and README.

**Alternatives considered:** read-both, write-new (the original
proposal) — rejected by the user for this change; would permanently
carry two readers and a conflict policy. read-old-write-old (no-op) —
rejects the whole point.

### D3: Frontmatter reader — stay string-level, add nesting

Do **not** add `serde_yaml`. Reasons:
- `ft`'s frontmatter is a tiny, controlled subset (`ft:` map +
  arbitrary user keys we never interpret). A full YAML parser is a
  heavyweight dependency for parsing ~4 known keys.
- The existing scalar extractor is already string-level by design and
  the codebase comment (`notes/append.rs`) calls this out explicitly.
- Obsidian frontmatter is usually well-formed but not guaranteed; a
  lenient hand-parser degrades more gracefully than a strict YAML
  lib on hand-edited quirks (mixed quotes, tab/space indentation).

Instead, extend the frontmatter helpers with a small
**indentation-aware nested-map reader** in a new
`ft-core/src/frontmatter.rs` module (or extend `notes/append.rs`'s
helpers and re-export). It extracts the leading `---\n…\n---` block
(reusing `extract_frontmatter_block`), then walks lines tracking
indentation to follow the `ft:` → `tasks:` / `append:` / `synth:`
→ `section` / `enabled` / `targets` paths. Target-parsing reuses the
existing flow-sequence / block-sequence logic from
`synth/callout.rs::parse_synth_targets`.

**Alternative considered:** `serde_yaml` — rejected per above. Could
be revisited if ft ever needs to read arbitrary user frontmatter
keys (it currently does not).

### D4: Shared reader module + orphan-cleaning writers

Centralize all four reads in one module (`ft-core/src/frontmatter.rs`)
so `notes/append.rs` and `synth/callout.rs` both call into it, instead
of each keeping its own parser. `is_synth_note`'s call sites
(`verify.rs`, `repair.rs`, `citations.rs`, `recent.rs`, `pulse.rs`)
go through the shared reader unchanged — they already call
`is_synth_note`, so the nested-only read lives in one place.

Writers: `upsert_synth_frontmatter` emits only the nested form. When
it rewrites the frontmatter of a note that still carries legacy flat
`ft-*` keys (`ft-synth:`, `ft-synth-targets:`), it **removes** those
lines — orphan cleanup. This guarantees a note ft touches is left in
the canonical nested form with no dead flat keys, giving a
migration-by-touch property even without a dedicated command. The
scaffold constant `SYNTH_FRONTMATTER` emits the nested form for fresh
notes. (The two scalar keys `ft-tasks-section` / `ft-append-section`
are user-authored only — no ft writer sets them — so there is no
writer to clean them; they're simply read nested-only.)

## Risks / Trade-offs

- **[Risk] Existing flat-keyed vaults break silently** → by design
  (the accepted exception). Mitigation: document loudly (README +
  guide + a "breaking change" callout); notes ft touches are
  auto-cleaned; the affected set is the maintainer's own vault. Users
  who want their untouched notes recognized must hand-edit
  `ft-synth: true` → `ft:\n  synth:\n    enabled: true` (and the
  targets/section analogues). There is no tool to do this for them.
- **[Risk] Two writers race on one note** → pre-existing (synth
  scaffold + manual edit). Unchanged by this design; `write_atomic`
  + the existing `LineChanged` guard cover the mutation paths.
- **[Trade-off] No `serde_yaml`** → the nested reader is more code
  than `#[derive(Deserialize)]`. Justified by the controlled-keyset
  argument (D3); revisit if the keyset grows beyond ~8 keys.
- **[Trade-off] Orphan cleanup is a write-time side effect** → a
  `ft synth grow` that sets targets will also strip a hand-written
  `ft-synth: true` line. This is desirable (canonicalization), but
  means a write is not a pure "add my key" operation — it may remove
  lines the user authored. Acceptable given the cutover goal; the
  lines removed are ones nothing reads anyway.

## Migration Plan

No automated migration. This is a clean cutover:

1. **Implement** the shared nested reader (nested-only); switch
   writers to nested + orphan cleanup.
2. **Ship** — existing flat-keyed notes stop being recognized.
3. **Hand-convert** untouched notes as desired (the maintainer's own
   vault). Notes ft writes to are auto-cleaned.
4. **Docs** show only the nested form.

Rollback: revert the code change. Notes that were hand-converted to
the nested form would, under the rolled-back (flat-only) code, **not**
be recognized — so a rollback after hand-conversion breaks those
notes. Mitigation: don't hand-convert until confident the change
sticks; or accept that rollback requires re-adding flat keys. This
asymmetry is inherent to a one-way format cutover with no migration
tool.

## Open Questions

- **`ft.synth.enabled: false` semantics.** Today `ft-synth: false`
  is treated as "not a synth note" (`is_synth_note` returns false for
  non-`true`). Preserve: `ft.synth.enabled: false` (or absent) means
  not a synth note. No behavior change. (Decided: preserve.)
- **Scope of orphan cleanup.** Should `upsert_synth_frontmatter`
  strip *only* `ft-synth:` / `ft-synth-targets:`, or any `ft-*`
  top-level key? Decided: only the two synth flat keys it owns —
  stripping `ft-tasks-section` / `ft-append-section` would be
  surprising on a synth write and those belong to other features.
  Revisit if a future writer owns multiple feature areas.
