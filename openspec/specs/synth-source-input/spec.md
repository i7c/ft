# synth-source-input Specification

## Purpose
The honest input type for synth-note scaffolding: `SynthSource` carries exactly the four fields a protected section pins (source path, inclusive line range, verbatim body), replacing the feed-specific `GatherEntry` / `RecentEntry` at the scaffold/accrete seams. Feed callers lower via `From` at the call boundary.

## Requirements
### Requirement: SynthSource input type
`ft-core::synth` SHALL define a `SynthSource` struct as the input type for synth-note scaffolding, carrying exactly the four fields a protected section pins: `source_path: PathBuf`, `line_start: u32`, `line_end: u32`, `body: String`. `SynthSource` SHALL NOT carry feed-specific fields such as blame date or matched link targets; those remain on `GatherEntry` / `RecentEntry`.

#### Scenario: SynthSource carries only pinning fields
- **WHEN** a `SynthSource` is constructed
- **THEN** it exposes `source_path`, `line_start`, `line_end`, and `body` and no other fields

### Requirement: Conversion from GatherEntry and RecentEntry
`SynthSource` SHALL implement `From<&GatherEntry>` and `From<&RecentEntry>`, copying the four pinning fields and dropping the feed-specific fields. These conversions SHALL be the single call-site boundary where feed entries lower into synth inputs.

#### Scenario: GatherEntry lowers to SynthSource
- **WHEN** a `GatherEntry` with `source_path`, `line_start`, `line_end`, `section_text`, `date`, `matched` is converted to `SynthSource`
- **THEN** the resulting `SynthSource` has the `source_path`, `line_start`, `line_end`, and `body == section_text` and carries no `date` or `matched`

#### Scenario: RecentEntry lowers to SynthSource
- **WHEN** a `RecentEntry` is converted to `SynthSource`
- **THEN** the resulting `SynthSource` has the four pinning fields and drops any feed-specific fields

### Requirement: Scaffold planner takes SynthSource
`plan_synth_scaffold` SHALL accept `&[SynthSource]` (previously `&[GatherEntry]`). Its behavior SHALL be unchanged: it SHALL build one `ProtectedSection` per `SynthSource` (path, line range, HEAD commit SHA, blake3 content hash of `body`), refuse dirty/untracked sources with `SynthDirtySources`, and dedup-on-append by `(source_path, body)` via `accrete::filter_missing`.

#### Scenario: Planner builds a section per SynthSource
- **WHEN** `plan_synth_scaffold` is called with two `SynthSource` values on a clean repo and a non-existent target
- **THEN** it returns a create plan with two `ProtectedSection`s, each pinning HEAD and a blake3 hash of its body

#### Scenario: Dirty source still refused
- **WHEN** a `SynthSource`'s file is dirty or untracked at plan time
- **THEN** `plan_synth_scaffold` returns `SynthDirtySources` listing that file

#### Scenario: Append dedup drops already-pinned sources
- **WHEN** `plan_synth_scaffold` is called for an existing target where one `SynthSource`'s `(source_path, body)` is already pinned
- **THEN** the plan reports `dedup_skipped >= 1` and omits a section for that source

### Requirement: Accrete filter_missing takes SynthSource
`accrete::filter_missing` SHALL accept `Vec<SynthSource>` (previously `Vec<GatherEntry>`) and SHALL drop sources whose `(source_path, body)` is already present among the existing parsed callouts. The dedup key SHALL remain `(source_path, body)`; `commit_sha` SHALL remain deliberately excluded from the key.

#### Scenario: Filter drops a pinned source
- **WHEN** `filter_missing` is called with a `SynthSource` whose `(source_path, body)` matches an existing callout
- **THEN** that source is dropped from the returned vec

#### Scenario: Filter keeps an updated-body source
- **WHEN** `filter_missing` is called with a `SynthSource` whose `source_path` matches an existing callout but whose `body` differs
- **THEN** that source is kept in the returned vec
