//! App-global commands and keymap.
//!
//! Cross-cutting bindings (tab cycling, quit, help, git-leader) live
//! here as a single `CommandDef` slice + `KeyMap`. The App's event
//! loop resolves chords against this map last, after any active
//! modal's and the active tab's keymaps had a chance to consume them.

use std::sync::LazyLock;

use crate::tui::command::{ArgSpec, CommandDef, CommandScope};
use crate::tui::keymap::KeyMap;

/// Every command callable from the global scope.
pub static APP_COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "app.quit",
        description: "Quit the TUI",
        scope: CommandScope::Global,
        group: "App",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "app.next-tab",
        description: "Switch to the next tab",
        scope: CommandScope::Global,
        group: "Tabs",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "app.prev-tab",
        description: "Switch to the previous tab",
        scope: CommandScope::Global,
        group: "Tabs",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "app.switch-tab",
        description: "Switch to the tab at the given 0-based index",
        scope: CommandScope::Global,
        group: "Tabs",
        args_schema: &[ArgSpec {
            name: "index",
            description: "0-based tab index",
            required: true,
        }],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "app.help",
        description: "Open the keymap help overlay",
        scope: CommandScope::Global,
        group: "App",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
    CommandDef {
        name: "app.git-leader",
        description: "Enter the git-sync leader (then `s` to sync)",
        scope: CommandScope::Global,
        group: "App",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    },
];

/// Default App-global keymap. Mirrors the pre-migration
/// `App::handle_global_key` match arms one-for-one.
///
/// The single `g` chord enters the git-leader mode — the `g s` second
/// keystroke is handled by `Mode::GitLeader` rather than as a chord
/// sequence in the keymap (chord sequences are out of scope in v1;
/// leaders are transient modals).
pub static APP_KEYMAP: LazyLock<KeyMap> = LazyLock::new(|| {
    KeyMap::new()
        .bind("q", "app.quit")
        .bind("Ctrl+c", "app.quit")
        .bind("Tab", "app.next-tab")
        .bind("BackTab", "app.prev-tab")
        .bind_with_args("1", "app.switch-tab", &[("index", "0")])
        .bind_with_args("2", "app.switch-tab", &[("index", "1")])
        .bind_with_args("3", "app.switch-tab", &[("index", "2")])
        .bind_with_args("4", "app.switch-tab", &[("index", "3")])
        .bind_with_args("5", "app.switch-tab", &[("index", "4")])
        .bind_with_args("6", "app.switch-tab", &[("index", "5")])
        .bind_with_args("7", "app.switch-tab", &[("index", "6")])
        .bind_with_args("8", "app.switch-tab", &[("index", "7")])
        .bind_with_args("9", "app.switch-tab", &[("index", "8")])
        .bind("?", "app.help")
        .bind("g", "app.git-leader")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_keymap_resolves_basic_chords() {
        use crate::tui::keymap::KeyChord;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let km = &*APP_KEYMAP;
        assert_eq!(
            km.lookup(KeyChord::new(KeyCode::Char('q'), KeyModifiers::NONE))
                .map(|c| c.name),
            Some("app.quit"),
        );
        assert_eq!(
            km.lookup(KeyChord::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                .map(|c| c.name),
            Some("app.quit"),
        );
        assert_eq!(
            km.lookup(KeyChord::new(KeyCode::Tab, KeyModifiers::NONE))
                .map(|c| c.name),
            Some("app.next-tab"),
        );
        assert_eq!(
            km.lookup(KeyChord::new(KeyCode::BackTab, KeyModifiers::NONE))
                .map(|c| c.name),
            Some("app.prev-tab"),
        );
        // `?` arrives with Shift+/ on many terminals; normalization
        // strips SHIFT for non-alpha chars so a single `?` binding
        // catches both forms.
        let q_shift =
            KeyChord::from_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert_eq!(km.lookup(q_shift).map(|c| c.name), Some("app.help"));
    }

    #[test]
    fn app_keymap_switch_tab_carries_index_arg() {
        use crate::tui::keymap::KeyChord;
        use crossterm::event::{KeyCode, KeyModifiers};

        let km = &*APP_KEYMAP;
        let cmd = km
            .lookup(KeyChord::new(KeyCode::Char('2'), KeyModifiers::NONE))
            .expect("`2` should be bound");
        assert_eq!(cmd.name, "app.switch-tab");
        assert_eq!(cmd.arg("index"), Some("1"));
    }

    #[test]
    fn all_app_command_names_unique() {
        use std::collections::HashSet;
        let names: HashSet<&str> = APP_COMMANDS.iter().map(|d| d.name).collect();
        assert_eq!(names.len(), APP_COMMANDS.len());
    }

    #[test]
    fn all_app_keymap_bindings_resolve_to_registered_commands() {
        let names: std::collections::HashSet<&str> = APP_COMMANDS.iter().map(|d| d.name).collect();
        for (_, cmd) in APP_KEYMAP.iter() {
            assert!(
                names.contains(cmd.name),
                "binding references unknown command: {}",
                cmd.name
            );
        }
    }
}
