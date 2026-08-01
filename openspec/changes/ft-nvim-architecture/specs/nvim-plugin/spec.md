# nvim-plugin

## Purpose

Define the architecture of `ft.nvim`, the Neovim plugin that surfaces ft
functionality inside the editor. The plugin is editor glue plus a
transport: all domain logic (task parsing/serialization, query
evaluation, dates, synth callouts, graph walks) lives in the `ft` CLI,
which the plugin drives through a single transport seam. The spec covers
the transport, the picker seam, the freshness model, the concurrency
contract with the ft TUI, the ft CLI protocol contract, and the
no-embeds invariant. The plugin repository is
`git@github.com:i7c/ft.nvim`; this spec lives in the `ft` repository as
the coordination artifact.

## Requirements

### Requirement: All ft interaction flows through the rpc transport seam

The plugin SHALL communicate with the `ft` binary exclusively through the
`ft.rpc` module. No other module SHALL invoke `vim.fn.system`, `io.popen`,
or equivalent process-spawning calls to run `ft`. `ft.rpc` SHALL construct
the command (honoring `FT_BIN` before `ft` on PATH), inject `--vault
<root>` when the vault is known, run the process, and return normalized
`(stdout, exit_code)` results with failures surfaced as `vim.notify`
notifications.

#### Scenario: Follow uses the seam
- **WHEN** the user presses the follow keymap on a `[[wikilink]]`
- **THEN** `ft.follow` resolves the target through `ft.rpc.call(...)`, and
  the resolved path is opened; no other code path spawns `ft`

#### Scenario: No bypass in the codebase
- **WHEN** the ft.nvim test suite scans `lua/ft/` for process-spawn calls
- **THEN** every call to spawn the `ft` binary is found inside
  `ft/rpc.lua` only

#### Scenario: Vault is passed explicitly
- **WHEN** `ft.rpc` builds a command and the vault root is known
- **THEN** the command includes `--vault <root>` so ft does not re-walk
  the filesystem on every invocation

### Requirement: Fast ops are synchronous, slow ops are asynchronous and single-flight

`ft.rpc` SHALL expose a synchronous `call()` for quick operations and an
asynchronous `job()` for expensive ones. `job()` SHALL run the ft process
via `vim.fn.jobstart` and deliver the result back through a
`User ft:rpc-done` autocmd carrying the job kind and output. Each job kind
SHALL have at most one in-flight job: a request while one is running SHALL
be coalesced (dirty flag) or rejected with a notification, never queued
unboundedly.

#### Scenario: Slow job does not freeze the editor
- **WHEN** the user triggers a gather (git-blame-scale work)
- **THEN** `ft.rpc.job` starts an async process and returns immediately,
  and nvim remains interactive until the `User ft:rpc-done` event fires

#### Scenario: Two gathers do not stack
- **WHEN** the user triggers a second gather while the first is in flight
- **THEN** the second request sets a dirty flag or is rejected with a
  notification, and exactly one additional job may run after the first
  completes

#### Scenario: Sync call for quick ops
- **WHEN** the user creates a task at the cursor line
- **THEN** `ft.rpc.call` runs `ft tasks create --at-line …` synchronously
  and the result is applied in the same interaction

### Requirement: Domain logic is never reimplemented in Lua

The plugin SHALL NOT parse or serialize task lines, evaluate query DSL,
compute dates, or serialize synth callouts in Lua. Operations that produce
or transform domain data SHALL be delegated to ft commands; Lua SHALL
only handle editor concerns (selection, insertion, cursor movement,
viewport rendering of ft-provided data).

#### Scenario: Task creation is delegated
- **WHEN** the user creates a task in place
- **THEN** the task line is produced by `ft tasks create` (or another ft
  command) and spliced into the buffer; the plugin does not assemble the
  line itself

#### Scenario: No emoji-format knowledge in the plugin
- **WHEN** the plugin source is reviewed for task-format knowledge
- **THEN** no priority/due/recurrence emoji handling or
  `✅ YYYY-MM-DD` completion-date serialization exists in Lua

### Requirement: Caches are derived and invalidated on events

The plugin's caches (note index, vault root) SHALL be derived from disk
and git state, never authoritative. The plugin SHALL mark caches dirty on
events that change vault state: writes to `.md` files inside the vault
(`BufWritePost`), file creation/deletion (`BufNewFile`, `BufDelete`), and
its own mutations. A dirty cache SHALL be rebuilt lazily on next use with
at most one rebuild in flight. Every cache build SHALL carry a monotonic
generation counter, and consumers that captured an older generation SHALL
detect it and re-derive.

