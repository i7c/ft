# synth-source-input

## MODIFIED Requirements

### Requirement: Conversion from GatherEntry and RecentEntry

`SynthSource` SHALL implement `From<&RecentEntry>`, copying the four pinning
fields and dropping the feed-specific fields. This conversion SHALL be the
single call-site boundary where feed entries lower into synth inputs.
(`From<&GatherEntry>` is removed with the gather engine; `--from` picks and the
TUI flows construct `SynthSource` directly.)

#### Scenario: RecentEntry lowers to SynthSource

- **WHEN** a `RecentEntry` is converted to `SynthSource`
- **THEN** the resulting `SynthSource` has the four pinning fields (`source_path`, `line_start`, `line_end`, `body == section_text`) and drops any feed-specific fields (e.g. `date`)
