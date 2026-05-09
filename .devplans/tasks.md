---
id: 001
name: tasks
title: "Tasks: foundation library + ft tasks CLI"
status: ready
created: 2026-05-09
updated: 2026-05-09
---

# Tasks: foundation library + ft tasks CLI

## Goal
Establish `ft` as a Rust workspace (library + binary) that can locate an
Obsidian vault, parse and serialize tasks in the Obsidian-Tasks emoji format
(matching plugin v7.22 behavior closely enough for round-trip safety), and
expose a first set of subcommands — `ft tasks list`, `ft tasks create`,
`ft tasks move`, `ft tasks complete` — that scratch real daily-driver itches
on a working PARA-style vault. This plan ships an end-to-end vertical slice;
later plans add a TUI, a cache layer, and notes commands on top.

## Motivation and Context
The user maintains a real vault at `/Users/cmw/git/fortytwo` (PARA layout,
Tasks plugin v7.22 active) and wants command-line + scriptable access to the
same task data Obsidian sees, without booting the Electron app. The CLI is
the foundation; a TUI (plan 002) and notes commands (plan 003) reuse this
library. Getting the parser and the data model right is load-bearing for
everything that follows, so this plan invests in a strong test bed
(fixture vaults, snapshot tests, proptest round-trips) before building UX.

## Acceptance Criteria

### Workspace & project skeleton
- [ ] Cargo workspace with members `ft` (binary, thin), `ft-core` (library)
- [ ] `cargo build --release` produces a single `ft` binary; `cargo test --workspace` passes
- [ ] `ft --version` and `ft --help` work; subcommand structure uses clap derive
- [ ] CI-ready: clippy clean with `-D warnings`, rustfmt clean, MSRV pinned in `rust-toolchain.toml` to a recent stable
- [ ] README with quick-start, install instructions (`cargo install --path ft`), and a one-page architecture overview

### Vault discovery & config
- [ ] Discovery precedence: `--vault` flag > `FT_VAULT` env > walk up from CWD looking for `.obsidian/` > named vaults in `~/.config/ft/config.toml` (`default` key)
- [ ] Per-vault config file at `<vault>/.ft/config.toml`, layered on top of user config (per-vault wins)
- [ ] Config schema covers: `default_task_location`, `daily_notes_path`, `daily_notes_format`, `ignored_paths`, `presets` (named queries)
- [ ] When no vault can be found, error message lists every location that was tried
- [ ] `ft vault info` subcommand prints resolved vault path, config files in effect, and merged config

### Task model (library)
- [ ] `Task` struct in `ft-core` with: `description`, `status` (enum: Open, Done, InProgress, Cancelled), `priority` (Highest..Lowest, optional), `tags`, `created`, `start`, `scheduled`, `due`, `done`, `cancelled` dates, `recurrence` (string form preserved verbatim for v1; semantic parsing deferred), `id`, `depends_on` (Vec<String>), `on_completion` field preserved verbatim, `block_link`, `source_file`, `source_line`, `indent_level`, `parent` (for subtask hierarchy)
- [ ] Standard statuses only: `[ ]` Open, `[x]`/`[X]` Done, `[/]` InProgress, `[-]` Cancelled. Unknown markers parse as Open with a warning surfaced via `tracing`
- [ ] Multi-level subtask support: indented `- [ ]` lines under a task become children; arbitrary depth
- [ ] Format module is trait-based (`TaskFormat` trait with `parse_line` / `serialize_line`); emoji format is the v1 implementation; dataview format is a deferred impl that will plug into the same trait
- [ ] Round-trip property: for any `Task` produced by parsing a real line, `serialize(parse(line)) == line` byte-for-byte (proptest covers generated tasks; snapshot tests cover real fixtures)
- [ ] Parser preserves unknown emojis/fields in a `raw_trailing` field so we never lose data on rewrite

### Vault scanning
- [ ] `Vault::scan()` walks markdown files using the `ignore` crate (respects `.gitignore` + configured `ignored_paths`)
- [ ] Parallel parsing with `rayon`; aim for sub-second scan on a vault with ~5k notes on the test machine
- [ ] Scan returns `Vec<Task>` with file/line provenance preserved
- [ ] Scan errors per-file are collected and reported, not fatal — one bad file does not abort the run
- [ ] Excludes binary files and any path under `.obsidian/`, `.git/`, `attachments/` (configurable)

