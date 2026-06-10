//! Keymap + command set for the shared [`EditBuffer`] widget.
//!
//! Every TUI surface that mounts an `EditBuffer` (graph query bar,
//! fuzzy picker input, rename modal, quickline, capture var prompt,
//! timeblocks form, journal entry) routes raw key events through
//! [`EditBuffer::handle_event`], which:
//!
//! 1. Looks the chord up in [`EDIT_KEYMAP`]. If found, dispatches the
//!    `edit.*` command via [`dispatch_edit_command`].
//! 2. Otherwise, for a printable `Char` with no `Ctrl` modifier, inserts
//!    the character.
//! 3. Otherwise, returns [`EditOutcome::NotHandled`] — the host modal
//!    sees the key.
//!
//! Bindings are flat: one chord, one command, no per-mount variation.
//! The host's keymap (modal or tab) only sees chords the buffer didn't
//! recognise.

use std::sync::LazyLock;

use crossterm::event::KeyEvent;

use crate::tui::command::{Command, CommandDef, CommandScope};
use crate::tui::keymap::{KeyChord, KeyMap};
use crate::tui::widgets::edit_buffer::EditBuffer;

const EDIT_SCOPE: CommandScope = CommandScope::Widget("edit-buffer");

const fn cmd(name: &'static str, description: &'static str, group: &'static str) -> CommandDef {
    CommandDef {
        name,
        description,
        scope: EDIT_SCOPE,
        group,
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    }
}

pub static EDIT_COMMANDS: &[CommandDef] = &[
    cmd(
        "edit.move-line-start",
        "Move cursor to start of line",
        "Navigation",
    ),
    cmd(
        "edit.move-line-end",
        "Move cursor to end of line",
        "Navigation",
    ),
    cmd(
        "edit.move-char-back",
        "Move cursor one char back",
        "Navigation",
    ),
    cmd(
        "edit.move-char-forward",
        "Move cursor one char forward",
        "Navigation",
    ),
    cmd(
        "edit.move-word-back",
        "Move cursor one word back",
        "Navigation",
    ),
    cmd(
        "edit.move-word-forward",
        "Move cursor one word forward",
        "Navigation",
    ),
    cmd(
        "edit.kill-to-end",
        "Delete to end of line; save to kill ring",
        "Editing",
    ),
    cmd(
        "edit.kill-to-start",
        "Delete to start of line; save to kill ring",
        "Editing",
    ),
    cmd(
        "edit.kill-word-back",
        "Delete the word before the cursor",
        "Editing",
    ),
    cmd(
        "edit.kill-word-forward",
        "Delete the word after the cursor",
        "Editing",
    ),
    cmd("edit.yank", "Insert kill ring at cursor", "Editing"),
    cmd(
        "edit.transpose-chars",
        "Swap the two chars around the cursor",
        "Editing",
    ),
    cmd(
        "edit.delete-char-back",
        "Delete the char before the cursor",
        "Editing",
    ),
    cmd(
        "edit.delete-char-forward",
        "Delete the char at the cursor",
        "Editing",
    ),
];

pub static EDIT_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        // Line jumps
        .bind("Ctrl+a", "edit.move-line-start")
        .bind("Home", "edit.move-line-start")
        .bind("Ctrl+e", "edit.move-line-end")
        .bind("End", "edit.move-line-end")
        // Char moves
        .bind("Ctrl+b", "edit.move-char-back")
        .bind("Left", "edit.move-char-back")
        .bind("Ctrl+f", "edit.move-char-forward")
        .bind("Right", "edit.move-char-forward")
        // Word moves — bind both Alt+letter and Alt+arrow forms so
        // macOS Opt+arrow (which emits Alt+arrow in modern terminals)
        // works without per-terminal config.
        .bind("Alt+b", "edit.move-word-back")
        .bind("Alt+Left", "edit.move-word-back")
        .bind("Alt+f", "edit.move-word-forward")
        .bind("Alt+Right", "edit.move-word-forward")
        // Kills
        .bind("Ctrl+k", "edit.kill-to-end")
        .bind("Ctrl+u", "edit.kill-to-start")
        .bind("Ctrl+w", "edit.kill-word-back")
        .bind("Ctrl+Backspace", "edit.kill-word-back")
        .bind("Alt+d", "edit.kill-word-forward")
        // Yank + transpose
        .bind("Ctrl+y", "edit.yank")
        .bind("Ctrl+t", "edit.transpose-chars")
        // Char deletes
        .bind("Backspace", "edit.delete-char-back")
        .bind("Ctrl+h", "edit.delete-char-back")
        .bind("Delete", "edit.delete-char-forward")
        .bind("Ctrl+d", "edit.delete-char-forward")
});

