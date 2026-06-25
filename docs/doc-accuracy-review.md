# Documentation accuracy review

*2026-06-25*

A pass over `docs/` (and the user-facing `README.md`) checking claims
against the current source. Findings are grouped **Inaccurate** (the
doc describes something that no longer exists or is wrong),
**Stale snapshots** (dated review docs whose numbers have drifted,
fine as history but worth flagging), and **Gaps / underexplained**.

Each item names the file, the wrong claim, the source-of-truth, and a
proposed fix.

> **Status:** All items below were addressed in the same pass that
> produced this review. Each item is marked ✅ (fixed) with a one-line
> summary of the change. The original findings are kept verbatim for
> the record. Only `docs/**/*.md` files were touched; no Rust changed,
> and all four build invariants (`build --release`, `test --workspace`,
> `clippy --tests -- -D warnings`, `fmt --check`) remain clean.

---

## A. Inaccurate (should fix)

### A1. `docs/architecture.md` — "Query language (`query::dsl`)" section is entirely obsolete — ✅

**Fixed:** Replaced the section with a "Profiles and the unified DSL"
subsection documenting `parse_with` / `Profile::Tasks` / `Profile::Default`,
the implicit prelude, and the absence of a `Filter` type. Removed the
phantom `filter.rs` / `expr.rs` / `dsl.rs` from the workspace-layout
tree (kept `preset.rs`, `sort.rs`, added `delete.rs`/`tests.rs` to the
graph dir listing). Also fixed the `parse(src)` prose to mention
`parse_with` as the real entry point.

**Original finding:**

**Claim:** the section says `query::dsl` exists with
`dsl::parse(src, today)` returning `Query { expr, sort_keys, limit }`,
an AST in `query::expr` (`Expr`/`Atom`), and "The CLI composes the DSL
with flag filters by AND-ing the parsed expression with a typed
`Filter`." It links to `docs/query-dsl.md`.

**Truth:**
- `ft-core/src/query/` contains only `mod.rs`, `preset.rs`, `sort.rs`.
  There is no `dsl.rs`, no `expr.rs`, no `filter.rs`.
- `query/mod.rs` states explicitly: *"No separate `Filter` type — the
  DSL is the single source of truth for what predicates exist."*
- The task DSL was removed (see `docs/migrating-task-queries.md` and
  `docs/query-dsl.md`, both of which say so). Task predicates now run
  through `graph::query::parse_with(src, Profile::Tasks, today)`.
- `Query { expr, sort_keys, limit }` does not exist; sort/limit are CLI
  flags (`--sort`, `--limit`), not DSL clauses.

The workspace-layout tree in the same file also lists `query/` with
`dsl.rs`, `expr.rs`, `filter.rs`, `preset.rs`, `sort.rs` — the first
three are phantoms.

**Fix:** Replace the "Query language (`query::dsl`)" subsection with a
short pointer to the unified graph DSL: task queries parse under
`Profile::Tasks` via `graph::query::parse_with`; sort/limit are CLI
flags; presets live in `query::preset`. Remove the phantom files from
the workspace-layout tree (keep `preset.rs`, `sort.rs`). Either delete
`docs/query-dsl.md` or repoint it to `docs/graph-query-dsl.md` /
`docs/migrating-task-queries.md` (it currently says "removed" and
links onward, which is fine, but it's still listed in the README
reference-docs index as if live).

### A2. `docs/task-format.md` — "Daily-notes resolution" section describes a non-existent config schema — ✅

**Fixed:** Deleted the entire section. Daily-note resolution is already
correctly documented in `docs/config.md` §`[periodic_notes.*]`.

**Original finding:**

**Claim:** "`[daily_notes].source` in `.ft/config.toml` picks one of
`core` / `periodic-notes` / `explicit`", reading
`.obsidian/daily-notes.json` or `periodic-notes/data.json`, using
"moment.js-style patterns" with tokens `YYYY YY MMMM MMM MM M DDDD DD
D ...` and reserved tokens `Q`/`Qo`.

**Truth:**
- There is no `[daily_notes]` table. Config uses
  `[periodic_notes.daily]` with `path` / `format` / `template`
  (chrono `strftime`, *not* moment.js). See `ft-core/src/config.rs`
  and `docs/config.md` §`[periodic_notes.*]`, which is correct.
- No code reads `.obsidian/daily-notes.json` or
  `periodic-notes/data.json` (`rg` finds zero references).
- Tokens are chrono strftime (`%Y %m %d %B …`), with ft extensions
  `%q` / `%Q` for quarters — *not* `YYYY`/`MM`/`DD`/`Q`.

