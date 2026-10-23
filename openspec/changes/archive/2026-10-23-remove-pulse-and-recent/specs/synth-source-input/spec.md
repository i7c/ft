## MODIFIED Requirements

### Requirement: SynthSource input type
`ft-core::synth` SHALL define a `SynthSource` struct as the input type for synth-note scaffolding, carrying exactly the four fields a protected section pins: `source_path: PathBuf`, `line_start: u32`, `line_end: u32`, `body: String`. `SynthSource` SHALL NOT carry feed-specific fields such as blame date or matched link targets; no feed entry type remains to lower from, so every caller (search results, `--from` picks, and TUI flows) SHALL construct `SynthSource` directly.

#### Scenario: SynthSource carries only pinning fields
- **WHEN** a `SynthSource` is constructed
- **THEN** it exposes `source_path`, `line_start`, `line_end`, and `body` and no other fields

## REMOVED Requirements

### Requirement: Conversion from RecentEntry
**Reason**: `ft_core::recent` (`RecentEntry`) is deleted with the Recent feed,
so the conversion has no source type. `SynthSource` is now only ever
constructed directly at the call boundary.
**Migration**: Build `SynthSource { source_path, line_start, line_end, body }`
directly, as the search, `--from`, and TUI call sites already do.