### `ft tasks list`
- [ ] Flag-based filters: `--status`, `--priority`, `--tag`, `--path`, `--due-before`, `--due-after`, `--scheduled-before`, `--scheduled-after`, `--has-due`, `--no-due`
- [ ] `--query "<DSL>"` accepts a subset of the Obsidian Tasks query language: status / priority / path / tag predicates, date comparisons, `and`/`or`/`not`, `sort by <field>`, `limit N`. Document the supported subset; reject the rest with a clear error pointing to the docs section
- [ ] Flags compose with `--query` (flags appended as additional `and` clauses)
- [ ] `--sort` flag with multiple keys; default sort: due date asc, then priority desc, then path
- [ ] Output formats: `--format table` (default, with terminal width awareness via `comfy-table`), `--format json`, `--format ndjson`, `--format markdown` (emits the source lines so output can be piped back as a task list)
- [ ] Presets: `ft tasks list <preset-name>` looks up the preset in config; ships with built-ins `today`, `overdue`, `upcoming`, `done-today` that users can override
- [ ] `--group-by path|folder|due|priority|tag` for the table format
- [ ] Exit code 1 if no tasks match (configurable via `--allow-empty`) — useful in scripting

### `ft tasks create`
- [ ] Positional arg is the description; flags add metadata: `--due`, `--scheduled`, `--start`, `--priority`, `--tag` (repeatable), `--recurrence`, `--id`, `--depends-on`
- [ ] Date parsing accepts ISO (`2026-05-10`), relative (`+3d`, `tomorrow`), and natural language (`next monday`) — `chrono` + `chrono-english`
- [ ] Default location: today's daily note resolved from the daily-notes core plugin config (`<vault>/.obsidian/daily-notes.json`). If that file doesn't exist (templater not yet integrated), fail with a message that tells the user to either create it or pass `--file`
- [ ] `--file <path>` overrides location (relative to vault root)
- [ ] `--under-heading "<heading>"` inserts at the end of the section under that heading; creates the heading at file end if missing
- [ ] `--at-line N` inserts at a specific 1-indexed line
- [ ] `--append` appends to file end (default for daily note path with no heading)
- [ ] `--edit` opens `$EDITOR` on the resulting line after writing, positioned at the new task (use `EDITOR` env var; fall back to `vi`)
- [ ] Atomic writes: write to a temp file in the same directory, then rename; preserves file mode
- [ ] Idempotency: refuses to create an exact duplicate task (same description + same dates) on the same day unless `--force`

### `ft tasks complete`
- [ ] `ft tasks complete <selector>` marks one or more tasks done. Selector forms: task `id` (the `🆔 abc123` field), `<file>:<line>`, or interactive picker with `fzf`-style prompt (use `dialoguer` or `inquire`) when ambiguous
- [ ] Sets done date to today (or `--on <date>`)
- [ ] If task has `recurrence`, creates the next instance at the original location and marks the current one done — matching plugin behavior. Recurrence semantics in v1 cover daily/weekly/monthly with a clearly-tested whitelist; unsupported patterns error out with the exact unsupported token

### `ft tasks move` and bulk move
- [ ] `ft tasks move <selector> --to <file>[#heading]` moves a single task (and its subtasks) to the new location
- [ ] `ft tasks move --query "<DSL>" --to <file>[#heading]` bulk-moves all matching tasks; prompts for confirmation showing a count and a sample of 5 unless `--yes`
- [ ] Move preserves indentation/subtask hierarchy, rewrites the source files atomically, and updates internal `[[wikilinks]]` ONLY if the target file is in a different folder (deferred — note in code comments that this needs a follow-up plan)
- [ ] Dry-run with `--dry-run` prints the diff of every affected file without writing

### Error model & UX
- [ ] Library uses `thiserror` enums; binary uses `anyhow` with `Context`
- [ ] All errors include vault-relative paths (not absolute) where possible
- [ ] `--verbose` / `-v` flags map to `tracing` levels
- [ ] `--json-errors` produces structured error output for scripting
- [ ] Color output via `owo-colors`, auto-disabled when stdout is not a TTY or `NO_COLOR` is set

