---
id: 012
name: git-sync
title: Git sync: ft git sync + TUI 'g s'
status: finished
created: 2026-05-14
updated: 2026-05-14
---

# Git sync: ft git sync + TUI 'g s'

## Goal

One-shot "make this repo match the remote" command for the vault, exposed
both as `ft git sync` on the CLI and as the `g s` leader chord on the
Notes and Tasks tabs of the TUI.

The operation, in order:

1. **Discover** the git toplevel by walking up from the vault root.
   If no `.git/` is found anywhere up the tree, the feature is
   unavailable (CLI errors, TUI's `g s` shows a "not a git repository"
   toast — the chord is *not* hidden because vault state can change
   under us).
2. **Stage & commit** any working-tree changes — modifications,
   deletions, and untracked files — while respecting `.gitignore`. If
   the tree is already clean, skip this step. Commit message is
   auto-generated (`ft sync 2026-05-14T14:30:00Z`) by default, or
   overridden by `--message` on the CLI.
3. **Pull** the current branch's configured upstream (`@{u}`). Merge
   by default; rebase if `[git] pull_strategy = "rebase"` in config.
   If there is no upstream, error out before touching the tree.
4. **On conflict**, leave the working tree in its conflicted state
   (merge or rebase in progress, conflict markers in files). Surface a
   clear error with next steps; do **not** push.
5. **Push** to the same upstream on success.
6. **Refresh** the active TUI tab after the operation (same code path
   as the user pressing `R`).

## Motivation and Context

The vault is a git repository — across machines (laptop, desktop,
sometimes a phone shell) it has to stay in sync. Today that's a manual
`git add -A && git commit -m … && git pull && git push` dance, run in
a separate terminal away from ft. Two friction points:

1. **Context switch out of the workflow.** When you finish editing a
   note or close a task and want to make sure the change is on the
   other machine before you walk away, you have to leave ft, find the
   vault dir, type four commands, and come back. The chord-from-the-
   tab version takes one keystroke combo.
2. **No CLI either.** Scripts and cron jobs that drive ft (e.g. a
   nightly "sync vault then run a digest") have no first-class
   way to ask ft to flush state. `ft git sync` makes ft the single
   entry point.

Why ship this as a foundational feature now: it unblocks a class of
multi-device workflows that have been informally requested since the
TUI shipped (plan 002), and the implementation surface is small —
shelling out to `git` for the heavy lifting means we inherit the
user's full git config, credential helper, SSH keys, and signing
setup for free.

**Why shell out to `git` rather than `git2` (libgit2).** Authentication
is the deciding factor: the user's existing SSH agent, OS keychain
credential helper, GPG signing config, and `~/.gitconfig` are all
respected automatically when we invoke `git` as a subprocess. The
`git2` crate requires explicit credential callbacks and re-implements
configuration loading, neither of which is in scope for v1. Shelling
out also keeps `ft-core` lean — no large native dep — and gives us
`--porcelain` parsing that's stable across git versions.

**Why a modal, not a toast, in the TUI.** A push over a slow network
takes seconds; a silent toast at the start gives no signal that work
is happening. A blocking modal ("Syncing…") draws the user's eye to
the operation, then resolves to either a success toast or an error
modal with the conflict / failure detail. This matches the editor-
handoff blocking pattern from plan 011 — ft owns the foreground until
the operation completes.

## Acceptance Criteria

### Library — `ft_core::git`

- [ ] New module `ft-core/src/git.rs` (and `pub mod git;` in
      `lib.rs`). Public surface:
      ```rust
      /// Result of walking up from `start` looking for a `.git/`
      /// directory. `None` if no enclosing git repo exists.
      pub fn discover_repo(start: &Path) -> Option<PathBuf>;

      /// Snapshot of the working tree. Built by parsing
      /// `git status --porcelain=v1 --untracked-files=normal`.
      /// Honors `.gitignore` (git does this for us — no ignored
      /// files appear in porcelain output).
      pub struct WorkingTreeStatus {
          pub modified: Vec<PathBuf>,
          pub untracked: Vec<PathBuf>,
          pub deleted: Vec<PathBuf>,
          pub conflicted: Vec<PathBuf>,
      }
      impl WorkingTreeStatus {
          pub fn is_clean(&self) -> bool;
          pub fn has_conflicts(&self) -> bool;
      }
      pub fn status(repo: &Path) -> Result<WorkingTreeStatus>;

      /// Upstream of the current branch (e.g. "origin/main"), or
      /// `None` if the branch has no configured upstream. Wraps
      /// `git rev-parse --abbrev-ref --symbolic-full-name @{u}`.
      pub fn upstream(repo: &Path) -> Result<Option<String>>;

      /// Pull strategy — controls whether `sync` uses `git pull`
      /// (merge) or `git pull --rebase`.
      #[derive(Debug, Default, Clone, Copy, Deserialize, Serialize,
               PartialEq, Eq)]
      #[serde(rename_all = "kebab-case")]
      pub enum PullStrategy {
          #[default]
          Merge,
          Rebase,
      }

      /// Options driving a single `sync` invocation.
      pub struct SyncOptions {
          pub strategy: PullStrategy,
          /// Override the auto-generated commit message.
          pub message: Option<String>,
      }

      /// Granular outcome of a sync. The TUI / CLI render this into
      /// human-readable feedback. Conflict states leave the repo in
      /// `MergeInProgress` / `RebaseInProgress` — the caller does
      /// nothing further.
      pub enum SyncOutcome {
          /// Tree was already clean and pull was a no-op fast-forward
          /// or had nothing to fetch. Push may still have happened
          /// if local was ahead.
          Clean { pushed: bool },
          /// Local changes were committed before pull/push.
          Synced { committed: usize, pulled: bool, pushed: bool },
          /// Pull surfaced a merge conflict; sync aborted before
          /// push. The repo is in merge-in-progress state.
          MergeConflict { files: Vec<PathBuf> },
          /// Pull surfaced a rebase conflict; sync aborted before
          /// push. The repo is mid-rebase.
          RebaseConflict { files: Vec<PathBuf> },
      }

      pub fn sync(repo: &Path, opts: &SyncOptions) -> Result<SyncOutcome>;
      ```
