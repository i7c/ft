## Why

The user's note-taking workflow is "quick-capture + post-connection": notes are written without filing, and `[[wikilinks]]` are used liberally (including for not-yet-created concepts). Today `ft` has no tooling for the *post-connecting* ritual — the ~30-minute session where the user reviews what has been on their mind recently, surfaces co-occurring concepts, and synthesizes new notes from cross-vault context. The existing journal helps when you already know which note to investigate; it doesn't help you *discover* which links deserve synthesis or aggregate context from several at once. This change adds the missing surface: a link-review screen, a multi-source journal, and synth notes with verifiable provenance.

## What Changes

- **Link review (new).** `ft review [--since 7d | --range X..Y]` and a new TUI `Review` tab list `[[wikilinks]]` whose mentions were added in the window, ranked by paragraph-frequency (paragraph-level dedup), with ghost targets visually marked (`?` suffix). Backed by a new `git log -p` scan engine that maps added lines to paragraphs via the existing HEAD paragraph index.
- **Multi-source journal (modification).** Generalize `ft_core::journal::build_journal` to accept a slice of targets. New CLI surface `ft journal --link "[[Foo]]" --link "[[Bar]]"` (repeatable). New TUI flow: `<enter>` from Review hands the selected links to the Journal tab in multi-target mode. Journal tab gains a `matched:` badge for entries that hit multiple selected links, an in-window-only toggle, entry multi-select, and a "send to synth" key. Default scope stays all-time. Existing single-note semantics (`ft notes journal <note>`) preserved by passing a one-element slice.
- **Synth notes (new).** Regular `.md` files marked `ft-synth: true` in frontmatter. Contain "protected sections" — single-paragraph excerpts wrapped in `> [!ft-source] <path> L<a>-<b> @<sha7> #<hash6>` Obsidian callouts with verifiable provenance (git commit + line range + blake3 content-hash prefix). Arbitrary user-written markdown lives between callouts.
- **Scaffold + validate CLI (new).** `ft synth <target.md> --link ... [--since|--range] [--all|--in-window] [--from path:line] [--no-edit]` creates or appends to a synth note with protected sections drawn from the multi-source journal, then opens `$EDITOR` at the bottom of the file. `ft synth verify [<note.md> | --all]` checks every protected section against its pinned source.
- **Config (new).** `synth.folder` (default `Synthesis/`) and `synth.exclude_prefixes` (default contains periodic-note prefix) under the existing config structure.
- **Out of scope for v1**: rebase/refresh of drifted sections, insertion-point picker for extending synth notes (append-only), interactive TUI picker from `ft synth` CLI, smarter dedup when paragraphs are rewritten between commit and HEAD.

## Capabilities

### New Capabilities
- `link-review`: git-log-based scan of added `[[wikilinks]]` in a commit/date window, paragraph-frequency ranking, ghost marking, exclude-prefix filter, and `ft review` CLI surface (default + `--json`).
- `synth-notes`: synth note format (frontmatter marker, callout grammar), scaffold generation, append-to-existing semantics, `ft synth` CLI for scaffold + verify, plan/apply split for synth mutations.
- `synthesis-review-tui-tab`: new TUI tab listing review-window links with multi-select and `<enter>` handoff to the Journal tab pre-populated with selected targets.

### Modified Capabilities
- `notes-journal`: generalize the journal API from a single target to a slice of targets; add `ft journal --link <link>` (repeatable) and `--in-window` flags; Related-aliases resolution remains active only when `targets.len() == 1` to preserve current single-note behavior.
- `journal-tui-tab`: add multi-target mode (accept N targets queued by another tab), render a `matched: X, Y` badge on entries hitting more than one selected target, in-window-only toggle key, entry multi-select with `<space>`, and a "send to synth" key that prompts for a target note and triggers scaffold generation + editor handoff.

## Impact

- **`ft-core` crate**: new modules `link_review`, `synth` (containing `callout` grammar, `plan_synth_scaffold` / `apply_synth_scaffold`, `verify_synth_note`); changes to `journal::build_journal` signature (slice of targets) — see CLAUDE.md "Signature changes on core APIs" — needs a sweep of all callers (CLI, TUI Journal tab, existing tests). New config fields on the top-level `Config` struct in `ft-core/src/config.rs`. New error variants in the relevant `thiserror` enums.
- **`ft` binary**: new subcommands `ft review` and `ft synth` (`ft synth verify` as a child); extension of `ft journal` to accept `--link` (repeatable) and `--in-window`. New output renderer for review tables and synth-verify reports. New TUI `Review` tab (`ft/src/tui/tabs/review.rs`); modifications to `ft/src/tui/tabs/journal.rs` for multi-target + send-to-synth.
- **Dependencies**: add `blake3` for content hashing. No other new crates expected.
- **Filesystem / vault**: synth notes are plain `.md` files with the `ft-synth: true` frontmatter marker; no schema changes elsewhere. Vault must be a git repo (already assumed by blame cache).
- **Tests**: fixture vaults gain a `synth/` scenario; insta snapshots for the new Review tab and the multi-target Journal mode; proptest round-trip for the callout grammar; integration tests for `ft review`, `ft journal --link`, `ft synth`, and `ft synth verify`. Real-vault tests remain opt-in via `FT_REAL_VAULT_TESTS=1`.
