# ft-nvim-architecture — tasks

All tasks land in the `ft.nvim` repository (sibling of this one). Nothing
in this change touches `ft`/`ft-core`. After completion, the ft.nvim
commit is recorded in the archive note of this change as the paired sha.

## 1. Transport seam

- [ ] 1.1 Add `lua/ft/rpc.lua` with bin resolution: `FT_BIN` env var first, then `ft` from PATH (`vim.fn.executable`), with a clear setup error when neither exists
- [ ] 1.2 Implement `rpc.call(args)` — synchronous run (via `vim.fn.system`), normalized `(stdout, exit_code)` return, `--vault <root>` injection when the vault is known, errors surfaced as `vim.notify` notifications
- [ ] 1.3 Implement `rpc.job(args, kind, on_done)` — `vim.fn.jobstart`, result delivered via a `User ft:rpc-done` autocmd carrying `(kind, stdout, exit_code)`
- [ ] 1.4 Implement single-flight slots per job kind: at most one in-flight job per kind; a second request while in flight sets a dirty flag for exactly one follow-up (or is rejected with a notification)
- [ ] 1.5 Add a module-level guard comment and a unit test asserting `rpc` is the only module that spawns the `ft` binary (see 7.1)

## 2. Migrate existing functionality onto the seam

- [ ] 2.1 `vault.lua`: remove `ft_run`/`ft_cmd`; keep discovery (FT_VAULT → explicit config → walk-up); expose the vault root to `rpc` for `--vault` injection
- [ ] 2.2 `cache.lua`: rebuild the note index via `rpc.job` (async, single-flight); keep the existing `graph query … --format ndjson` shape; add the generation counter
- [ ] 2.3 `complete.lua` + `blink.lua`: route queries through `rpc`; consume the generation counter so a stale session re-derives
- [ ] 2.4 `follow.lua`: resolve targets via `rpc.call`
- [ ] 2.5 Grep-guard: no `vim.fn.system` / `io.popen` calls to `ft` outside `rpc.lua` (scripted check, e.g. `rg "ft_run|system" lua/ft/`)

## 3. Picker seam

- [ ] 3.1 Add `lua/ft/picker.lua` with `select(items, opts)` defaulting to `vim.ui.select`, plus feature-detected telescope and fzf-lua backends (used only when installed and enabled)
- [ ] 3.2 Declare `multi(items, opts)` raising a clear "multi-select not yet implemented" error (per design D5); no backend work until a feature requires it

## 4. Embeds removal (BREAKING)

- [ ] 4.1 Delete `lua/ft/embed.lua`
- [ ] 4.2 Strip embed setup wiring from `init.lua` (`_setup_buffer` arm) and remove the `embeds` / `max_lines` config keys from defaults and docs
- [ ] 4.3 Update README: remove the embed feature bullet, update config examples, and add an explicit "embeds no longer supported" note

## 5. Version check

- [ ] 5.1 Add `min_ft_version` check in `setup()`: run `ft --version`, warn (never fail) when the installed binary is older than the documented minimum

## 6. Documentation

- [ ] 6.1 Write `ARCHITECTURE.md` in the ft.nvim repository covering: the four pillars (domain-in-ft, transport seam, freshness model, TUI-handoff concurrency contract), the picker seam, the no-embeds decision, the ft CLI protocol contract table (every command/flag/output format the plugin depends on, mapped to plugin features), the decision log with alternatives, and "when to add an ft command vs Lua" guidance
- [ ] 6.2 Point the README at `ARCHITECTURE.md` with a one-paragraph summary of the architecture

## 7. Verification

- [ ] 7.1 Add a minimal unit test (Lua `vim.fn` mock or source scan) asserting every `ft` process spawn in `lua/ft/` lives inside `rpc.lua`
- [ ] 7.2 Smoke test: create a temp fixture vault (`.obsidian/` marker + two notes with a wikilink), run nvim headless with the plugin, and verify follow resolves, completion returns the fixture's titles, and setup warns correctly on an artificially old `ft --version`
- [ ] 7.3 Smoke test embeds removal: a config containing `embeds = { … }` loads without error and no embed rendering occurs
- [ ] 7.4 Commit the ft.nvim work as one commit; record its sha here in the change (archive note) as the paired sha
