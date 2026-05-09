---
id: 003
name: notes
title: Notes commands (browse, create, reorganize)
status: proposed
created: 2026-05-09
updated: 2026-05-09
---

# Notes commands (browse, create, reorganize)

## Goal
Extend `ft` beyond tasks to general note operations: list, create from
templates, move (with link rewriting), rename (with link rewriting), and a
Notes tab in the TUI for browsing and reorganizing the vault. This plan is a
**placeholder backlog** — concrete acceptance criteria will be refined when we
get there. It exists so we can capture decisions and ideas as they come up
without polluting plans 001/002.

## Motivation and Context
The user described `ft` as a "swiss army knife for obsidian-style vaults that
use git as VCS". Tasks are the first vertical; notes are the natural second.
Many of the trickiest concerns (atomic file mutation, vault scanning, config,
test fixtures, TUI tab framework) are already solved by plans 001/002, so the
notes work mostly composes existing primitives plus a link-rewriting engine
that's genuinely new. We defer detailed scoping until plans 001/002 are far
enough along that we know what shape the library API takes.

## Acceptance Criteria
*To be refined when this plan is promoted from `proposed` to `ready`. Initial
sketch:*

### `ft notes` CLI
- [ ] `ft notes list` with filters (folder, tag, modified-since, contains-link-to, orphan, etc.)
- [ ] `ft notes create` from a template (read template list from `<vault>/.obsidian/templates/` or the templates core plugin config)
- [ ] `ft notes move <src> <dst>` with link rewriting across the entire vault
- [ ] `ft notes rename <src> <new-name>` with link rewriting (filename + heading-link forms)
- [ ] `ft notes orphans` lists notes nothing links to
- [ ] `ft notes graph` exports vault link graph as JSON (for piping into other tools)

### Templater plugin support (separate sub-feature)
- [ ] Read templater config and process `<%= %>` blocks for create flows
- [ ] At minimum: date insertion, filename insertion, simple variable substitution

### Notes tab in TUI
- [ ] File tree on the left, preview on the right (rendered markdown)
- [ ] Create / move / rename via keybindings, with link-rewrite preview before commit
- [ ] Backlinks pane

### Link rewriting (the genuinely new engine)
- [ ] Handles `[[wikilink]]`, `[[wikilink|alias]]`, `[[wikilink#heading]]`, `[[wikilink#^block]]`, and Markdown `[text](path.md)` forms
- [ ] Resolves Obsidian's "shortest-path-when-unique" link resolution rules
- [ ] Preview mode shows every change as a unified diff
- [ ] Atomic batch: either all files update or none do

## Technical Notes
- This plan inherits the workspace layout, config system, and atomic-write
  primitives from plan 001. New module: `ft-core/src/notes/` with `link.rs`
  for the link parser/rewriter
- Link rewriting needs its own fixture vault under `tests/fixtures/links/`
  with edge cases (case-insensitive matches, ambiguous shortest paths,
  embedded images, MD vs wikilink mixed)
- Probably the right time to introduce the index/cache layer that plan 001
  deferred — orphan/backlink queries are slow without it
- The Notes tab in the TUI is a new `Tab` impl; the framework from plan 002
  should accept it without changes

## Sessions
