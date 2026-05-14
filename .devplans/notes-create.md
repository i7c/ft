---
id: 009
name: notes-create
title: Notes: create new notes from templates
status: implementing
created: 2026-05-13
updated: 2026-05-13
---

# Notes: create new notes from templates

## Goal
Add a "create new note" capability to ft with three call sites and one
shared core:

1. **CLI** — `ft notes create <vault-relative-path> [--template P] [--title T] [--var K=V]... [--no-open] [--obsidian]`.
2. **TUI Notes tab** — `c` (blank) and `C` (template-picker) flows that
   prompt for a destination folder, filename, and (for `C`) a template,
   then open the new file in `$EDITOR`.
3. **TUI section-move flow** (plan 003) — a new sub-flow off step 3
   (`n` from the target picker) that creates the move's target on the
   fly via a template before the compose step.

Templates are rendered by a Rust-native engine (**MiniJinja**) with a
small, well-defined surface — standard variables (`title`, `today`,
`now`, `vars`) and a handful of chrono-backed filters (`date`,
`parse_date`, `add_days`, etc.). The user maintains an ft-specific
template folder (default `templates-ft/`) hand-ported from Obsidian's
Templater templates; the Obsidian Templater plugin keeps using its own
`templates/` folder and remains unaffected.

## Motivation and Context
Plan 003 shipped the Notes tab and the section-move flow but stopped at
moving content *between existing files*. Two friction cases remain:

- **Creating a fresh note from a template** still requires switching to
  Obsidian (to invoke the Templater plugin) or hand-copying a template
  file and editing the frontmatter. From the CLI there's no path at
  all.
- **Moving content into a new note** is currently a two-step dance:
  create the target by hand in Obsidian, then re-enter ft's
  section-move flow and pick it as target. The "wait, this should
  become its own project note" instinct is exactly when the friction
  bites.

Solving both with one core renderer is the simplification: the section-
move integration is a thin sub-flow that reuses the same template
picker, folder picker, filename prompt, and collision handling as the
standalone `c`/`C` flow, then hands the rendered content to the existing
compose state as an **in-memory** target (deferred creation — the file
isn't written until the move commits, so a cancelled flow leaves no
trace on disk).

**Why MiniJinja over a Templater-compatible parser:** Templater uses
Moment.js date tokens (`YYYY [Week] ww`) and a JS-flavored scripting
syntax (`<% tp.date.weekday("YYYY-MM-DD", 1, tp.file.title, "gggg [Week] ww") %>`)
that would require a token converter plus a partial expression
evaluator to render the templates this vault actually uses. MiniJinja
is a mature Rust-native engine that gives us strict-mode rendering,
custom filters, conditionals, and includes for free. The one-time cost
is hand-porting 16 templates from `templates/` to `templates-ft/`;
after that we never write Moment.js parsing code.

## Acceptance Criteria

### Library — `ft_core::notes::template`

- [x] New submodule `ft_core::notes::template` housing a MiniJinja-based
      renderer. Public surface:
      ```rust
      pub struct TemplateContext {
          pub title: String,
          pub today: NaiveDate,           // resolved from FT_TODAY when set
          pub now: NaiveDateTime,         // local time, FT_TODAY-overridable
          pub vars: BTreeMap<String, String>,
      }

      pub fn render(template_source: &str, ctx: &TemplateContext) -> Result<String>;
      pub fn render_path(template_path: &Path, ctx: &TemplateContext) -> Result<String>;
      ```
      `render_path` reads the file and delegates to `render`. Both
      return `Error::Notes(...)` on render failure with the engine's
      message preserved (line/column when MiniJinja provides it).
- [x] MiniJinja `Environment` is built in **strict undefined mode**
      (`UndefinedBehavior::Strict`) so a typo like `{{ titel }}` errors
      instead of silently emitting an empty string. Autoescape is
      disabled (we render Markdown, not HTML). `keep_trailing_newline`
      is enabled so `# {{ title }}\n` templates produce output ending
      in the expected newline.
- [x] Built-in globals exposed to every template:
      - `title: String` — the new file's basename without `.md`.
      - `today: NaiveDate` — local today (or `FT_TODAY`).
      - `now: NaiveDateTime` — local now (or `FT_TODAY` at 00:00:00 when
        the override is set, for deterministic snapshots).
      - `vars: Map<String, String>` — caller-supplied custom variables
        (CLI `--var K=V`, TUI prompt; empty map if none).
