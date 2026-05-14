---
id: 010
name: notes-periodic
title: Notes: periodic notes (daily/weekly/monthly/quarterly/yearly)
status: implementing
created: 2026-05-14
updated: 2026-05-14
---

# Notes: periodic notes (daily/weekly/monthly/quarterly/yearly)

## Goal
Open (and create-if-missing) periodic notes — daily, weekly, monthly,
quarterly, yearly — with one or two keystrokes from the TUI Notes tab and
one CLI subcommand. Inspired by Obsidian's Periodic Notes plugin but
implemented as a self-contained, explicit configuration: no plugin readers,
no moment.js syntax, no auto-discovery.

Three call sites:

1. **TUI Notes tab idle keymap** — `t` opens today's daily note; `p` enters
   a brief leader mode that takes a second key (`d`/`w`/`m`/`q`/`y`) to
   open the corresponding periodic note for the current period.
2. **CLI — `ft notes periodic <period>`** with `--date`/`--offset` for
   navigating to a specific occurrence. `ft notes today` aliases
   `ft notes periodic daily`.
3. **Shared library — `ft_core::periodic`** resolves the absolute path for
   a given (period, date), renders the configured template (or `# <title>\n`
   if no template is set) into a new file when one doesn't exist, and
   leaves opening to the caller.

At the same time, **remove** the existing `[daily_notes]` config block, the
Obsidian core/periodic-notes plugin readers in `ft-core/src/daily.rs`, and
the moment.js → chrono `translate_format` helper. The new configuration
uses chrono `%`-tokens everywhere, mirroring how MiniJinja templates already
format dates in plan 009.

## Motivation and Context
The vault already accumulates content along periodic axes — daily journal
entries under `journal/%Y/%Y-%m-%d.md`, weekly review notes, quarterly
goal check-ins. Today, opening any of these requires either:

- Switching to Obsidian, where the Periodic Notes plugin handles the
  "open or create" flow with templates;
- Or typing the full path into `ft notes open <query>` and hand-creating
  the file from a template if it doesn't exist yet.

Neither fits the ergonomic baseline ft has built for the rest of the Notes
tab (`o` open, `m` move, `c`/`C` create). A single keystroke for today's
daily and a two-key chord for the others is the natural extension.

**Why drop the plugin readers and moment.js tokens at the same time:** the
existing `ft-core/src/daily.rs` carries a `source` enum with three
variants (`core`, `periodic-notes`, `explicit`), two JSON readers for
Obsidian's plugin config files, and a 100-line `translate_format` that
converts moment tokens to chrono. None of that is wired into any CLI
today — `daily.rs` is dead code with tests. As we'd already have to
extend it (weekly/monthly/quarterly/yearly path resolution, plus per-
period template config), the simpler move is to delete the dead code and
rebuild the surface around one explicit configuration scheme with chrono
tokens. Users who keep Obsidian's Periodic Notes plugin alongside ft just
configure both with the same format strings (chrono in ft, moment.js in
the plugin) — same path on disk, no auto-sync, no surprise diff.

This shrinks `ft-core/src/daily.rs` from ~480 lines to a leaner
`ft-core/src/periodic.rs` that handles all five periods, including a
small `%q`/`%Q` pre-processor extension for quarter substitution (chrono's
strftime has no native quarter token).

## Acceptance Criteria

### Library — `ft_core::periodic` (replaces `ft_core::daily`)

- [ ] New module `ft_core::periodic`. Old `ft_core::daily` module is
      deleted entirely (file + `pub mod daily` line in lib.rs + its 20+
      unit tests + the `DailyError`/`DailySource`/`translate_format`
      surface). No back-compat shim — the previous module wasn't wired
      into any user-facing surface, so there's nothing to deprecate
      externally.
- [ ] `Period` enum:
      ```rust
      #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
      pub enum Period { Daily, Weekly, Monthly, Quarterly, Yearly }
      ```
      with `Period::as_str() -> &'static str` (`"daily"`/`"weekly"`/...)
      and `impl FromStr` that accepts both the long form and the short
      single-letter form (`"d"`, `"w"`, `"m"`, `"q"`, `"y"`).