/// Result of feeding a key event to [`EditBuffer::handle_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOutcome {
    /// The buffer consumed the key — char insert, cursor move,
    /// kill/yank, etc. The host should treat the key as handled.
    Consumed,
    /// The buffer did not recognise the key. The host should fall
    /// through to its own keymap (modal-level, then tab-level).
    NotHandled,
}

/// Run `cmd` against `buf`. Returns `true` if the command name is one
/// of the `edit.*` set; `false` if it's something the buffer doesn't
/// own (caller may try a different scope).
pub fn dispatch_edit_command(buf: &mut EditBuffer, cmd: &Command) -> bool {
    match cmd.name {
        "edit.move-line-start" => buf.home(),
        "edit.move-line-end" => buf.end(),
        "edit.move-char-back" => buf.left(),
        "edit.move-char-forward" => buf.right(),
        "edit.move-word-back" => buf.move_word_back(),
        "edit.move-word-forward" => buf.move_word_forward(),
        "edit.kill-to-end" => buf.kill_to_end(),
        "edit.kill-to-start" => buf.kill_to_start(),
        "edit.kill-word-back" => buf.kill_word_back(),
        "edit.kill-word-forward" => buf.kill_word_forward(),
        "edit.yank" => buf.yank(),
        "edit.transpose-chars" => buf.transpose_chars(),
        "edit.delete-char-back" => buf.backspace(),
        "edit.delete-char-forward" => buf.delete(),
        _ => return false,
    }
    true
}