- [x] Custom filters registered on the env, all chrono-backed:
      - `date(value, format="...")` — formats a `NaiveDate` or
        `NaiveDateTime` with a chrono `strftime` format string.
        Default format (no argument) is `"%Y-%m-%d"`.
      - `parse_date(value, format="...")` — parses a string into
        `NaiveDate` using a chrono format. Used to pull a date out of
        the new file's title for week/quarter dailies.
      - `add_days(value, n)` / `add_weeks(value, n)` / `add_months(value, n)` —
        arithmetic on `NaiveDate`. Negative `n` shifts backwards.
      - `weekday_of(value, n)` — given a `NaiveDate` `value` and `n` in
        1..=7 (Mon..Sun, ISO), returns the `NaiveDate` of that weekday
        in the same ISO week. Replaces the `tp.date.weekday(...)`
        chain used in `weeks.md`.
- [x] Filter errors surface as render errors with a useful message
      (e.g. `"parse_date: could not parse \"2026 Q2\" with format \"%G Week %V\""`).
- [x] Unit tests covering variable substitution, each filter happy +
      error path, strict undefined behavior, and the
      `parse_date → weekday_of → date` chain that the `weeks.md` port
      will use. End-to-end golden snapshots for the 16 hand-ported
      templates land in session 2.

### Library — `ft_core::notes` (section-move integration)

- [ ] `Composing` (the section-move state from plan 003) gains a
      `target_is_new: bool` field. When true, `commit_move` uses the
      in-memory `target_content` as input to `move_sections` and skips
      any "target changed on disk" check; `write_pair` writes the
      target for the first time. When false, behavior is unchanged.
- [ ] No new public function on `ft_core::notes` — the section-move
      integration is purely a state-shape addition. The template render
      and folder/filename/collision plumbing lives at the call-site
      layer (CLI module + TUI tab).
- [ ] Update the `Composing` docstring to spell out the
      `target_is_new=true` semantics (no target-freshness check;
      `write_pair` creates rather than overwrites).

### Library — `ft_core::config` (templates folder)

- [ ] Extend the vault config (`ft_core::config::VaultConfig` or
      equivalent — check current shape) with:
      ```toml
      [notes]
      templates_dir = "templates-ft"   # default; vault-relative
      ```
      Resolved against the vault root at read time.
- [ ] Helper `Vault::templates_dir() -> PathBuf` that joins the vault
      root with the configured (or default) templates dir. Existence
      is **not** required — a missing folder is fine; template pickers
      just show an empty list.
- [ ] Unit test for config parsing: default, override, malformed value.

### CLI — `ft notes create`

- [ ] Subcommand `ft notes create <PATH> [...flags]` registered on the
      existing `notes` subcommand group.
- [ ] `<PATH>` is a vault-relative path. If it doesn't end in `.md`,
      `.md` is appended. If it contains `/` and intermediate folders
      don't exist, they're created (analogous to `mkdir -p`).
- [ ] `--template <PATH>` — template source path. Resolution order:
      (1) absolute path used as-is, (2) path relative to the configured
      templates folder, (3) path relative to CWD. Missing template
      exits 2.
- [ ] `--title <TEXT>` — overrides the auto-derived title (basename
      without `.md`). Useful when the on-disk filename differs from
      the heading text the template emits.
- [ ] `--var <KEY=VAL>` — repeatable. Each entry populates
      `ctx.vars[KEY] = VAL`. Templates that reference an unset var
      error out under strict-undefined mode; that's the intended UX
      ("you forgot to pass `--var name=...`").
- [ ] `--obsidian` — after creating, also fire the
      `obsidian://open?vault=...&file=...` URL (reuses
      `ft_core::notes::obsidian_url`). Honors `FT_OBSIDIAN_DRY_RUN=1`.
- [ ] `--no-open` — suppress the default behavior of opening the new
      file in `$EDITOR`. Default is open-after-create (the user
      confirmed (a) on question 8).
- [ ] `--editor <BIN>` — overrides `$EDITOR` for this invocation
      (mirrors `ft notes open --editor`).
- [ ] Collision handling: if `<PATH>` already exists, exit 2 with a
      message — the CLI does not prompt. The 3-way prompt
      (overwrite/use-existing/cancel) is a TUI affordance only; in
      script contexts the caller passes `--force` to overwrite or
      uses `ft notes open` to open an existing file.
- [ ] `--force` — opt-in to overwriting an existing target.
- [ ] Exit codes: 0 success, 2 bad args / render error / collision
      without `--force` / missing template.