### Testing
- [ ] Unit tests live with the modules in `ft-core/src/`
- [ ] Integration tests under `ft/tests/` use `assert_cmd` + `assert_fs` against fixture vaults checked into `tests/fixtures/`
- [ ] At least three fixture vaults: `tiny/` (a few tasks, all formats), `realistic/` (tens of notes mirroring PARA layout), `pathological/` (deep subtasks, weird emoji combos, malformed lines)
- [ ] Snapshot tests with `insta` for every output format on each fixture
- [ ] Proptest round-trip on the parser (generated tasks → serialize → parse → equal)
- [ ] At least one test that runs against the real fortytwo vault if present (gated on env var `FT_REAL_VAULT_TESTS=1` so CI doesn't depend on it), comparing list output before/after `ft tasks complete` is a no-op
- [ ] Coverage target: 80%+ on `ft-core` (track via `cargo-llvm-cov` but don't gate CI on it)

### Documentation
- [ ] `docs/architecture.md` — workspace layout, key traits, where to add a new subcommand, where to add a new task format
- [ ] `docs/task-format.md` — exactly which Obsidian Tasks emoji fields are supported, with examples and a "deferred" section
- [ ] `docs/query-dsl.md` — the supported subset of the query language with grammar and examples
- [ ] `man/ft.1` and per-subcommand man pages generated from clap (use `clap_mangen`)
- [ ] Shell completions generated for bash/zsh/fish via `clap_complete`

## Technical Notes

### Workspace layout
```
ft/
├── Cargo.toml                  # workspace manifest
├── rust-toolchain.toml         # MSRV pin
├── ft/                         # binary crate (thin)
│   ├── Cargo.toml
│   ├── src/main.rs             # clap dispatch only
│   ├── src/cmd/                # one file per subcommand: list.rs, create.rs, move.rs, complete.rs, vault.rs
│   ├── src/output/             # table.rs, json.rs, markdown.rs
│   └── tests/                  # integration tests with assert_cmd
├── ft-core/                    # library crate (the brain)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── vault.rs            # discovery + scan
│       ├── config.rs           # layered config (user + vault)
│       ├── task/
│       │   ├── mod.rs          # Task struct, Status/Priority enums
│       │   ├── format.rs       # TaskFormat trait
│       │   └── emoji.rs        # emoji format impl
│       ├── query/
│       │   ├── mod.rs
│       │   ├── filter.rs       # programmatic filter API
│       │   ├── dsl.rs          # parser for the query subset
│       │   └── sort.rs
│       ├── daily.rs            # daily-notes core plugin config reader
│       └── error.rs
├── tests/
│   └── fixtures/
│       ├── tiny/
│       ├── realistic/
│       └── pathological/
└── docs/
```

### Library/binary boundary
The binary owns clap parsing, output rendering, terminal/TTY concerns, and the
editor handoff. Everything else — vault discovery, config, parsing, scanning,
filtering, sorting, mutation primitives — lives in `ft-core` and is consumed
unchanged by the TUI in plan 002. The library exposes both an "operations" API
(`scan_tasks`, `create_task`, `move_tasks`, `complete_task`) and the underlying
types so the TUI can compose finer-grained workflows.

### Why a trait for task formats
The plugin supports two serialization modes (emoji + dataview). v1 ships emoji
only, but the trait shape lets us add dataview as a sibling impl without
touching the rest of the codebase. Format detection per-line (a file can mix
both, in theory) is supported by trying parsers in priority order configured
via `.ft/config.toml` (`task_formats = ["emoji"]` initially).

### Dependencies (locked-in stack)
```
clap 4 (derive), pulldown-cmark, ignore, rayon, chrono, chrono-english,
serde, toml, figment, anyhow (binary), thiserror (lib), tracing,
tracing-subscriber, ratatui (plan 002 only), crossterm, comfy-table,
owo-colors, dialoguer or inquire, clap_mangen, clap_complete,
insta, assert_cmd, assert_fs, proptest
```

### Atomic file writes
Every task mutation goes through a single `write_atomic(path, content)` helper
that writes to `path.tmp-XXXX` in the same directory then renames. Same dir
matters for atomicity guarantees on POSIX. Preserve file mode and (where
practical) mtime semantics that don't fight git.

### Editor handoff
Only `--edit` triggers `$EDITOR`. The TUI may invoke it later for
"edit task in editor" actions but that's plan 002. Use `std::process::Command`
with the file path; pass `+<line>` for vim-family editors (parse `$EDITOR`
basename to decide).

### Parser strategy
The Tasks-plugin emoji format is **not** standard markdown extension syntax.
Each task is a line that starts (after indentation) with `- [<status>] ` and
then has the description with embedded emoji-prefixed fields. We do NOT need
pulldown-cmark for the task line itself — a hand-written parser scoped to "one
line at a time" is cleaner and gives us byte-accurate provenance. We use
pulldown-cmark only to find the list-item ranges in a file (so we know which
lines are actually task lines vs prose that happens to start with `- [`).

### Daily-notes resolution
Read `<vault>/.obsidian/daily-notes.json` for `folder`, `format` (moment.js
format string — translate to chrono format, with a small allowlist of tokens
documented in `docs/task-format.md`), and `template`. Fail loudly if the
moment.js format uses tokens we don't support, with a pointer to the doc.

### Out of scope for this plan
- Dataview format (trait is in place; impl is a future session)
- Custom statuses beyond the four standard ones
- Index/cache layer (mtime-based or sled) — only added if scan-on-demand
  is too slow on the real vault
- Templater integration to auto-create missing daily notes
- Rewriting wikilinks during moves (cross-folder)
- Recurrence patterns beyond daily/weekly/monthly basics
- Anything UI beyond the CLI (TUI = plan 002)

## Sessions
