# Git sync

If your vault is a git repository, `ft git sync` is a one-shot
commit + pull + push. The same operation is available from inside the
TUI on a background thread (`g s`), so you can keep working while it
completes.

`ft git sync` is unavailable when there's no `.git/` anywhere up the
tree from the vault root. The discovery walks up from `vault.path`,
so the repo doesn't have to be the vault itself — a parent repo
containing the vault works just as well.

## What it does

```sh
ft git sync                     # commit, pull, push
ft git sync -m "msg override"   # override the auto-generated message
ft git sync --dry-run           # plan only — no writes, no network
```

Step by step:

1. **Upstream pre-check.** Look up the current branch's `@{u}`. No
   upstream → error out **before** touching the tree, so you don't
   end up with a stray local commit on a branch that doesn't track
   anything.
2. **Snapshot.** `git add -A` (modified, deleted, untracked, all in)
   then `git commit -m "ft sync <iso8601-utc>"` if there's anything
   to stage. Override the auto-generated message with `-m`.
   `.gitignore` is honored — git's normal staging rules apply.
3. **Pull.** `git pull --no-rebase` by default, `--rebase` if
   `[git].pull_strategy = "rebase"` is in your config.
4. **Push.** `git push`. Authentication uses your existing credential
   helper / SSH agent / GPG signing — `ft` inherits the process
   environment.

## Conflicts

If the pull (merge or rebase) hits a conflict, `ft` leaves the working
tree in its conflicted state — markers in the files, merge or rebase
in progress — and exits **2** with the conflicted file list on stderr.
Resolve manually, then `git commit` (merge) or `git rebase --continue`
(rebase), then sync again.

The exit code is deliberately separate from the "no upstream" or
"network failure" case (exit `1`) so scripts can distinguish "you
have work to do" from "the operation failed before it really started."

## Configuration

```toml
[git]
pull_strategy = "merge"   # default; also: rebase
```

That's the only knob. Everything else inherits from the user's normal
git environment (credential helpers, SSH config, GPG signing, hooks).
The two strategies map straight to git:

- `"merge"` → `git pull --no-rebase`
- `"rebase"` → `git pull --rebase`

## In the TUI

`g` arms the git-leader (you'll see a status-bar hint). `s` triggers
the sync. The sync runs on a background thread:

- The status bar's right cell shows `⟳ sync` while in progress.
- You can keep navigating, editing tasks, opening notes — the UI
  doesn't block.
- A toast announces success on the next event tick after the worker
  finishes.
- Conflicts pop a status-bar message + toast naming the affected
  files; resolve from `$EDITOR` (or any tool you prefer) and re-run.

## Common workflows

### Periodic background sync

Pair `ft git sync` with cron or a systemd timer for "every N minutes,
push whatever I changed":

```sh
*/15 * * * *  /usr/bin/env -i HOME=$HOME PATH=$HOME/.cargo/bin:/usr/bin \
              FT_VAULT=$HOME/my-vault ft git sync >/dev/null 2>&1
```

`--json-errors` at the top level is handy if you want structured
failures in the cron job log:

```sh
ft --json-errors git sync
```

### Sync before opening Obsidian

A pre-launch hook that pulls before the GUI opens:

```sh
ft git sync && obsidian
```

### Catch a dry-run from a script

```sh
ft git sync --dry-run
```

prints the upstream, the strategy, and the working-tree summary, then
exits `0` for "would sync cleanly", `2` for "would hit conflicts",
`1` for the structural errors (no repo, no upstream). Useful in
pre-commit checks or CI guards.

## What this is not

- Not a replacement for `git`. `ft git sync` only does the
  three-step round-trip; any non-trivial git operation (rebases,
  branches, history rewrites, stashes) belongs in `git` itself.
- Not a conflict resolver. When markers appear, `ft` steps aside.
- Not multi-vault. The operation runs against whichever vault the
  current invocation discovered.
- Not networked beyond the pull and push. There's no Git LFS handling
  beyond whatever `git lfs` filters you've already wired in.
