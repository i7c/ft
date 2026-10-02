## Context

Periodic-note resolution lives in `ft-core/src/periodic.rs`. `Period`
(`Daily`/`Weekly`/`Monthly`/`Quarterly`/`Yearly`) currently only knows how to
*shift* a date by whole periods (`Period::offset_date`); it has no notion of a
period's *start*. `resolve_periodic_path(vault_root, cfg, date)` formats
`cfg.path` and `cfg.format` against whatever `date` it is handed.
`create_or_get_periodic_path(vault_root, templates_dir, cfg, date, today, now)`
resolves the path from `date` but renders the template with a separate `today`,
so the two can disagree.

Every caller passes the raw target date — CLI `run_periodic_inner`
(`ft/src/cmd/notes.rs`), the TUI periodic flow
(`ft/src/tui/notes_actions/periodic.rs`), graph-tab periodic navigation
(`ft/src/tui/tabs/graph/mod.rs`), and the `@…` sigil interpolator
(`ft-core/src/query/interpolate.rs`) — so the period start is never computed.
Daily notes work by accident: for Daily the start equals the date. Weekly notes
with week-number formats (`%G-W%V`) work by accident too: every day of the week
formats to the same string. Any day-bearing format (naming the weekly note after
its Monday, e.g. `%Y-%m-%d`) exposes the bug, and the raw date still leaks into
the template context.

The codebase already has the Monday-anchoring math in
`ft_core::timeblock::report::week_bounds` (`date - num_days_from_monday`), and
its docs state the convention "Weeks are Mon–Sun to match blockary and ISO
8601". `docs/config.md` documents weekly formats but never says the date is
anchored.

## Goals / Non-Goals

**Goals:**
- Resolve a periodic note from the **start of the period** containing the target
  date, on every surface (CLI, TUI, sigils).
- Make the week start configurable (default Monday).
- Make the template context deterministic: a periodic note's `today` is its
  anchored period-start date.
- Keep daily behavior (and all existing `%Y-%m` / `%Y-Q%q` / `%Y` /
  `%G-W%V` configs) unchanged.

**Non-Goals:**
- Changing the token surface (`%q`/`%Q`, strftime) or adding new tokens.
- Changing `Period::offset_date` semantics.
- Editing or renaming existing note files on disk (a repo of weekly notes named
  after their open day is left as-is; only future resolution changes).
- Adding a global (cross-period) week-start setting; the knob lives on the
  period block.

## Decisions

**Decision 1: One anchoring function, `Period::start_of`, covering all five
periods.**

```rust
impl Period {
    /// Representative date for a note of this period.
    pub fn start_of(self, date: NaiveDate, week_start: WeekStart) -> NaiveDate {
        match self {
            Period::Daily => date,
            Period::Weekly => /* back up to week_start */,
            Period::Monthly => /* first of month */,
            Period::Quarterly => /* first day of the quarter */,
            Period::Yearly => /* Jan 1 */,
        }
    }
}
```

Applied to all periods (not just weekly) so the model is uniform and
day-bearing monthly/quarterly/yearly formats are correct too. Daily is the
identity, so existing daily callers are unaffected.

*Alternatives considered:*
- **Weekly only.** Smaller blast radius, but leaves the same latent bug for
  monthly/quarterly/yearly day-bearing formats and an inconsistent mental model.
- **Do the anchoring in `timeblock::report` instead.** Rejected: that module is
  about range queries over timeblocks, a different concern; `periodic.rs` is the
  owner of "what date is this periodic note for".

**Decision 2: Anchor inside the core resolution functions, by threading
`Period` in — not at each call site.**

`resolve_periodic_path(vault_root, period, cfg, date)` and
`create_or_get_periodic_path(vault_root, templates_dir, period, cfg, date, now)`
anchor `date` once and compute everything from the result. This is
correct-by-construction: a new caller cannot forget to snap, and the sigil,
CLI, and TUI paths converge on one implementation the day a caller is added.