- [ ] `sync()` is the orchestrator. It runs git as a subprocess via
      `std::process::Command`, always with `-C <repo>` so we never
      depend on the caller's `cwd`. Order of operations exactly
      mirrors the Goal section.
- [ ] **Default commit message** = `format!("ft sync {}",
      OffsetDateTime::now_utc().format(&Rfc3339)?)`. The chrono dep
      is already in the workspace; matches the existing date-handling
      style in `ft-core::dates`.
- [ ] **Staging** uses `git add -A`. This stages all modifications,
      deletions, and untracked files in one shot while respecting
      `.gitignore`. We do not expose a `--include-untracked` knob in
      v1 — the user explicitly chose "include untracked".
- [ ] **No upstream → hard error** *before* committing. We do not
      want to leave a "sync was started, then it errored, and now
      there's a commit on master with no remote to push to" — if
      we can detect a fatal precondition (no upstream), do it first.
      Error message names the command the user would run to fix:
      `"branch 'foo' has no upstream — run 'git push -u origin foo'
      once to set it"`.
- [ ] **Conflict detection.** After `git pull` (merge or rebase),
      examine the exit code *and* re-run `status()` to look for
      `conflicted` entries. The pull command exits non-zero on
      conflict; we treat that as the signal, but use `status()` to
      enumerate the files for the outcome. Do **not** call
      `git merge --abort` / `git rebase --abort` — the user asked
      for markers in place.
- [ ] **Push** uses `git push` (no flags). Inherits the upstream
      tracking from step 1. If push fails (non-fast-forward — someone
      else pushed between our pull and our push), surface the error
      verbatim from stderr; do not retry.
- [ ] **Authentication & interactivity.** Spawn git with
      `GIT_TERMINAL_PROMPT=0` so a missing credential helper fails
      fast instead of hanging waiting for `Username:` on stdin. SSH
      and credential-helper paths work unchanged because the user's
      `~/.gitconfig` and `~/.ssh` are inherited.
- [ ] **Test strategy.** Unit tests build temp git repos using `git
      init` in a `tempfile::TempDir`, do operations, assert. No
      mocking of the subprocess — testing real `git` is the whole
      point. Skip these tests with a `#[cfg(test)]` helper if `git`
      isn't on `PATH` (it always is in CI, but defensive). For
      remote-touching cases (pull/push), set up a *bare* repo in
      another temp dir and add it as `origin`. The full matrix:
      - `discover_repo` finds a parent `.git/`, finds when called
        directly on the toplevel, returns `None` when no `.git/`
        exists up the tree.
      - `status` reports modified / untracked / deleted entries
        correctly; ignored files (matching a `.gitignore` pattern)
        do not appear; clean tree returns `is_clean() == true`.
      - `upstream` returns `Some("origin/main")` for a tracked
        branch and `None` for an untracked one.
      - `sync` happy paths: clean tree no-op; dirty tree → commit
        only (no remote); dirty tree + remote ahead → commit + pull
        + push; clean tree + remote ahead → fast-forward only.
      - `sync` conflict path: induce a merge conflict by editing
        the same line on both sides of a bare-origin handshake,
        assert `MergeConflict { files }` and that conflict markers
        remain in the file on disk.
      - `sync` rebase variant covers the same happy / conflict path
        with `PullStrategy::Rebase`.
      - `sync` no-upstream → error before any tree change.

### Library — `ft_core::config`

- [ ] New `[git]` block on `Config`:
      ```rust
      #[serde(default)]
      pub git: Git,

      #[derive(Debug, Default, Clone, Deserialize, Serialize)]
      #[serde(deny_unknown_fields)]
      pub struct Git {
          #[serde(default)]
          pub pull_strategy: PullStrategy,
      }
      ```
