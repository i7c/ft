## Context

`ft notes` has grown organically. Sessions added `open`, `today`, `periodic`, `move-section`, `create`, `backlinks`/`links`, `rename`, `mv`, `journal`, `update-related`, `append` — 13 subcommands. Some of them are "operate on this note" (open, create, rename, mv, append). Others are "read across notes from this anchor" (backlinks, links, journal). `update-related` is borderline (operates on one note's `## Related` section but is computed from graph signal).

Separately, `ft find` was the first vault-wide fuzzy command — it predates `ft notes open` and survived after `notes open` added editor-launching on top of the same picker.

The mental model that emerges if you split by *intent* rather than by *historical accident*:

- `ft notes <verb>` — verbs that operate on one note (open, create, rename, mv, append, find, move-section, today, periodic, update-related)
- `ft graph <verb>` — verbs that read across notes (query, backlinks, links, journal)

`update-related` stays under `notes` because the user-facing semantics are "edit this note's Related section" — the graph signal is implementation.

## Goals / Non-Goals

**Goals:**

- Clean split: notes = single-note ops; graph = cross-note reads.
- `ft find` folds into `ft notes find` — one fuzzy-find surface, with `--open` for the launch-editor variant of the same query.
- Hard break: no aliases, no deprecation, no warning mode.
- One change covers all renames so the migration is atomic.

**Non-Goals:**

- No top-level `ft today` umbrella command.
- No deprecation period — the project is pre-1.0, the user is the only operator.
- No restructuring of `ft tasks` or `ft timeblocks` — they're already coherent (single namespace, single domain).

## Decisions

### `ft find` folds into `ft notes find`

`ft find` and `ft notes open` already share the same fuzzy syntax. The difference is "print vs. open." Rather than keep two top-level commands, fold `find` under `notes`:

```sh
ft notes find meeting                  # print top hits (was: ft find meeting)
ft notes find meeting --format ndjson  # same surface
ft notes open meeting                  # open top hit (unchanged)
```

`ft notes find` is the discoverable equivalent of `ft notes open --print`. We don't actually add `--print` to `open` — the two commands have different "natural" defaults (find lists, open opens), and forcing a flag is more friction than a dedicated subcommand.

**Alternative considered:** keep `ft find` as a top-level shortcut. Rejected — the duplication confuses users into thinking they're different mechanisms.

### `update-related` stays under `notes`

`ft notes update-related <note>` operates on one note's `## Related` section. It opens a TUI modal under the hood, but the semantics are "modify this note." Moving it to graph would suggest "this returns a graph query result" which it doesn't.

### Hard break, no aliases

```sh
$ ft find foo
error: 'ft find' has been removed. Use 'ft notes find foo' instead.

$ ft notes backlinks foo
error: 'ft notes backlinks' has moved to 'ft graph backlinks foo'.
```

The error messages name the new path explicitly so script authors can `sed` their way out. We do *not* implement these as actual subcommands routing to the new ones — that's an alias by another name. The errors come from clap's "unknown subcommand" mechanism plus a small fallback in `main` that recognizes the removed paths and prints the helpful message before clap's default error.

**Alternative considered:** hidden aliases for one release. Rejected per Q3.2.

### `update-related` is the most controversial keep

It computes from the link graph (co-occurrence scoring). One could argue it's a graph op that happens to write a single note. But: it's user-initiated *on* a note ("update *this* note's Related section"), and the result is a write to one file. The user wraps it mentally as "fix this note," not "query the graph."

### Implementation: lift logic, not copy

`run_backlinks` / `run_links` / `run_journal` move from `cmd/notes.rs` to `cmd/graph.rs` by `git mv`-equivalent file edits — the function bodies are unchanged, only the subcommand wrapper around them moves. Same for `run_find` from `cmd/find.rs` to a private helper called by both the old-removed and the new-canonical entry point (which is just the `notes` one after this change).

### Migration of CLAUDE.md and tests

CLAUDE.md mentions `ft notes backlinks` in the "Notes" example block — updated. Integration tests in `ft/tests/` reference each moved path — updated. The TUI tests are unaffected.

### Man pages and completions

Regenerated with `ft man --out` and `ft completions {bash,zsh,fish}`. The `docs/commands.md` index (from `commands-and-keymaps`) gets a new section reflecting the rename.

## Risks / Trade-offs

- **[User scripts break instantly]** → Documented in CHANGELOG. Errors instruct users on the new path. Acceptable for pre-1.0.
- **[`update-related` placement is a judgment call]** → Yes. If it bothers a future user, moving it to `ft graph update-related` is a single-file edit later. Not blocking.
- **[`ft notes find` is verbose compared to `ft find`]** → True. Mitigated by tab-completion and `find` being a discoverable subcommand of `notes`. The five extra characters are worth the consistency.
- **[Removing the top-level `Find` clap variant changes shell completion behaviour]** → Regenerate completions in this change so the discoverable completion paths reflect the new structure.

## Open Questions

- Does the TUI's notes tab need re-labelling? **Leaning:** no — tabs are by domain, not by CLI namespace. The Notes tab spans both single-note ops and graph-backed read ops in one UI.
- Should `ft notes update-related` be renamed to `ft notes related` (shorter, less verbose)? **Leaning:** keep `update-related` — `related` could read as "show related notes" (which is a different operation we may add later).