This whole section contradicts both `docs/config.md` and the actual
`periodic.rs` implementation. It looks like a leftover from a
pre-unification design that was never shipped.

**Fix:** Delete the "Daily-notes resolution" section from
`task-format.md` entirely (it duplicates and contradicts
`config.md`). If a cross-reference is wanted, replace it with one
sentence: "New tasks default to today's daily note; see
`docs/config.md` §`[periodic_notes.*]` for path/format/template."

### A3. `docs/guide/tasks.md` — every DSL example uses the removed task DSL — ✅

**Fixed:** Rewrote all query examples to the unified DSL
(`priority = High`, `status in {Open, InProgress}`, `tags includes
"work"`, `due < today`), converted `sort by … limit N` to `--sort` /
`--limit` flags, added a `not-done` preset example, and repointed the
intro link from the removed `query-dsl.md` to `graph-query-dsl.md`.
Also fixed the same stale-DSL pattern in `docs/config.md`,
`docs/guide/vault-and-config.md`, and `docs/guide/scripting.md`
(preset examples + `ft tasks move --query`), and a stale `query-dsl.md`
link in `config.md`'s top-level keys table.

**Original finding:**

**Claim:** the guide shows `priority is high`, `status is open`,
`tag is work`, `due before today`, `not done`, `sort by due limit 10`,
etc. (11 occurrences across the file).

**Truth:** `docs/migrating-task-queries.md` states this is a **hard
break — old task DSL queries do not parse.** Running any of these
examples today fails. The correct forms are `priority = High`,
`status = Open`, `tags includes "work"`, `due < today`, `status in
{Open, InProgress}` (or the `not-done` preset), and `--sort`/`--limit`
flags. The reference docs (`graph-query-dsl.md`,
`migrating-task-queries.md`) are correct; only the guide is stale.

**Fix:** Rewrite the query examples in `guide/tasks.md` to the unified
DSL, using the translation table in `migrating-task-queries.md`. This is
the highest-impact fix because the guide is the entry point users
follow. Lines 36–86 and 228 are the offenders.

### A4. `docs/architecture.md` — `ActiveModal` enum listing is incomplete — ✅

**Fixed:** Updated the enum block to the 16 actual variants (added
`ConfirmDelete`, `CreateSubdir`, `JournalSources`, `JournalAppendOrReplace`)
and added a note pointing to `ft/src/tui/modal.rs` as the source of truth.

**Original finding:**

**Claim:** the `ActiveModal` enum is shown with 12 variants:
`Create, Append, CapturePicker, CaptureVar, SectionMove, MoveOuter,
Rename, PresetPicker, Related, Search, PeriodicLeader, QueryBar`.

**Truth:** the enum (`ft/src/tui/modal.rs`) has 16 variants. Missing
from the doc: `ConfirmDelete`, `CreateSubdir`, `JournalSources`,
`JournalAppendOrReplace`. Also `PeriodicLeader` is listed in the doc
but the enum actually has it too — however the doc presents the list as
exhaustive ("```rust pub enum ActiveModal { … }```") so the four
omissions read as "these don't exist."

**Fix:** Update the enum listing to match the 16 actual variants, or
replace the exhaustive block with "…(see `ft/src/tui/modal.rs` for the
current variant set)" and name only the *shapes* the prose discusses.

### A5. `docs/architecture.md` — `Modal` trait listing omits `commands()` and `keymap()` — ✅

**Fixed:** Added `commands()` and `keymap()` (with default-impl note) to
the trait block, with a pointer to `commands.md` for the registry
integration.

**Original finding:**

**Claim:** the `Modal` trait is shown with four methods:
`handle_event`, `render`, `keymap_help`, `name`.

**Truth:** the trait (`ft/src/tui/modal.rs`) also has `commands(&self)
-> &[CommandDef]` and `keymap(&self) -> &KeyMap` (both with default
impls). These matter because `docs/commands.md` describes the
Command/Keymap registry as "one source of truth" and modals
participate in it via exactly these two methods.

**Fix:** Add the two methods (with their default-impl note) to the
trait block, or add a sentence pointing to `commands.md` for the
registry integration.

### A6. `docs/architecture.md` — "GraphMoveOuter is the one modal still using the legacy tab-resident dispatch path" is wrong — ✅

**Fixed:** Replaced the sentence with a statement that the migration is
complete (every modal, `GraphMoveOuter` included, now goes through
`impl Modal`).

**Original finding:**

**Claim:** the modal-driver section ends: *"`GraphMoveOuter` is the one
modal still using the legacy tab-resident dispatch path and is the
subject of a follow-up migration."*

