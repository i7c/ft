## Why

`ft` only auto-discovers Obsidian vaults — a directory qualifies as a vault root
iff it contains a `.obsidian/` folder. This locks `ft` to Obsidian users even
though its task/notes/graph feature set works on any plain-Markdown tree. To
support standalone (non-Obsidian) note vaults, discovery should also recognize
an `ft`-native marker: a `.ft/` directory. `.ft/` already exists as the
per-vault config home (`<vault>/.ft/config.toml`) and cache dir, so a
hand-made `.ft/` is a natural, self-documenting way to mark a vault that does
not use Obsidian.

## What Changes

- Treat a directory as a vault root when it contains **either** `.obsidian/`
  **or** `.ft/`. The two markers are equivalent; discovery no longer requires
  `.obsidian/` specifically.
- Apply this relaxed marker check uniformly across all four discovery rungs:
  `--vault` flag, `FT_VAULT` env var, walk-up from CWD, and `default_vault` in
  `~/.config/ft/config.toml`.
- Generalize the `Error::VaultNotFound` "tried" list wording from
  `.obsidian/`-specific strings to `.obsidian/ or .ft/`, and generalize the
  top-level CLI context string from "could not locate an Obsidian vault" to
  "could not locate a vault" (part of the ongoing removal of Obsidian from
  `ft`'s framing).
- Add a code comment noting `.ft/` is excluded from scans by the walker's
  `.hidden(true)` filter (so it need not be added to `DEFAULT_IGNORED`).

## Capabilities

### New Capabilities
- `vault-discovery`: Defines how `ft` resolves the vault root at startup —
  the four-rung precedence (`--vault`, `FT_VAULT`, CWD walk-up, user-config
  `default_vault`), the set of recognized vault markers (`.obsidian/` and
  `.ft/`), and the debuggable failure listing. Codifies behavior that today
  lives only in code comments and prose docs.

### Modified Capabilities
<!-- No existing spec owns vault discovery; introduced as a new capability above. -->

## Impact

- **Code:** `ft-core/src/vault.rs` — extract a shared `is_vault_root(path)`
  predicate, use it in `find_vault` (all four rungs) and `walk_up`; refresh the
  `tried` strings and the `discover()` doc comment. `ft/src/cmd/common.rs` —
  generalize the `anyhow` context string.
- **Behavior:** A directory containing only `.ft/` (no `.obsidian/`) now
  succeeds as a vault for `--vault`, `FT_VAULT`, walk-up, and `default_vault`.
  No behavior change for existing `.obsidian/` vaults. A directory with both
  markers is treated as one vault (no preference, no warning).
- **Docs:** `docs/guide/vault-and-config.md` (precedence list + error block
  + intro paragraph) and the `README.md` discovery one-liner.
- **Tests:** New unit tests in `ft-core/src/vault.rs` for `.ft/`-only discovery
  across `find_vault`/`walk_up`; refresh the `error_message_lists_tried_locations`
  assertion for the new wording.
- **No new dependencies.** No new CLI flags. No `ft init` command — a standalone
  vault is bootstrapped by the user creating a `.ft/` directory (typically
  `.ft/config.toml`).
