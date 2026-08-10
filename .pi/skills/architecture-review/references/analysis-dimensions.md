# Analysis Dimensions

The critique catalog. Work through these dimensions during Phase 5; for each suspected problem, gather evidence (file:line or a count) before listing it. The example evidence is genericized from a real Rust/TypeScript/Go review — adapt the shapes, not the reasoning.

## 1. Cohesion — does each module do one thing?

A module should be describable in one sentence. When a module's doc comment needs a list, it has a cohesion problem.

**Signals**
- A module whose name is the system's name (e.g. `vault.rs` doing discovery + scanning + parsing + path utilities + format selection).
- Multiple walker/parser implementations of the same concept in different modules.
- One module hosting both the *model* and the *features* that analyze it.

**Example evidence**
- `vault.rs` (996 LOC) enumerated: discovery, config loading, scan orchestration, two file walkers, per-file parsing, path utilities, template resolution, format seam — eight responsibilities, one module.
- `synth/callout.rs` held the section grammar AND synth-note detection AND frontmatter surgery — three concerns, one filename.

**Fix direction**
Extract the cohesive sub-unit (often the *pipeline contract*: the types one stage hands the next) into its own module; the extraction itself usually breaks a cycle (see 3).

## 2. Coupling — who depends on whom?

Map the dependency graph (Phase 3) and look for violations of the layering you'd expect: foundation → model → features.

**Signals**
- A low-level/analysis module importing a *downstream feature* module for a single predicate.
- A module with 10+ sibling dependencies that could be served by one small extraction.
- Duplicated logic across modules because the shared helper lives in the wrong place.

**Example evidence**
- `recent → synth` and `pulse → synth`: link review and history feeds imported the synthesis feature just to check `is_synth_note`.
- `recent` imported `pulse::compute_pulse` only for its `added_lines` side map — paying for the whole ranked scan it never used.

**Fix direction**
Move the shared capability to the level where both consumers live (a foundation module), or extract the single function actually needed (`window_added_lines` instead of `compute_pulse`).

## 3. Cycles — module A imports B and B imports A

A use-level cycle is the #1 architecture smell. Cycles are almost always an artifact of *file placement*: the pipeline contract type lives in the wrong module.

**Signals**
- `use crate::a` in b and `use crate::b` in a.
- The cycle is acyclic at runtime (scan → build → query) but tangled at the source level.

**Example evidence**
- `vault → graph::parser` (link extraction) and `graph → vault` (`Graph::build` needed the vault only for its directory walk). The fix: extract a `scan` module owning `Scan`/`ParsedFile`/`parse_file`; graph consumes the snapshot, vault delegates, both edges die.

**Fix direction**
Find the artifact both sides need (often the parse output or the walk result), extract it to a neutral module, and pass data instead of services (`Graph::build(scan)` not `Graph::build(vault, scan)`).

## 4. Interface surface discipline — what's actually public?

The declared public surface is a promise. Measure it against consumption.

**Signals**
- Public exports referenced by nothing outside their own module (dead API).
- Exports only used internally or in tests (should be crate-private).
- A crate/library whose only real consumer is one binary — every `pub` is a promise to that consumer.

**Example evidence**
- 380 named public items; 41 dead (referenced by nothing, not even tests); 75 used only inside the crate; ~31% of the declared surface not consumed by the only external consumer.
- Outcome types "returned but never named" (`CompleteOutcome`, `MovePlan`) — callers infer them without naming them.

