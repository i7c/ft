#![allow(dead_code)] // wired in §§4–9 (per-tab/modal commands, ft do, ft commands list)

//! Command primitives for the TUI's keymap-driven dispatch.
//!
//! Every TUI action has a stable `<context>.<verb>` name and a single
//! [`CommandDef`] declared at compile time. Tabs and modals expose
//! their command sets via `commands() -> &'static [CommandDef]` and
//! their key bindings via `keymap() -> KeyMap`; the App's event loop
//! resolves a chord → command → `dispatch_command(&Command, &mut ctx)`
//! through whichever scope is active (modal → tab → global).
//!
//! Construction of a [`Command`] value is cheap: a static name plus
//! sparse inline args. The [`CommandRegistry`] is a build-time union
//! of every tab/modal/global command set, used by `ft commands list`,
//! the `?` overlay, the docs generator, and the eventual user keymap
//! config.

use std::collections::HashMap;

/// A specific command invocation — a stable name and zero or more
/// inline string args. Cheap to clone; held by `KeyMap` entries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Command {
    pub name: &'static str,
    pub args: CommandArgs,
}

impl Command {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            args: CommandArgs::None,
        }
    }

    pub fn with_args(name: &'static str, args: Vec<(&'static str, String)>) -> Self {
        Self {
            name,
            args: if args.is_empty() {
                CommandArgs::None
            } else {
                CommandArgs::Inline(args)
            },
        }
    }

    /// Look up a single arg by key.
    pub fn arg(&self, key: &str) -> Option<&str> {
        match &self.args {
            CommandArgs::None => None,
            CommandArgs::Inline(v) => v
                .iter()
                .find_map(|(k, val)| (*k == key).then_some(val.as_str())),
        }
    }
}

/// Sparse args attached to a [`Command`] at bind time. `None` for the
/// common case; `Inline` for the small handful of bindings that pass a
/// flag (e.g. `("from_selection", "true")`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommandArgs {
    #[default]
    None,
    Inline(Vec<(&'static str, String)>),
}

/// Metadata for one command. Lives in `static` arrays declared next to
/// the tab/modal that owns the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub scope: CommandScope,
    pub group: &'static str,
    pub args_schema: &'static [ArgSpec],
    /// `true` if invoking this command opens a modal (gates `ft do`).
    pub opens_modal: bool,
    /// `true` if this command's chord should appear in the modal's
    /// status-bar hint (up to three primaries per modal).
    pub is_primary: bool,
}

/// Where a command lives — informs `?` overlay grouping and `ft commands
/// list --scope <…>` filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandScope {
    /// Available from every tab and from every modal that doesn't
    /// consume the chord (App-global bindings).
    Global,
    /// Owned by a specific tab; the inner name matches the tab's
    /// `Tab::title()` lowercased (e.g. `"graph"`, `"tasks"`).
    Tab(&'static str),
    /// Owned by a specific modal variant; the inner name matches the
    /// `Modal::name()` (e.g. `"create"`, `"section-move"`).
    Modal(&'static str),
}

impl CommandScope {
    pub fn as_str(&self) -> String {
        match self {
            CommandScope::Global => "global".into(),
            CommandScope::Tab(t) => format!("tab/{t}"),
            CommandScope::Modal(m) => format!("modal/{m}"),
        }
    }
}

/// One arg slot in a [`CommandDef::args_schema`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

/// What a `dispatch_command` returns. Cross-scope side effects (open a
/// modal, push a toast, suspend for editor, …) flow through
/// `ctx.pending_request` as `AppRequest` variants — same path tabs
/// already use — so this enum stays small.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Dispatcher recognised and executed the command (side effects
    /// went via `ctx.pending_request` if any).
    Handled,
    /// Dispatcher didn't recognise the command name — caller may try
    /// the next scope (modal → tab → global).
    NotHandled,
}

/// Build-time union of every command in the binary. Constructed once
/// at App start by composing each tab's and modal's static command
/// slices.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    by_name: HashMap<&'static str, &'static CommandDef>,
    ordered: Vec<&'static CommandDef>,
}

