## Context

The unified graph/task query DSL (`ft_core::graph::query`) parses a
predicate grammar over node/edge attributes. `path` is a **string**
attribute holding the vault-relative path (`journal/2026/2026-07-29.md`).
The DSL's date keywords (`today`, `tomorrow`, `+3d`, …) only resolve on
**date-typed** attributes (`due`, `scheduled`, …) — see `parse_date_rhs` /
`parse_date_literal` in `parser.rs`, which call `dates::parse_date_value`.

Consequence: to filter "tasks in today's daily note" a user must write
`path includes "2026-07-29"` — typing the ISO date in a quoted string.
This is awkward and, worse, **cannot be stored as a preset**: the date is
literal, so a `[tasks.presets]` entry goes stale overnight. There is no
way to say "today's daily-note path" dynamically, and no way to express it
for non-ISO daily formats (e.g. `%G-W%V` weeklies, or dailies nested in
`journal/%Y/`) without hard-coding the format/folder.

The DSL parser/evaluator currently take only `&Graph` (eval) or
`NaiveDate` (parse). Periodic-note config (`Config::periodic_notes`) lives
on `Vault`. Threading periodic config *into* the evaluator would ripple
through `GraphQuery::select`/`expand`/`walk` and the TUI's shared
`Arc<GraphSnapshot>` — a large, invasive change for a string-literal
convenience.

Existing seams this design builds on:
- `ft_core::dates::today()` — the single "today" override seam (`FT_TODAY`).
- `ft_core::dates::parse_date_value(s, today) -> Option<NaiveDate>` —
  already understands `today`/`tomorrow`/`yesterday`/`+Nd`/`-Nw`/ISO. The
  DSL's date-literal parser already routes through it.
- `ft_core::periodic::resolve_periodic_path(vault_root, cfg, date)` —
  renders `[periodic_notes.<period>]` `path`+`format` to an absolute path.
- `ft_core::periodic::Period::offset_date(date, n)` — period-correct
  shifts (days for daily, ×7 for weekly, calendar months for monthly/…).