- [ ] Integration tests (against per-test `assert_fs::TempDir` vaults,
      same pattern as `notes_open` / `notes_move_section`):
      - Blank create (no `--template`): writes a file with just
        `# <title>\n`, exits 0, opens via shim editor.
      - Create with `--template templates-ft/proj.md` against a fixture
        vault that contains the hand-ported template; assert rendered
        output matches a golden.
      - `--var name=quick` populates `{{ vars.name }}` in the
        `quick-add` template.
      - Template referencing a missing var errors with a useful message
        and exits 2.
      - Path normalization: `notes/foo` becomes `notes/foo.md`;
        intermediate dirs are created.
      - Collision without `--force` exits 2; with `--force` overwrites.
      - `--no-open` does not invoke the shim editor; `--obsidian`
        prints the URL when `FT_OBSIDIAN_DRY_RUN=1`.
      - Missing `--template` path exits 2 with a clear message.

### TUI — Notes tab `c` (blank) and `C` (template) create flows

- [ ] `NotesState` gains a `Creating(CreateState)` variant. `CreateState`
      is a small state machine:
      ```rust
      enum CreateState {
          TemplatePicking { /* only on the C path */ },
          FolderPicking { template: Option<TemplatePick> },
          FilenamePrompt { template, folder, buf: EditBuffer, vars_so_far },
          VarPrompt { template, folder, filename, var_key, buf, remaining_keys, vars_so_far },
          CollisionPrompt { template, folder, filename, vars, target_path, choice: CollisionChoice },
      }
      ```
      `CollisionChoice` tracks the focused option in a 3-way
      menu (`Overwrite | UseExisting | Cancel`).
- [ ] **`c` keybind (idle Notes tab):** transitions to
      `CreateState::FolderPicking { template: None }`. No template;
      the file is written with just `# <title>\n` followed by a single
      blank line.
- [ ] **`C` keybind (idle Notes tab):** transitions to
      `CreateState::TemplatePicking`. A fuzzy picker over the
      configured templates dir (recursive; only `.md` files). `Enter`
      advances to `FolderPicking`. `Esc` returns to idle.
- [ ] **Folder picking:** a fuzzy picker over existing folders in the
      vault (vault root + every subdirectory, excluding the templates
      dir and any `.obsidian/` paths). `Enter` advances to
      `FilenamePrompt`. `Esc` returns to the previous step
      (TemplatePicking on the `C` path, or idle on the `c` path).
