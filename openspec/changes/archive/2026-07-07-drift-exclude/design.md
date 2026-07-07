# Design: drift-exclude

Single decision worth recording: the glob matcher is a ~20-line
dependency-free `*`/`?` matcher (case-insensitive, `*` crosses `/`)
rather than pulling `globset` into ft-core — same trade as the inline
Levenshtein in the original drift change. Patterns match the ghost raw
target (which carries the attachment extension — the polluting case),
and for notes both the stem and the vault-relative path. Exclusion
happens at concept-universe construction, so excluded concepts cost
nothing in the O(n²) gate and can never appear on either side of a
pair.
