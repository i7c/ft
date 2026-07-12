## Context

The Tasks SearchView (`ft/src/tui/tabs/tasks/search.rs`) renders task rows with a fixed-width column scheme. Today the row is built span-by-span in `task_line`, with every span inheriting a per-row `row_style` (warm-brown `bg` when selected, `Modifier::DIM` for done/cancelled, plain otherwise). The `Task` model (`ft-core/src/task/mod.rs`) already carries `created: Option<NaiveDate>`, parsed from the ➕ emoji, and the emoji format already serializes it — so no model or parser work is needed. However, all four create sites (two in `search.rs`, one in `ft/src/tui/tabs/graph/tasks.rs`, one in `ft/src/cmd/tasks.rs`) pass `created: None`, so ft-created tasks currently never age. `dates::today()` is the established `FT_TODAY`-aware seam.

## Goals / Non-Goals

**Goals:**
- Stamp `created` on every create path so aging is meaningful for ft-created tasks.
- Surface task staleness in the Tasks SearchView via an age badge whose grey shade reflects fixed absolute age bands.
- Keep the badge self-contained (span-scoped `bg`) so it composes cleanly with the existing selected-row brown and done/cancelled `DIM`.

**Non-Goals:**
- Relative (cohort-normalized) aging — rejected during exploration: dancing colors under filtering, degenerate at small N, snapshot-fragile, and badly degraded by the current ~96% missing-`created` rate in fixtures. Absolute bands degrade gracefully ("no date → no shade").
- Aging visualization in the graph tab, Board, or Calendar views. (The band computation lives in `ft-core` so other views can adopt it later, but only the Tasks SearchView renders it now.)
- Configurable age thresholds or band count. The four bands are hardcoded constants; calibration is a future dial, not an architecture change.
- Aging as a sort key or a query DSL predicate. Aging is render-only.
- Backfilling `created` for pre-existing tasks that lack one.

## Decisions

### Decision: Absolute banded aging, not relative

Age is classified by fixed thresholds (`Fresh` 0–3d, `Aging` 4–10d, `Stale` 11–30d, `Rotten` >30d) computed purely from `(created, today)`. Rejected alternative: relative/cohort-normalized shading (lightest = youngest in view, darkest = oldest). Relative's one genuine win — maximizing contrast within a due-window cohort — was outweighed by instability under filtering, degeneracy at small N, snapshot brittleness, and especially its behavior given sparse `created` coverage (bands computed over 1–3 points are noise). Absolute is stable, learnable, deterministic in `insta` snapshots, and degrades cleanly when `created` is `None`.

### Decision: Band computation lives in `ft-core`, not in the TUI

A pure function `age_band(created: Option<NaiveDate>, today: NaiveDate) -> AgeBand` is added to `ft-core` (e.g. `ft-core/src/task/aging.rs` or alongside the task model). Rationale: it's pure logic with no rendering concerns, the CLI or future views can reuse it, and it can be unit-tested in isolation without a `TestBackend`. The TUI maps `AgeBand → Color` via the palette; the band enum itself knows nothing about colors.

### Decision: Age badge is a span-scoped background, not a row background

The badge is a single fixed-width `Span` that carries its own `.bg(grey_shade)`. Because `task_line` builds each span with `row_style` individually (rather than setting a line-level `bg`), a span-level `bg` tints only that span's cells. This sidesteps the entire precedence tangle with the selected-row brown and the done/cancelled `DIM` modifier — they apply to *other* spans, the badge applies to *its* span, and ratatui composes them cell-by-cell with no conflict.

### Decision: Badge keeps its grey on the selected row

On the cursor-selected row, the badge span retains its grey shade rather than inheriting the warm-brown selection background. Selecting a row is an inspection act; age is part of what's being inspected, so hiding it under brown would defeat the feature precisely when attention is highest. This is implemented by giving the badge span a style that sets `bg` explicitly to the band's grey (not deriving from `row_style`), while its `fg` stays a single fixed readable color.

### Decision: No `created` → blank badge, same width

Tasks with `created = None` render an empty badge cell with no background, padded to the same fixed width as a populated badge. This preserves column alignment and treats "unknown age" as a first-class state (the common case today) rather than a special-case layout shift.

### Decision: Stamp `created: Some(today)` at all four create sites

Rather than defaulting inside `CreateInput` or `create_task` (which would make the field's presence implicit and break the "caller decides" clarity of the current struct), each of the four call sites explicitly sets `created: Some(dates::today())`. This keeps the prerequisite fix a localized, greppable change and avoids changing `CreateInput`'s semantics. `dates::today()` is used (not `Local::now().date_naive()` directly) to stay `FT_TODAY`-aware for tests.

**Alternative considered:** default `created` inside `ops::create_task` when `None`. Rejected because it would silently change behavior for any future caller and obscure which paths stamp the date; explicit call-site setting is clearer and matches the existing pattern where each site already sets the fields it cares about.

### Decision: Fixed column budget grows by the badge width

Current fixed budget is `cursor(2) + status(2) + pri(4) + due(13) + scheduled(13) = 34`, with description flexing. The age badge adds a fixed-width column (target ~5 chars: ` 4d `, ` 30d`, etc.), growing the fixed budget to ~39. The `desc_width` computation in `render_list` (`inner_width.saturating_sub(34)`) is updated to subtract the new total. Scheduled already renders conditionally; age renders unconditionally (blank when `None`) to keep alignment stable.

## Risks / Trade-offs

- **[Snapshot churn]** Adding a column changes every Tasks-view `insta` snapshot. → Mitigation: regenerate all affected snapshots in one commit; the new column is the only diff. Bound to `FT_TODAY` in tests so band values are deterministic.
- **[Existing create tests assert the written line]** Tests that check the exact serialized task line will now expect a `➕ <today>` segment. → Mitigation: update those assertions; use `FT_TODAY` in tests so the date is stable.
- **[Band thresholds are guesses]** 0–3/4–10/11–30/>30 are reasonable but unvalidated against real vault age distributions. → Mitigation: thresholds are named constants in one place; recalibration is a one-line change per band, not an architecture change. If real-world data shows everything clusters in one band, adjust thresholds rather than revisiting the scheme.
- **[Column-width pressure on narrow terminals]** Adding ~5 fixed chars squeezes the flexing description column. → Mitigation: `desc_width` already floors at 16; verify the worst case (narrow terminal + long description + all columns populated) still truncates cleanly with `…`.
- **[Grey shades may be hard to distinguish on some terminals]** Terminal color fidelity varies. → Mitigation: the badge *text* also carries the age (`4d`, `30d`), so the information is available even when the grey shade is imperceptible. Shade is reinforcement, not the sole channel.
- **[Tasks created via direct file edit still lack `created`]** Only ft-mediated creates get stamped; hand-edited or externally-imported tasks without ➕ stay `Unknown`. → Accepted: this is consistent with how all task fields work (ft sets what it knows on create; manual edits are free to omit). Backfilling is an explicit Non-Goal.
