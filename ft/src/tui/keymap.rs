#![allow(dead_code)]

//! Key chord parsing + the `KeyMap` data table.
//!
//! A [`KeyChord`] is a (`KeyCode`, `KeyModifiers`) pair. `KeyChord`s
//! are normalized so that shifted ASCII letters (`Char('A')` with no
//! modifier) match bindings stored as `Char('a')+Shift`, regardless of
//! which form the terminal sends.
//!
//! A [`KeyMap`] is a small `Vec<(KeyChord, Command)>` built via a
//! fluent `.bind("c", "graph.create-note")` builder. `lookup(chord)`
//! is a linear scan; the per-scope map is small (<~30 entries) so this
//! is the right complexity. Duplicate chords inside one map panic at
//! build time so collisions surface immediately.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::command::{Command, CommandArgs, CommandRegistry, CommandScope};

/// A key chord — code + modifier set, normalized so that an ASCII
/// uppercase `Char` is represented as the lowercase variant with the
/// `SHIFT` modifier set. Terminals are inconsistent about which form
/// they emit; the normalization step makes lookups stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl KeyChord {
    pub const fn new(code: KeyCode, mods: KeyModifiers) -> Self {
        Self { code, mods }.normalized_const()
    }

    /// `const fn` flavour of [`Self::normalize`] — used by `new()`.
    ///
    /// Normalization rules for `KeyCode::Char(c)`:
    /// 1. If `c` is ASCII uppercase: lowercase it and set `SHIFT`. This
    ///    folds `Char('C')+NONE` and `Char('c')+SHIFT` to the same chord.
    /// 2. Otherwise, if `c` is not ASCII alphabetic (digits, punctuation,
    ///    symbols): strip `SHIFT`. Shift state is already encoded in the
    ///    character identity (e.g. `?` is shift+`/`); terminals are
    ///    inconsistent about whether they additionally report the modifier.
    ///
    /// Non-`Char` codes (Tab, BackTab, F-keys, arrows, …) pass through —
    /// `Shift+Tab` is a distinct chord from `Tab`.
    const fn normalized_const(mut self) -> Self {
        if let KeyCode::Char(c) = self.code {
            if c.is_ascii_uppercase() {
                self.code = KeyCode::Char(c.to_ascii_lowercase());
                self.mods =
                    KeyModifiers::from_bits_truncate(self.mods.bits() | KeyModifiers::SHIFT.bits());
            } else if !c.is_ascii_alphabetic() {
                // Drop SHIFT for digits / punctuation / symbols.
                self.mods = KeyModifiers::from_bits_truncate(
                    self.mods.bits() & !KeyModifiers::SHIFT.bits(),
                );
            }
        }
        self
    }

    /// Construct from a crossterm [`KeyEvent`]. Always normalized.
    pub fn from_key_event(ev: KeyEvent) -> Self {
        Self {
            code: ev.code,
            mods: ev.modifiers,
        }
        .normalized()
    }

    /// Re-normalize an arbitrary chord (idempotent).
    pub fn normalized(self) -> Self {
        self.normalized_const()
    }
}

/// Parse a chord description like `"Ctrl+Shift+a"`, `"Space"`, `"Esc"`,
/// `"Alt+Left"`, `"F2"`, `"C"`. Returns `None` for unrecognised input.
///
/// Modifier names are case-insensitive (`Ctrl` / `ctrl` / `CONTROL`
/// all work). The last `+`-separated token is the key name.
pub fn chord_from_str(s: &str) -> Option<KeyChord> {
    let mut mods = KeyModifiers::NONE;
    let parts: Vec<&str> = s.split('+').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let (last, prefix) = parts.split_last().unwrap();
    for p in prefix {
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            "shift" => mods |= KeyModifiers::SHIFT,
            "alt" | "opt" | "option" => mods |= KeyModifiers::ALT,
            _ => return None,
        }
    }
    let code = match *last {
        "Space" | "space" => KeyCode::Char(' '),
        "Esc" | "Escape" | "esc" => KeyCode::Esc,
        "Enter" | "Return" | "enter" => KeyCode::Enter,
        "Tab" | "tab" => KeyCode::Tab,
        "BackTab" | "backtab" => KeyCode::BackTab,
        "Backspace" | "backspace" => KeyCode::Backspace,
        "Delete" | "Del" | "delete" | "del" => KeyCode::Delete,
        "Left" | "left" => KeyCode::Left,
        "Right" | "right" => KeyCode::Right,
        "Up" | "up" => KeyCode::Up,
        "Down" | "down" => KeyCode::Down,
        "PageUp" | "pageup" => KeyCode::PageUp,
        "PageDown" | "pagedown" => KeyCode::PageDown,
        "Home" | "home" => KeyCode::Home,
        "End" | "end" => KeyCode::End,
        "Insert" | "insert" => KeyCode::Insert,
        s if s.starts_with('F') && s.len() > 1 && s.len() < 4 => {
            let n: u8 = s[1..].parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            KeyCode::F(n)
        }
        s if s.chars().count() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        _ => return None,
    };
    Some(KeyChord { code, mods }.normalized())
}

