---
id: 011
name: editor-tmux
title: Editor handoff: tmux popup / window / split
status: ready
created: 2026-05-14
updated: 2026-05-14
---

# Editor handoff: tmux popup / window / split

## Goal
When ft's TUI is running inside tmux (`$TMUX` is set), route every
"open in $EDITOR" handoff through a configurable tmux strategy —
`tmux-popup` (default), `tmux-window`, or `tmux-split` — instead of
suspending the alt-screen and running the editor inline. Outside tmux,
or when the user explicitly picks `suspend`, keep today's behavior.

Affected call sites are all TUI surfaces that currently raise
`AppRequest::OpenInEditor`:

1. Notes-tab `o` open picker → `Enter` / `Ctrl+O` (open / Obsidian).
2. Notes-tab `c` / `C` create flow → on commit.
3. Notes-tab `m` move-section new-target sub-flow → "use existing".
4. Notes-tab `t` / `p<letter>` periodic flow (plan 010).
5. Tasks-tab `e` edit / `Enter` open-source (already routes through
   the same request).

The CLI (`ft notes open` / `create` / `periodic` / `today`) is **out of
scope** — it's already a one-shot: spawn editor, wait, exit. There's
no TUI to preserve, so the tmux dance buys nothing.

## Motivation and Context

The current handoff (`ft/src/tui/app.rs:284-308`) is the standard
"suspend the alt-screen, run editor, restore" pattern:

```rust
suspend_terminal(terminal)?;     // disable_raw_mode + LeaveAlternateScreen
let status = spawn_editor(...);   // blocks until editor exits
restore_terminal(terminal)?;      // re-enable raw mode + alt screen
events.drain(Duration::from_millis(120));
terminal.clear()?;
```

This works, but two real problems show up in practice:

**1. Terminal-mode leakage causes swallowed keystrokes in nvim.**
crossterm enables a handful of CSI modes when ft starts (raw mode,
mouse capture, possibly bracketed paste / focus reporting / kitty
keyboard depending on the terminal). `suspend_terminal` disables raw
mode + mouse capture, but **does not** disable bracketed paste
(`?2004h`), focus reporting (`?1004h`), or any kitty-keyboard flags
crossterm may have enabled. When nvim takes over, those modes are
still active and nvim interprets `\e[?…l` sequences (from the
terminal) or focus-in/out events as garbage keystrokes — the
first few keys after the editor opens get eaten, or worse, ESC
combinations get mis-parsed. The fix would be to disable every mode
crossterm enabled, but the simpler structural fix is **not to
suspend at all** when we're inside tmux: tmux already owns the
terminal-mode handshake, and running the editor in a popup / sibling
window means ft never has to give up its raw-mode state.

**2. The full-screen takeover is jarring.** When ft launches nvim
"inline", the user loses ft's UI completely until nvim quits. Many
users already work in a tmux layout where ft lives in one pane / window
and the editor lives in another — letting ft route to a popup or
sibling window matches that ergonomic and keeps ft visible.

**Why pick `tmux-popup` as the default when inside tmux.** Modern tmux
(≥3.2) ships `display-popup -E "<cmd>"`, which:

- Opens a centered overlay sized to a percentage of the parent
  (default 90% × 90% in this plan).
- Forwards **all** input to the running command — including ESC, so
  nvim's mode switches and `:q` work normally. (Without `-E`, tmux
  intercepts ESC / `q` to dismiss the popup; with `-E`, the popup's
  lifetime is bound to the command's lifetime.)
- Closes automatically when the command exits.
- Keeps ft visible behind / around the popup — no alt-screen dance,
  no `events.drain`, no terminal.clear.

`tmux-window` (open in a fresh window) and `tmux-split` (split the
current pane) cover users on older tmux or who prefer a
non-overlapping layout. Implementation cost is small once the popup
path is wired — same argv-builder shape with different first args.

## Acceptance Criteria

### Library — `ft_core::config`

- [ ] New `[editor]` config block on `Config`:
      ```rust
      #[serde(default)]
      pub editor: Editor,

      #[derive(Debug, Default, Clone, Deserialize, Serialize)]
      #[serde(deny_unknown_fields)]
      pub struct Editor {
          /// How to launch $EDITOR from the TUI. `tmux-*` strategies
          /// require ft to be running inside tmux ($TMUX set); when
          /// $TMUX is unset, they fall back to `suspend` at use time.
          /// Default: `tmux-popup` (works as `suspend` outside tmux).
          #[serde(default)]
          pub strategy: EditorStrategy,
          /// Popup geometry (used only when strategy = tmux-popup).
          /// Accepts tmux-style dimensions: `"90%"`, `"80"`, etc.
          #[serde(default = "default_popup_width")]
          pub popup_width: String,
          #[serde(default = "default_popup_height")]
          pub popup_height: String,
      }

      #[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
      #[serde(rename_all = "kebab-case")]
      pub enum EditorStrategy {
          #[default]
          TmuxPopup,
          TmuxWindow,
          TmuxSplit,
          Suspend,
      }
      ```