- [ ] `Period::offset_date(date: NaiveDate, n: i32) -> Option<NaiveDate>`
      shifts a date by N period units:
      - `Daily` → `chrono::Days(N)`
      - `Weekly` → `chrono::Days(7*N)`
      - `Monthly` → `chrono::Months(N)` (handles month-end clamping —
        adding 1 month to Jan 31 produces Feb 28/29)
      - `Quarterly` → `chrono::Months(3*N)`
      - `Yearly` → calendar-year shift via `chrono::Months(12*N)`
      Returns `None` only when the shift overflows chrono's date range.
- [ ] `resolve_periodic_path(vault_root: &Path, cfg: &PeriodicPeriod, date: NaiveDate) -> Result<PathBuf>`
      formats the configured `path` and `format` against `date`, joins
      under `vault_root`, appends `.md`. Empty `path` is fine (note at
      vault root). Returns `Error::Periodic(...)` on format errors.
- [ ] `%q` and `%Q` quarter-token extension. Chrono's `strftime` has no
      native quarter token, so before delegating to chrono we run a
      pre-processor over the format string that substitutes:
      - `%q` → the quarter as a digit (`1`..=`4`)
      - `%Q` → `Q1`..`Q4`
      `%%q`/`%%Q` (escaped percent) pass through to chrono as literal
      `%q`/`%Q` after one pass. Unit-tested in isolation.
- [ ] `render_periodic_note(period_cfg: &PeriodicPeriod, vault_templates_dir: &Path, title: &str, today: NaiveDate, now: NaiveDateTime) -> Result<String>`
      renders the note body. When `period_cfg.template` is `None`, returns
      `format!("# {title}\n\n")` (the same blank stub the standalone `c`
      flow writes). When set, resolves the template path under the
      vault's templates dir (or as an absolute path if the value starts
      with `/`) and renders it via `ft_core::notes::template::render_path`.
- [ ] `create_or_get_periodic_path(vault_root: &Path, vault_templates_dir: &Path, cfg: &PeriodicPeriod, date: NaiveDate, today: NaiveDate, now: NaiveDateTime) -> Result<(PathBuf, bool)>`
      — high-level helper that:
      1. Computes the absolute path via `resolve_periodic_path`.
      2. If the file already exists → return `(path, false)`.
      3. Otherwise, render via `render_periodic_note`, `create_dir_all`
         the parent, `fs::write_atomic` the contents → return `(path, true)`.
      The `bool` lets callers tailor toasts/messages ("Opened…" vs
      "Created…"). Title is the filename stem (basename minus `.md`).
