## 1. Foundations

- [x] 1.1 Add `ureq` (rustls + platform verifier, json) and `keyring` to workspace deps / ft-core; `cargo build --release` on macOS. Binary size delta: 12.4 MiB → 15.2 MiB (+2.9 MB).
- [x] 1.2 `Config::sources: HashMap<String, SourceCfg>` with defaults and validation (`kind`, `base_url` https rule, jira requires `email`, `status_map` values); config tests incl. unknown-key rejection.
- [x] 1.3 `frontmatter::upsert_ft_keys` (key-level set/remove under `ft.<sub>`, YAML quoting, creates fence/`ft:` map, preserves other lines); tests incl. round-trip through the readers. `upsert_synth_frontmatter` left as is (its keep-targets-when-absent contract differs; no benefit in rewiring).
- [x] 1.4 Readers for `ft.source.*` (`name`, `key`, `url`, `status`, `assignee`, `state`, `updated`, `synced.status`) and `ft.tasks.parent`.

## 2. Source core

- [x] 2.1 `sources/mod.rs`: `TaskSource`, `RemoteIssue`, `Comment`, `SourceError`, `FetchResult`, `fetch_all(cfg, creds, known_keys)` (queries → union = active; parent chain ≤5 = context; known-but-missing keys → `fetch_keys` for scope).
- [x] 2.2 `sources/http.rs`: agent (timeouts, UA), error classification (`Offline`/`Unauthorized`/`RateLimited`/`Remote`/`Parse`), single `Retry-After` retry, `Secret` newtype; tests against a local `TcpListener` stub (200, 401, 429+Retry-After, connection refused → Offline, slow → timeout).
- [x] 2.3 `sources/credentials.rs`: env → `gh auth token` (github, 5s timeout) → keyring; `login`/`logout`; tests for env precedence and name normalisation (`FT_MY_JIRA_TOKEN`).

## 3. Jira

- [ ] 3.1 Verify the live Jira Cloud search endpoint + field set once with a real token (manual; record a sanitised response as a fixture).
- [x] 3.2 `sources/jira.rs`: `/rest/api/3/search/jql` with `nextPageToken`, `key in (…)` batching (100), `/myself` check; field mapping + status categories + resolution-based Cancelled + `status_map`; fixture tests.
- [x] 3.3 `sources/adf.rs`: ADF → markdown for the listed node/mark types; unknown nodes → text; tests per node type + a mixed real-world fixture.

## 4. GitHub

- [x] 4.1 `sources/github.rs`: search with `Link` pagination (exclude PRs), per-key fetch (404 → absent), comments (last N), sub-issue parent; key normalisation; fixture tests.

## 5. Issue notes + sync planner

- [x] 5.1 `sources/note.rs`: render a new issue note (frontmatter, heading, anchor line, rendered template body with `vars` {key,title,url,source}, managed sections); parse an existing note into an `IssueNote` view (anchor line idx, marker ranges, frontmatter values).
- [x] 5.2 `neutralise` (task lines → ☐/☑, heading demotion, `---` → `***`, marker stripping) + tests incl. "no phantom task after scan".
- [x] 5.3 `sources/sync.rs::plan_sync`: index notes by `(source,key)`; create (path collision → `<key> (<source>).md`); update owned fields keeping local ones; status 3-way rules; scope states (active/context/out-of-scope, reactivation); parent frontmatter; managed sections (respect deleted markers); duplicate-key warning; report counts.
- [x] 5.4 Determinism tests: same input → identical bytes; unchanged remote → no edits; key order stable.
- [x] 5.5 Planner scenario tests with a fake `FetchResult` for every `task-source-sync` scenario.

## 6. Edit guard + hierarchy + DSL

- [x] 6.1 `UpdateError::OwnedByRemote` in `update_task_line` (description-minus-hashtags / due / priority on issue-note anchors); render in CLI (`ft tasks edit`, `ft do`) and TUI toasts (tasks tab + graph tab call sites); tests (bump due refused, retag allowed, status allowed).
- [x] 6.2 `resolve_parent_links` reads `ft.tasks.parent` for anchor notes (line link wins → frontmatter value as extra); `LinkError::ParentFromFrontmatter` in `plan_unlink`; tests (story→epic, cycle via frontmatter, dangling).
- [x] 6.3 `Graph::task_remote` side table + `Attr::{Source, Key, RemoteStatus}` (optional Str) + `remote` preset; tests; docs in `docs/graph-query-dsl.md`.

## 7. CLI

- [x] 7.1 `ft tasks sync [--source]… [--dry-run] [--format]` with per-source status lines/JSON and exit codes; writes `.ft/cache/sources.json` attempt times.
- [x] 7.2 `ft sources login|logout|list` (`cmd/sources.rs`; dialoguer password prompt; verify before store).
- [x] 7.3 Integration tests with a stub HTTP server (`base_url = "http://localhost:<port>"`): create, update, pending, out-of-scope, offline skip (exit 0), unauthorized (exit 1), dry-run diff, JSON shape snapshot, `ft tasks list remote --tree`.

## 8. TUI

- [x] 8.1 `BgEvent::SourcesFetched`, `JobKind::Sources` (indicator `sources`), worker (scan + fetch + plan, reads only) with owned inputs, main-thread apply of unchanged files + graph refresh.
- [x] 8.2 `Tick` scheduling (startup after first `GraphReady`, per-source interval, busy-slot retry). No seeding from `.ft/cache/sources.json`, since the startup sync resets every schedule.
- [x] 8.3 `app.sync-sources` command + `g r` in the git leader; toasts per spec (silent offline/no-creds on background runs; always summarise on `g r`).
- [x] 8.4 Injectable source factory for tests; `TestBackend` tests: startup sync creates anchors visible in Tasks tab, `g r` offline toast, owned-field toast on `]`, pending-push summary.
- [x] 8.5 Regenerate `docs/keybindings.md`.

## 9. ft.nvim

- [ ] 9.1 [ft.nvim] `:FtSync` → `ft tasks sync --format json`, summary via `vim.notify`, reload affected buffers.
- [ ] 9.2 [ft.nvim] Document the `sync` JSON contract in `ARCHITECTURE.md`.

## 10. Docs + build invariants

- [x] 10.1 `docs/guide/tasks.md` "Remote sources" section (config, login, note format, owned fields, pending/conflict, out-of-scope, offline/multi-machine behaviour); `docs/config.md` `[sources.*]` table + `FT_<NAME>_TOKEN`.
- [x] 10.2 `cargo build --release`
- [x] 10.3 `cargo test --workspace`
- [x] 10.4 `cargo clippy --workspace --tests -- -D warnings` (clean except the 4 pre-existing `redundant_field_names` hits, also on main)
- [x] 10.5 `cargo fmt --check`
- [x] 10.6 `cargo run --release -q -- commands docs --check`
