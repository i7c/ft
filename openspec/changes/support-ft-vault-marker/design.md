## Context

`ft` resolves its vault root once at startup in `ft_core::vault::find_vault`. The
four-rung precedence (`--vault` flag → `FT_VAULT` env → walk-up from CWD →
user-config `default_vault`) is uniform except for one detail: every rung, plus
the `walk_up` helper, gates acceptance on `path.join(".obsidian").exists()`. A
directory qualifies as a vault iff it contains a `.obsidian/` folder.

`.ft/` is already an established `ft`-native path under the vault root — it
holds per-vault config (`<vault>/.ft/config.toml`) and the blame cache
(`<vault>/.ft/cache/blame.msgpack`). Today it is only ever *created* as a
side-effect of running `ft` inside an already-discovered `.obsidian/` vault, so
it has never been able to stand on its own as a marker. The change makes a
pre-existing `.ft/` directory a first-class vault marker so a non-Obsidian
note tree can be operated on by `ft` standalone.

The discovery algorithm is otherwise unchanged: same four rungs, same
first-hit-wins ordering, same `Error::VaultNotFound { tried }` debuggable
failure listing.

## Goals / Non-Goals

**Goals:**
- A directory with `.ft/` (and no `.obsidian/`) is accepted as a vault by every
  discovery rung.
- A single predicate owns the "is this a vault root?" test so the four rungs and
  `walk_up` cannot drift apart again.
