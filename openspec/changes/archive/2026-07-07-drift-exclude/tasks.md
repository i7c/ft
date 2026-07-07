# Tasks: drift-exclude

## 1. Implementation

- [x] 1.1 Add the `[drift]` config table (`exclude`, default empty,
  deny_unknown_fields)
- [x] 1.2 Filter the concept universe in `graph::drift::detect_drift`
  via a small `*`/`?` case-insensitive glob matcher (no new deps)
- [x] 1.3 Unit tests: pattern excludes ghost pair; note stem/path
  matching; empty config unchanged; matcher edge cases
- [x] 1.4 Integration test: vault with `.ft/config.toml` excluding
  `*.png`; docs (config.md table + [drift] section, guide notes.md
  drift section)
- [x] 1.5 Invariant sweep