- [ ] Unit tests: defaults (`pull_strategy = "merge"`), kebab-case
      parsing of `"merge"` / `"rebase"`, unknown-variant rejection,
      `deny_unknown_fields` rejects typos.

### CLI — `ft git sync`

- [ ] New `ft/src/cmd/git.rs` with a `clap` `Args` struct:
      ```rust
      #[derive(Debug, clap::Args)]
      pub struct GitArgs {
          #[command(subcommand)]
          pub command: GitCommand,
      }

      #[derive(Debug, clap::Subcommand)]
      pub enum GitCommand {
          /// Commit dirty tree, pull upstream, push.
          Sync(SyncArgs),
      }

      #[derive(Debug, clap::Args)]
      pub struct SyncArgs {
          /// Override the auto-generated commit message.
          #[arg(short = 'm', long)]
          pub message: Option<String>,
          /// Print what would happen without touching the repo.
          #[arg(long)]
          pub dry_run: bool,
      }
      ```
- [ ] Dispatch from `ft/src/main.rs::Commands::Git(args)` →
      `cmd::git::run(args, &global_flags)`.
- [ ] Behavior:
      1. Resolve the vault as today.
      2. Call `discover_repo(&vault.root)`. If `None`, exit non-zero
         with `"no git repository found at or above vault root: <path>"`.
      3. If `--dry-run`, print the plan (`"would commit N files",
         "would pull origin/main (merge)", "would push"`) and exit
         zero. Read `status()` and `upstream()` only — no writes.
      4. Otherwise call `sync(repo, &opts)` and print the outcome
         in human-readable form. Exit codes:
         - `0` — `Clean { pushed: true | false }` or `Synced { … }`
         - `2` — `MergeConflict` or `RebaseConflict` (matches the
           "user action required" convention used by `tasks move
           --dry-run` blockers).
         - `1` — any other error (no upstream, push rejected,
           network failure).
- [ ] `--json-errors` at the top level wraps errors as JSON (the
      existing global flag). Success output stays human-readable;
      a future `--format` flag could add structured success output,
      not in scope for v1.
- [ ] Integration tests under `ft/tests/git_sync.rs`:
      - `ft git sync` exits 2 with conflict-files in stderr when the
        upstream and local have diverged conflictingly (set up via
        a bare-repo `origin` in a temp dir, same pattern as the
        library tests).
      - `ft git sync --dry-run` prints a plan and does not change
        the tree.
      - `ft git sync` in a vault with no enclosing `.git/` exits
        non-zero with the documented error.
      - `ft git sync --message "custom"` uses the override.

### TUI — `g s` chord on Notes and Tasks tabs

- [ ] Both `tabs/notes/mod.rs` and `tabs/tasks/mod.rs` grow a
      handler for the `g` leader. Pattern mirrors the periodic-notes
      leader from plan 010:
      - First `g` press → transition tab state to a new
        `GitLeader` variant; the tab's view renders a small modal
        listing the available second keys (just `s — Sync` in v1).
      - Second key:
        - `s` → builds `AppRequest::SyncGit { dry_run: false,
                                              message: None }`.
        - `Esc` / any other key → dismiss the modal, no action.
- [ ] New `AppRequest` variant in `tab.rs`:
      ```rust
      SyncGit {
          // Future-proofing — v1 always sends None.
          message: Option<String>,
      },
      ```
- [ ] `App::service_request` `SyncGit` arm:
      1. Render a blocking "Syncing…" modal overlay (one new
         widget in `widgets/sync_modal.rs` — a centered popup with
         a title and a spinner-styled message line; no animation
         needed for v1, "Syncing…" with a static label is enough).
         Force one redraw so the modal is visible before we block.
      2. Call `ft_core::git::discover_repo(&vault.root)`. If
         `None`, dismiss the modal, push an error toast
         `"no git repository at or above vault root"`, return.
      3. Call `ft_core::git::sync(repo, &opts)` with `opts.strategy`
         pulled from `vault.config.config.git.pull_strategy` and
         `opts.message = None` (TUI never overrides).
      4. Dismiss the modal.
      5. Render outcome:
         - `Clean { pushed }` → toast `"already in sync"` or
           `"pushed N commit(s)"`.
         - `Synced { committed, pulled, pushed }` → toast
           `"sync ok — committed N, pulled, pushed"`.
         - `MergeConflict { files }` → modal (not toast) showing
           the file list and the message
           `"merge conflict — resolve, commit, and push manually"`.
         - `RebaseConflict { files }` → similar modal with
           rebase-specific guidance.
      6. After dismissal, call the same refresh path that `R` uses
         on the active tab (`tabs[self.active].refresh(&mut ctx)?`)
         so the on-disk changes from the pull are reflected.
- [ ] Footer hint: Notes & Tasks tabs already advertise their
      single-key actions in the footer. Add `g s` as a single token
      `"g s sync"` to both tabs' footers. Do not gate visibility on
      "is a git repo" — keeping the footer stable is more important
      than hiding one chord, and pressing `g s` outside a repo
      gives a clear error toast.