- [ ] Errors collected into a new `Error::Periodic(String)` variant on
      `ft_core::error::Error` (mirrors the existing `Error::Notes`
      pattern); messages are user-readable ("period 'weekly' is not
      configured — add `[periodic_notes.weekly]` to your config").
- [ ] Unit tests:
      - `Period::FromStr` happy path (long + short forms) and error path.
      - `Period::offset_date` for each variant, including month-end
        clamp (Jan 31 + 1 month → Feb 28/29) and quarter shift
        (May → Aug).
      - `%q`/`%Q` pre-processor: every quarter, escaped `%%q`, mixed
        with other tokens.
      - `resolve_periodic_path` for each period using a representative
        config (see Technical Notes table).
      - `render_periodic_note` with no template (returns `# <title>\n\n`)
        and with a template (renders via MiniJinja).
      - `create_or_get_periodic_path` round-trip: first call creates,
        second call returns `(_, false)` without rewriting.

### Library — `ft_core::config`

- [ ] Remove `DailyNotes` and `DailySource` types from `config.rs`.
      Remove the `daily_notes: DailyNotes` field from `Config`. The
      config block `[daily_notes]` is no longer accepted; thanks to
      `deny_unknown_fields` already on `Config`, presence of the old
      block produces a typo-rejection error pointing at the new
      `[periodic_notes]` shape.
- [ ] New `PeriodicNotes` struct on `Config`:
      ```rust
      #[serde(default)]
      pub periodic_notes: PeriodicNotes,

      #[derive(Debug, Default, Clone, Deserialize, Serialize)]
      #[serde(deny_unknown_fields)]
      pub struct PeriodicNotes {
          pub daily: Option<PeriodicPeriod>,
          pub weekly: Option<PeriodicPeriod>,
          pub monthly: Option<PeriodicPeriod>,
          pub quarterly: Option<PeriodicPeriod>,
          pub yearly: Option<PeriodicPeriod>,
      }

      #[derive(Debug, Clone, Deserialize, Serialize)]
      #[serde(deny_unknown_fields)]
      pub struct PeriodicPeriod {
          /// Folder pattern (chrono strftime + %q/%Q). Vault-relative.
          /// Empty string = vault root.
          pub path: String,
          /// Filename pattern (without `.md`). Required.
          pub format: String,
          /// Template path under [notes].templates_dir (or absolute).
          /// When unset, the new note's body is `# <title>\n\n`.
          pub template: Option<String>,
      }
      ```
- [ ] Helper `PeriodicNotes::for_period(period) -> Option<&PeriodicPeriod>`
      returns the per-period config or `None`. The `None` case is the
      caller's "not configured" signal (CLI exits 2, TUI shows a toast).
- [ ] Unit tests: each period parses cleanly; old `[daily_notes]` block
      is rejected with a helpful error mentioning `[periodic_notes.daily]`;
      missing periods deserialize to `None`; typos within a `[periodic_notes.*]`
      block are rejected (`deny_unknown_fields`).
- [ ] All references to `config.daily_notes` are removed from the
      workspace (grep-clean). The `daily.rs` module's references to
      `DailyNotes`/`DailySource` go with it.

### CLI — `ft notes periodic` + `ft notes today`

- [ ] New subcommand `ft notes periodic <PERIOD>` registered on the
      existing `notes` subcommand group. `<PERIOD>` is one of
      `daily | weekly | monthly | quarterly | yearly` (also accepts
      single-letter short forms `d|w|m|q|y` via `Period::FromStr`).
- [ ] Flags:
      - `--date YYYY-MM-DD` — overrides today (chrono ISO format).
        Honors `FT_TODAY` as the implicit default. Errors on unparseable
        input.
      - `--offset N` — shifts the target date by N period units (e.g.,
        `--offset -1` on `weekly` is "last week"). Composable with
        `--date` (shifts from the explicit date).
      - `--no-open` — suppress opening the file in `$EDITOR`.
      - `--obsidian` — open via `obsidian://open?vault=...&file=...`
        instead of `$EDITOR`. Honors `FT_OBSIDIAN_DRY_RUN=1`.
      - `--editor <BIN>` — overrides `$EDITOR`.
      - `--vault-name <NAME>` — overrides the vault basename in the
        Obsidian URL (same as `ft notes open`/`create`).
- [ ] If `[periodic_notes.<period>]` is unset, exit 2 with a message
      pointing at the config key to add.
- [ ] On success:
      - If file existed: open via `$EDITOR` (line 1) and print
        `Opened <vault-rel>` to stdout.
      - If file was created: same, with `Created <vault-rel>`.
      - With `--obsidian`: hand off to the URL handler instead of
        spawning `$EDITOR`.
- [ ] Exit codes: 0 success; 2 bad args / missing config / template
      render error / unparseable `--date`.
- [ ] `ft notes today [--date YYYY-MM-DD] [--no-open] [--obsidian] ...`
      is an alias for `ft notes periodic daily` (same flags except no
      `--offset` is accepted on the alias — keep it dead-simple; users
      who want offsets type out `ft notes periodic daily --offset N`).
- [ ] Integration tests in `ft/tests/notes_periodic.rs` against per-test
      `assert_fs::TempDir` vaults (same pattern as `notes_create.rs`):
      - Daily: file missing → created with blank `# <title>\n\n`,
        opened, prints `Created`; second invocation opens without
        rewriting, prints `Opened`.
      - Daily with `template = "daily"`: rendered against the
        `templates-ft/daily.md` fixture, matches a golden snapshot.
      - Weekly: `--offset -1` resolves to the previous ISO week.
      - Monthly: `--date 2026-01-31 --offset 1` resolves to Feb 2026
        (month-end clamp).
      - Quarterly: format `"%Y-Q%q"` produces `2026-Q2.md` for May.
      - Yearly: minimal config (empty `path`, `format = "%Y"`).
      - Missing config: `ft notes periodic weekly` with no
        `[periodic_notes.weekly]` exits 2 with the hinted error.
      - Unparseable `--date "garbage"` exits 2.
      - `--no-open` skips the editor; default invokes it.
      - `--obsidian` with `FT_OBSIDIAN_DRY_RUN=1` prints the URL +
        skips the editor.
      - `--vault-name "MyVault"` overrides the URL `vault=` param.
      - `ft notes today` reaches the same code as
        `ft notes periodic daily` (covered by one test).