**Fix direction**
Default to crate-private; promote only what external consumers import. Consider a facade module documenting the real contract. Delete dead items outright (the repo's own conventions usually say so).

## 5. Deep vs shallow modules

Deep: big implementation, small interface (ideal). Shallow: many exports, little logic behind them.

**Signal**
LOC-per-export ratios across modules; flat lists of shell-out wrappers with no internal structure.

**Example evidence**
- `gather` (1097 LOC, 4 public items) — deep, exemplary.
- `git.rs` (1478 LOC, ~32 exports, zero internal structure) — a flat wide module serving two audiences (mutation: sync/commit; analysis: blame/diff) that don't overlap.

**Fix direction**
Split flat grab-bags along audience lines; keep deep modules intact. The split shrinks every consumer's import surface.

## 6. God objects / duplicated models

One concept, two representations, kept in sync by hand. Or one struct mixing domain fields with plumbing.

**Signals**
- A domain struct mirrored by a string-typed variant for query/eval (every new field must be added in three places).
- A struct whose fields split into content, format-residue, and positional metadata.
- A config struct that is 15+ sections deep in one flat object (sometimes fine; sometimes a sign).

**Example evidence**
- `task::Task` (20 fields: description, dates, format residue like `raw_trailing`, positional like `source_file`/`parent`) mirrored by `graph::TaskData` (string versions of the same dates) for DSL evaluation.

**Fix direction**
Evaluate against the real model, or derive the mirror mechanically. Split positional/plumbing fields from content where they vary independently.

## 7. Error handling policy — one policy, not three

Inconsistent error strategies across a codebase make handling impossible and display-only.

**Signals**
- A central enum mixing structured variants, string-erased variants (`Error::Notes(String)`), and one feature's specific shapes (reslice variants) while other features use local typed errors that never compose.
- Some modules return local error enums, others string-erase into the shared one — no rule.

**Example evidence**
- Central `Error` had infrastructure variants (VaultNotFound, Config, Io) + string erasers (Notes, Periodic, Git, Timeblock) + a feature cluster (ResliceSectionNotFound, ResliceAmbiguous, …), while the query DSL, timeblock parser, and recurrence each defined their own error that callers only see stringified through `anyhow`.

**Fix direction**
Decide: infrastructure errors in the central enum; domain errors as local typed enums implementing `std::error::Error`; move one feature's specific shapes into that feature's own enum.

## 8. Seams and testability

The plug-in points: clock injection, environment overrides, format interfaces, config loading. Consumers should use the seam, not hard-code the concrete thing.

**Signals**
- A "today"/"now" reader that some call sites bypass by reading the env var directly.
- A format/backend interface with one implementation, hard-coded at the one place that should do detection.
- A singleton interface returning `&'static dyn Trait` that can't carry state for the future config-driven case.

**Example evidence**
- `dates::today()` was the intended clock seam but a few call sites read `FT_TODAY` directly.
- `parse_file` hard-coded the concrete format instead of going through `vault.task_format()`.
- `task_format() -> &'static dyn TaskFormat` works only for a zero-sized singleton.

**Fix direction**
Route everything through the seam; note where the seam needs to grow (owned trait object, detection order).

## 9. Naming — collisions and overloaded namespaces

**Signals**
- Near-identical module names for different concepts (`recent.rs` the history feed vs `recents.rs` the log — both imported by the binary).
- Two modules named the same (`query` vs `graph::query`) imported side by side in the same file.

**Example evidence**
- `ft_core::query` (interpolation/presets/sort) and `ft_core::graph::query` (the DSL) imported together in `cmd/graph.rs` — a genuine readability tax.

**Fix direction**
Rename or move one; at minimum document the split in both module docs.

## 10. Dead capabilities

Whole subsystems exercised only by tests, or capabilities built but never wired into production.

**Signals**
- A function/capability whose only callers are `#[cfg(test)]` blocks.
- An incremental/optimization path that production never takes (always does the full version).
- An exported constant documenting a policy that the code re-hardcodes as literals.

**Example evidence**
- `Graph::refresh_note` (incremental single-file re-parse) used only by graph tests — production always full-rebuilds.

**Fix direction**
Wire it into production (it may be the future of the hot path) or delete it. Don't keep it as a curiosity.

## 11. I/O hygiene — one pass, one walker

Repeated full reads/walks where one pass serves all consumers; multiple walkers with divergent semantics.

**Signals**
- A hot path doing two complete vault/repo reads where one scan could serve both.
- Separate walker implementations that disagree about what "every file" means.
- Consumers reading the same file individually for data the scan already captured (frontmatter markers, headings, mtimes).

**Example evidence**
- `build_graph_snapshot` ran `vault.scan()` (read everything) then `CitationIndex::build` (read everything again) — two full passes per snapshot.
- A third walker (`walk_markdown_files`) with dotfile-only exclusion while the scanner also applied gitignore + config ignores — synth sweeps saw a different universe than the scanner.
- The TUI search picker walked + read files on every keystroke of a heading query; the scan already held the headings.

**Fix direction**
One scan captures the artifacts (plus metadata like mtime); consumers read the snapshot, with a narrow direct-read fallback for files the scan missed. Capture mtimes in the same walk (cheap) so even recency consumers stop re-walking.

## 12. Ripple cost of change

Before proposing any signature change, count the call sites and classify the ripple.

**Signals**
- A widely-called function gaining a parameter: 235 call sites that stay untouched (method kept as delegator) vs 232 that change (mechanical).
- A change that would be a regression for one consumer while fixing another (e.g., forcing a filesystem-walk CLI command through a full parse).

**Example evidence**
- Keeping `Vault::scan()` as a one-line delegator meant zero call-site churn; changing `Graph::build` meant a mechanical sweep that was codemoddable.
- CLI `ft find` stayed a filesystem walk (no parse cost) while the TUI picker got the scan-fed path — the same change, two shapes.

**Fix direction**
Keep convenience signatures that absorb the change where the ripple is huge. Never force a consumer to pay for capability it doesn't use. Codemod mechanical sweeps; hand-edit only the non-uniform cases.