**Truth:** `GraphMoveOuter` now `impl Modal for GraphMoveOuter`
(`ft/src/tui/tabs/graph.rs`), so it has been migrated. There is no
remaining "legacy tab-resident dispatch path" for it. Either the
migration landed and the caveat wasn't removed, or the caveat refers
to something else.

**Fix:** Delete the sentence, or replace it with the actual current
state (if any modal is still on a non-`Modal` path, name it
specifically; otherwise state the migration is complete).

### A7. `docs/architecture.md` — `markdown.rs` described as "heading extractor + shared LineSkipState" — ✅

**Fixed:** Dropped "+ shared LineSkipState" from the tree annotation.

**Original finding:**

**Claim:** workspace-layout tree annotates `markdown.rs` as
"heading extractor + shared LineSkipState".

**Truth:** the module's own doc (`ft-core/src/markdown.rs`) says
*"Today this module only ships a heading extractor used by `search`."*
No `LineSkipState` exists in the module (or has been removed).

**Fix:** Drop "+ shared LineSkipState" from the tree annotation.

### A8. `docs/architecture.md` — `GraphQuery` API prose says `parse(src)` and `GraphQuery { initial, expansion }` mostly, but the section header references `parse_with` — ✅

**Fixed:** Folded into A1's rewrite of the graph-query section: prose now
names both `parse` (defaulted) and `parse_with(src, profile, today)` as
the load-bearing entry point, and the `Profile` enum's effect is
documented in the new "Profiles and the unified DSL" subsection.

**Original finding:**

