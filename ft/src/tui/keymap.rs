#![allow(dead_code)] // wired in §§4–7 (per-tab keymaps, ? overlay, docs gen)

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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::command::{Command, CommandArgs};

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
}