- The graph stores `path` as a vault-relative `PathBuf` (see
  `graph::NoteData.path` doc: "Vault-relative path, e.g.
  `Areas/finance.md`"). So a sigil that expands to the vault-relative
  periodic-note path matches what `path includes` / `path =` compares
  against.

## Goals / Non-Goals

**Goals:**
- Let users write `path includes @daily` (and `@today`, `@weekly`, …) and
  have it expand to the right vault-relative string at parse time.
- Make **presets dynamic** — a stored preset containing `@daily` resolves
  fresh on every run.
- Keep the predicate DSL grammar, AST, and evaluator **byte-for-byte
  unchanged**. The `@` layer is a separate pre-parse pass.
- Reuse existing seams (`dates`, `periodic`) rather than duplicating
  date/path logic.
- Surface interpolation errors through the same UX as DSL parse errors
  (CLI exit code 2 where parse errors already yield 2; TUI inline error).

**Non-Goals:**
- No new DSL attributes (e.g. a date-typed `daily` attribute). The DSL
  grammar gains no tokens.
- No threading of periodic config into `Graph` / `GraphSnapshot` / the
  evaluator. Interpolation happens before the parser ever sees tokens.
- No arbitrary expression language inside `@…`. Just `<period>[±<int>]`
  (and `@today`), no math, no nesting.
- No span/error-position remapping back to the pre-expansion source in v1.
  Error positions cite the expanded text (see Risks).
- No change to how `path` is compared — still a vault-relative string,
  still `includes`/`starts_with`/`=`/`in`.

## Decisions

### D1: Pre-parse string→string substitution, not a grammar extension

A new `ft_core::query::interpolate(src, resolver) -> Result<String, _>`
runs **before** `graph::query::parse_with`. It scans `src` for `@…` tokens
and replaces each with a double-quoted DSL string literal
(`"journal/2026/2026-07-29.md"`). The parser then operates on a normal
DSL string with no sigils.

**Why over a grammar extension (new `daily` attribute):**
- The evaluator stays `&Graph`-only; no periodic-config plumbing into
  `GraphSnapshot`/`walk`/the TUI. Smallest ripple.
- `path` semantics stay uniform (string compare). A `daily = today`
  attribute would be a date-typed peer that secretly does a string
  comparison against a configured path — conceptually muddy and coupled
  to vault config at the eval layer.
- The `@` sigil is visually a *different layer* (interpolation, not
  predicates), matching the user's "don't pollute the simple language"
  constraint.
- Generalizes: `@daily` works for *any* path comparison, not just "today's
  tasks", and `@today` is a general "today as a string" for ISO-filename
  vaults that lack periodic config.

**Alternative considered:** letting `path = today` reuse date-keyword
resolution. Rejected — `path` is string-typed, `today` is date-typed;
mixing semantics *is* polluting the language, and it's ambiguous for
vaults with weekly/monthly configs (which period does `today` mean?).

### D2: Sigil set and offset grammar

Recognized placeholders (sigil `@`, which is currently an
`IllegalCharacter` in the DSL, so no collision with any valid query):

| Sigil | Resolves to | Needs periodic config? |
|---|---|---|
| `@today` | raw ISO date `YYYY-MM-DD` for today | no |
| `@daily` | vault-relative path of today's daily note | `[periodic_notes.daily]` |
| `@weekly` | vault-relative path of today's weekly note | `[periodic_notes.weekly]` |
| `@monthly` | … monthly | `[periodic_notes.monthly]` |
| `@quarterly` | … quarterly | `[periodic_notes.quarterly]` |
| `@yearly` | … yearly | `[periodic_notes.yearly]` |

Optional signed integer offset, e.g. `@daily-1`, `@daily+7`, `@today-3`,
`@weekly-2`. Offset unit = the period's own units via
`Period::offset_date` (days for `@daily`/`@today`, ×7 days for `@weekly`,
calendar months for `@monthly`/`@quarterly`/`@yearly`). This intentionally
mirrors the existing relative-offset feel (`+Nd`) but in the sigil's
period units, which is what a user of `@weekly-2` expects ("two weeks
ago's weekly note"), not raw days.

**Lexing rule:** an `@`-token is `@` + ASCII-alpha run (`today`/`daily`/…)
+ optional `[+-]\d+`. A `@` not followed by a known name is a hard
`UnknownSigil` error (not silently passed through) so typos like
`@datly` don't quietly filter to nothing. A `@` inside an existing
double-quoted DSL string is left untouched (strings are scanned as
opaque spans), so a query like `title includes "me@you"` still works.

### D3: Path form — vault-relative, via `resolve_periodic_path` + strip

`periodic::resolve_periodic_path(vault_root, cfg, date)` returns an
**absolute** path. The graph's `path` attribute is **vault-relative**
(`journal/2026/2026-07-29.md`). The interpolator strips the `vault_root`
prefix (via `Path::strip_prefix`, falling back to the full string if the
prefix doesn't apply — which would indicate misconfiguration) and emits
that relative form as the DSL string literal. This matches what
`path includes` / `path =` already compare against in `eval_cond_on_node`.

**Why vault-relative not bare filename:** `path = @daily` should work
even when dailies nest in `journal/%Y/`; the relative path is the
unambiguous identity. `path includes @today` still works for users who
only want the date substring and have flat ISO-named dailies.

### D4: Resolver shape — `&PeriodicNotes` + `&Path` (vault_root) + `NaiveDate`, not `&Vault`

`interpolate` takes a small borrowed resolver rather than `&Vault` so it
is unit-testable without constructing a full `Vault`. Concretely:

```rust
pub struct SigilCtx<'a> {
    pub today: NaiveDate,
    pub vault_root: &'a Path,
    pub periodic: &'a PeriodicNotes,
}
pub fn interpolate(src: &str, ctx: &SigilCtx<'_>) -> Result<String, InterpolationError>;
```

Call sites that already hold a `&Vault` build `SigilCtx` from
`vault.task_format()`-adjacent fields (`vault.root_path()`,
`&vault.config.config.periodic_notes`, `dates::today()`). Where a call
site has only `today` and no vault (none of the real call sites do —
they all hold `&Vault`), interpolation is a no-op pass-through with no
sigils recognized... but since all real call sites have the vault, this
branch is moot; `SigilCtx` simply always carries periodic config.

### D5: Where interpolation runs

Applied at every `parse_with` site that has `&Vault` + `today`, **and**
on the resolved preset string before parsing:

1. `ft/src/cmd/tasks.rs::run_list` — interpolate `src` (the positional or
   `--query` string) before `parse_query`.
2. `ft/src/cmd/tasks.rs::resolve_preset` — interpolate the returned DSL
   string. (Built-in presets contain no sigils, so this is a no-op for
   them, but user presets may contain `@daily`.)
3. `ft/src/cmd/graph.rs::run` — interpolate `src` before `parse_with`.
4. `ft/src/cmd/graph.rs::resolve_preset` — interpolate the returned DSL
   string.
5. `ft/src/tui/tabs/tasks/search.rs` — interpolate the search-box text
   before `parse_query`.
6. `ft/src/tui/tabs/graph` view-apply path (`apply_query` and the
   preset-picker apply) — interpolate before parsing.
7. Other `parse_with` call sites in `tasks.rs` used by bulk
   complete/move/etc. (`do_complete`, `do_move`, …) — interpolate their
   query argument too, for consistency.

Interpolation is idempotent (expanded output contains no `@`-sigils
because string literals are opaque and the expanded values are
paths/dates with no `@`), so running it twice is harmless.

### D6: Errors

New `thiserror` enum in `ft-core`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum InterpolationError {
    #[error("unknown sigil `@{name}` at position {pos} (valid: today, daily, weekly, monthly, quarterly, yearly)")]
    UnknownSigil { name: String, pos: usize },
    #[error("sigil `@{period}` at position {pos} requires [periodic_notes.{period}] to be configured")]
    MissingPeriodicConfig { period: &'static str, pos: usize },
    #[error("invalid offset `{raw}` in sigil at position {pos}")]
    InvalidOffset { raw: String, pos: usize },
}
```

At the binary boundary, `anyhow::Context` wraps it; in `tasks`/`graph`
CLI commands it maps to exit code 2 (the same code already used for DSL
parse errors / unknown presets). In the TUI it surfaces as the inline
query-error string the search box already shows on `DslError`.

### D7: `@today` vs `@daily` both shipped in v1

`@today` needs no periodic config and immediately helps flat-ISO-filename
vaults; `@daily`/`@weekly`/… are the robust general form. Shipping both
in one change avoids a two-stage rollout and lets the docs present the
sigil layer as one feature. The cost is negligible — `@today` is a
one-line arm off the same resolver.

## Risks / Trade-offs

- **[Parse-error positions cite expanded text, not original]** After
  interpolation, byte offsets in `DslError` point into the expanded
  string (e.g. an error "at position 47" may land inside a substituted
  path). → Mitigation: the expanded string is still human-readable and
  the substituted values are self-explanatory (`"journal/2026/…"`);
  on error, the CLI can print the expanded query alongside the error so
  the user sees what was parsed. Full span remapping is deferred (v2)
  and only worth adding if users report confusion. No silent
  misbehavior — errors still surface, just at expanded positions.

- **[Sigil inside a DSL string literal]** `title includes "@daily"` would
  naively expand the sigil inside the quotes. → Mitigation: the
  interpolator scans the source with a mini-state machine that treats
  `"…"` / `'…'` spans as opaque (honoring `\\` escapes, reusing the
  lexer's string rules in spirit), so sigils inside string literals are
  left verbatim. A `@` outside any string is the only trigger.

- **[Missing periodic config yields a hard error, not empty match]** A
  preset using `@daily` in a vault with no `[periodic_notes.daily]`
  could either error or match nothing. → Mitigation: error explicitly
  (`MissingPeriodicConfig`) so the user learns the config gap rather
  than seeing a silent empty result. This is the friendlier failure.

- **[Offset units surprise]** `@monthly-1` shifts one calendar month
  (Jan 31 → Feb 28), not 30 days. → Mitigation: this is exactly
  `Period::offset_date`'s existing, tested semantics; documented in the
  sigil section. Consistent with the rest of ft's periodic handling.

- **[Two error types at the CLI]** Call sites now convert both
  `InterpolationError` and `DslError` to the exit-2 path. → Mitigation:
  a tiny helper maps either to the CLI error; no behavior change beyond
  a new error message shape.

- **[Presets now execute interpolation]** A user preset is now run
  through interpolation, which is a (very small) new capability on an
  existing input. → Mitigation: interpolation is total on
  sigil-free strings (passthrough), so existing presets are unaffected;
  the only new behavior is `@`-detection, which previously would have
  been a DSL `IllegalCharacter` error anyway, so no previously-valid
  preset changes meaning.