Minor, but the prose is inconsistent: the "Graph query DSL" section
says *"`ft-core::graph::query::parse(src)` returns a `GraphQuery {
initial, expansion }`"* without noting `parse_with(src, profile,
today)` is the real entry point used by tasks (`Profile::Tasks`) and
that `parse` is just `parse_with` defaulted to `Profile::Default` +
system today. Given the whole "unified DSL" story, `parse_with` is
the load-bearing one.

**Fix:** Mention both: `parse(src)` defaults to `Profile::Default`;
`parse_with(src, profile, today)` is what `ft tasks list` and the TUI
Tasks tab use with `Profile::Tasks`. The `Profile` enum and its
effect (implicit `node where kind = Task and …` prelude) is the
load-bearing detail and is currently only in `migrating-task-queries.md`.

### A9. `docs/architecture.md` — output module list vs `Format` enum — ✅

**Fixed:** Annotated the output tree line to distinguish `Format`
variants (table/json/markdown/ndjson) from command-specific renderers
(links.rs, graph.rs).

**Original finding:**

**Claim:** the workspace tree lists `src/output/` as
"table.rs, json.rs, markdown.rs, ndjson.rs, links.rs, graph.rs",
implying all are `Format` variants.

**Truth:** `output::Format` (`ft/src/output/mod.rs`) has only
`Table, Json, Ndjson, Markdown`. `links.rs` and `graph.rs` are
separate renderers used directly by `ft notes` (backlinks/links) and
`ft graph query` — they are not `--format` options for `ft tasks list`.

**Fix:** Either annotate the tree ("`links.rs`, `graph.rs` —
command-specific renderers, not `Format` variants") or split the
listing. Low severity but reads as a wrong implication.

---

## B. Stale snapshots (date-stamped, fine as history — but flag)

### B1. `docs/2026-06-02-architecture-review.md` — numbers have drifted — ✅

**Fixed:** Added an "As-of snapshot" blockquote header noting the date,
that counts/DSL landscape have drifted, and pointing to
`docs/architecture.md` for the current state.

This doc is explicitly dated `2026-06-02` and reads as a point-in-time
review, so drift is *expected and acceptable*. But because nothing in
the doc says "these numbers are as-of the date and will drift," a
reader can mistake them for current. Concrete drift:

| Claim in doc | Current |
|---|---|
| "9 top-level commands" | 14 (`vault, tasks, timeblocks, find, notes, review, synth, graph, git, tui, completions, man, commands, do`) |
| "`ft notes` alone has 13 subcommands" | 12 (`open, move-section, create, today, periodic, backlinks, links, rename, move, journal, update-related, append`) |
| "TUI has 5 tabs (Graph, Tasks, Notes, Timeblocks, Journal)" | 6 (added **Review**) |
| "~55k LoC of Rust" | ~82k LoC |
| "146 commits" | 242 commits |
| "archived 12 changes" | (not re-verified) |

Also the review still describes the "Dual-DSL preset pattern" (task DSL
+ graph DSL) as load-bearing — the task DSL has since been removed
(see A1). And it lists `Filter` as a real type.

**Fix:** Add a one-line header note: *"Snapshot of the codebase as of
the date above; counts and the DSL landscape have since changed — see
`docs/architecture.md` for the current state."* Optionally strike
the dual-DSL paragraph. (Deleting the file is also defensible since
it's unreferenced from README/index, but it has historical value.)

### B2. `docs/2026-06-12-refactor-review.md` — same status — ✅

**Fixed:** Added an "As-of snapshot" blockquote header pointing to
`docs/architecture.md` for the current state.

Dated `2026-06-12`. Several of its "top duplications" may already be
addressed. It's unreferenced from README/guide. Same recommendation:
add an as-of header or delete. Not re-audited item-by-item here.

---

## C. Gaps / underexplained

### C1. The Review tab is undocumented in `architecture.md` — ✅

**Fixed:** Added a one-sentence tab roster ("six tabs today: Graph,
Tasks, Notes, Timeblocks, Journal, and Review") at the top of the
"A new TUI tab" recipe, cross-referencing the synthesis-ritual section
where the Review→Journal handoff is described.

`ft/src/tui/tabs/review.rs` exists and is wired into `build_tabs`
(6th tab), and `docs/guide/index.md` correctly says "Six tabs (Graph,
Tasks, Notes, Timeblocks, Journal, Review)." But `architecture.md`'s
"Adding a new TUI tab" recipe and the modal-driver section never
mention the Review tab or its `AppRequest::JournalForMulti` /
`MultiTargetRequest` handoff to the Journal tab (the handoff *is*
described under the synthesis-ritual section, but not framed as a
Review-tab feature). A reader of architecture.md alone would not know
the tab exists.

**Fix:** Add Review to the tab roster in architecture.md, or at least
cross-reference the synthesis-ritual section where the
Review→Journal flow is described.

### C2. `notes_actions/reslice.rs` flow is absent from the modal-driver taxonomy — ✅

**Fixed:** Added a "Tab-resident state machines outside the `Modal`
slot" bullet documenting `ResliceState` / `NotesState::Reslicing` as
the one remaining multi-step flow that predates the modal driver, with
a note that new flows should go through `Modal`.

The modal-driver section's "three patterns by modal shape" lists flow
modals (`Create`, `Append`, `SectionMove`, `CaptureVar`) but not the
reslice flow (`notes_actions/reslice.rs`, backing
`notes.reslice`/`graph.reslice`-adjacent commands and the
`SynthScaffoldPlan` reslice path). It's a flow modal of the same
shape.

**Fix:** Add `Reslice` to the flow-modal list, or note the list is
illustrative not exhaustive.

### C3. `Profile::Tasks` prelude behavior is documented only in the migration doc — ✅

**Fixed:** Added a "Profiles" section to `docs/graph-query-dsl.md`
(the reference doc) covering `Profile::Default` vs `Profile::Tasks`,
the implicit `node where kind = Task and …` prelude, enum-value
case-insensitivity, and that sort/limit are CLI flags. Also covered in
the new architecture.md "Profiles and the unified DSL" subsection (A1).

The fact that `Profile::Tasks` prepends an implicit
`node where kind = Task and …` so users can type `priority = High` is
the central usability feature of the unified DSL, but it's only
mentioned in `docs/migrating-task-queries.md` (a migration doc) and
`AGENTS.md`. `docs/graph-query-dsl.md` (the reference) doesn't mention
profiles at all, and `docs/architecture.md`'s graph-query section
omits it (see A8). A user reading the reference cold wouldn't know
why `ft tasks list priority = High` works without a `node where`
prefix.

**Fix:** Add a "Profiles" subsection to `graph-query-dsl.md` covering
`Profile::Default` vs `Profile::Tasks` and the implicit prelude, with
a forward pointer from `architecture.md`.

### C4. `[synth]` config table — `exclude_prefixes` semantics underexplained — ✅

**Fixed:** Added a comment in the `[synth]` example clarifying literal
starts-with matching on the vault-relative path, with the `journal/`
vs `journaling/` example and the trailing-slash guidance.

`docs/config.md` says `exclude_prefixes` "excluded from `ft review`"
with conventional use "filter out your periodic-notes folder." But it
doesn't say whether the match is path-prefix-on-path (does `journal/`
match `journal/2026/foo.md`? does `journal` without slash match
`journaling/`?). The architecture doc says "path-prefix exclude
filter" which implies starts-with, but the config doc should state it
explicitly and whether a trailing slash matters.

**Fix:** One sentence in `config.md` §`[synth]`: "Match is
vault-relative path starts-with; `journal/` excludes
`journal/2026/...` but not `journaling/...`; include the trailing
slash to be safe." (Verify against `link_review.rs` first.)

### C5. `FT_OBSIDIAN_DRY_RUN` and the `--obsidian` flag are thinly documented — ✅

**Fixed:** Added an "Obsidian integration" subsection to `config.md`
(after the env-var table) enumerating the `ft notes` subcommands that
accept `--obsidian`/`--vault-name`, noting `FT_OBSIDIAN_DRY_RUN`, and
cross-referencing the `[editor]` strategies.

`docs/config.md` env-var table lists `FT_OBSIDIAN_DRY_RUN` ("prints
the `obsidian://` URL instead of opening it") and `append-and-capture.md`
mentions `--obsidian` / `--vault-name` flags for `ft notes append`, but
the full set of subcommands that accept `--obsidian` and the URL
scheme aren't collected anywhere. Users discover the flag per-command.

