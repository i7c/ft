//! Help overlay data model.
//!
//! Sections rendered by the `?` overlay are derived from `KeyMap` data
//! and the central `CommandRegistry` via [`sections_from_keymap`]. Each
//! `(chord, command)` row in the active context's keymap produces one
//! `HelpEntry`; the row's group comes from `CommandDef.group`. Aliases
//! (multiple chords bound to the same command) collapse into one row
//! with the chords joined by `" / "`.
//!
//! Hand-curated sections are no longer the source of truth — the
//! renderer is generated from the same data the dispatcher uses, which
//! makes the `?` overlay automatically stay in sync with bindings.

/// One row in the help overlay: the key combo on the left, the description
/// on the right.
#[derive(Debug, Clone)]
pub struct HelpEntry {
    pub keys: String,
    pub desc: String,
}

impl HelpEntry {
    pub fn new(keys: impl Into<String>, desc: impl Into<String>) -> Self {
        Self {
            keys: keys.into(),
            desc: desc.into(),
        }
    }
}

/// Named group of entries (e.g. "Navigation", "Mutations", "Modals"). The
/// renderer prints the title in cyan above its rows so tabs with many
/// bindings stay readable.
#[derive(Debug, Clone)]
pub struct HelpSection {
    pub title: String,
    pub entries: Vec<HelpEntry>,
}

impl HelpSection {
    pub fn new(title: impl Into<String>, entries: &[(&str, &str)]) -> Self {
        Self {
            title: title.into(),
            entries: entries
                .iter()
                .map(|(k, d)| HelpEntry::new(*k, *d))
                .collect(),
        }
    }
}

/// App-level bindings rendered first in every `?` overlay regardless
/// of active tab. Built from the effective global keymap + the central
/// registry; accepts the effective map so user overlays are reflected.
pub fn global_section(
    keymap: &crate::tui::keymap::KeyMap,
    registry: &crate::tui::command::CommandRegistry,
) -> HelpSection {
    let mut sections = sections_from_keymap(keymap, registry);
    // Coalesce all global sections into one titled "Global" — the
    // pre-migration overlay grouped every global chord under that name.
    let entries: Vec<HelpEntry> = sections.drain(..).flat_map(|s| s.entries).collect();
    HelpSection {
        title: "Global".to_string(),
        entries,
    }
}

/// Build a list of help sections from a keymap and the registry.
///
/// Aliases (multiple chords bound to the same command) collapse onto
/// one row with chords joined by `" / "`. Rows are grouped by the
/// command's `CommandDef.group`; group order follows the first-bind
/// order in the keymap, so the keymap's declaration order controls
/// the help overlay's section order.
pub fn sections_from_keymap(
    keymap: &crate::tui::keymap::KeyMap,
    registry: &crate::tui::command::CommandRegistry,
) -> Vec<HelpSection> {
    use std::collections::HashMap;

    // Collect chords per command, preserving first-seen order.
    let mut by_cmd: HashMap<&'static str, Vec<crate::tui::keymap::KeyChord>> = HashMap::new();
    let mut ordered: Vec<&'static str> = Vec::new();
    for (chord, cmd) in keymap.iter() {
        if !by_cmd.contains_key(cmd.name) {
            ordered.push(cmd.name);
        }
        by_cmd.entry(cmd.name).or_default().push(*chord);
    }

    // Build (group, entries) in the order groups are first encountered.
    let mut sections: Vec<(&'static str, Vec<HelpEntry>)> = Vec::new();
    for name in ordered {
        let chords = by_cmd.get(name).unwrap();
        let Some(def) = registry.lookup(name) else {
            continue;
        };
        let formatted = format_chord_list(chords);
        let entry = HelpEntry::new(formatted, def.description);
        if let Some((_, rows)) = sections.iter_mut().find(|(g, _)| *g == def.group) {
            rows.push(entry);
        } else {
            sections.push((def.group, vec![entry]));
        }
    }
    sections
        .into_iter()
        .map(|(title, entries)| HelpSection {
            title: title.to_string(),
            entries,
        })
        .collect()
}

/// Format a list of chords bound to the same command for the help
/// overlay. Up to 3 chords are joined with `" / "`; sequential
/// modifier+digit aliases (e.g. `Alt+1`..`Alt+9`) are collapsed into
/// a range form (`Alt+1..Alt+9`) so the key column doesn't eat the
/// description.
fn format_chord_list(chords: &[crate::tui::keymap::KeyChord]) -> String {
    use crossterm::event::KeyCode;
    // Detect a contiguous mod+digit run (1..9) — the common case is
    // `Alt+1..Alt+9` for view jumps. Same modifiers across all chords
    // and an ascending run of consecutive digits.
    if chords.len() >= 4 {
        let mods0 = chords[0].mods;
        let digits: Option<Vec<char>> = chords
            .iter()
            .map(|c| match c.code {
                KeyCode::Char(d) if d.is_ascii_digit() && c.mods == mods0 => Some(d),
                _ => None,
            })
            .collect();
        if let Some(ds) = digits {
            let sorted = {
                let mut s = ds.clone();
                s.sort();
                s
            };
            let contiguous = sorted
                .windows(2)
                .all(|w| (w[1] as u32) == (w[0] as u32 + 1));
            if contiguous {
                return format!(
                    "{}..{}",
                    chord_display(&chords[0]),
                    chord_display(chords.last().unwrap())
                );
            }
        }
    }
    let mut shown: Vec<String> = chords.iter().take(3).map(chord_display).collect();
    if chords.len() > 3 {
        shown.push("…".to_string());
    }
    shown.join(" / ")
}

/// Up to three `(chord, label)` pairs for the status-bar modal hint.
///
/// Walks the modal's keymap, keeps only bindings whose `CommandDef`
/// has `is_primary = true`, and renders the chord with [`chord_display`]
/// plus a short label derived from the verb portion of the command
/// name (`section-move.toggle` → `toggle`). The cap of three keeps the
/// hint inside the status bar's center cell at narrow terminal widths.
///
/// Order follows the keymap's bind order — the modal author controls
/// which chords are surfaced by ordering primaries first in the
/// keymap's `bind` chain.
pub fn modal_primary_hints(
    keymap: &crate::tui::keymap::KeyMap,
    registry: &crate::tui::command::CommandRegistry,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::with_capacity(3);
    for (chord, cmd) in keymap.iter() {
        if out.len() == 3 {
            break;
        }
        let Some(def) = registry.lookup(cmd.name) else {
            continue;
        };
        if !def.is_primary {
            continue;
        }
        let label = cmd
            .name
            .rsplit_once('.')
            .map(|(_, v)| v)
            .unwrap_or(cmd.name);
        out.push((chord_display(chord), label.to_string()));
    }
    out
}

/// Pretty-print a chord for the help overlay. Arrows render as
/// unicode glyphs (`↑↓←→`); everything else uses the canonical form
/// from `chord_to_str` (`Shift+c`, `Ctrl+r`, `Space`, `Esc`, …).
fn chord_display(chord: &crate::tui::keymap::KeyChord) -> String {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (chord.code, chord.mods) {
        (KeyCode::Up, KeyModifiers::NONE) => "↑".to_string(),
        (KeyCode::Down, KeyModifiers::NONE) => "↓".to_string(),
        (KeyCode::Left, KeyModifiers::NONE) => "←".to_string(),
        (KeyCode::Right, KeyModifiers::NONE) => "→".to_string(),
        _ => crate::tui::keymap::chord_to_str(chord),
    }
}