impl CommandRegistry {
    /// Build a registry from a list of static command slices.
    ///
    /// Panics if two `CommandDef`s share a `name`.
    pub fn from_slices(slices: &[&'static [CommandDef]]) -> Self {
        let mut by_name: HashMap<&'static str, &'static CommandDef> = HashMap::new();
        let mut ordered: Vec<&'static CommandDef> = Vec::new();
        for slice in slices {
            for def in *slice {
                if by_name.insert(def.name, def).is_some() {
                    panic!("duplicate command name in registry: {}", def.name);
                }
                ordered.push(def);
            }
        }
        Self { by_name, ordered }
    }

    /// Compose a registry from the live App state — every tab's
    /// commands plus every modal variant's commands plus the App-global
    /// command set. Called once at App startup.
    ///
    /// The `modal_slices` argument supplies the static `<MODAL>_COMMANDS`
    /// slice for each modal variant (none ship in this section; the
    /// per-modal conversions land later).
    pub fn build(
        tabs: &[Box<dyn crate::tui::tab::Tab>],
        modal_slices: &[&'static [CommandDef]],
        global: &'static [CommandDef],
    ) -> Self {
        let mut slices: Vec<&'static [CommandDef]> = Vec::with_capacity(tabs.len() + 2);
        for t in tabs {
            slices.push(t.commands());
        }
        slices.extend_from_slice(modal_slices);
        slices.push(global);
        Self::from_slices(&slices)
    }

    pub fn lookup(&self, name: &str) -> Option<&'static CommandDef> {
        self.by_name.get(name).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = &'static CommandDef> + '_ {
        self.ordered.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.ordered.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ordered.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static FOO_COMMANDS: &[CommandDef] = &[
        CommandDef {
            name: "foo.bar",
            description: "Run bar",
            scope: CommandScope::Tab("foo"),
            group: "Mutations",
            args_schema: &[],
            opens_modal: false,
            is_primary: false,
        },
        CommandDef {
            name: "foo.baz",
            description: "Run baz",
            scope: CommandScope::Tab("foo"),
            group: "Navigation",
            args_schema: &[ArgSpec {
                name: "id",
                description: "Target id",
                required: true,
            }],
            opens_modal: false,
            is_primary: true,
        },
    ];

    static GLOBAL_COMMANDS: &[CommandDef] = &[CommandDef {
        name: "app.quit",
        description: "Quit",
        scope: CommandScope::Global,
        group: "App",
        args_schema: &[],
        opens_modal: false,
        is_primary: false,
    }];

    #[test]
    fn command_args_inline_lookup() {
        let cmd = Command::with_args("foo.bar", vec![("k", "v".into())]);
        assert_eq!(cmd.arg("k"), Some("v"));
        assert_eq!(cmd.arg("missing"), None);
    }

    #[test]
    fn command_no_args() {
        let cmd = Command::new("foo.bar");
        assert_eq!(cmd.arg("anything"), None);
    }

    #[test]
    fn registry_from_slices_unions_and_lookups() {
        let reg = CommandRegistry::from_slices(&[FOO_COMMANDS, GLOBAL_COMMANDS]);
        assert_eq!(reg.len(), 3);
        assert_eq!(
            reg.lookup("foo.bar").map(|d| d.description),
            Some("Run bar")
        );
        assert_eq!(
            reg.lookup("app.quit").map(|d| d.scope),
            Some(CommandScope::Global)
        );
        assert!(reg.lookup("missing").is_none());
    }

    #[test]
    fn registry_iter_preserves_input_order() {
        let reg = CommandRegistry::from_slices(&[FOO_COMMANDS, GLOBAL_COMMANDS]);
        let names: Vec<&str> = reg.iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["foo.bar", "foo.baz", "app.quit"]);
    }

    #[test]
    #[should_panic(expected = "duplicate command name in registry: foo.bar")]
    fn registry_panics_on_duplicate_name() {
        let _ = CommandRegistry::from_slices(&[FOO_COMMANDS, FOO_COMMANDS]);
    }

    #[test]
    fn scope_as_str() {
        assert_eq!(CommandScope::Global.as_str(), "global");
        assert_eq!(CommandScope::Tab("graph").as_str(), "tab/graph");
        assert_eq!(CommandScope::Modal("create").as_str(), "modal/create");
    }
}
