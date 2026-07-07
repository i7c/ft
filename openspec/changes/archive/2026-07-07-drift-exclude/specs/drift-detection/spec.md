# drift-detection — delta

## ADDED Requirements

### Requirement: Config-driven concept exclusion
A `[drift]` config table SHALL provide `exclude: Vec<String>` glob
patterns (`*` matches any sequence including `/`, `?` one character;
matching is case-insensitive). A concept SHALL be excluded from the
drift candidate universe when any pattern matches its ghost target
string, or — for notes — its filename stem or vault-relative path.
Unknown keys in `[drift]` SHALL be rejected like every other config
table. The default is an empty list (no exclusions).

#### Scenario: Attachment ghosts excluded
- **WHEN** config sets `exclude = ["*.png", "*.pdf"]` and the vault
  links `[[diagram-v1.png]]` and `[[diagram-v2.png]]`
- **THEN** `ft notes drift` reports no pair for them

#### Scenario: Unconfigured behavior unchanged
- **WHEN** no `[drift]` table is present
- **THEN** the report is identical to pre-change behavior