#### Scenario: Completion picks up a newly created note
- **WHEN** the plugin creates a note and the user then types `[[`
- **THEN** the completion index has been rebuilt (or is rebuilding) and
  the new note appears in completions

#### Scenario: External edit invalidates the index
- **WHEN** a `.md` file inside the vault is written outside nvim and the
  user then opens completion
- **THEN** the index is marked dirty by the write event and is rebuilt
  before or during the completion session

#### Scenario: Stale snapshot is detected
- **WHEN** a picker was opened against an older index generation and the
  index is rebuilt before the user picks
- **THEN** the picker's data is re-derived from the new generation before
  the selection is acted on

### Requirement: The picker backend is a seam with a delegation default

`ft.picker` SHALL expose `select(items, opts)` and declare
`multi(items, opts)`. The default backend SHALL be `vim.ui.select`
(which delegates to the user's own override when one is installed);
telescope and fzf-lua backends SHALL be feature-detected and used only
when installed and enabled. `multi()` SHALL raise a clear
not-yet-implemented error until a feature requires it.

#### Scenario: No picker dependency required
- **WHEN** the user installs ft.nvim with no telescope, fzf-lua, or
  dressing
- **THEN** pickers work through stock `vim.ui.select`

#### Scenario: User's picker override is honored
- **WHEN** the user has a global `vim.ui.select` override (e.g. telescope)
- **THEN** `ft.picker.select` uses the override automatically

#### Scenario: Multi-select is honest about its state
- **WHEN** a feature calls `ft.picker.multi` before the backend decision
  is made
- **THEN** it raises a clear "multi-select not yet implemented" error
  rather than silently degrading to single-select

### Requirement: Embed rendering is not supported

The plugin SHALL NOT implement inline rendering of `![[embeds]]`. The
`embeds` / `max_lines` configuration keys SHALL be removed, and the
README SHALL state explicitly that embed rendering is no longer
supported.

#### Scenario: No embed code remains
- **WHEN** the plugin source is reviewed
- **THEN** no embed-rendering module or configuration exists

#### Scenario: Config key is dropped
- **WHEN** a user's config contains `embeds = { … }` after upgrading
- **THEN** the key is ignored without error, and the README documents the
  removal

### Requirement: The plugin never requires a live ft process during an editor session

When nvim is launched from inside the ft TUI, the TUI SHALL be treated as
parked for the duration of the editor session (it cannot respond to IPC
while blocked waiting for the editor) and as force-refreshing its graph
snapshot on editor exit. The plugin SHALL NOT attempt to communicate with
the running TUI process, and SHALL NOT require any ft process to be
responsive while nvim is open. Discovery SHALL rely on the inherited
`FT_VAULT` environment variable first, exactly as it does when nvim is
launched standalone.

#### Scenario: Launched from the TUI, discovery is free
- **WHEN** the ft TUI opens nvim as `$EDITOR`
- **THEN** the plugin resolves the vault from the inherited `FT_VAULT`
  with no filesystem walk, and no live-process channel is attempted

#### Scenario: No deadlock with a parked TUI
- **WHEN** nvim is open inside a suspended ft TUI
- **THEN** every plugin ft operation spawns a fresh `ft` child process
  and never waits on the parked TUI to answer

### Requirement: The ft CLI protocol contract is documented and version-checked

`ARCHITECTURE.md` in the plugin repository SHALL contain a protocol
contract table listing every ft command, flag, and output format the
plugin depends on (e.g. `ft graph query … --format ndjson`, `ft tasks
create --at-line`, `ft notes gather --json`). The plugin SHALL check
`ft --version` at setup and SHALL warn (not fail) when the installed
binary is older than the minimum the contract requires.

#### Scenario: Contract is discoverable
- **WHEN** a developer wants to know which ft features the plugin relies
  on
- **THEN** the protocol contract table in `ARCHITECTURE.md` lists the
  commands, flags, and output formats with the plugin features that use
  them

#### Scenario: Old binary warns
- **WHEN** the installed `ft` binary predates the documented minimum
  version
- **THEN** setup prints a warning naming the minimum version, and the
  plugin continues to load
