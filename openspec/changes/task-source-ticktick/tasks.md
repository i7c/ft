## 1. Config and shared plumbing

- [ ] 1.1 Add `chrono-tz` to the workspace deps and ft-core. Confirm `scripts/check-no-cpp.sh` stays clean and note the release binary size delta here.
- [ ] 1.2 Add `SourceKind::Ticktick` (`kind = "ticktick"`).
- [ ] 1.3 Make `SourceCfg::comments` an `Option<usize>` with `effective_comments()` (jira and github 5, ticktick 0), and update the 3 call sites in `sources/sync.rs`.
- [ ] 1.4 Give ticktick an empty default `mine` query in `effective_queries`.
- [ ] 1.5 Change `validate`: `transitions` are rejected on any non-Jira kind. Add config tests for a minimal TickTick source, the defaults, and the transitions rejection.
- [ ] 1.6 `credentials.rs`: keychain account `<name>:ticktick`. Extend the account and precedence tests (ticktick never asks `gh`).
- [ ] 1.7 `http.rs`: classify a 500 whose body contains `exceed_query_limit` as `RateLimited` (no retry), and scrub the client's own token out of `Remote` excerpts (`***`). Stub-server tests for both.

## 2. TickTick backend (`ft-core/src/sources/ticktick.rs`)

- [ ] 2.1 Query parser: terms `list:` / `-list:` / `tag:` / `priority:`, quoted values, and the unknown-term error. Unit tests, including the empty query.
- [ ] 2.2 List resolution: `GET /project` (skip `closed`), plus `inbox`. Match names case-insensitively or ids exactly; report `unknown TickTick list "<name>"`. Cache each list's `/data` once per run.
- [ ] 2.3 Field mapping (`map_task`):
  - status name from status code and `columnName`, then `Status` via `status_map` or the defaults;
  - priority 5/3/1/0;
  - due and completion dates in the task's `timeZone` via `chrono-tz`, with a UTC fallback;
  - parent `tt-<parentId>`;
  - `updated` from `modifiedTime`, else `etag`;
  - description from `content`, or `desc` plus checklist lines;
  - the key and the web-app URL with a tolerant parser.

  Fixture tests built from the verified response shapes (no real data), including the São Paulo all-day case.
- [ ] 2.4 `fetch_query`: scan the selected lists, apply the client-side filters, populate the `key → (projectId, raw)` cache, and fetch comments only when `effective_comments() > 0`.
- [ ] 2.5 `fetch_keys`: cache first, then `GET /project/{p}/task/{t}`. The project id comes from the URL or from a child's `projectId`; 404 means absent.
- [ ] 2.6 `push_status`: fetch, then the AlreadyThere check, then Done via `/complete`, or Cancelled/Open by sending the full fetched object with only `status` changed. 404 and 403 become `NotPushable`. Stub tests assert method, path and body.
- [ ] 2.7 `create_child`: `POST /task` with `projectId`, `parentId`, title, priority, and an all-day `dueDate` in the `/preference` time zone. Fall back to an update when `parentId` isn't echoed. Stub tests.
- [ ] 2.8 `whoami`/verify: `GET /project`, reporting the list count. Wire `build_source` and `verify_token` in `run.rs`.

## 3. Live verification (needs the user's OK for writes)

- [ ] 3.1 Read-only: run `ft tasks sync --source ticktick --dry-run` against the real account and check the notes, dates, columns and parents. Record the outcome here, without task content.
- [ ] 3.2 Confirm the web-app task URL format in a browser, and adjust the URL builder and parser if it differs.
- [ ] 3.3 With consent, on a throwaway task in a scratch list:
  - (a) set `status` -1 and then 0 via update;
  - (b) create with `parentId`;
  - (c) complete a recurring task and observe the result;
  - delete the scratch task and list afterwards.

  Adjust push and promote (or downgrade them to `NotPushable`) per the result, and record the findings in design.md.

## 4. Integration, docs, invariants

- [ ] 4.1 CLI integration tests (`ft/tests/tasks_sync.rs` style) for a TickTick source against the stub server: sync creates `ticktick/tt-<id>.md`, reruns are idempotent, and push completes.
- [ ] 4.2 `ft sources list` / `login` show the ticktick kind and default base URL. Update the snapshots, if any.
- [ ] 4.3 `docs/config.md`: TickTick section covering the personal API token location, the query syntax, the column → `status_map` example, the `comments` default, and the Dida365 `base_url`.
- [ ] 4.4 All six build invariants clean (build, test, clippy `-D warnings`, fmt, `commands docs --check`, `check-no-cpp.sh`).
