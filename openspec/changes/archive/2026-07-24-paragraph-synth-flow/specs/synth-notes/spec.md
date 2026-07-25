## MODIFIED Requirements

### Requirement: Plan/apply split for synth mutations
A pure planner `plan_synth_scaffold(vault, target, sources: &[SynthSource]) -> SynthScaffoldPlan` SHALL compute the file changes without performing any I/O writes. The planner SHALL accept `SynthSource` inputs (the honest 4-field input type: `source_path`, `line_start`, `line_end`, `body`), NOT `GatherEntry`; feed callers (`GatherEntry`, `RecentEntry`) SHALL lower into `SynthSource` via `From` at the call boundary. A separate `apply_synth_scaffold(vault, plan)` SHALL perform writes exclusively via `ft_core::fs::write_atomic`. The plan SHALL distinguish create-vs-append cases and SHALL include the frontmatter content (if creating).

#### Scenario: Planner does no I/O
- **WHEN** `plan_synth_scaffold` is invoked
- **THEN** no files on disk are modified

#### Scenario: Applier uses write_atomic
- **WHEN** `apply_synth_scaffold` writes the scaffold
- **THEN** the write goes through `ft_core::fs::write_atomic` (same-dir tempfile + rename)

#### Scenario: Planner accepts SynthSource inputs
- **WHEN** `plan_synth_scaffold` is called with a slice of `SynthSource` values
- **THEN** it builds one `ProtectedSection` per `SynthSource`, pinning that source's `source_path`, `line_start`, `line_end`, and a blake3 hash of `body` to the repo HEAD commit SHA