### TUI — Notes tab `t` and `p` leader

- [ ] `NotesState::PeriodicLeader` variant: a transient modal state
      entered by pressing `p` from idle. Renders an overlay (or footer
      hint, whichever reads cleanest in the existing notes-tab view
      layout) listing
      `d daily · w weekly · m monthly · q quarterly · y yearly · Esc cancel`.
      Any key not in `{d, w, m, q, y, p, Esc}` cancels back to idle with
      no toast. (`p` re-entry just stays in the leader — harmless.)
      Duration is not time-bounded; the user dismisses with `Esc` (or
      any other key) when they want out.
- [ ] **`t` keybind (idle):** synonym for `p` + `d`. Fires the
      periodic-open flow for `Period::Daily` directly.
- [ ] **`p` keybind (idle):** transitions to `PeriodicLeader`.
- [ ] **Second key (`d|w|m|q|y` in `PeriodicLeader`):** fires the
      periodic-open flow for the corresponding period and returns to
      idle. Flow:
      1. Read `config.periodic_notes.for_period(period)`.
      2. If `None` → toast `<period> not configured` (style: warning),
         return to idle.
      3. Compute the path via `create_or_get_periodic_path` (today's
         date; `FT_TODAY` honored for tests).
      4. Queue `AppRequest::OpenInEditor { path, line: 1 }`.
      5. Toast `Opened <vault-rel>` or `Created <vault-rel>` per the
         `bool` returned by the helper. Style: info on Opened, success
         on Created.
- [ ] **Errors during flow** (path-format error, template render error,
      write error) surface as an error toast and return to idle without
      partial state.
- [ ] `?` help overlay (idle) updated to list `t` and the `p<letter>`
      chord. The existing `IDLE_KEYS` constant covers both the help
      overlay and the idle keymap footer (per the notes-create plan's
      session-4 outcome), so one update covers both.
- [ ] Snapshot tests:
      - `notes_idle_with_periodic_keys.snap` — the new idle keymap row
        (replaces the existing `notes_idle_80x24.snap` after
        re-acceptance — only diff is the added `t` and `p` rows).
      - `notes_periodic_leader_80x24.snap` — the leader overlay with
        the five period choices.
- [ ] Behavior tests in `tui::tests`:
      - `t` from idle creates today's daily on a vault with
        `[periodic_notes.daily]` and queues `OpenInEditor`.
      - `t` from idle on a vault *without* `[periodic_notes.daily]`
        configured → toast `daily not configured`, no editor request.
      - `p` enters leader; `Esc` exits back to idle.
      - `p` then `d` is equivalent to `t` (same file created, same
        request queued).
      - `p` then `w` creates the weekly note.
      - `p` then `m`/`q`/`y` likewise.
      - `p` then `x` (unknown letter) cancels back to idle silently.
      - File already exists → `OpenInEditor` queued without re-write;
        toast is `Opened …`, not `Created …` (asserted on the
        `ToastStyle` and message prefix).

### Documentation

- [ ] Module doc comment on `ft_core::periodic` documenting the
      configuration shape (with a worked example for each period), the
      `%q`/`%Q` extension, and the "no template = blank" convention.
- [ ] `?` help overlay text updated as described above.
- [ ] No vault-side README update is required — the `templates-ft/`
      README already documents the template engine; periodic notes are
      "just another caller" of it.

## Technical Notes

- **Why chrono tokens everywhere.** Plan 009 already uses chrono
  `strftime` in template bodies. Using the same token grammar for path
  and filename patterns keeps the surface coherent: a user copy-pasting
  `%Y-%m-%d` between a template's `{{ today | date(format="%Y-%m-%d") }}`
  and the config's `format = "%Y-%m-%d"` sees the same characters mean
  the same thing. The previous moment.js surface in `daily.rs` was a
  100-line `translate_format` with its own token table, reserved tokens
  for future moment.js parity (`Q`/`Qo`), and a `UnsupportedToken` error
  — all delete-on-sight under the simpler design.
