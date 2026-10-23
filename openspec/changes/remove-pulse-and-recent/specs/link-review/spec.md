## REMOVED Requirements

### Requirement: ft notes pulse subcommand
**Reason**: `ft notes search` (with `--sort date` and date predicates) is the
synthesis discovery surface; the windowed link ranking it replaced has no
remaining users.
**Migration**: Run `ft notes search`, optionally with `--sort date`, over the
same period; link-shaped discovery is `ft notes search "[[concept]]"`.

### Requirement: Link extraction via git-log scan
**Reason**: The git-diff based extraction engine (`ft_core::pulse`) is deleted
with the subcommand and the Pulse tab.
**Migration**: Search indexes the whole vault from one scan pass; no
equivalent git-window extraction exists or is needed.

### Requirement: Paragraph-level frequency dedup
**Reason**: Dedup existed to make the pulse ranking stable. The ranking is
gone.
**Migration**: None — use `ft notes search` result ranking instead.

### Requirement: Frequency ranking and ghost marking
**Reason**: Ranking and the `?` ghost marker were pulse-only output grammar.
Ghost ranking survives as `ft notes ghosts`.
**Migration**: Use `ft notes ghosts` for mention-ranked unresolved targets.

### Requirement: Path-prefix exclude filter
**Reason**: Pulse was one of two consumers of `[synth] exclude_prefixes`; the
search index remains a consumer, so the config field stays while the pulse
requirement goes.
**Migration**: None — `[synth] exclude_prefixes` still filters
`ft notes search`.

### Requirement: Synth-note callout exclusion
**Reason**: Excluding quoted callouts prevented double-counting on the next
pulse. There is no next pulse.
**Migration**: None.

### Requirement: Default table output
**Reason**: The command that rendered the table is deleted.
**Migration**: None; use `ft notes search` output formats.

### Requirement: JSON output mode
**Reason**: `ft notes pulse --json` is deleted.
**Migration**: Use `ft notes search --json`.

### Requirement: Empty-window handling
**Reason**: There is no window and no scan to come up empty.
**Migration**: None.
