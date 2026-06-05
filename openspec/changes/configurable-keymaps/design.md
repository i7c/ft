## Context

After `commands-and-keymaps`, every TUI action is a registered `Command` with a stable `<context>.<verb>` name and a `CommandRegistry` that powers the `?` overlay, `docs/keybindings.md`, `ft commands list`, and `ft do`. The chord side of the lookup is still source-only: each tab/modal/App ships a `pub static <NAME>_KEYMAP: LazyLock<KeyMap>` built once at process start by chaining `.bind(...)` calls.

The `KeyMap` data shape was deliberately designed for overlays: bindings are `Vec<(KeyChord, Command)>`, chords round-trip cleanly via `chord_from_str` / `chord_to_str`, commands are name-keyed strings rather than enum variants, and the registry already validates that every keymap binding resolves. The static maps are also one of the only things in the App that today's `Config` does not touch — the user can already tweak `[git]`, `[daily_notes]`, `[periodic_notes]`, queries/graph presets, but not bindings.

This change closes that gap with a `[keymap]` table in the existing layered `config.toml`.

## Goals / Non-Goals

**Goals:**

- Users can rebind any registered command from `config.toml` without recompiling.
- Overrides are *additive*: defaults stay the source of truth (and what `docs/keybindings.md` documents); user config layers on top.
- Three operations: replace the command for a chord, bind a new chord to a registered command, unbind a chord (drop the default binding without replacing it).
- A `ft commands check-keymap` lint flags every error (unknown chord string, unknown command name, collision within a scope, unbind-on-missing) so users can validate their config before launching the TUI.
- Errors at startup are *opt-in fatal*: by default, an invalid `[keymap]` entry logs a warning and is skipped, and the TUI launches with the unaffected defaults. A `keymap.strict = true` flag promotes the warnings to a hard failure for users who'd rather fail fast.
- All four build invariants stay green; the new code path is keymap-only and doesn't touch the command/registry/dispatch core.

**Non-Goals:**

- No vim-style multi-chord sequences (`g g`, `, f r`). Same boundary as `commands-and-keymaps`; transient leader modals stay the way to express them.
- No GUI / TUI keymap editor. The flow is "edit `config.toml`, restart `ft tui`".
- No hot-reload. Restart-to-apply is acceptable v1; the App constructs the effective maps once at startup. Hot-reload can land as a follow-up if anyone hits the friction.
- No per-vault keymap *separate* from the existing layered config. The user's `[keymap]` table can live in either the user-level config or the vault-level config; the existing `Config::load` merge order (vault overrides user) decides who wins on conflict.
- No new commands. Rebinding only — adding a command continues to require a code change so the registry stays the single source of truth on what exists.

## Decisions

### Config schema

```toml
# config.toml — user-level or vault-level

[keymap]
strict = false          # default: warn-and-skip. true: fail App startup on any error.
unbind = [              # explicit removal of default bindings (per scope).
  { scope = "global", chord = "g" },          # drop the git-leader default
  { scope = "tab/graph", chord = "Ctrl+r" },
]

[keymap.global]
"Ctrl+s" = "app.sync-git"          # currently bound to `g` leader → `s`; reachable directly.
"Ctrl+q" = "app.quit"

[keymap."tab/graph"]
"R" = "graph.refresh"              # capital-R for refresh; default Ctrl+r stays unless unbound.
"Ctrl+f" = "graph.search"

[keymap."modal/section-move"]
"x" = "section-move.toggle"        # add `x` as an alias for Space:toggle.
```

The TOML quote rules (`tab/graph` needs quoting because of the slash) are an artefact of using the same scope strings the registry already exposes via `CommandScope::as_str()` — same names show up in `ft commands list --scope ...`, the `?` overlay headers, and `docs/keybindings.md`. The cost of "you have to quote it" is worth the one-namespace-everywhere consistency.

**Alternative considered: flat single-table.** `[keymap]` with entries like `"global:Ctrl+s" = "app.sync-git"`. Rejected — TOML's sub-table form is more readable and the scope is a first-class concept in the rest of the system, not a packed key prefix.

**Alternative considered: separate `keymap.toml`.** A standalone file under `~/.config/ft/keymap.toml`. Rejected — `ft` already ships layered `config.toml` with vault override semantics; users would have to learn a second layered system. A `[keymap]` table reuses what they already know.

### The overlay type

```rust
// ft/src/tui/keymap.rs

pub struct KeymapOverlay {
    pub overrides: Vec<KeymapBinding>,   // chord → command
    pub unbinds:   Vec<KeyChord>,        // chord to drop from the base
}

pub struct KeymapBinding {
    pub chord:   KeyChord,         // already parsed + normalized
    pub command: Command,          // already validated against the registry
}

pub enum KeymapOverlayError {
    InvalidChord { raw: String, source: ChordParseError },
    UnknownCommand { name: String, scope: CommandScope },
    UnbindMissing { chord: KeyChord, scope: CommandScope },
    OverlayCollision { chord: KeyChord, first: String, second: String, scope: CommandScope },
}
```