- [ ] `EditorStrategy::resolve(&self) -> EditorStrategy` — returns the
      effective strategy after the `$TMUX`-fallback rule: any
      `Tmux*` value collapses to `Suspend` when `$TMUX` is unset.
      Pure function; tested in isolation by toggling the env var.
- [ ] Unit tests:
      - default values: `strategy = "tmux-popup"`, `popup_width = "90%"`,
        `popup_height = "90%"`.
      - kebab-case parsing accepts `"tmux-popup"`, `"tmux-window"`,
        `"tmux-split"`, `"suspend"`; rejects unknown values.
      - `deny_unknown_fields` on the new struct (typo rejected with
        a helpful figment error).
      - resolution: `$TMUX` set → strategy passes through;
        `$TMUX` unset → every `Tmux*` collapses to `Suspend`,
        `Suspend` stays `Suspend`.

### Library — `ft` (TUI): argv builder

- [ ] New module `ft/src/tui/editor.rs` (kept inside the binary crate
      because it threads tmux-specific concerns the library doesn't
      need). Public functions:
      ```rust
      pub struct EditorInvocation {
          pub program: String,        // first argv element
          pub args: Vec<String>,      // remaining argv elements
      }

      /// Build the argv for launching the user's editor on `(path,
      /// line)` under `strategy`. Pure — does no I/O, no env lookup
      /// beyond `$EDITOR` / `$VISUAL`. Tested by asserting argv shape.
      pub fn build_invocation(
          strategy: EditorStrategy,
          editor: &str,
          path: &Path,
          line: usize,
          popup_width: &str,
          popup_height: &str,
      ) -> EditorInvocation;
      ```
- [ ] Argv shapes:
      - `Suspend` → `program = first whitespace token of $EDITOR`,
        `args = rest of $EDITOR's tokens ++ ["+N", path]`. (Identical
        to today's `spawn_editor`; lift it from `app.rs` into this
        module so all four strategies live in one file.)
      - `TmuxPopup` → `program = "tmux"`,
        `args = ["display-popup", "-E", "-w", W, "-h", H, "--",
                 <editor token 1>, <…tokens>, "+N", path]`.
        `--` separates the popup's own flags from the command argv,
        so paths starting with `-` are safe.
      - `TmuxWindow` → `program = "tmux"`,
        `args = ["new-window", "--", <editor tokens>, "+N", path]`.
      - `TmuxSplit` → `program = "tmux"`,
        `args = ["split-window", "--", <editor tokens>, "+N", path]`.
        Default horizontal split; users who want vertical can switch
        to `tmux-window` (we don't expose a knob in v1).
- [ ] Quoting: paths are passed as their own argv element (not
      interpolated into a shell string), so embedded spaces / single
      quotes / `$` are safe. The only fragile point is splitting
      `$EDITOR` on whitespace (existing behavior — `EDITOR="code -w"`
      already breaks if `code` lives at a path with a space, which is
      fine to keep for parity).
- [ ] Unit tests in `editor.rs`:
      - `Suspend` matches the current `spawn_editor`'s argv for
        `EDITOR=nvim` and `EDITOR="code -w"`.
      - `TmuxPopup` builds the documented argv including `--`.
      - `TmuxWindow` and `TmuxSplit` build the documented argv.
      - Path with embedded space passes through as one argv element.
      - Custom `popup_width = "80"` (no `%`) and `popup_height = "50%"`
        propagate to `-w` / `-h` verbatim.

### Library — `ft` (TUI): service_request dispatch

- [ ] Update `App::service_request`'s `OpenInEditor` arm
      (`ft/src/tui/app.rs:284-308`):
      ```rust
      AppRequest::OpenInEditor { path, line } => {
          let cfg = &self.vault.config.config.editor;
          let strategy = cfg.strategy.resolve(); // $TMUX-aware
          let editor = std::env::var("VISUAL")
              .or_else(|_| std::env::var("EDITOR"))
              .unwrap_or_else(|_| "vi".to_string());
          let inv = build_invocation(
              strategy, &editor, &path, line,
              &cfg.popup_width, &cfg.popup_height,
          );
          match strategy {
              EditorStrategy::Suspend => {
                  // Current path — suspend, spawn, restore, drain.
                  suspend_terminal(terminal)?;
                  let status = run_invocation(&inv);
                  restore_terminal(terminal)?;
                  events.drain(Duration::from_millis(120));
                  terminal.clear()?;
                  // ... existing refresh ...
                  status?;
              }
              EditorStrategy::TmuxPopup
              | EditorStrategy::TmuxWindow
              | EditorStrategy::TmuxSplit => {
                  // Fire and wait — tmux owns the popup/window
                  // lifetime, but we still wait for the editor to
                  // exit so the post-edit refresh runs.
                  let status = run_invocation(&inv);
                  // ... existing refresh ...
                  status?;
              }
          }
          Ok(())
      }
      ```
- [ ] For the tmux strategies, **do** wait on the child (`cmd.status()`,
      not `cmd.spawn()`). `tmux display-popup -E` returns when the
      inner command exits, so `status()` gives us the
      "editor closed → refresh now" trigger. Same for `new-window`
      and `split-window` — both return immediately after creating the
      window/pane, but we want to wait for the editor to finish so
      ft re-scans the file. The trick: for `new-window` /
      `split-window`, append `\; wait-for editor-done-<pid>` and have
      the editor command end with `\; wait-for -S editor-done-<pid>`.
      Or, simpler v1 fallback: for `new-window` / `split-window`,
      don't wait — fire and continue. The user can `r` to refresh
      explicitly. Document this clearly.
      (See "Technical Notes" for the trade-off.)
- [ ] If `tmux` is not on `PATH`, surface a clear error toast at the
      `service_request` site: `"tmux not found — falling back to
      suspend"`. Then run the `Suspend` path in the same invocation.
      Test by stubbing `Command::new("tmux")` failure (or just by
      checking the error wrap message in the manual path).

### Documentation

- [ ] `docs/config-reference.md` (or wherever `[notes]` /
      `[periodic_notes]` are documented today): a new `[editor]`
      section with the four strategies, the `$TMUX` fallback rule,
      and the popup-geometry knobs.
- [ ] `?` help overlay text untouched — strategies are config, not
      keybinds.

## Technical Notes

- **`tmux display-popup -E` semantics.** With `-E`, the popup's
  lifetime is tied to the inner command: ESC and other keys forward
  through to the command; the popup closes when the command exits
  (regardless of status — `-EE` is "close only on success", which is
  the opposite default of what you'd guess from the name). v1 uses
  plain `-E` so a non-zero editor exit (e.g. user `:cq` in nvim) still
  tears down the popup.

- **Why `--` before the editor argv in every tmux invocation.** tmux's
  argument parser is greedy: without `--`, a path like
  `-/tmp/important.md` would be interpreted as a tmux flag. `--`
  tells tmux "everything after this is the command argv". Cheap
  insurance; matches what `git` / `kill` / etc. do.

- **Waiting on `new-window` / `split-window`.** Unlike `display-popup`,
  these tmux subcommands return immediately after creating the new
  pane/window — the editor child is reparented to tmux's server and
  ft has no PID to wait on. Two paths to keep ft's "refresh on
  editor exit" behavior:
  1. **`wait-for` trick.** Compose a shell command:
     `nvim +N <path> ; tmux wait-for -S ft-editor-<uuid>`, then in
     ft after spawning the window run `tmux wait-for ft-editor-<uuid>`.
     Synchronizes cleanly; the editor signals tmux, tmux unblocks ft.
     Downside: tmux signal names are global to the tmux server, so we
     need a per-invocation UUID to avoid collisions. Use
     `uuid::Uuid::new_v4().simple()` — tiny dep, well-trodden.
  2. **Don't wait — fire and forget.** Simpler, but the user has to
     `r` in ft to pick up changes after editing. Acceptable for v1
     and the `popup` default already waits cleanly; document the
     gap.

  v1 picks **path 1** for `new-window` / `split-window` because
  refresh-on-exit is a load-bearing piece of the post-edit UX
  (commit_create / commit_move both rely on it for the post-write
  re-scan). The UUID dep is already in the workspace's transitive
  deps (`uuid` ships with `chrono` for serde), so no new direct dep.

- **No tests against a real tmux server.** Argv construction is pure
  and exhaustively unit-tested. The actual `tmux ...` invocation is
  manually verified in three scenarios:
  - Inside tmux + `tmux-popup` → popup opens, nvim works, ESC stays
    inside nvim, popup closes on `:q`.
  - Inside tmux + `tmux-window` → new window, ft refreshes after
    `:q` (via `wait-for`).
  - Outside tmux + any `tmux-*` strategy → `resolve()` collapses to
    `Suspend`, behavior matches today.

  An integration test that spins up a real tmux server is possible
  (`tmux -L test-socket new-session -d 'ft …'`) but the value-to-cost
  ratio is poor — fragile under CI, slow, and the bugs we'd catch
  there are mostly argv shape (already covered by unit tests).

- **`$EDITOR` splitting.** Existing behavior keeps: `EDITOR="code -w"`
  splits on whitespace, first token is the program, rest become the
  leading args before `+N <path>`. We don't try to shell-parse —
  `shlex` would handle quoted args but adds a dep and the existing
  fleet of users have never asked for it. Document the limitation.

- **GUI editors under tmux strategies.** `EDITOR=code -w` (waits for
  the file to be closed in vscode) works fine under `Suspend` and
  under `tmux-popup` (the popup is a thin process wrapper — vscode
  raises its own window, tmux popup blocks on `code -w`'s status).
  `EDITOR=open -W file` on macOS likewise. We don't restrict the
  strategy by editor type — that's the user's call.

- **Refresh-on-exit hook.** Today's refresh is keyed by the
  `OpenInEditor` arm returning. The new branches preserve that
  shape — for popup, `cmd.status()` blocks until the popup closes;
  for window/split, `tmux wait-for` blocks until the editor signals
  done. In both cases, the `tabs[self.active].refresh(&mut ctx)?`
  call still fires before `service_request` returns, so the existing
  re-scan logic works untouched.

- **Recents log.** Already updated by the tab's request-builder
  (`request_open_in_editor`, `run_periodic_open`, etc.) before the
  request is queued — independent of the strategy. No change needed.

- **Test snapshot drift.** None expected. The strategy enum, config
  block, and dispatch live in code paths that don't touch any
  rendered UI surface. Existing snapshots stay green.

## Future (explicitly out of scope for this plan)

- **CLI tmux strategies.** Once the TUI surface is wired, the same
  argv-builder could feed `ft/src/cmd/notes.rs::spawn_editor` so
  `ft notes today` from inside tmux opens nvim in a popup too. The
  ergonomic case is weaker (no ft TUI to preserve) but non-zero.
  Lift after one cycle of feedback on the TUI path.
- **Vertical-split flag for `tmux-split`.** v1 ships horizontal only;
  add `[editor] split = "horizontal" | "vertical"` later if anyone
  asks. Cheap follow-up.
- **A `tmux-pane` strategy** that targets a named pre-existing pane
  (`tmux send-keys -t name "edit /path" Enter`). Useful for users
  who keep a dedicated "editor pane" in their layout. Different
  enough from the spawn-a-new-pane strategies to merit its own
  design pass.
- **Auto-disabling crossterm modes on `Suspend`** (the underlying
  cause of the swallowed-keystroke bug). Worth doing eventually as
  a defensive cleanup, but the tmux strategies sidestep the bug
  entirely so it stops being urgent. Track as its own item.
- **Per-call strategy override** from the CLI / a TUI modifier key
  (e.g. `Ctrl+Enter` on the open picker = always popup, regardless
  of config). No demand yet.

## Sessions

### Session 1 · 2026-05-14 · planned
**Goal:** Config + argv builder. Add `[editor]` block on `Config`
(`EditorStrategy` enum, `popup_width`, `popup_height`, `resolve()`
with `$TMUX` fallback). New `ft/src/tui/editor.rs` with
`build_invocation()` covering all four strategies (Suspend / Popup /
Window / Split). Lift `spawn_editor`'s body from `app.rs` into the
new module so the dispatch arm shrinks to a match on strategy. Unit
tests for config parsing, defaults, `resolve()` env-var behavior, and
every argv shape. No live tmux required.
**Outcome:**

### Session 2 · 2026-05-14 · planned
**Goal:** Wire the dispatch in `App::service_request`. New code paths
for the three tmux strategies: popup blocks on `cmd.status()`,
window/split use a `tmux wait-for` UUID handshake for the post-edit
refresh. Tmux-not-on-PATH falls back to `Suspend` with an error
toast. Manual verification matrix (inside-tmux popup / window /
split + outside-tmux fallback) documented in the session outcome.
Add a `docs/config-reference.md` entry for `[editor]`.
**Outcome:**