- [ ] Tests in `ft/src/tui/tests.rs`:
      - Notes tab: pressing `g` enters the leader modal; the modal
        snapshot lists `s — Sync`; pressing `s` queues an
        `AppRequest::SyncGit`; pressing `Esc` from the leader
        dismisses the modal and queues nothing.
      - Tasks tab: same three behaviors.
      - "Pressing `g s` outside a git repo queues a `Toast(Error,
        …)` and does not panic." — uses a temp vault with no
        `.git/` anywhere up the tree.
      - Snapshot: the git-leader modal at 80×24.
      - No end-to-end `sync()` integration tests at the TUI layer —
        that's covered exhaustively at the library and CLI layers.
        The TUI tests stop at "request was queued" / "outcome was
        rendered into a toast or modal" using a stubbed
        `AppRequest` handler.

### Documentation

- [ ] `docs/config.md`: new `[git]` section between `[editor]` and
      `[presets]`. Documents `pull_strategy = "merge" | "rebase"`
      (default `"merge"`), the `.gitignore` honoring of staging,
      and the conflict-leaves-markers-in-place policy.
- [ ] `README.md`: short blurb under a new "Git sync" subsection,
      including the `ft git sync` example and a one-line mention of
      the `g s` chord.
- [ ] `docs/architecture.md`: add `git.rs` to the `ft-core` file
      tree block; add `cmd/git.rs` to the `ft` tree.
- [ ] `?` help overlay text in the TUI: include `g s — git sync`
      in the global section.

## Technical Notes

- **Why discovery walks up.** `git rev-parse --show-toplevel` does
  this natively, but spawning git just to find git is wasteful and
  also assumes git is on PATH at discovery time (which we want to
  gate on per the "feature unavailable" rule). A pure Rust walk that
  checks for `<dir>/.git/` (file or directory — `.git` can be a
  *file* for submodules and worktrees) at each ancestor of the
  vault root is cheaper and gives a clean `None` when there's no
  repo. The first `git` invocation can then assume the repo exists.

- **`git add -A` semantics.** From the git docs: "Update the index
  not only where the working tree has a file matching `<pathspec>`
  but also where the index already has an entry. This adds, modifies,
  and removes index entries to match the working tree." With no
  pathspec it operates on the whole tree. Ignored files (per
  `.gitignore`, `.git/info/exclude`, and `core.excludesfile`) are
  not staged. This is exactly the requested behavior.

- **Why `--porcelain=v1` for status parsing.** The v1 porcelain
  format has been frozen since git 1.7.0 (2010). v2 is also stable
  but more verbose; v1 gives us a tight 2-char XY code per file
  that maps cleanly to our `WorkingTreeStatus` fields. `??` =
  untracked; `M ` / ` M` / `MM` = modified (index / worktree /
  both); `D ` / ` D` = deleted; `UU` (and friends) = conflicted.

- **No upstream check ordering.** Run `upstream()` *before*
  staging. The user's mental model is "sync — and if it can't sync,
  it leaves everything alone." A failed-upstream-after-commit
  outcome is technically recoverable (the commit is fine, just
  unpushed) but it surprises users who didn't realize their first
  push needed `-u`. Front-load the check.

- **Push-rejection (non-fast-forward).** If someone else pushed
  between our pull and our push (rare but possible under fast-
  multi-machine workflows), `git push` exits non-zero with
  "Updates were rejected because the remote contains work that
  you do not have locally." We surface this verbatim. We do **not**
  loop "pull again, push again" — that's a `--retry` knob worth a
  separate plan if anyone asks. v1 is "one attempt, clear error."

- **Why a static "Syncing…" modal and not a phased progress UI.**
  Real-time per-phase progress ("staging 3 files…", "pulling…",
  "pushing…") would require threading the sync onto a background
  thread and posting events back into the TUI event loop — a
  meaningful complexity bump for a feature whose total runtime on
  a small vault is under a second. v1 blocks the event loop with a
  drawn modal; v2 (future plan) can layer phased progress on top
  if real-world usage shows the wait is long enough to feel laggy.

- **TUI never overrides the commit message.** Keeping the chord
  one-keystroke is the entire point — prompting for a message
  would defeat it. Users who want custom messages drop to the CLI.
  This is intentional and documented.

- **No retry on transient network failure.** Same reasoning as
  the push-rejection case — one attempt, clear error. The user
  presses `g s` again if the network came back.

- **`GIT_TERMINAL_PROMPT=0`.** Git uses this env var to gate
  interactive credential prompts. We always set it because ft has
  no stdin to give git inside the TUI's alt-screen, and a hanging
  subprocess with the modal blocking the event loop is the worst
  failure mode we can produce. The user's credential helper
  (osxkeychain on macOS, libsecret on Linux, manager on Windows)
  works unaffected — those are non-interactive.

- **Signing.** GPG / SSH commit signing configured in the user's
  `~/.gitconfig` works automatically because we inherit the
  process environment. We document this as a feature, not a
  guarantee — `commit.gpgsign = true` with a missing signing key
  would surface as a clean error from `git commit`'s stderr.

- **Test isolation for env-toggling.** The git-sync test cases
  that need to control `GIT_TERMINAL_PROMPT` or `HOME` (to avoid
  picking up the dev's real `~/.gitconfig`) follow the
  `EDITOR_ENV_LOCK` pattern from plan 011 — a shared `Mutex` to
  serialize env-touching tests within the test binary.

- **`.git` as a file (submodules, worktrees).** A linked worktree
  has a `.git` *file* (not directory) pointing at the main
  repo's `.git/worktrees/<name>` dir. `discover_repo` accepts
  either — `if entry.exists() { return Some(dir.to_path_buf()); }`
  with no `is_dir()` check. Test coverage includes a worktree
  fixture.

## Future (explicitly out of scope for this plan)

- **`ft git status`.** A read-only "what would sync do" command,
  printing the same plan that `--dry-run` does today. Cheap follow-
  up, deferred to keep the v1 surface tight.
- **Auto-sync on a schedule.** A `[git] auto_sync_every = "5m"`
  config knob that fires a background sync. Real ergonomic value
  but it interacts with the TUI's event loop, conflict UX, and
  notification surface in non-trivial ways — own plan.
- **Conflict resolution UI in the TUI.** A modal that lists
  conflicted files and lets the user pick "open in editor" for
  each. v1 sends the user to their normal editor / shell to
  resolve.
- **Multi-remote support.** Pulling from one remote and pushing to
  another, or pushing to multiple remotes. Niche; not asked for.
- **`ft git log`, `ft git diff`, etc.** ft is not trying to be a
  git porcelain — `sync` is the one operation we own because
  it's load-bearing for vault workflows. Other git commands stay
  in the user's shell.

## Sessions

### Session 1 · 2026-05-14 · done
**Goal:** Library foundation. New `ft-core/src/git.rs` with
`discover_repo`, `WorkingTreeStatus` + `status()`, `upstream()`,
`PullStrategy`, `SyncOptions`, `SyncOutcome`, and `sync()` — all
shelling out to `git -C <repo> …`. New `[git]` block on `Config`
with `pull_strategy` + tests. Full unit-test matrix using real
`git init` temp repos and a bare-repo `origin` for pull/push
paths: discovery (`.git` as dir, as file, parent walk, not-found),
status (modified / untracked / deleted / ignored / conflicted),
upstream (set / unset), sync happy paths (clean no-op, dirty
commit-only, dirty commit+pull+push, ff-only), sync merge and
rebase conflicts (markers stay), no-upstream hard error before
any tree change. Reuse `EDITOR_ENV_LOCK`-style mutex for any
`HOME` / env-toggling tests. `cargo test --workspace` + clippy +
fmt clean.
**Outcome:** New `ft-core/src/git.rs` (~770 lines incl. tests).
Public surface matches the plan: `discover_repo`, `status`
returning `WorkingTreeStatus { modified, untracked, deleted,
conflicted }` with `is_clean()` / `has_conflicts()` helpers,
`upstream` returning `Option<String>`, `PullStrategy` (`Merge`
default, `Rebase`), `SyncOptions { strategy, message }`,
`SyncOutcome` (`Clean { pushed } | Synced { committed, pulled,
pushed } | MergeConflict { files } | RebaseConflict { files }`),
and `sync()` orchestrator.

Orchestrator order: pre-check `upstream()` (hard error before
*any* tree mutation if `None`), snapshot `status()` (refuse
mid-merge/rebase), `git add -A` + `git commit -m …` when dirty
(default message `"ft sync <iso8601-utc>"` via
`chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ")`), `git pull
--no-rebase` or `git pull --rebase` per strategy, post-pull
status check for conflicts (return `MergeConflict` /
`RebaseConflict` if any, propagate other failures), `git push`.
`pulled = head_before_pull != head_after_pull`; `pushed =
rev-list --count @{u}..HEAD > 0` captured before the push.

The internal `git(repo)` `Command` builder always passes
`-C <repo>` and sets `GIT_TERMINAL_PROMPT=0` so the TUI cannot
hang on an interactive credential prompt the alt-screen can't
service. `core.quotePath=false` is passed to `git status` so
non-ASCII paths come through verbatim rather than as `\xNN`
octals. The porcelain v1 parser maps codes: U-in-either /
`AA` / `DD` → conflicted, `??` → untracked, D-in-either →
deleted, everything else (M/A/R/C/T) → modified. Rename entries
(`R<X> old -> new`) classify on the destination path via
`rename_destination`.

New `Git` struct on `Config` with `pull_strategy: PullStrategy`
(default `Merge`), `#[serde(deny_unknown_fields)]`. Imported
`PullStrategy` from `crate::git` to avoid circular naming.