- The `VaultNotFound` "tried" listing and the CLI context string stay accurate
  and stop asserting Obsidian-specifically (part of the ongoing de-Obsidian-izing
  of `ft`'s framing).
- Existing `.obsidian/` vaults behave identically.

**Non-Goals:**
- No `ft init` / scaffold command. A standalone vault is bootstrapped by the
  user creating a `.ft/` directory (typically by writing `.ft/config.toml`).
- No marker precedence. A directory with both markers is one vault; neither is
  preferred, and no warning is emitted.
- No change to scan exclusions. `.ft/` is already excluded as a dotfile via the
  walker's `.hidden(true)`; it is deliberately not added to `DEFAULT_IGNORED`
  (which only lists non-hidden defaults).
- No change to the `--obsidian` URL-scheme flags or the "Notes — Obsidian-flavoured
  editing" TUI header — those describe Obsidian interop, not discovery.

## Decisions

### Decision: one `is_vault_root(path) -> bool` predicate

Extract `fn is_vault_root(path: &Path) -> bool` returning
`path.join(".obsidian").is_dir() || path.join(".ft").is_dir()`. Use it in all
four `find_vault` rungs and in `walk_up`.

- **Why a predicate, not inlining the `||`:** the check is repeated in five
  places today; centralizing it is the whole point of the change and prevents
  future drift (e.g. a third marker added later).
- **Why `.is_dir()` not `.exists()`:** a stray file named `.ft` (e.g. created by
  a misdirected redirect `> .ft`) must not be mistaken for a vault marker.
  `.obsidian` is a directory by convention; `.ft` should be too. The current
  code uses `.exists()`, which accepts a file — switching to `.is_dir()` is a
  strict tightening that applies equally to `.obsidian` and is more correct.
- **Why no preference between markers:** a directory will not legitimately hold
  both. If it did, both identify the same root, so returning the directory is
  correct with no branching needed.

**Alternatives considered:**
- *Configurable marker list (`vault_markers = [".ft", ".obsidian"]`):* rejected —
  adds config surface and ambiguity (which marker "won"?) for no real user
  benefit. The two markers are fixed by this design.
- *`.ft` only, deprecating `.obsidian`:* rejected — would break every existing
  Obsidian vault on upgrade. Both stay valid.

### Decision: generalize the "tried" listing wording, not its structure

Each rung's failure string changes from `.obsidian/`-specific to
`.obsidian/ or .ft/`, e.g. `"--vault {}: no .obsidian/ or .ft/ found"` and
`"CWD walk from {}: no ancestor contains .obsidian/ or .ft/"`. The
`Error::VaultNotFound` enum variant and its `Display` impl are unchanged.

- **Why keep the structure:** the listing is consumed by humans and asserted on
  by one test; only the substring changes.
- **Why mention both markers explicitly:** the message is the debugging
  affordance — telling the user "make a `.ft/`" requires naming `.ft`.

### Decision: scan exclusion of `.ft/` stays implicit (comment, not code)

`.ft/` is *not* added to `DEFAULT_IGNORED`. The `WalkBuilder` is constructed
with `.hidden(true)`, which already excludes all dot-prefixed entries
including `.ft/` and `.obsidian/`. `DEFAULT_IGNORED` lists `.obsidian` and
`.git` redundantly for clarity/intent even though `.hidden(true)` covers them.

- **Why not add `.ft`:** it would be dead config — never the active exclusion
  path. A code comment at `DEFAULT_IGNORED` records that dotfile dirs (incl.
  `.ft/`, `.obsidian/`, `.git/`) are excluded by `.hidden(true)`, so future
  readers understand why `.ft` is absent from the list.

### Decision: generalize the CLI context string, not the `Error` enum

`ft/src/cmd/common.rs` changes its `anyhow::Context` string from
`"could not locate an Obsidian vault"` to `"could not locate a vault"`. The
`ft_core::error::Error::VaultNotFound` variant and its `#[error("vault not
found; searched:\n…")]` text are unchanged.

- **Why touch the context string at all:** the user asked to progressively
  remove Obsidian from the framing; this string is the single user-visible
  "couldn't find your vault" surface in the CLI.
- **Why leave the `Error` text:** it already says "vault not found" (not
  "Obsidian vault"); only the marker names in the `tried` body change.

## Risks / Trade-offs

- **[Risk: a stray `.ft/` directory falsely qualifies an unrelated tree as a
  vault]** → Mitigated by the `.is_dir()` tightening (a file named `.ft` no
  longer qualifies, matching the `.obsidian` tightening) and by the fact that
  `.ft/` is a distinctive enough name that accidental creation is unlikely.
  Walk-up will stop at the first ancestor with either marker, which is the
  existing walk-up semantics — no new surprise.
- **[Risk: existing tests assert on `.obsidian/`-specific error wording]**
  → Only `error_message_lists_tried_locations` checks wording, and it asserts
  `--vault` is *mentioned*, not the marker name — so it survives. New tests
  cover the `.ft/`-only path; no snapshot tests reference discovery wording.
- **[Risk: tests that build vaults by creating `.obsidian/` regress]**
  → No change to `.obsidian/` acceptance, so all existing vault-construction
  helpers (`make_obsidian_dir`, the TUI test helpers that `create_dir_all(.ft)`)
  keep working unchanged. They already create `.obsidian/`; the `.ft/` they
  create is now *also* a sufficient marker, which only makes tests more
  permissive, not less.
- **[Trade-off: `.ft/` can be created as a side-effect of running `ft` in a
  `.obsidian/` vault (blame cache save)]** → This is benign: such a `.ft/` sits
  *inside* an already-accepted vault and never participates in discovery of a
  *different* root (walk-up stops at the outer `.obsidian/` ancestor first). No
  behavior change.

## Migration Plan

- No data migration. No config migration. No breaking change to existing
  `.obsidian/` vaults.
- Deploy: ship the code change + docs. Existing users see no difference.
  Standalone users gain discovery by creating a `.ft/` directory.
- Rollback: revert the commit; `.obsidian/`-only discovery is restored. No
  on-disk state to clean up.

## Open Questions

None. All four clarifying questions (marker precedence, `--vault`/`FT_VAULT`/
`default_vault` semantics, no `ft init`, generalizing the context string) were
resolved with the user before proposal.