*Alternatives considered:*
- **Call `start_of` at each of the four call sites.** No core signature change,
  but easy to forget at the fifth call site and duplicates the anchor-then-format
  sequence.
- **Wrap cfg in a period-aware struct.** More machinery than the signature
  change it replaces; the callers all already `match period` to pick `cfg`.

**Decision 3: The anchored date is also the template `today`; drop the separate
`today` parameter.**

`create_or_get_periodic_path` currently takes both `date` (path) and `today`
(template). For a periodic note those are the same fact, and letting them
diverge is what makes weekly templates render the open day. The new signature
keeps `now` (for `{{ now }}` timestamps) and sets `today` to the anchored date.
Consequently `Vault::ensure_target(date, file_override, now)` loses its `today`
parameter.

*Trade-off:* a daily note created with `--date`/`--offset` (or the timeblocks
"tomorrow" pane) now renders its template against that note's date rather than
the invocation day. This is the intended "a periodic note is *for* its date"
semantics and matches Obsidian's Periodic Notes. It is called out in the
proposal and covered by a test.

**Decision 4: `week_start` is a per-period config key, defaulting to Monday.**

```toml
[periodic_notes.weekly]
path = "journal/%G"
format = "%G-W%V"
week_start = "monday"   # or "sunday"; default monday
```

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    #[default]
    Monday,
    Sunday,
}
```

`PeriodicPeriod` gains `#[serde(default)] pub week_start: WeekStart`. Placing it
on the period block (rather than a scalar under `[periodic_notes]`) keeps it
next to the weekly config a user is already editing and avoids the TOML
ordering trap where a bare key must precede all `[periodic_notes.*]`
sub-tables. Only `Period::Weekly` reads it; the field is inert on other periods.
`PeriodicPeriod` gains `#[derive(Default)]` so struct-literal construction stays
terse.

*Alternatives considered:*
- **Global `[periodic_notes] week_start`.** Reads well but must appear before
  any sub-table and only applies to one period anyway.
- **ISO 8601 numbering toggle only (`%V` semantics).** Rejected: the issue is
  which *date* is formatted, not the week-number token.

**Decision 5: Sigils inherit the anchor through the shared resolver.**

The interpolator already knows the `Period` and delegates path construction to
`resolve_periodic_path`; passing `period` through is the whole change. No sigil
grammar or offset logic changes.

## Risks / Trade-offs

- **[Risk] Signature-change ripple.** `resolve_periodic_path`,
  `create_or_get_periodic_path`, and `Vault::ensure_target` are widely called
  (~10 `ensure_target` sites; ~16 `PeriodicPeriod` literals). Mechanical but
  broad. Mitigation: the compiler enumerates every site; the `Default` derive
  keeps literal edits to `..Default::default()`.
- **[Risk] Unexpected behavior change for daily `--date`/`--offset` templates.**
  Mitigation: documented in the proposal and `docs/config.md`, and asserted by a
  new integration test; daily `%Y-%m-%d` paths themselves are unchanged.
- **[Risk] Anchoring monthly/quarterly/yearly changes day-bearing formats.**
  Intended, but could surprise a user who deliberately wants a day-stamped
  period note. Mitigation: anchoring is the documented model; week-number and
  month-precision formats are unaffected.
- **[Risk] Non-Monday week start requires correct week-year tokens.** With
  `week_start = "sunday"`, `%G-W%V` is ISO-year/week and will not align with the
  Sunday-anchored date. Mitigation: document that Sunday-start users should use
  Sunday-compatible tokens (the anchor changes the date, not the token
  semantics); do not attempt to synthesise a Sunday week-number token in this
  change.

## Migration

No data migration. Existing weekly files named after their open day are simply
no longer the resolution target; users who want continuity can rename them (via
`ft notes rename`) to the Monday date. New notes resolve to the period start.

## Open Questions

None — the design is settled pending review. The date token note for
Sunday-start week numbers (Risk 4) is a documentation caveat, not a blocker.