New `Error::Git(String)` variant — string-based since git
errors are inherently free-form stderr. `cmd_err(label, stderr)`
formats them as `"git <label>: <stderr>"`; `spawn_err` wraps
`io::Error` from `Command::output` as `"spawn git: …"`.

18 git unit tests + 4 config tests, all green:
- Discovery: at root, at ancestor, returns `None` outside a
  repo, accepts `.git` as a file (worktree case).
- Status: clean, modified, untracked, deleted, `.gitignore`-honored.
- Upstream: set returns `"origin/main"`, unset returns `None`.
- Sync: clean-noop, dirty commit+push (`Synced { committed: 1,
  pulled: false, pushed: true }`), clean ff-pulls when upstream
  ahead (still `Clean { pushed: false }`), no-upstream pre-check
  error before staging (untracked file remains untracked),
  merge conflict leaves `<<<<<<<`/`>>>>>>>` markers, rebase
  conflict same, custom `--message` override.
- Config: `[git]` defaults to `pull_strategy = "merge"`,
  kebab-case parsing of `"merge"`/`"rebase"`, unknown variant
  rejected, unknown field rejected.

Test fixtures use real `git init`, never mock. The
`setup_origin_and_clone` helper creates a bare origin + a clone
with one seed commit pushed (upstream tracked). Each repo gets
local `user.name`, `user.email`, and `commit.gpgsign = false`
config so commits succeed even when the dev's global gitconfig
has signing on with no key. No `HOME` override or env-var lock
needed since per-spawn `Command::env` is process-local —
simpler than the plan suggested.