impl EditBuffer {
    /// Dispatch one key event against [`EDIT_KEYMAP`]. Returns
    /// [`EditOutcome::Consumed`] if the chord matched a binding *or*
    /// the key was a printable char inserted as-is. Returns
    /// [`EditOutcome::NotHandled`] for chords with `Ctrl`/`Alt`
    /// modifiers that don't match any binding (so e.g. an unbound
    /// `Ctrl+R` falls through to the host modal or tab).
    pub fn handle_event(&mut self, key: KeyEvent) -> EditOutcome {
        let chord = KeyChord::from_key_event(key);
        if let Some(cmd) = EDIT_KEYMAP.lookup(chord) {
            if dispatch_edit_command(self, cmd) {
                return EditOutcome::Consumed;
            }
        }
        // No binding matched. Fall back to "printable char insert" for
        // plain or shift-modified chars; everything else falls through.
        use crossterm::event::{KeyCode, KeyModifiers};
        if let KeyCode::Char(c) = key.code {
            let mods = key.modifiers;
            let printable =
                !mods.contains(KeyModifiers::CONTROL) && !mods.contains(KeyModifiers::ALT);
            if printable {
                self.insert(c);
                return EditOutcome::Consumed;
            }
        }
        EditOutcome::NotHandled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn ctrl_a_jumps_to_start() {
        let mut buf = EditBuffer::from("hello world");
        let out = buf.handle_event(key(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(out, EditOutcome::Consumed);
        assert_eq!(buf.cursor, 0);
    }

    #[test]
    fn ctrl_e_jumps_to_end() {
        let mut buf = EditBuffer {
            text: "hello".to_string(),
            cursor: 0,
            kill_ring: None,
        };
        let out = buf.handle_event(key(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(out, EditOutcome::Consumed);
        assert_eq!(buf.cursor, 5);
    }

    #[test]
    fn ctrl_k_kills_to_end_populates_ring() {
        let mut buf = EditBuffer {
            text: "hello world".to_string(),
            cursor: 5,
            kill_ring: None,
        };
        buf.handle_event(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(buf.text, "hello");
        assert_eq!(buf.kill_ring.as_deref(), Some(" world"));
    }

    #[test]
    fn ctrl_y_yanks_kill_ring() {
        let mut buf = EditBuffer {
            text: "hello".to_string(),
            cursor: 5,
            kill_ring: Some(" world".to_string()),
        };
        buf.handle_event(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(buf.text, "hello world");
    }

    #[test]
    fn alt_b_moves_word_back() {
        let mut buf = EditBuffer::from("foo bar baz");
        // cursor at 11 (end)
        buf.handle_event(key(KeyCode::Char('b'), KeyModifiers::ALT));
        assert_eq!(buf.cursor, 8, "land at start of `baz`");
    }

    #[test]
    fn alt_left_also_moves_word_back() {
        let mut buf = EditBuffer::from("foo bar baz");
        buf.handle_event(key(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(buf.cursor, 8);
    }

    #[test]
    fn alt_d_kills_word_forward() {
        let mut buf = EditBuffer {
            text: "foo bar".to_string(),
            cursor: 3,
            kill_ring: None,
        };
        buf.handle_event(key(KeyCode::Char('d'), KeyModifiers::ALT));
        assert_eq!(buf.text, "foo");
        assert_eq!(buf.kill_ring.as_deref(), Some(" bar"));
    }

    #[test]
    fn ctrl_t_transposes_chars() {
        let mut buf = EditBuffer::from("helol");
        // cursor at 5; transpose at end swaps last two.
        buf.handle_event(key(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert_eq!(buf.text, "hello");
    }

    #[test]
    fn printable_char_inserts() {
        let mut buf = EditBuffer::default();
        let out = buf.handle_event(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(out, EditOutcome::Consumed);
        assert_eq!(buf.text, "x");
    }

    #[test]
    fn shift_char_inserts_uppercase() {
        let mut buf = EditBuffer::default();
        let out = buf.handle_event(key(KeyCode::Char('X'), KeyModifiers::SHIFT));
        assert_eq!(out, EditOutcome::Consumed);
        assert_eq!(buf.text, "X");
    }

    #[test]
    fn unbound_ctrl_chord_falls_through() {
        // Ctrl+R isn't bound by EDIT_KEYMAP, so the buffer reports
        // NotHandled and leaves itself untouched.
        let mut buf = EditBuffer::from("hello");
        let out = buf.handle_event(key(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(out, EditOutcome::NotHandled);
        assert_eq!(buf.text, "hello");
    }

    #[test]
    fn backspace_routes_through_keymap() {
        let mut buf = EditBuffer::from("hello");
        buf.handle_event(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(buf.text, "hell");
    }

    #[test]
    fn home_and_end_keys_route_through_keymap() {
        let mut buf = EditBuffer::from("hello");
        buf.handle_event(key(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(buf.cursor, 0);
        buf.handle_event(key(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(buf.cursor, 5);
    }

    #[test]
    fn every_keymap_command_is_in_edit_commands() {
        // Every binding in EDIT_KEYMAP must have a matching CommandDef
        // — same invariant the central registry enforces for tabs and
        // modals.
        let names: std::collections::HashSet<&str> = EDIT_COMMANDS.iter().map(|d| d.name).collect();
        for (_chord, cmd) in EDIT_KEYMAP.iter() {
            assert!(
                names.contains(cmd.name),
                "binding for {:?} has no CommandDef",
                cmd.name
            );
        }
    }
}