The overlay is a *parsed and validated* structure — every chord and every command name has been resolved before the type is constructed. The TOML deserialiser (`ft-core/src/config.rs`) holds raw strings; the `KeymapOverlay::from_raw(raw, registry, scope) -> Result<...>` constructor does the validation in one pass and produces either the typed overlay or a list of errors. Splitting parse-time from apply-time means `ft commands check-keymap` can run the validator without instantiating an `App`.

### Overlay semantics — apply order

```rust
impl KeyMap {
    pub fn with_overlay(&self, overlay: &KeymapOverlay) -> KeyMap {
        let mut out = self.clone();
        // 1. Drop unbinds first, so an override on the same chord
        //    behaves as "remove then add" not "duplicate-collision panic".
        for chord in &overlay.unbinds {
            out.bindings.retain(|(c, _)| c != chord);
        }
        // 2. Apply overrides. If the chord still exists post-unbind
        //    (override on top of a default), the existing entry is
        //    *replaced*, not appended.
        for b in &overlay.overrides {
            if let Some(slot) = out.bindings.iter_mut().find(|(c, _)| *c == b.chord) {
                slot.1 = b.command.clone();
            } else {
                out.bindings.push((b.chord, b.command.clone()));
            }
        }
        out
    }
}
```

Applying unbinds first solves the "rebind a default chord to a different command" case naturally: the user adds the chord to both the unbind list and the override sub-table, the unbind clears the default, then the override sets the new command. The "just want to add a brand-new chord" case (chord not in the base map) skips both steps and falls through to the append branch.

`with_overlay` is intentionally *infallible* — every error case is caught at validation time by `KeymapOverlay::from_raw`. The runtime function is a pure data transform.

**Alternative considered: in-place mutation on the static maps.** Rejected — the static `LazyLock<KeyMap>` is shared, and mutating it would propagate the user's bindings into every test. Per-instance `with_overlay` keeps the static maps as the immutable defaults.

**Alternative considered: override priority via "user wins, then default".** Two-map lookup at runtime — check user overlay first, fall back to the static map. Rejected — it doubles the lookup cost on the hot path, and "user wins on chord" is exactly the behavior `with_overlay` produces by replacement, so the runtime stays a single linear scan.

### Where the effective maps live

Today: `Tab::keymap()` and `Modal::keymap()` return `&'static KeyMap` borrowed from a `LazyLock<KeyMap>` in each tab's source file. That works because there's exactly one keymap per type.