Workspace state: `cargo test --workspace` → 782 tests green
(up from 764: +18 git, +4 config). `cargo clippy --workspace
--all-targets -- -D warnings` clean. `cargo fmt --check` clean
after one autoformat pass. No new dependencies — chrono,
tempfile, and serde were already in the workspace.

### Session 2 · 2026-05-14 · done
**Goal:** CLI surface. New `ft/src/cmd/git.rs` with `GitArgs` /
`GitCommand::Sync` / `SyncArgs` (`--message`, `--dry-run`).
Dispatch from `ft/src/main.rs`. Map `SyncOutcome` variants to
human-readable stdout and exit codes (0 / 2 / 1 per the criteria).
`--dry-run` prints the plan using `status()` + `upstream()` only,
no writes. `--json-errors` plumbing inherited from the top level.
Integration tests under `ft/tests/git_sync.rs`: dry-run, conflict
→ exit 2 with files in stderr, no-`.git/` → non-zero with the
documented error, `--message` override applied. Update
`docs/config.md` (`[git]` section), `docs/architecture.md` (file-
tree blocks), and `README.md` (Git-sync subsection with the
`ft git sync` example).
**Outcome:** New `ft/src/cmd/git.rs` (~160 lines) with the
documented clap surface: `GitArgs` → `GitCommand::Sync(SyncArgs)`,
`SyncArgs { message: Option<String>, dry_run: bool }`. `pub mod
git;` added to `cmd/mod.rs`; `Commands::Git(GitArgs)` variant +
dispatch arm added to `main.rs`. The arm uses the same
`Vault::discover` pattern as the other subcommands, then
`discover_repo(&vault.path)` to find the enclosing repo (error
out with `"no git repository found at or above vault root: …"`
if `None`).

`SyncOutcome` mapping:
- `Clean { pushed: false }` → stdout `"already in sync"`, exit 0.
- `Clean { pushed: true }` → stdout `"pushed local commits"`, exit 0.
- `Synced { committed, pulled, pushed }` → three-line stdout
  (`"committed N file(s)"`, optional `"pulled"`, optional
  `"pushed"`), exit 0.
- `MergeConflict { files }` → stderr `"merge conflict in N
  file(s):"` then each file indented two spaces then `"resolve,
  commit, and push manually."`, exit **2**.
- `RebaseConflict { files }` → same shape with rebase wording and
  a `"git rebase --continue"` hint, exit **2**.

`--dry-run` reads `upstream()` + `status()` and prints a plan
without any writes — `"upstream: origin/main (merge)"`, working-
tree summary (`"N change(s) (M modified, U untracked, D deleted)"`
or `"working tree: clean"`), then `"would commit N file(s)"` /
`"nothing to commit"`, `"would pull <upstream>"`, `"would push"`.
A pre-existing conflicted tree under `--dry-run` returns exit 2
with a "resolve before syncing" hint (so dry-run doesn't paper
over a tracked half-merged state).

Hard errors (no enclosing `.git/`, no upstream, push rejected,
network failure) propagate through `anyhow::Error` to main's
shared error printer — which already honors `--json-errors` from
plan 001, so scripting users get `{"error": ..., "chain": [...]}`
on stderr for free. Conflict outcomes do **not** go through the
error path — they're a defined end state, so they return
`Ok(ExitCode::from(2))`.