/// Render a chord back to its canonical `"Mod+Key"` form.
/// `chord_from_str(chord_to_str(c)).unwrap() == c` for every well-formed `c`.
pub fn chord_to_str(chord: &KeyChord) -> String {
    let mut out = String::new();
    if chord.mods.contains(KeyModifiers::CONTROL) {
        out.push_str("Ctrl+");
    }
    if chord.mods.contains(KeyModifiers::ALT) {
        out.push_str("Alt+");
    }
    // For Char codes, SHIFT is part of normalization — render as
    // "Shift+a" so round-trip is stable.
    if chord.mods.contains(KeyModifiers::SHIFT) {
        out.push_str("Shift+");
    }
    let key = match chord.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        other => format!("{other:?}"),
    };
    out.push_str(&key);
    out
}

/// Scoped key-binding table. Built via the `bind` / `bind_with_args`
/// fluent API; queried via `lookup`.
#[derive(Debug, Clone, Default)]
pub struct KeyMap {
    bindings: Vec<(KeyChord, Command)>,
}

impl KeyMap {
    pub const fn empty() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Bind a chord (parsed from `chord_str`) to `command_name` with no
    /// args. Panics if `chord_str` is unparseable or the chord is
    /// already bound in this map.
    pub fn bind(mut self, chord_str: &str, command_name: &'static str) -> Self {
        let chord =
            chord_from_str(chord_str).unwrap_or_else(|| panic!("invalid chord: {chord_str:?}"));
        self.push(chord, Command::new(command_name), chord_str);
        self
    }