After this change: each tab and modal needs to return *its own* effective map (defaults overlaid with the user's `[keymap."tab/<name>"]` entries). Two options:

**A. Per-instance field.** Each tab/modal struct gains a `keymap: KeyMap` field initialised at construction from the static defaults + overlay. `Tab::keymap(&self) -> &KeyMap` returns `&self.keymap`. App-global lives on `App` (already non-static).

**B. App-level keymap registry.** The App owns a `HashMap<CommandScope, KeyMap>` and tabs/modals look themselves up. `Tab::keymap(&self, app: &App)` — but this means changing every trait signature in the codebase.

**Decision: A**, per-instance fields. It's the smaller change (the `LazyLock` was always a stop-gap; the design.md from `commands-and-keymaps` already calls out that "returning a borrow avoids cloning per keystroke" is the only constraint). Modal variants stored inside `ActiveModal` get their map computed at construction time too — modals are short-lived enough that the one-time copy on open is irrelevant.

The static `<NAME>_KEYMAP: LazyLock<KeyMap>` slices stay as the source of truth for the *defaults*. The construction path becomes:

```rust
// At App::new (and at every modal open):
let user_overlay = config.keymap_overlay_for(scope, &registry)?;
let effective = APP_KEYMAP.with_overlay(&user_overlay);   // for global
```

### Where the overlay is validated

Three places:

1. **`KeymapOverlay::from_raw`** — pure function over `(raw_toml_section, &CommandRegistry, scope)`. Returns `Result<KeymapOverlay, Vec<KeymapOverlayError>>`. No App, no I/O. This is the validator.
2. **`App::new`** — calls the validator per scope. Behaviour on error governed by `keymap.strict`: false (default) → log each error via `tracing::warn!` and proceed with the partial overlay (errors omitted, valid entries still applied); true → return an `anyhow::Error` from `App::new` and let the binary's top-level error path render it. `ft tui` exits with code 1 in strict mode, code 0 (with warnings to stderr) otherwise.
3. **`ft commands check-keymap`** — calls the validator per scope, reports every error to stderr (or as a JSON array under `--json-errors`), exits 2 on any error, 0 on clean. No App required.

The three callers share one function. Tests live next to the validator.

### Module placement and trait surface

- `KeymapOverlay` and `KeymapOverlayError` live in `ft/src/tui/keymap.rs` (same module as `KeyMap`). The TOML parsing lives in `ft-core/src/config.rs` (where every other schema field lives), producing raw `HashMap<String, String>` per scope sub-table; `KeymapOverlay::from_raw` consumes the raw form.
- No new trait method on `Tab` or `Modal`. `keymap(&self) -> &KeyMap` stays; the implementation switches from `&STATIC_KEYMAP` to `&self.keymap`.
- App holds the `CommandRegistry` already (`commands-and-keymaps` §1.3). It also gains an `effective_global_keymap: KeyMap` field; `global_keymap()` returns `&self.effective_global_keymap` instead of `&APP_KEYMAP`.

### CLI surface

- New subcommand: `ft commands check-keymap`. Same dispatch group as `ft commands list` / `ft commands docs`. Returns 0 on clean, 2 on any validation error. Supports `--format text|json` (default text); JSON shape is `[{ "scope": "tab/graph", "chord": "??", "error": "...", "raw": "..." }, ...]`. Honours top-level `--json-errors`.
- `ft commands list` and `ft commands docs` are unchanged — they list/document the registry, not the effective keymap. `docs/keybindings.md` continues to reflect the defaults so it stays stable across users.
- A new flag on `ft commands list`: `--effective` shows the user's resolved bindings rather than the defaults. Useful for "what is `Ctrl+s` actually doing in my setup".

## Risks / Trade-offs

- **[`docs/keybindings.md` diverges from what the user actually has bound]** → Mitigation: documented behaviour. The file is the canonical defaults reference; `ft commands list --effective` is the per-user view. The docs file rendering does not load the user's `[keymap]`, so the CI freshness check stays stable. A note in `docs/commands.md` explains this contract.
- **[A typo in `config.toml` silently disables a binding in non-strict mode]** → Mitigation: every error logs a `tracing::warn!` (which `ft tui` already routes to its trace sink, but `ft commands check-keymap` surfaces). The `--strict` setting is the user's escape hatch. A startup-time toast ("Y of N keymap entries failed; see `ft commands check-keymap`") could be added as a follow-up if it bites.
- **[Per-instance keymap field grows every tab/modal struct]** → Acceptable. A `KeyMap` is a `Vec<(KeyChord, Command)>` ~24 bytes empty + ~32 bytes per entry; the largest is `GRAPH_KEYMAP` with 44 entries → ~1.4 KB. Modal variants are short-lived enough that the per-open allocation doesn't matter.
- **[Overlay layered config interaction]** → The existing `Config::load(user_config, vault_config)` merges fields with vault-overriding-user. The `[keymap]` table follows the same rule: vault's `[keymap]` *replaces* user's `[keymap]` whole rather than per-entry merging. That's the cheaper and clearer semantics (per-entry merging across two TOML levels is hard to explain). Documented.
- **[Future: chord sequences (`g s`) need different storage]** → Out of scope here. When that lands, the overlay schema can extend to `{ chord = ["g", "s"], command = "..." }` array form without breaking the current `"Ctrl+s" = "app.sync-git"` single-key form. The split overlay/static design isolates the change.
- **[Snapshot churn in `?` overlay tests for users with overrides]** → No churn for the project's own tests (they construct `App` without a user `[keymap]`, so the overlay is empty). Documented in `docs/commands.md`: snapshots are taken against defaults.

## Migration Plan

No migration needed. Existing `config.toml` files with no `[keymap]` table get an empty overlay; the App produces byte-identical effective keymaps to today.

The `commands-and-keymaps` change already ensured every binding is a registered command, so the rebind target name set is stable today. Future commands added via the normal `commands-and-keymaps` pattern automatically become rebindable without further work here.

## Open Questions

- Should `ft commands check-keymap` also flag *semantic* warnings (e.g., "you've unbound `Tab`, no way to cycle tabs from the keyboard")? **Leaning:** no. Power users may bind their own equivalents; lint should stay syntactic.
- Should overrides survive a typo in `chord` by suggesting nearest matches (`"Ctrl+SHift+a"` → did you mean `"Ctrl+Shift+a"`)? **Leaning:** the existing `chord_from_str` error is informative enough for v1; add suggestions later if users hit it.
- Should the App emit a startup-time toast when `[keymap]` had errors? **Leaning:** v1 says no — tracing warnings + `ft commands check-keymap` is enough. Promote to a toast only if real users miss the warnings.