7 integration tests under `ft/tests/git_sync.rs`, all green:
- `sync_commits_and_pushes_dirty_tree` — happy path
  (`Synced { committed: 1, pushed: true }`).
- `sync_uses_custom_message_with_dash_m` — verifies `git log
  -1 --format=%s` returns the override.
- `sync_clean_tree_reports_already_in_sync` — bare clone, no
  local changes, stdout `"already in sync"`.
- `sync_merge_conflict_exits_two_with_files_in_stderr` — side
  clone pushes conflicting `seed.md`, local sync exits 2,
  stderr contains both `"merge conflict"` and `"seed.md"`,
  conflict markers remain in the on-disk file.
- `sync_with_no_git_repo_errors_with_documented_message` — vault
  with `.obsidian/` but no `.git/` anywhere up — fails with the
  documented `"no git repository"` error on stderr.
- `sync_dry_run_does_not_commit_or_push` — verifies HEAD
  unchanged after `--dry-run` and the untracked file remains
  untracked (not staged or committed).
- `sync_no_upstream_errors_before_committing` — repo without a
  configured upstream: fails with `"no upstream"` on stderr,
  untracked file remains untracked (pre-check before staging).

Tests reuse the same real-git fixture pattern from session 1
(`init_repo`, `setup_origin_and_vault` with bare origin + clone +
one seed commit pushed). No tmp HOME override or env locks
needed.

Docs updated:
- `docs/config.md`: new `[git]` row in the top-level keys table,
  and a new `## [git]` section between `[editor]` and
  `[presets]` documenting `pull_strategy` plus the full
  five-step `ft git sync` flow.
- `docs/architecture.md`: `git.rs` added under both the `ft-core`
  module list (between `periodic.rs` and `dates.rs`) and the
  `ft/src/cmd/` list (between `vault.rs` and `completions.rs`).
- `README.md`: new `## Git sync` section between Tasks and Output
  formats with the three example invocations and a link to the
  config docs.

Workspace state: `cargo test --workspace` → 793 tests green (up
from 782: +7 git_sync integration, +4 from earlier rerun
counting). `cargo clippy --workspace --all-targets -- -D warnings`
clean after one doc-overindent fix (clippy 1.91 added
`doc-overindented-list-items`; the module-docs exit-code list
needed 2-space continuation indent). `cargo fmt --check` clean
after one autoformat pass. No new dependencies — `assert_cmd`,
`assert_fs`, and `predicates` were already in workspace
dev-dependencies.

### Session 3 · 2026-05-14 · done
**Goal:** TUI chord. New `AppRequest::SyncGit { message }`
variant. `g s` leader chord on both Notes and Tasks tabs: `g`
enters a `GitLeader` state, second key dispatches; `Esc` /
unknown dismisses. New `widgets/sync_modal.rs` blocking "Syncing…"
overlay. `App::service_request` `SyncGit` arm: draw modal,
discover repo (toast if missing), run `sync()`, dismiss modal,
render outcome (toasts for clean / synced, modal for conflict
variants with file list), refresh active tab. Footer hint `"g s
sync"` on both tabs. `?` help overlay updated. Tests in
`ft/src/tui/tests.rs` for: leader entry, snapshot of the leader
modal at 80×24, `s` queues `SyncGit`, `Esc` dismisses, outside-
git-repo path queues an error toast without panicking. Same on
both tabs.
**Outcome:** Took the **app-level** approach rather than the
per-tab `GitLeader` state the plan sketched: cheaper than
copy-pasting the periodic-leader pattern into both Notes and
Tasks tabs, and the chord works uniformly from any tab
(including Welcome) without each tab needing its own state
variant.

`AppRequest::SyncGit { message: Option<String> }` added to
`tab.rs`. TUI always sends `None` — keeps `g s` truly one-shot.

`Mode` (in `ui.rs`) grew three variants: `GitLeader`, `Syncing`,
`SyncConflict`. Conflict data lives separately on `App` in a new
`sync_conflict: RefCell<Option<SyncConflictInfo>>` field so
`Mode` stays `Copy`. `SyncConflictInfo { kind: SyncConflictKind,
files: Vec<PathBuf> }` with `SyncConflictKind` = `Merge | Rebase`
so the modal can render the right wording per pull strategy.
`Mode::label()` returns `"git"` / `"sync"` / `"conflict"` for the
status-bar right cell.