- **Why drop the Obsidian plugin readers.** `daily.rs` had two JSON
  readers for Obsidian's `daily-notes.json` and `periodic-notes/data.json`
  files. Their value was zero-config for users running both Obsidian and
  ft, but the reverse cost was: a parallel schema to maintain whenever
  Obsidian's plugins evolve, two new failure modes (plugin not present,
  daily disabled), and code paths that aren't currently exercised by any
  user-facing surface. The simplification: ft owns its config, users
  who run both apps duplicate the path string between Obsidian's plugin
  config and `[periodic_notes.*]`. Worth the ~10 seconds of one-time
  setup.
- **Quarter token extension.** chrono's `strftime` has no quarter
  token. Rather than add a custom format engine, we pre-process the
  format string before handing it to chrono:
  ```text
  "%Y-Q%q" → chrono(date.replace_q(quarter)) → "2026-Q2"
  ```
  with `%q` = `(month - 1) / 3 + 1` as a digit, `%Q` = `Q{digit}`. We
  handle `%%q`/`%%Q` as the standard chrono escape for a literal
  percent + token character (matching how chrono itself handles `%%`).
  Implementation: a small loop that splits on `%`, substitutes the
  quarter tokens, re-joins, then hands off to chrono. Documented next
  to the function.
- **Representative configs for tests** (used in unit and integration
  tests):
  | Period    | path                    | format        | template (optional) |
  |-----------|-------------------------|---------------|---------------------|
  | daily     | `journal/%Y`            | `%Y-%m-%d`    | `daily`             |
  | weekly    | `journal/%Y`            | `%G-W%V`      | `weeks`             |
  | monthly   | `journal/%Y`            | `%Y-%m`       | (none)              |
  | quarterly | `journal/%Y`            | `%Y-Q%q`      | `quarterly`         |
  | yearly    | `journal`               | `%Y`          | (none)              |
- **Title for new periodic notes.** The filename basename — same rule
  the `c`/`C` create flow uses. For weekly with `format = "%G-W%V"`,
  the title is `2026-W19` (Mon..Sun of ISO week 19, 2026). The
  hand-ported `weeks.md` template already expects this format
  via `{{ title | parse_date(format="%G-W%V") | ... }}` — verify the
  template's parse pattern matches the config's format string at
  session-2 time and adjust the template if needed (this is a config
  authoring concern, not a code concern; the renderer just enforces
  internal consistency).
