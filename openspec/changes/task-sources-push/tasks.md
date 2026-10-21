## 1. Config + HTTP

- [x] 1.1 `SourceCfg::{push (default true), transitions: BTreeMap<Status, String>}`; validation (transitions only for jira); config tests.
- [x] 1.2 `Http::post_json` / `patch_json` (same timeouts, classification, secret handling); stub-server tests asserting method, path, body.

## 2. Conflicts stored in notes

- [x] 2.1 `plan_sync` writes / updates / clears `ft.source.conflict`; conflicted notes are not pending; reader `FtSource::conflict`.
- [x] 2.2 `sources::resolve_conflict(content, Take, today, format)` pure planner (take remote / take local) + tests.
- [x] 2.3 `ft tasks check` kinds `conflict` and `push-failed` (from cache) + tests.

## 3. Push

- [x] 3.1 `TaskSource::push_status` + `PushOutcome`; `transitions` selection logic as a pure function with tests (category match, default-name preference, ambiguity, override, Cancelled rules).
- [x] 3.2 Jira: `GET …/transitions`, `POST …/transitions`, pre-check of current status (`AlreadyThere`); stub tests.
- [x] 3.3 GitHub: `PATCH` close (completed / not_planned) / reopen; stub tests.
- [x] 3.4 `sources::push::pending(scan, source)` + `push_pending(source, pending, cfg) -> PushReport`; failure cache in `AttemptLog` (`push_failures`, set/clear); offline/unauthorized abort the step.
- [x] 3.5 Wire push-before-fetch into `run::fetch_source` (or a new `run_source`) honouring `push`/`--no-push`; `SyncReport` gains `pushed`, `push_failures`.

## 4. Promote

- [ ] 4.1 `TaskSource::issue_meta` + `create_child` (`NewIssue`); Jira: issue meta, createmeta sub-task type, `/myself` accountId, `POST /rest/api/3/issue`; GitHub: create + `sub_issues`; stub tests for both incl. epic parent and sub-task refusal.
- [ ] 4.2 Summary cleaning (strip wikilinks, md links, hashtags, block id).
- [ ] 4.3 Generalise `plan_retarget` to accept explicit `(src_rel, id) → dst_rel` mappings; `plan_remove_line` with guard.
- [ ] 4.4 `sources::promote::apply_local(…)`: new note via `plan_sync` (block id carried to anchor), move children under new anchor, remove line, retarget block id; tests for the spec scenario (linked daily-note task with child + external block ref).
- [ ] 4.5 Preconditions (parent anchor required, push enabled, credentials, online) with the exact messages.

## 5. CLI

- [ ] 5.1 `ft tasks sync --no-push | --resolve | --take local|remote <KEY>…`; summary `· N pushed`, `push failed` lines; JSON `pushed`, `push_failures`.
- [ ] 5.2 `ft tasks promote <SEL> [--format json]`.
- [ ] 5.3 Integration tests against the stub (push via transition, AlreadyThere, Cancelled not pushable, 403 failure stays pending + check, `--take` offline, promote end-to-end with note + moved child + retargeted ref, promote preconditions).

## 6. TUI

- [ ] 6.1 Push step inside `SourcesJob`; summary `N pushed`; toast new push failures only.
- [ ] 6.2 `ActiveModal::ConflictResolve` (rows, `j`/`k`/`l`/`r`/`s`/`Enter`/`Esc`), opened on new conflicts (or manual sync); applies `resolve_conflict` with the unchanged-file guard.
- [ ] 6.3 `tasks.promote` (`I`), precondition toasts, `JobKind::Promote` + `BgEvent::PromoteDone`, main-thread apply, cursor on new anchor.
- [ ] 6.4 `TestBackend` tests with fake source: tick → push → toast; conflict modal snapshot + take remote; promote happy path + precondition toast.
- [ ] 6.5 Regenerate `docs/keybindings.md`.

## 7. ft.nvim

- [ ] 7.1 [ft.nvim] `:FtPromote` → `ft tasks promote <file>:<line> --format json`, open the new note.
- [ ] 7.2 [ft.nvim] Document `promote` / sync `pushed` fields in `ARCHITECTURE.md`.

## 8. Docs + build invariants

- [ ] 8.1 `docs/guide/tasks.md`: pushing, conflicts, promote; `docs/config.md`: `push`, `transitions`.
- [ ] 8.2 `cargo build --release`
- [ ] 8.3 `cargo test --workspace`
- [ ] 8.4 `cargo clippy --workspace --tests -- -D warnings`
- [ ] 8.5 `cargo fmt --check`
- [ ] 8.6 `cargo run --release -q -- commands docs --check`