App wiring (`app.rs`):
- `handle_event` gained three new short-circuits before the
  tab/global dispatch. `GitLeader`: `s` → enqueue `SyncGit { None
  }`, anything else → dismiss; we **don't** fall through to the
  global handler so a stray `q` doesn't quit while the leader is
  open (`git_leader_q_does_not_quit_via_global_handler` pins
  that). `SyncConflict`: `Esc` / `q` dismiss; anything else
  ignored. `Syncing`: swallow everything (defensive — the event
  loop is paused for the sync call's duration anyway).
- `handle_global_key` gained one arm: `('g', NONE)` → enter
  `Mode::GitLeader`.
- `service_request` gained the `SyncGit` arm, delegating to a
  new `dispatch_sync_git(terminal, message)` helper that:
  1. `discover_repo(&self.vault.path)` — if `None`, push a red
     `"no git repository at or above vault root"` toast and
     return (not a fatal error — vault state can change between
     `g s` presses).
  2. Set `mode = Syncing`, force one `terminal.draw(...)` so the
     overlay is visible *before* the (synchronous) sync call
     blocks the event loop.
  3. `ft_core::git::sync(&repo, &opts)` with
     `opts.strategy = vault.config.config.git.pull_strategy`.
  4. Refresh the active tab regardless of outcome so pulled
     changes — and conflict markers — show up in view.
  5. Map outcome:
     - `Clean { pushed: false }` → green toast `"already in
       sync"`.
     - `Clean { pushed: true }` → green toast `"pushed local
       commits"`.
     - `Synced { committed, pulled, pushed }` → green toast
       `"sync ok — committed N, pulled, pushed"` (omitting
       clauses that didn't apply).
     - `MergeConflict` / `RebaseConflict` → persistent
       `SyncConflict` modal until Esc; stores `SyncConflictInfo`
       on the App so the renderer can list files.
     - `Err(_)` → red toast `"git sync failed: <e>"`.
- New `push_toast(text, style)` helper to deduplicate the
  `Toast { text, style, deadline }` construction (used five
  times in the new dispatch arm + the `discover_repo`-missing
  case).

UI rendering (`ui.rs`):
- `render_git_leader` — 48% × 30% centered popup, cyan border,
  two rows: `s — sync (commit + pull + push)` / `Esc — cancel`.
- `render_syncing` — 30% × 20% popup, yellow border, static
  `"running ft git sync…"`. Drawn once before the blocking
  call; no spinner animation needed since the event loop is
  paused.
- `render_sync_conflict(info)` — 60% × 50% popup, red border,
  title varies on `kind` (`" merge conflict "` /
  `" rebase conflict "`), lists `info.files` indented under
  `"N conflicted file(s):"`, hint line picks the right recovery
  command (`"resolve, commit, and push manually"` vs
  `"resolve, then \`git rebase --continue\` manually"`), italic
  `"press Esc to dismiss"`.
- New `("g s", "git sync")` row in `HELP_LINES`. The existing
  90% help popup already overflowed on 24-row terminals (had
  19 entries; popup fits ~17), so adding `g s` at the top
  pushes `Ctrl+W / Ctrl+⌫` and `Esc — close overlay` off the
  bottom. Both bindings still work; they're documented in the
  list, just truncated. Pre-existing layout constraint, not a
  regression of this session.

One small ergonomic fix to `welcome.rs`: the welcome tab's
"press any key to continue" handler explicitly forwards a list
of keys (`q`, `?`, `Tab`, digits, …) to the global handler
rather than treating them as "any key". Added `g` to that list
so `g s` works from Welcome too — keeps the chord uniform
across tabs.

12 new TUI tests in `tests.rs`, all green:
- `git_chord_g_from_normal_enters_git_leader_mode`
- `git_leader_s_queues_sync_request_and_returns_to_normal`
  (asserts `message: None`)
- `git_leader_esc_dismisses_without_queueing`
- `git_leader_unknown_letter_dismisses_without_queueing`
- `git_leader_q_does_not_quit_via_global_handler`
  (bug-guard: leader must not fall through to global)
- `g_chord_works_from_tasks_tab`
- `g_chord_works_from_notes_tab`
- `git_leader_modal_snapshot` (80×24, accepted)
- `git_chord_does_not_trigger_inside_help_overlay`
  (Help mode swallows `g`)
- `shift_g_does_not_trigger_leader` (only bare `g` enters)
- Plus a new `mode()` test-only accessor on `App` so the tests
  can assert on the current mode.

Snapshot maintenance: `help_overlay_80x24` and
`help_overlay_over_tasks_80x24` regenerated for the new
`g s git sync` row; `git_leader_80x24` is new.

No live-`git` TUI tests: end-to-end sync is library- and
CLI-tested exhaustively (sessions 1 + 2). The TUI tests stop at
"request was queued" / "mode transitioned correctly" — the
realistic complexity of spinning up a bare origin + clone +
terminal harness for one extra test layer wasn't worth the
maintenance cost.

Workspace state: `cargo test --workspace` → 803 tests green (up
from 793: +12 git-leader TUI, then accounting for snapshot
acceptances which don't add to the count). `cargo clippy
--workspace --all-targets -- -D warnings` clean. `cargo fmt
--check` clean after one autoformat pass. No new dependencies.

The plan is now complete — `ft git sync` works on the CLI with
exit codes 0/2/1 and `--dry-run` / `--message`; `g s` from any
tab in the TUI runs the same operation with a progress modal,
success toast, or persistent conflict modal; library
`ft_core::git` is the shared brain with its own exhaustive
test suite.