    /// Bind a chord to a command with inline args.
    pub fn bind_with_args(
        mut self,
        chord_str: &str,
        command_name: &'static str,
        args: &[(&'static str, &str)],
    ) -> Self {
        let chord =
            chord_from_str(chord_str).unwrap_or_else(|| panic!("invalid chord: {chord_str:?}"));
        let owned_args: Vec<(&'static str, String)> =
            args.iter().map(|(k, v)| (*k, v.to_string())).collect();
        let command = if owned_args.is_empty() {
            Command::new(command_name)
        } else {
            Command {
                name: command_name,
                args: CommandArgs::Inline(owned_args),
            }
        };
        self.push(chord, command, chord_str);
        self
    }

    fn push(&mut self, chord: KeyChord, command: Command, chord_str: &str) {
        if self.bindings.iter().any(|(c, _)| *c == chord) {
            let existing = self
                .bindings
                .iter()
                .find(|(c, _)| *c == chord)
                .map(|(_, cmd)| cmd.name)
                .unwrap_or("?");
            panic!(
                "duplicate chord in keymap: {chord_str:?} (already bound to {existing}, \
                 now trying to bind to {})",
                command.name
            );
        }
        self.bindings.push((chord, command));
    }

    /// Look up the command bound to `chord`. `chord` is normalized
    /// before comparison so callers can pass raw `KeyEvent`-derived
    /// chords.
    pub fn lookup(&self, chord: KeyChord) -> Option<&Command> {
        let n = chord.normalized();
        self.bindings
            .iter()
            .find(|(c, _)| *c == n)
            .map(|(_, cmd)| cmd)
    }

    pub fn iter(&self) -> impl Iterator<Item = &(KeyChord, Command)> {
        self.bindings.iter()
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ── Overlay types ─────────────────────────────────────────────────────────

/// Parse error for a chord string — produced by [`KeymapOverlay::from_raw`].
#[derive(Debug, thiserror::Error)]
#[error("invalid chord string")]
pub struct ChordParseError;

/// One user-defined binding: a parsed chord bound to a validated command.
#[derive(Debug, Clone)]
pub struct KeymapBinding {
    pub chord: KeyChord,
    pub command: Command,
}

/// A validated overlay: unbinds to drop from the base map, then
/// overrides to replace-or-append. Built by [`KeymapOverlay::from_raw`];
/// applied by [`KeyMap::with_overlay`].
#[derive(Debug, Clone, Default)]
pub struct KeymapOverlay {
    pub overrides: Vec<KeymapBinding>,
    pub unbinds: Vec<KeyChord>,
}

impl KeymapOverlay {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Errors produced by [`KeymapOverlay::from_raw`].
#[derive(Debug, thiserror::Error)]
pub enum KeymapOverlayError {
    #[error("invalid chord {raw:?} in scope {scope}: {source}")]
    InvalidChord {
        raw: String,
        #[source]
        source: ChordParseError,
        scope: String,
    },
    #[error("unknown command {name:?} in scope {scope}")]
    UnknownCommand { name: String, scope: String },
    #[error("chord {chord} listed in unbind for scope {scope} is not in the default keymap")]
    UnbindMissing { chord: String, scope: String },
    #[error("chord {chord} appears twice in scope {scope} overrides ({first:?} vs {second:?})")]
    OverlayCollision {
        chord: String,
        first: String,
        second: String,
        scope: String,
    },
}

/// Map a canonical scope string to a [`CommandScope`] for the known scopes.
pub fn parse_scope(s: &str) -> Option<CommandScope> {
    match s {
        "global" => Some(CommandScope::Global),
        "tab/graph" => Some(CommandScope::Tab("graph")),
        "tab/tasks" => Some(CommandScope::Tab("tasks")),
        "tab/notes" => Some(CommandScope::Tab("notes")),
        "tab/timeblocks" => Some(CommandScope::Tab("timeblocks")),
        "tab/journal" => Some(CommandScope::Tab("journal")),
        "modal/create" => Some(CommandScope::Modal("create")),
        "modal/append" => Some(CommandScope::Modal("append")),
        "modal/section-move" => Some(CommandScope::Modal("section-move")),
        "modal/capture-var" => Some(CommandScope::Modal("capture-var")),
        "modal/periodic-leader" => Some(CommandScope::Modal("periodic-leader")),
        "modal/query-bar" => Some(CommandScope::Modal("query-bar")),
        "modal/rename" => Some(CommandScope::Modal("rename")),
        "modal/search" => Some(CommandScope::Modal("search")),
        "modal/preset-picker" => Some(CommandScope::Modal("preset-picker")),
        "modal/capture-picker" => Some(CommandScope::Modal("capture-picker")),
        "modal/related" => Some(CommandScope::Modal("related")),
        "modal/move" => Some(CommandScope::Modal("move")),
        "widget/edit-buffer" => Some(CommandScope::Widget("edit-buffer")),
        _ => None,
    }
}

impl KeymapOverlay {
    /// Validate a raw scope table and unbind list against `base` and `registry`.
    ///
    /// Returns an `Ok(overlay)` if every entry parses cleanly, or
    /// `Err(errors)` with *all* problems (not just the first).
    ///
    /// `scope_str` is the canonical scope string (e.g. `"tab/graph"`) used in
    /// error messages.
    pub fn from_raw(
        raw_scope_table: &HashMap<String, String>,
        raw_unbinds: &[(String, String)],
        registry: &CommandRegistry,
        scope_str: &str,
        base: &KeyMap,
    ) -> Result<Self, Vec<KeymapOverlayError>> {
        let mut errors: Vec<KeymapOverlayError> = Vec::new();
        let mut overrides: Vec<KeymapBinding> = Vec::new();

        // Parse and validate override entries.
        for (raw_chord, cmd_name) in raw_scope_table {
            let chord = match chord_from_str(raw_chord) {
                Some(c) => c,
                None => {
                    errors.push(KeymapOverlayError::InvalidChord {
                        raw: raw_chord.clone(),
                        source: ChordParseError,
                        scope: scope_str.to_string(),
                    });
                    continue;
                }
            };
            if registry.lookup(cmd_name).is_none() {
                errors.push(KeymapOverlayError::UnknownCommand {
                    name: cmd_name.clone(),
                    scope: scope_str.to_string(),
                });
                continue;
            }
            // Collision: two override entries normalise to the same chord.
            if let Some(existing) = overrides.iter().find(|b| b.chord == chord) {
                errors.push(KeymapOverlayError::OverlayCollision {
                    chord: chord_to_str(&chord),
                    first: existing.command.name.to_string(),
                    second: cmd_name.clone(),
                    scope: scope_str.to_string(),
                });
                continue;
            }
            overrides.push(KeymapBinding {
                chord,
                command: Command::new(
                    registry.lookup(cmd_name).unwrap().name, // static lifetime
                ),
            });
        }

        // Validate unbind entries for this scope.
        let mut unbinds: Vec<KeyChord> = Vec::new();
        for (unbind_scope, raw_chord) in raw_unbinds {
            if unbind_scope != scope_str {
                continue;
            }
            let chord = match chord_from_str(raw_chord) {
                Some(c) => c,
                None => {
                    errors.push(KeymapOverlayError::InvalidChord {
                        raw: raw_chord.clone(),
                        source: ChordParseError,
                        scope: scope_str.to_string(),
                    });
                    continue;
                }
            };
            if base.lookup(chord).is_none() {
                errors.push(KeymapOverlayError::UnbindMissing {
                    chord: chord_to_str(&chord),
                    scope: scope_str.to_string(),
                });
                continue;
            }
            unbinds.push(chord);
        }

        if errors.is_empty() {
            Ok(Self { overrides, unbinds })
        } else {
            Err(errors)
        }
    }
}

impl KeyMap {
    /// Return a new `KeyMap` with `overlay` applied on top of `self`.
    ///
    /// Apply order:
    /// 1. Drop all unbinds from the base.
    /// 2. For each override: if the chord exists post-unbind, replace the
    ///    command; otherwise append as a new binding.
    ///
    /// Infallible — all errors are caught at validation time in
    /// [`KeymapOverlay::from_raw`].
    pub fn with_overlay(&self, overlay: &KeymapOverlay) -> KeyMap {
        let mut out = self.clone();
        for chord in &overlay.unbinds {
            out.bindings.retain(|(c, _)| c != chord);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: &str) -> String {
        chord_to_str(&chord_from_str(s).unwrap_or_else(|| panic!("parse failed: {s:?}")))
    }

    #[test]
    fn chord_round_trip_letters() {
        assert_eq!(round_trip("c"), "c");
        assert_eq!(round_trip("Shift+c"), "Shift+c");
        // Capital letter normalizes to Shift+lowercase.
        assert_eq!(round_trip("C"), "Shift+c");
    }

    #[test]
    fn chord_round_trip_modifiers() {
        assert_eq!(round_trip("Ctrl+a"), "Ctrl+a");
        assert_eq!(round_trip("Alt+Left"), "Alt+Left");
        assert_eq!(round_trip("Ctrl+Shift+p"), "Ctrl+Shift+p");
    }

    #[test]
    fn chord_round_trip_named() {
        assert_eq!(round_trip("Space"), "Space");
        assert_eq!(round_trip("Esc"), "Esc");
        assert_eq!(round_trip("Enter"), "Enter");
        assert_eq!(round_trip("Tab"), "Tab");
        assert_eq!(round_trip("BackTab"), "BackTab");
        assert_eq!(round_trip("PageDown"), "PageDown");
        assert_eq!(round_trip("F1"), "F1");
        assert_eq!(round_trip("F12"), "F12");
    }

    #[test]
    fn chord_modifier_aliases() {
        assert_eq!(chord_from_str("Ctrl+a"), chord_from_str("control+a"));
        assert_eq!(chord_from_str("Alt+x"), chord_from_str("opt+x"));
        assert_eq!(chord_from_str("Alt+x"), chord_from_str("option+x"));
    }

    #[test]
    fn chord_invalid_inputs() {
        assert!(chord_from_str("").is_none());
        assert!(chord_from_str("+").is_none());
        assert!(chord_from_str("Ctrl+").is_none());
        assert!(chord_from_str("Frobnicate+a").is_none());
        assert!(chord_from_str("F0").is_none()); // F0 is invalid (range 1..=24).
        assert!(chord_from_str("F25").is_none());
    }

    #[test]
    fn chord_from_key_event_normalizes_shift() {
        let ev = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::NONE);
        let chord = KeyChord::from_key_event(ev);
        assert_eq!(chord.code, KeyCode::Char('c'));
        assert!(chord.mods.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn chord_strips_shift_for_non_alpha_chars() {
        // `?` may arrive as Char('?')+SHIFT (Shift+/) or Char('?')+NONE
        // depending on the terminal. Normalization strips SHIFT so a
        // single `?` binding catches both.
        let with_shift =
            KeyChord::from_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));
        let without_shift =
            KeyChord::from_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(with_shift, without_shift);
        assert!(!with_shift.mods.contains(KeyModifiers::SHIFT));

        // Digits: same rule.
        let one_shift =
            KeyChord::from_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::SHIFT));
        assert!(!one_shift.mods.contains(KeyModifiers::SHIFT));

        // Tab+Shift is NOT stripped — it's a distinct chord.
        let shift_tab = KeyChord::from_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
        assert!(shift_tab.mods.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn keymap_bind_and_lookup() {
        let km = KeyMap::new()
            .bind("c", "graph.create-note")
            .bind("Shift+c", "graph.create-from-template")
            .bind("Ctrl+r", "graph.refresh");
        let lookup_c = km.lookup(KeyChord::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(lookup_c.map(|c| c.name), Some("graph.create-note"));

        // Capital-C (sent as Char('C') no mod) should match "Shift+c".
        let lookup_cap = km.lookup(KeyChord::from_key_event(KeyEvent::new(
            KeyCode::Char('C'),
            KeyModifiers::NONE,
        )));
        assert_eq!(
            lookup_cap.map(|c| c.name),
            Some("graph.create-from-template")
        );

        // Char('c') with SHIFT modifier should also match Shift+c.
        let lookup_shift_c = km.lookup(KeyChord::from_key_event(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::SHIFT,
        )));
        assert_eq!(
            lookup_shift_c.map(|c| c.name),
            Some("graph.create-from-template")
        );

        let lookup_ctrl_r = km.lookup(KeyChord::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(lookup_ctrl_r.map(|c| c.name), Some("graph.refresh"));

        // Unbound chord.
        assert!(km
            .lookup(KeyChord::new(KeyCode::Char('z'), KeyModifiers::NONE))
            .is_none());
    }

    #[test]
    fn keymap_bind_with_args() {
        let km =
            KeyMap::new().bind_with_args("C", "graph.create-note", &[("from_template", "true")]);
        let cmd = km
            .lookup(KeyChord::new(KeyCode::Char('C'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(cmd.name, "graph.create-note");
        assert_eq!(cmd.arg("from_template"), Some("true"));
    }

    #[test]
    #[should_panic(expected = "duplicate chord in keymap")]
    fn keymap_panics_on_duplicate_chord() {
        let _ = KeyMap::new()
            .bind("c", "graph.create-note")
            .bind("c", "graph.different-command");
    }

    #[test]
    #[should_panic(expected = "duplicate chord in keymap")]
    fn keymap_panics_on_duplicate_chord_via_normalization() {
        // "Shift+c" and "C" normalize to the same chord.
        let _ = KeyMap::new()
            .bind("Shift+c", "graph.first")
            .bind("C", "graph.second");
    }

    #[test]
    #[should_panic(expected = "invalid chord")]
    fn keymap_panics_on_unparseable_chord() {
        let _ = KeyMap::new().bind("NotAKey", "graph.foo");
    }

    #[test]
    fn keymap_iter_preserves_bind_order() {
        let km = KeyMap::new()
            .bind("c", "graph.create-note")
            .bind("o", "graph.open")
            .bind("r", "graph.refresh");
        let names: Vec<&str> = km.iter().map(|(_, cmd)| cmd.name).collect();
        assert_eq!(
            names,
            vec!["graph.create-note", "graph.open", "graph.refresh"]
        );
    }

    // ── Overlay tests ─────────────────────────────────────────────────────

    use crate::tui::command::{CommandDef, CommandRegistry, CommandScope};
    use std::collections::HashMap;

    static TEST_COMMANDS: &[CommandDef] = &[
        CommandDef {
            name: "graph.create-note",
            description: "Create",
            scope: CommandScope::Tab("graph"),
            group: "Mutations",
            args_schema: &[],
            opens_modal: false,
            is_primary: false,
        },
        CommandDef {
            name: "graph.refresh",
            description: "Refresh",
            scope: CommandScope::Tab("graph"),
            group: "Navigation",
            args_schema: &[],
            opens_modal: false,
            is_primary: false,
        },
        CommandDef {
            name: "graph.open",
            description: "Open",
            scope: CommandScope::Tab("graph"),
            group: "Navigation",
            args_schema: &[],
            opens_modal: false,
            is_primary: false,
        },
    ];

    fn test_registry() -> CommandRegistry {
        CommandRegistry::from_slices(&[TEST_COMMANDS])
    }

    fn base_map() -> KeyMap {
        KeyMap::new()
            .bind("c", "graph.create-note")
            .bind("r", "graph.refresh")
            .bind("Ctrl+r", "graph.refresh")
    }

    #[test]
    fn overlay_empty_round_trip() {
        let base = base_map();
        let overlay = KeymapOverlay::empty();
        let result = base.with_overlay(&overlay);
        assert_eq!(result.len(), base.len());
        assert_eq!(
            result.lookup(chord_from_str("c").unwrap()).map(|c| c.name),
            Some("graph.create-note")
        );
    }

    #[test]
    fn overlay_new_chord_append() {
        let base = base_map();
        let reg = test_registry();
        let mut table = HashMap::new();
        table.insert("o".to_string(), "graph.open".to_string());
        let overlay = KeymapOverlay::from_raw(&table, &[], &reg, "tab/graph", &base).unwrap();
        let result = base.with_overlay(&overlay);
        assert_eq!(
            result.lookup(chord_from_str("o").unwrap()).map(|c| c.name),
            Some("graph.open")
        );
        // Existing entries intact.
        assert_eq!(
            result.lookup(chord_from_str("c").unwrap()).map(|c| c.name),
            Some("graph.create-note")
        );
    }

    #[test]
    fn overlay_replace_existing_chord() {
        let base = base_map();
        let reg = test_registry();
        let mut table = HashMap::new();
        table.insert("c".to_string(), "graph.refresh".to_string());
        let overlay = KeymapOverlay::from_raw(&table, &[], &reg, "tab/graph", &base).unwrap();
        let result = base.with_overlay(&overlay);
        // c now triggers refresh.
        assert_eq!(
            result.lookup(chord_from_str("c").unwrap()).map(|c| c.name),
            Some("graph.refresh")
        );
    }

    #[test]
    fn overlay_unbind_without_replacement() {
        let base = base_map();
        let reg = test_registry();
        let unbinds = vec![("tab/graph".to_string(), "r".to_string())];
        let overlay =
            KeymapOverlay::from_raw(&HashMap::new(), &unbinds, &reg, "tab/graph", &base).unwrap();
        let result = base.with_overlay(&overlay);
        assert!(result.lookup(chord_from_str("r").unwrap()).is_none());
        // Ctrl+r still present.
        assert!(result.lookup(chord_from_str("Ctrl+r").unwrap()).is_some());
    }

    #[test]
    fn overlay_unbind_then_rebind() {
        let base = base_map();
        let reg = test_registry();
        let unbinds = vec![("tab/graph".to_string(), "r".to_string())];
        let mut table = HashMap::new();
        table.insert("r".to_string(), "graph.open".to_string());
        let overlay = KeymapOverlay::from_raw(&table, &unbinds, &reg, "tab/graph", &base).unwrap();
        let result = base.with_overlay(&overlay);
        assert_eq!(
            result.lookup(chord_from_str("r").unwrap()).map(|c| c.name),
            Some("graph.open")
        );
    }

    #[test]
    fn overlay_unknown_command_error() {
        let base = base_map();
        let reg = test_registry();
        let mut table = HashMap::new();
        table.insert("c".to_string(), "graph.no-such-command".to_string());
        let errs = KeymapOverlay::from_raw(&table, &[], &reg, "tab/graph", &base).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(
            matches!(&errs[0], KeymapOverlayError::UnknownCommand { name, .. } if name == "graph.no-such-command")
        );
    }

    #[test]
    fn overlay_invalid_chord_error() {
        let base = base_map();
        let reg = test_registry();
        let mut table = HashMap::new();
        table.insert("Frobnicate+x".to_string(), "graph.refresh".to_string());
        let errs = KeymapOverlay::from_raw(&table, &[], &reg, "tab/graph", &base).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(
            matches!(&errs[0], KeymapOverlayError::InvalidChord { raw, .. } if raw == "Frobnicate+x")
        );
    }

    #[test]
    fn overlay_unbind_missing_chord_error() {
        let base = base_map();
        let reg = test_registry();
        let unbinds = vec![("tab/graph".to_string(), "z".to_string())];
        let errs = KeymapOverlay::from_raw(&HashMap::new(), &unbinds, &reg, "tab/graph", &base)
            .unwrap_err();
        assert_eq!(errs.len(), 1);
        assert!(matches!(&errs[0], KeymapOverlayError::UnbindMissing { .. }));
    }

    #[test]
    fn overlay_collision_two_overrides_same_chord() {
        // Build a base without 'o' so both entries are "new chord" appends.
        let _base = KeyMap::new().bind("c", "graph.create-note");
        let reg = CommandRegistry::from_slices(&[TEST_COMMANDS]);
        let mut table = HashMap::new();
        table.insert("o".to_string(), "graph.open".to_string());
        table.insert("o".to_string(), "graph.refresh".to_string()); // same key in HashMap → deduped
                                                                    // Actually HashMap deduplication means we can only test via normalization collision:
                                                                    // Shift+c and C normalize to the same chord.
        let mut table2 = HashMap::new();
        table2.insert("Shift+r".to_string(), "graph.open".to_string());
        // "Shift+r" normalizes to different from "r", so let's use caps to get collision:
        // "C" and "Shift+c" both normalize to Shift+c.
        let base2 = KeyMap::new().bind("o", "graph.open");
        let mut col_table = HashMap::new();
        col_table.insert("Shift+c".to_string(), "graph.open".to_string());
        col_table.insert("C".to_string(), "graph.refresh".to_string()); // C == Shift+c
        let errs = KeymapOverlay::from_raw(&col_table, &[], &reg, "tab/graph", &base2).unwrap_err();
        let has_collision = errs
            .iter()
            .any(|e| matches!(e, KeymapOverlayError::OverlayCollision { .. }));
        assert!(has_collision);
    }

    #[test]
    fn overlay_all_errors_not_just_first() {
        let base = base_map();
        let reg = test_registry();
        let mut table = HashMap::new();
        table.insert("Frobnicate+x".to_string(), "graph.refresh".to_string());
        table.insert("c".to_string(), "graph.no-such-command".to_string());
        let errs = KeymapOverlay::from_raw(&table, &[], &reg, "tab/graph", &base).unwrap_err();
        assert_eq!(errs.len(), 2);
    }

    #[test]
    fn overlay_normalization_collision() {
        let base = KeyMap::new().bind("o", "graph.open");
        let reg = test_registry();
        let mut table = HashMap::new();
        table.insert("Shift+c".to_string(), "graph.open".to_string());
        table.insert("C".to_string(), "graph.refresh".to_string());
        let errs = KeymapOverlay::from_raw(&table, &[], &reg, "tab/graph", &base).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, KeymapOverlayError::OverlayCollision { .. })));
    }
}