**Fix:** Either a short "Obsidian integration" subsection in
`config.md` or a line in each relevant command's help. Low priority.

### C6. `ft do` headless surface is described aspirationally — ✅

**Fixed:** Added a discovery paragraph to `commands.md` connecting
`ft commands list --opens-modal false` to `ft do` eligibility, noting
that "non-modal" is necessary not sufficient (still exits 3 without a
handler), and stating that today only `tasks.complete-by-id` has a
headless handler.

`docs/commands.md` says `ft do tasks.complete-by-id --arg id=…` works
and that's the spec scenario, but it also says "most registered
commands mutate TUI state … and don't have a meaningful headless
analog" and "Add a new handler when the underlying `ft-core` operation
is atomic enough." It doesn't enumerate *which* commands currently
have headless handlers vs exit-3. A user can't tell without trying.

**Fix:** Either list the headless-capable commands in `commands.md`,
or document `ft commands list --opens-modal false` as the discovery
path (it's mentioned but not tied to "these are the ft-do-able ones").

### C7. Editor `[editor]` strategies and the TUI-vs-CLI split could cross-link — ✅

**Fixed:** Added a cross-reference from `append-and-capture.md`'s
flag table to `config.md §[editor]`, noting the CLI always spawns
`$EDITOR` one-shot while the TUI uses the configured strategy.

`docs/config.md` §`[editor]` is thorough but says "The CLI (`ft notes
open` / `create` / …) is unaffected — it's always a one-shot spawn."
Good. But `append-and-capture.md` documents `--no-open`/`--editor`
flags that interact with the same `$EDITOR` resolution without
pointing back to `[editor]`. Minor discoverability gap.

**Fix:** Cross-link from `append-and-capture.md` to `config.md#editor`.

---

## D. Minor / cosmetic

- **D1.** `docs/architecture.md` workspace tree lists `ft/src/output/`
  as "links.rs, graph.rs" but the `Format` enum note (A9) — pick one
  representation. — ✅ Folded into A9 (the tree line now annotates both
  groups).
- **D2.** `docs/keybindings.md` is generated and appears accurate
  (cross-checked against `ft/src/tui/*/` command slices); no issues
  found. Good candidate to re-verify with `ft commands docs --check`
  in CI. — ✅ Verified in sync via `ft commands docs --check` (clean).
- **D3.** `docs/timeblocks.md` TUI keymap table uses bare keys (`a`,
  `A`, `d d`, `]`/`[`) which is accurate but predates the
  Command/Keymap registry; the registered names are
  `timeblocks.add-quickline`, `timeblocks.delete-start`, etc. The
  table could note "see `docs/keybindings.md` for the canonical
  command names." Not wrong, just a style mismatch with the rest of
  the docs. — ✅ Added a cross-reference sentence above the keymap
  table pointing to `keybindings.md` under `tab/timeblocks`.

---

## Recommended priority

1. **A3** (guide/tasks.md old DSL) — actively misleads users on the
   entry path; examples don't run. — ✅
2. **A2** (task-format.md daily-notes section) — documents a config
   schema that doesn't exist; contradicts `config.md`. — ✅
3. **A1** (architecture.md `query::dsl` section + phantom files) —
   describes removed modules as current. — ✅
4. **A4–A7** (architecture.md modal/enum/markdown drift) — bunch of
   small staleness in the same file; fix in one pass. — ✅
5. **C3** (Profile::Tasks in the reference DSL doc) — fills the
   biggest conceptual gap for new readers. — ✅
6. **B1/B2** (dated review headers) — cheap one-line fix. — ✅
7. **A8, A9, C1, C2, C4–C7, D** — polish. — ✅

All items resolved. No Rust changed; all four build invariants clean.