- [ ] **Filename prompt:** an inline `EditBuffer` (reuse
      `ft/src/tui/widgets/edit_buffer.rs`) with a header showing the
      resolved folder. `Enter` commits the filename. Validation:
      non-empty after trimming; no path separators (`/`, `\`); `.md`
      auto-appended when missing. Invalid input keeps the prompt open
      with a toast.
- [ ] **Var prompts (template path only):** after the filename commits,
      ft parses the template source for variable references that are
      neither built-ins (`title`, `today`, `now`) nor already provided.
      For each unknown `vars.KEY` reference, the user is prompted in
      sequence with a single-line `EditBuffer` titled `KEY = `. Empty
      input is allowed (the template author may have intended an
      optional value); the user can also press `Esc` on any var prompt
      to cancel the entire create flow. Prompts are dispatched in
      stable order (first appearance in the template source).
- [ ] **Variable discovery:** implemented by walking MiniJinja's parsed
      AST for the template and collecting `Var` nodes whose path
      starts with `vars.`. Fallback if AST inspection isn't ergonomic:
      a regex pass for `\{\{\s*vars\.([a-zA-Z_][a-zA-Z0-9_]*)` over
      the template source. Either is acceptable — author's call at
      implementation time, but document the choice in the module doc.
- [ ] **Collision prompt:** if the resolved target path exists, after
      the last var prompt (or immediately after filename for the
      blank path), show a centered 3-way prompt:
      ```
      target already exists: <vault-rel path>
      [O]verwrite   [U]se existing (open in editor)   [C]ancel
      ```
      Keys: `o`/`O` → overwrite, `u`/`U` → open existing in `$EDITOR`
      via `AppRequest::OpenInEditor`, `c`/`C`/`Esc` → cancel back to
      filename prompt. `←/→` + `Enter` also work for mouse-free
      navigation. (On the `c`/`C` standalone flows; the section-move
      flow has its own collision branching, see below.)
- [ ] **Commit (blank or template, no collision OR overwrite chosen):**
      build the `TemplateContext`, render (or use `# <title>\n\n` for
      blank), write atomically via `ft_core::fs::write_atomic`,
      emit `AppRequest::OpenInEditor { path, line: 1 }`, toast
      `Created <vault-rel path>`, return to idle.
- [ ] **Step indicators in the footer**, e.g.
      `1/4 template · 2/4 folder · 3/4 filename · 4/4 vars · collision`
      so the user always knows where they are. Step count varies (the
      `c` path skips template selection and var prompts).
- [ ] `?` help overlay (idle state) updated to mention `c` and `C`.
- [ ] Snapshot tests:
      - `notes_create_template_picker.snap`
      - `notes_create_folder_picker.snap`
      - `notes_create_filename_prompt.snap` (with a partial filename
        typed)
      - `notes_create_var_prompt.snap` (with one var prompt visible)
      - `notes_create_collision_prompt.snap`
- [ ] Behavior tests in `tui::tests`:
      - `c` from idle opens folder picker (no template step).
      - `C` from idle opens template picker.
      - End-to-end blank create writes the file and queues
        `OpenInEditor`.
      - End-to-end template create against a fixture template writes
        the rendered output to disk.
      - Var prompt sequence: a template referencing `vars.a` and
        `vars.b` prompts for both in order.
      - Collision prompt: `O` overwrites, `U` queues `OpenInEditor` on
        the existing file (no write), `C`/`Esc` cancels back to
        filename prompt.
      - Filename validation: empty, with `/`, missing `.md` (auto-
        appended).
      - Esc at every step returns to the previous step (folder → idle
        on `c` path, folder → template on `C` path, etc.).

### TUI — section-move flow integration (plan 003 step 3)

- [ ] In `TargetPicking`, register a new keybind **`n`** (mnemonic:
      *new*) alongside the existing picker keys. When pressed, the
      flow transitions into a `NewTargetCreating(NewTargetState)`
      sub-state of `SectionMoveState`:
      ```rust
      enum NewTargetState {
          TemplatePicking { clipboard, allow_blank: bool },
          FolderPicking { clipboard, template: Option<TemplatePick> },
          FilenamePrompt { clipboard, template, folder, buf },
          VarPrompt { clipboard, template, folder, filename, var_key, buf, remaining, vars_so_far },
          CollisionPrompt { clipboard, template, folder, filename, vars, target_path, existing_content, choice },
      }
      ```
      The shape mirrors `CreateState` but threads `clipboard` through
      every variant — at the end of the sub-flow we land in
      `Composing { ..., target_is_new: <bool>, target_content: <String> }`
      with the in-memory rendered content.
- [ ] **Template-picking semantics in this sub-flow:** unlike standalone
      `C` (template required), the section-move sub-flow lets the
      user pick "(no template / blank)" as the first row so they can
      drop their clipboard into a fresh file with just `# <title>\n`.
      `allow_blank: true` controls this — keeps the type generic enough
      that we could reuse it for a future `c`-from-idle template
      picker variant if we ever want one.
- [ ] **No file is written during the sub-flow.** When the user
      finishes the filename prompt (and any var prompts), ft renders
      the template into a `target_content: String` and runs the
      collision check by `stat`'ing the path:
      - If the path doesn't exist → transition straight to `Composing`
        with `target_content = <rendered>`, `target_is_new = true`,
        `target_headings = extract_headings(&target_content)`.
      - If it does exist → enter `CollisionPrompt`. The three choices:
        - **Overwrite** → `target_content = <rendered>`,
          `target_is_new = false` (the file exists; `write_pair` will
          overwrite at commit). Note: `target_is_new` is really a
          rename of "did we render this from a template?" — the
          relevant invariant is that the rendered content is the
          source of truth, not whatever is on disk. We don't read the
          existing file's content; the user picked Overwrite.
        - **Use existing** → set `target_content = fs::read_to_string(path)`,
          `target_is_new = false`. This is the normal target-picker
          path; the template render is discarded.
        - **Cancel** → return to `FilenamePrompt` with the buffer
          preserved.
- [ ] **Deferred creation invariant:** until `commit_move` fires, no
      file is written. If the user cancels (`Esc` out of compose, `Esc`
      back through the sub-flow to the source picker, switching tabs,
      quitting), the filesystem is untouched. The rendered content
      lives entirely on `Composing.target_content`.
- [ ] **`commit_move` change:** when `target_is_new` is true, skip the
      "read target from disk" step and use the in-memory
      `target_content` as the input to `move_sections`. `write_pair`
      is unchanged — it writes target first via `fs::write_atomic`,
      which creates the file if absent.
- [ ] Step-3 footer in `TargetPicking` updated to advertise
      `n new target` alongside the existing keys.
- [ ] Snapshot tests:
      - `notes_move_new_target_template_picker.snap`
      - `notes_move_new_target_filename_prompt.snap`
      - `notes_move_new_target_collision_prompt.snap`
      - `notes_move_compose_new_target.snap` — compose view where the
        target is a freshly-rendered (in-memory) template file.
- [ ] Behavior tests in `tui::tests`:
      - `n` from `TargetPicking` enters the new-target sub-flow.
      - End-to-end: source pick → multi-select → `n` → template pick →
        folder → filename → compose → Enter; assert both source and
        the freshly-created target on disk match expected post-move
        content.
      - Cancel paths leave the filesystem untouched (no new file
        created on disk).
      - Collision-Overwrite path: target exists pre-flow; after commit
        the file has been replaced with the rendered template +
        moved sections.
      - Collision-UseExisting path: target's pre-existing content is
        preserved, with the moved sections inserted into it (per the
        compose layout).

### Templates — hand-ported set under `templates-ft/`

- [x] Create `~/git/fortytwo/templates-ft/` (in the user's vault, not
      in the ft repo) and hand-port each of the 16 templates from
      `~/git/fortytwo/templates/`. The originals remain untouched so
      Obsidian's Templater plugin continues to work.
- [x] Conversion rules (document as a short README in `templates-ft/`):
      - `{{title}}` → `{{ title }}`
      - `{{date}}` → `{{ today | date }}`
      - `{{date:YYYY-MM-DD}}` → `{{ today | date(format="%Y-%m-%d") }}`
      - `{{time:HHmm}}` → `{{ now | date(format="%H%M") }}`
      - `{{name}}` → `{{ vars.name }}`
      - `<% tp.file.title %>` → `{{ title }}`
      - `<% tp.date.now("YYYY-MM-DD", 0, tp.file.title) %>`
        → `{{ title | parse_date(format="%Y-%m-%d") | date(format="%Y-%m-%d") }}`
      - `<% tp.date.weekday("YYYY-MM-DD", 1, tp.file.title, "gggg [Week] ww") %>`
        → `{{ title | parse_date(format="%G Week %V") | weekday_of(1) | date(format="%Y-%m-%d") }}`
        (chrono treats non-`%` characters as literal — no `[...]` escape,
        unlike Moment.js)
- [x] Per-template golden snapshots under
      `ft-core/tests/snapshots/templates__*.snap` with a fixed
      `today/now = 2026-05-13` and a representative title per template
      (table in technical notes below).
- [x] CI sanity check: every file in `templates-ft/` renders cleanly
      against its representative title. Implemented as one
      parameterized test in `ft-core/tests/templates.rs` that
      enumerates the fixture templates dir and a sibling test
      (`fixture_set_matches_expectations`) that catches drift between
      the on-disk set and the title table.

### Documentation

- [x] Module doc comment on `ft_core::notes::template` enumerating the
      built-in globals, the registered filters, and the chrono format
      syntax (one short example per filter).
- [x] `templates-ft/README.md` (in the vault) documenting the
      conversion rules above, the variable surface, and a worked
      example showing the Templater → ft port for one of the more
      complex templates (e.g. `weeks.md`).
- [ ] Help overlay (`?` on Notes tab) updated to list `c`, `C`, and
      `n` (from `m`'s target picker step).

## Technical Notes

- **Engine choice — MiniJinja.** Smaller dep footprint than Tera, no
  built-in autoescape, ergonomic `add_filter`/`add_function` API,
  strict-undefined mode toggleable per-env. We add it as a workspace
  dep on `ft-core` (already used by ratatui for some templating; if
  not, it's a new dep). Pin to the latest 2.x at plan kick-off.
- **Why strict undefined.** Templates with typos like `{{ titel }}`
  emit empty strings under default mode, which silently produces
  broken Markdown. Strict mode turns them into render errors that the
  CLI/TUI can surface to the user. The cost — every variable must
  exist — is mitigated by the explicit `vars.*` namespace for custom
  prompts: `vars` is always a (possibly empty) map, so
  `{{ vars.maybe }}` errors only when `maybe` was supposed to be
  provided.
- **Why chrono `%`-tokens (not Moment).** chrono's `format::strftime`
  is the canonical Rust date-format surface; using its tokens means
  the renderer is a thin shim over chrono and the user doesn't pay a
  tax for being on the Rust side of the fence. Conversion table for
  the tokens that appear in the user's vault:
  | Moment | chrono | meaning |
  |---|---|---|
  | `YYYY` | `%Y` | 4-digit year |
  | `YY` | `%y` | 2-digit year |
  | `MM` | `%m` | 2-digit month |
  | `DD` | `%d` | 2-digit day |
  | `HH` | `%H` | 2-digit hour (24h) |
  | `mm` | `%M` | 2-digit minute |
  | `gggg` | `%G` | ISO-week-year |
  | `ww` | `%V` | ISO-week number |
- **`weekday_of` filter semantics.** The Moment expression
  `tp.date.weekday("YYYY-MM-DD", 1, tp.file.title, "gggg [Week] ww")`
  reads as: "parse the file title (e.g. `2026 Week 19`) as an ISO
  week, then return the date of Monday (day-of-week 1) in that
  week, formatted `YYYY-MM-DD`." `weekday_of(parsed_date, n)`
  returns the `NaiveDate` of weekday `n` (1=Mon..7=Sun, ISO) in the
  same ISO week as `parsed_date`. The `| date(format=...)` filter
  formats the result. The decomposition (parse → weekday_of → date)
  is verbose but each filter does one thing.
- **Template representative titles for golden snapshots:**
  | Template | Title |
  |---|---|
  | `daily.md` | `2026-05-13` |
  | `weeks.md` | `2026 Week 19` |
  | `quarterly.md` | `2026 Q2` |
  | `proj.md` | `Test Project` |
  | `wrklg.md` | `Test Worklog` |
  | `inbox.md` | `Inbox 2026-05-13` |
  | `quick-add.md` | `Quick` |
  | `new.md` | `Test Note` |
  | `father-watson.md` | `Father Watson 2026-05-13` |
  | `goal-checkin.md` | `Goal Checkin` |
  | `progress-checkin.md` | `Progress Checkin` |
  | `restaurant.md` | `Test Restaurant` |
  | `stry.md` | `Test Story` |
  | `tags.md` | `Tags` |
  | `tasks-in-this-path.md` | `Tasks Here` |
  | `travel.md` | `Travel 2026-05-13` |
  Adjustable at session 2 time once we read each template.
- **Templates dir default.** `templates-ft/` at vault root. Configurable
  via `[notes] templates_dir = "..."` (vault-relative). Missing dir is
  fine — pickers show an empty list. The Obsidian Templater plugin's
  own `templates/` folder stays in place; the two folders are
  independent.
- **Section-move deferred creation.** The compose state from plan 003
  already operates on `target_content: String`, so the only schema
  change is adding `target_is_new: bool`. The expensive part is the
  sub-flow state machine, not the compose-time wiring. `commit_move`
  branches once on `target_is_new`: when true, use in-memory content
  as input to `move_sections`; when false, re-read from disk as
  today.
- **Collision UX precedence.** Standalone `c`/`C` collision menu and
  section-move collision menu share the same key handling and visual
  treatment. Factor the prompt widget into `tui/widgets/collision_prompt.rs`
  if it gets reused; otherwise inline it. Author's call at session
  3/4.
- **Var prompts.** The variable-discovery pass (regex or AST walk)
  runs once per template selection. We do *not* re-parse after each
  prompt — the set of `vars.*` references is static in the template
  source. Built-in keys (`title`, `today`, `now`) and already-provided
  `vars.*` keys are excluded. Order is stable (first appearance in
  the template source).
- **Editor open after section-move new-target commit.** The
  section-move flow today does *not* open the target in `$EDITOR`
  after committing — it stays in the TUI with a toast. We preserve
  that behavior for new-target commits too. (If the user wants to
  edit the new file, they `o`-open it from idle.)
- **No `<% tp.file.cursor() %>` support.** Per user direction, cursor
  markers are skipped in v1. Any `tp.file.cursor()` in a template
  would render as a literal `tp.file.cursor()` text (or error under
  strict undefined — likely the latter since `tp` is not in scope).
- **Same-file move stays out of scope.** The section-move flow
  already errors on same-file picks; the new-target sub-flow can't
  reach that case (the target doesn't exist yet, by construction).
- **No CLI shape for moving into a fresh target.** A future
  `ft notes move-section --to-new <path> --template <p>` would be
  trivial to add (compose the existing `--to` resolution with the
  template renderer), but it's not in this plan. The standalone
  `ft notes create` + the existing `ft notes move-section --to ...`
  cover the scriptable case in two steps.

## Future (explicitly out of scope for this plan)

- CLI flag for moving into a freshly-templated target
  (`ft notes move-section --to-new <p> --template <t>`).
- Templater `<% tp.file.cursor() %>` support / post-render cursor
  placement.
- Templater compatibility shim (rendering original `templates/`
  Templater-syntax files directly).
- Auto-suggesting a template based on the destination folder (the
  `folder_templates` feature of Templater).
- Template inheritance / `{% include %}` graphs that span multiple
  files. (MiniJinja supports this; we just don't ship any included
  partials in v1.)
- Renaming an existing note (covered by a future plan; the
  section-move flow's per-pick rename from plan 007 is the closest
  thing today).
- Bulk note creation (e.g. "create 7 daily notes for next week").

## Sessions

### Session 1 · 2026-05-13 · done
**Goal:** Library — MiniJinja renderer in `ft_core::notes::template`.
Add the `TemplateContext` struct, `render` / `render_path` functions,
strict-undefined env, and the chrono-backed filters (`date`,
`parse_date`, `add_days`, `add_weeks`, `add_months`, `weekday_of`).
Unit-test each filter's happy path and one error path; verify strict
undefined mode catches typos. No CLI, no TUI, no hand-port yet —
fixtures use inline template strings.
**Outcome:** Added `minijinja 2.19` to the workspace with
`default-features = false` and `features = ["builtins", "serde"]`
(serde is needed for the `vars` map to flow through `Value::from_serialize`).
New submodule `ft-core/src/notes/template.rs` wired as `pub mod template;`
inside the existing `notes.rs` (no need to convert `notes.rs` → `notes/mod.rs`
because Rust 2018 supports a `foo.rs` + `foo/` pair). Public surface:
`TemplateContext { title, today, now, vars }`, `TemplateContext::new(...)`,
`render(source, &ctx) -> Result<String>`, `render_path(path, &ctx) -> Result<String>`.
The MiniJinja env is strict-undefined, autoescape-off, with
`keep_trailing_newline = true` so `# {{ title }}\n` produces output
ending in `\n` (default MiniJinja behavior is to strip — caught it on the
first test run).

Typed date plumbing uses `Object` wrappers (`DateValue(NaiveDate)`,
`DateTimeValue(NaiveDateTime)`) with `ObjectRepr::Plain` and a custom
`render` impl that defaults to `%Y-%m-%d` / `%Y-%m-%d %H:%M:%S`. The
context flows through MiniJinja's `context!` macro (NOT
`Value::from_serialize`) so the typed Objects survive — round-tripping
through serde would lose the wrapper. Filters downcast with
`value.downcast_object_ref::<DateValue>()` and chain by emitting fresh
`Value::from_object(...)` results, so `parse_date | weekday_of | date`
keeps the date typed end-to-end.

Six filters registered: `date`/`parse_date` take `Kwargs` for the
`format=...` keyword arg (default `%Y-%m-%d`); `add_days`/`add_weeks`/
`add_months`/`weekday_of` take a positional integer. `kwargs.assert_all_used()`
catches typos like `formaat="..."` instead of silently defaulting.

Notable gotcha worth remembering for session 2: chrono's strftime
treats every non-`%` character as a literal — there is no `[...]`
escape syntax like Moment.js. So `"%G Week %V"` is the correct format
for the `weeks.md` title; `"%G [Week] %V"` (which I'd written in the
plan) would have required the input to literally contain
`[Week]`. Updated the plan's conversion table accordingly.

Second gotcha: `NaiveDate::parse_from_str` (and `Parsed::to_naive_date`)
fail when only ISO-week tokens (`%G %V`) are present because there's
no day-of-week. Added a fallback chain in `parse_date_filter`: try
`to_naive_date()` first; if that fails, try
`NaiveDate::from_isoywd_opt(isoyear, isoweek, Mon)`; then year+month
→ 1st-of-month. This is exactly what session 2's `weeks.md` port
needs.

26 unit tests in `notes::template::tests`, all green:
- Variable substitution: `title`, `today` (default `%Y-%m-%d`), `now`
  (with `%H%M`), `vars.name`.
- `date` filter: explicit format, default format on `now`, error on
  non-date.
- `parse_date`: round-trip, custom ISO-week format, error on
  unparseable string.
- `add_days`/`weeks`/`months`: positive, negative-wraps-year for
  months, error on non-date.
- `weekday_of`: Monday (1), Sunday (7), out-of-range error.
- Full chain: `title | parse_date(format="%G Week %V") | weekday_of(1) | date(format="%Y-%m-%d")`
  → `"2026-05-04"` for title `"2026 Week 19"`.
- Strict undefined: typo (`titel`), missing `vars.missing`, unknown
  kwarg (`formaat`).
- `render_path` reads a tempfile and renders it.

Workspace state: `cargo test --workspace` → 344 lib + 1 integration
(real_vault) + 0 doc tests, all green. `cargo clippy --workspace
--all-targets -- -D warnings` clean. `cargo fmt --check` clean
(after one autoformat pass on the test block).

### Session 2 · 2026-05-13 · done
**Goal:** Hand-port the 16 vault templates from `templates/` to
`templates-ft/` and write the per-template golden snapshots. Add a
parameterized test in `ft-core` that enumerates the fixture templates
dir and renders each against its representative title under
`FT_TODAY=2026-05-13`. Add the `templates-ft/README.md` conversion
guide. The original `templates/` folder is untouched.
**Outcome:** Added 16 hand-ported templates to
`ft-core/tests/fixtures/templates-ft/` and a sibling copy to
`~/git/fortytwo/templates-ft/`, plus `~/git/fortytwo/templates-ft/README.md`
with the full Templater→ft conversion guide (Moment→chrono token
table, `<% tp.date.* %>` chain decompositions, caveats including
`tp.file.cursor()` being unsupported). Original `templates/` folder
untouched.

One new filter required mid-session: `quarter` (returns 1..=4 from a
date's month). chrono has no `%q` token; `weeks.md`'s
`<% tp.date.now("YYYY [Q]Q", ...) %>` port needs it. Added the filter
+ 2 tests + a module-doc entry; clean addition since the existing
`extract_date` helper already does the heavy lifting.

The `weeks.md` port uses a `{%- set monday = title | parse_date(format="%G Week %V") %}`
binding so the ISO-week parse runs once instead of 9 times (7
weekdays + year + quarter). The `{%-` strips the leading newline so
the frontmatter `---` sits directly above `# {{ title }}` matching
the original layout.

Parameterized golden-snapshot test at `ft-core/tests/templates.rs`:
- `fixture_set_matches_expectations` — asserts the on-disk fixture set
  exactly matches the in-source `TEMPLATES` constant. Catches a
  forgotten title for a new template OR a forgotten fixture for an
  expected title.
- `renders_all_fixture_templates` — iterates the `TEMPLATES` table,
  renders each at fixed `today/now = 2026-05-13`, asserts via
  `insta::assert_snapshot!`. `quick-add.md` gets a `vars.name = title`
  injection (it's the only template that prompts for a custom var).

Representative title table (per session-2 acceptance criteria):

| Template | Title | Title | Template |
|---|---|---|---|
| daily | `2026-05-13` | quarterly | `2026 Q2` |
| father-watson | `Father Watson 2026-05-13` | quick-add | `Quick` |
| goal-checkin | `Goal Checkin` | restaurant | `Test Restaurant` |
| inbox | `Inbox 2026-05-13` | stry | `Test Story` |
| new | `Test Note` | tags | `Tags` |
| progress-checkin | `Progress Checkin` | tasks-in-this-path | `Tasks Here` |
| proj | `Test Project` | travel | `Travel 2026-05-13` |
| weeks | `2026 Week 19` | wrklg | `Test Worklog` |

Sanity-checked the `weeks.md` golden snapshot by hand:
- Mon..Sun of ISO 2026-W19 are `2026-05-04` through `2026-05-10` ✓
- Year (`%Y` of Monday) is `2026` ✓
- Quarter is `Q2` (May → month 5 → `(5-1)/3+1 = 2`) ✓
- Title `2026 Week 19` rendered directly ✓

Workspace state: `cargo test --workspace` → 346 ft-core unit tests
+ 1 real_vault + 2 templates + the ft binary's tests, all green.
`cargo clippy --workspace --all-targets -- -D warnings` clean.
`cargo fmt --check` clean.

The original `templates/` folder in `~/git/fortytwo/` is untouched
(verified by `ls` before and after).

### Session 3 · 2026-05-13 · planned
**Goal:** CLI — `ft notes create <PATH> [--template P] [--title T]
[--var K=V]... [--obsidian] [--no-open] [--editor BIN] [--force]`.
Vault config gains `[notes] templates_dir`. `Vault::templates_dir()`
helper. Integration tests against per-test `TempDir` vaults covering
blank create, template create, var prompts (via repeated `--var`),
collision with and without `--force`, missing template, `--no-open`,
`--obsidian` dry-run, path normalization (`.md` auto-append +
intermediate dir creation).
**Outcome:** 

### Session 4 · 2026-05-13 · planned
**Goal:** TUI Notes tab — `c` (blank) and `C` (template) flows.
`NotesState::Creating(CreateState)` state machine with template/folder
pickers, filename `EditBuffer`, var prompts, collision prompt
(Overwrite / Use existing / Cancel). After-create opens the new file
via `AppRequest::OpenInEditor`. Snapshots: template picker, folder
picker, filename prompt, var prompt, collision prompt. Behavior tests
covering each path and each Esc transition. `?` help overlay updated.
**Outcome:** 

### Session 5 · 2026-05-13 · planned
**Goal:** TUI section-move integration. Add `n` keybind in
`TargetPicking` that enters `NewTargetState` (template → folder →
filename → vars → optional collision prompt). At sub-flow completion,
land in `Composing` with `target_content: String` and `target_is_new:
bool` set. `commit_move` branches on `target_is_new` to use in-memory
content instead of re-reading from disk; `write_pair` creates the file
for the first time. Cancel paths leave the filesystem untouched.
Snapshots + end-to-end test (source pick → multi-select → `n` →
template → folder → filename → compose → Enter; assert both files on
disk). Collision-Overwrite and Collision-UseExisting paths covered.
**Outcome:** 