- **`weeks.md` template format mismatch.** The session-2 hand-port of
  `weeks.md` from plan 009 uses `{{ title | parse_date(format="%G Week %V") }}`
  to match the original Templater title `2026 Week 19`. If the user
  picks `format = "%G-W%V"` here, the template's parse format needs to
  change to match (or the config's format needs to change). We don't
  prescribe one — both are equally valid. The plan's representative
  config uses `%G-W%V` to keep filenames `.md`-friendly; the user can
  swap to `%G Week %V` in their `[periodic_notes.weekly]` if they
  prefer human-readable filenames. Either way, the template's
  parse-date format string must agree with the config's filename
  format string.
- **`FT_TODAY` propagation.** Both the CLI and the TUI build the
  `today`/`now` pair the same way as plan 009's create flow:
  if `FT_TODAY` is set, `today` is parsed from it and `now` is
  pinned to `today.and_hms_opt(0, 0, 0)`. Otherwise, `today =
  chrono::Local::now().date_naive()` and `now = chrono::Local::now().naive_local()`.
  This is the existing pattern in `ft/src/cmd/notes.rs` —
  `build_template_context`. The new periodic path can reuse the same
  helper (lift it into ft-core if a third caller emerges; for now,
  duplicate the four-line stanza into the periodic command and accept
  the small repetition).
- **No `--vault-name` on `ft notes today`.** Keeping the alias minimal:
  the same flags as `ft notes periodic daily` minus `--offset`. If a
  user needs more, they invoke the long form. (Specifically: `--no-open`,
  `--obsidian`, `--editor`, `--vault-name`, `--date` are all accepted on
  `today`; only `--offset` is denied. Document this in the help string.)
- **State machine fit.** `NotesState::PeriodicLeader` is a peer of
  `Idle`, `OpenPicking`, `Creating(...)`, `MoveSection(...)`. Key
  dispatch lives in a new `handle_periodic_leader_key`. Single
  transition target: either back to `Idle` (Esc / unknown key) or back
  to `Idle` after firing a side-effect (period chosen). The view layer
  renders a small centered modal — reuse the visual treatment of the
  collision prompt (`render_collision_prompt`-shaped widget) for
  consistency.
- **What we are not building.**
  - Navigation between adjacent periodic notes
    ("open next/previous weekly"). The `--offset` flag covers it for
    the CLI; the TUI would need a "currently-viewed periodic note" piece
    of state, which doesn't fit naturally with the editor-suspend model.
  - A periodic-notes archive or calendar view.
  - A "move to today's daily" affordance in the section-move flow.
  - Auto-creating an entire month of periodic notes (`ft notes periodic
    daily --range 2026-05-01..2026-05-31` or similar bulk operation).
  - Reverse migration: detecting an existing `[daily_notes]` block in a
    user's config and rewriting it to `[periodic_notes.daily]`. We hard-
    deprecate; users edit their config by hand once.

## Future (explicitly out of scope for this plan)

- Open-next / open-previous periodic note from the TUI.
- A `--range` flag for bulk creation of periodic notes.
- Section-move integration: "move the focused section to today's daily."
- An `ft notes periodic-list` command (or TUI view) showing the matrix
  of existing periodic notes by period × date.
- Auto-detecting and rewriting `[daily_notes]` blocks in user configs
  into `[periodic_notes.daily]` (one-time migration helper).
- Reading from Obsidian's Periodic Notes plugin config file as a
  zero-config source (intentionally removed — see Technical Notes).

## Sessions

### Session 1 · 2026-05-14 · done
**Goal:** Library — delete `ft_core::daily`, add `ft_core::periodic` with
`Period` enum, `Period::offset_date`, `%q`/`%Q` pre-processor,
`resolve_periodic_path`, `render_periodic_note`, and
`create_or_get_periodic_path`. Replace the `[daily_notes]` config block
with `[periodic_notes.*]` (`PeriodicNotes`/`PeriodicPeriod`). Unit-test
every helper. No CLI, no TUI yet.
**Outcome:** New `ft-core/src/periodic.rs` (~480 lines incl. tests). Public
surface: `Period` enum (Daily/Weekly/Monthly/Quarterly/Yearly), `Period::as_str`,
`Period::offset_date`, `impl FromStr` (long + short forms, case-insensitive),
`substitute_quarter_tokens` (`%q` → digit, `%Q` → `Q{digit}`, `%%q`/`%%Q`
pass-through), `resolve_periodic_path`, `render_periodic_note`,
`create_or_get_periodic_path`. New `Error::Periodic(String)` variant on
`ft_core::error`. The old `ft-core/src/daily.rs` (~480 lines), the
`DailyError`/`DailySource`/`translate_format` surface, and the two Obsidian-
plugin JSON readers are all deleted; `pub mod daily` removed from lib.rs.

Config refactor in `ft-core/src/config.rs`: removed `DailyNotes`/`DailySource`,
added `PeriodicNotes { daily, weekly, monthly, quarterly, yearly: Option<PeriodicPeriod> }`
and `PeriodicPeriod { path, format, template: Option<String> }`, both with
`#[serde(deny_unknown_fields)]`. The old `[daily_notes]` block is now
rejected by figment's unknown-field check, with the error message hinting
at the valid keys.

Scope grew beyond the library-only plan because `daily.rs` was actually
used by `Vault::resolve_target` (not dead code as I'd initially concluded).
Migrated all live callers in the same session to keep the workspace green:
- `Vault::resolve_target` now returns `crate::error::Result<PathBuf>` and
  reads from `periodic_notes.daily`; surfaces `Error::Periodic("no
  [periodic_notes.daily] configured — add it to your config or pass
  --file <PATH>")` when the daily period isn't configured.
- `ft/src/cmd/vault.rs` pretty-printer updated to enumerate all five
  periods of `[periodic_notes.*]` (showing `(not configured)` for unset
  ones) instead of the old `daily_notes.source` triple.
- `ft/src/tui/tests.rs` quickline_vault + target_picker_vault helpers
  write `[periodic_notes.daily]` blocks with chrono `%Y-%m-%d` tokens
  instead of moment.js `YYYY-MM-DD`.
- `ft/tests/tasks_create.rs` `vault_with_daily` rewritten to emit the new
  config block; four tests covering the deleted Obsidian-plugin readers
  (`periodic_notes_source_resolves_daily_path`,
  `periodic_notes_disabled_errors_with_hint`, `explicit_source_*`)
  collapsed into two new tests (`daily_path_with_year_token_resolves_per_year`,
  `old_daily_notes_block_rejected_with_config_error`); the
  `missing_daily_notes_config_explains_remedy` test became
  `missing_periodic_daily_config_explains_remedy` and now asserts the
  error mentions `periodic_notes.daily`.
- `tests/fixtures/tiny/.ft/config.toml` migrated to the new shape.

Implementation notes worth remembering for session 2:
- `Period::offset_date` for Monthly uses `chrono::Months`, which already
  clamps month-ends correctly (Jan 31 + 1 month → Feb 28/29; leap-year
  case verified too). Yearly leap-day clamp also works
  (2024-02-29 + 1 year → 2025-02-28).
- `format_with_quarter` is the small helper that runs the `%q`/`%Q`
  pre-processor before chrono `strftime`. `NaiveDate::format(...).to_string()`
  is the canonical surface; we trust it not to panic for the tokens we
  accept (the unsupported-token failure mode would surface as a literal
  `%X` in the output, which the user would see immediately).
- `create_or_get_periodic_path` returns `(path, created)`; the bool is
  what session 3's TUI uses to choose between the `Opened` and `Created`
  toast prefixes. `write_atomic` already handles parent-directory
  creation, so we don't need a separate `create_dir_all` call.
- The `render_periodic_note` template resolver joins the template name
  under `vault_templates_dir` and adds `.md` when no extension is given;
  this matches plan 009's `c`/`C` flow convention. Session 2's CLI
  template resolver should reuse this rather than duplicate.

Workspace state: `cargo test --workspace` → 359 ft-core lib + 199 ft
binary + every integration suite, all green. `cargo clippy --workspace
--all-targets -- -D warnings` clean. `cargo fmt --check` clean (one
autoformat pass collapsed two adjacent `assert_eq!` blocks in
`periodic::tests`). 11 new unit tests in `periodic::tests`, 6 new tests
in `config::tests` covering the new periodic shape, all passing.

The plan called for an `Error::Periodic` message hinting "add
`[periodic_notes.weekly]` to your config" for the not-configured case.
That ended up only wired up at the `Vault::resolve_target` call site
(the only place that asks for a period today); the library function
`resolve_periodic_path` itself just takes a `&PeriodicPeriod`, so the
"not configured" check happens at the caller. Sessions 2 and 3 will
each duplicate that check in their period-lookup paths — fine, it's a
two-liner and lifting it into ft-core would mean threading the whole
`PeriodicNotes` map through, which is more ceremony than savings. 

### Session 2 · 2026-05-14 · planned
**Goal:** CLI — `ft notes periodic <period>` with `--date`/`--offset`/
`--no-open`/`--obsidian`/`--editor`/`--vault-name`. `ft notes today` as
the alias. Integration tests against per-test `TempDir` vaults covering
every period, `--offset` behavior, missing-config exit-2, `--obsidian`
dry-run, file-exists-just-opens vs. file-missing-creates.
**Outcome:** 

### Session 3 · 2026-05-14 · planned
**Goal:** TUI — `t` (today) and `p<d|w|m|q|y>` leader chord on the Notes
tab idle state. `NotesState::PeriodicLeader` variant + view layer +
toast wiring. Snapshots: idle keymap with new keys, leader modal.
Behavior tests for every period chord, missing-config toast, file-
exists vs. file-created toast prefix, unknown-leader-key cancel.
**Outcome:** 
