## Why

The synth flow is comfortable when you already know what to look for, but two recurring cases are not: (1) accreting only the entries **created/updated since the last synth** — the natural "catch up" loop for a note you revisit periodically; and (2) from the CLI, completing a synth note with **all missing** journal entries for a specific target, treating the note as a persisted journal that grows toward completeness. Both reduce to the same gap: scaffold's append path never reads what's already pinned, so it duplicates; and there's no notion of a last-synth watermark to scope "only the new ones."

## What Changes

- **Self-describing synth notes (format addition).** A synth note MAY record its journal target(s) in frontmatter (`ft-synth-targets: ["[[Foo]]", "[[Bar]]"]`). When present, the new `grow` command sources from these targets with no `--link` required — the note becomes a literal persisted journal. Absent the key, behavior is unchanged. The marker is written by scaffold/grow when `--link` is supplied and is optional on hand-authored notes.
- **Dedup-on-append invariant (behavior change).** `plan_synth_scaffold`'s append path SHALL drop entries whose body text is already pinned in the target note (key: `(source_path, body)` exact string match). This makes append idempotent across all callers (CLI, TUI). No existing test relies on duplicate appends.
- **`ft synth grow` subcommand (new CLI surface).** `ft synth grow <note.md> [--link "[[X]]" ...] [--new-only] [--since <dur> | --range X..Y | --in-window] [--from path:line] [--limit N] [--no-edit]`. `grow` sources journal entries, dedups against the note, optionally filters to "new since last synth," and appends the missing ones. With `--new-only` and no `--link`, reads targets from frontmatter. `--limit` caps the entry count (case-2 volume control). `--new-only` on a note with no existing callouts falls back to "all missing" with a warning.
- **Last-synth watermark (new primitive).** The newest pinned `commit_sha` among a note's callouts is the last-synth watermark; its commit date scopes `--new-only`. Computed via one `git rev-list`/`git log` call over the pinned SHAs, skipping any that are unreachable in the local history.
- **TUI accrete (new commands).** The Journal tab's `s` (send-to-existing) SHALL dedup by default against the picked note. A new `journal.send-to-synth-new-only` command (bound to `n`) picks an existing note, computes its watermark, and appends only entries newer than that watermark. Both reuse the planner's dedup-on-append invariant.

## Capabilities

### New Capabilities
- `synth-grow`: the `ft synth grow` subcommand and its selection semantics (dedup-on-append, `--new-only` watermark filter, `--limit`, frontmatter-target sourcing), plus the last-synth watermark primitive.

### Modified Capabilities
- `synth-notes`: add the optional `ft-synth-targets` frontmatter key to the synth-note format; make append dedup against existing callouts (the dedup-on-append invariant replaces unconditional append).
- `journal-tui-tab`: the send-to-synth flow gains dedup-on-append and a new-only variant sourced from the picked note's watermark.

## Impact

- **`ft-core` crate**: new module `ft-core/src/synth/accrete.rs` (`filter_missing`, `last_synth_watermark`); new frontmatter helpers in `synth::callout` for parsing/serializing `ft-synth-targets`; the append branch of `plan_synth_scaffold` gains dedup (signature stable — `entries` is filtered in place before section construction). New `Error::SynthWatermark` variant for unreachable-SHA diagnostics. No new dependencies (blake3 already present).
- **`ft` binary**: new `ft synth grow` subcommand in `ft/src/cmd/synth.rs` reusing the existing scaffold plumbing (`resolve_link_to_id`, `pick_paragraph`, `dedup_entries`, window resolution); new `GrowArgs`. `ScaffoldArgs` is unchanged.
- **TUI**: new `CommandDef`s `journal.send-to-synth-new-only` (+ keymap `n`); `JournalTab::commit_send` gains a "new-only" branch that reads the picked note's callouts + watermark; `SynthSendState` gains a watermark-aware path. Regenerate `docs/keybindings.md`.
- **Format / vault**: a new optional YAML key `ft-synth-targets` (a YAML list of `[[wikilink]]` strings). Backward compatible — old synth notes without the key behave exactly as today; verify/repair/reslice ignore it.
- **Tests**: unit tests for `filter_missing` (unchanged/updated/brand-new bodies, hash fast-path), `last_synth_watermark` (descendant tip, unreachable-SA skip, brand-new-note None); integration tests for `ft synth grow` (all-missing, `--new-only` post-scaffold, frontmatter-target sourcing, `--limit`); TUI frame-assertion tests for dedup-on-append and the new-only flow.
- **Non-goals**: changing the dedup key away from exact body match; auto-refreshing drifted pins (that's `repair`/`reslice`); background-worker-ization of the watermark computation; a TUI folder/target picker for the new-only variant beyond the existing note picker.
