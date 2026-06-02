## ADDED Requirements

### Requirement: Every TUI action has a stable `<context>.<verb>` command name

Every action that today is bound to a key, triggered by a chord, or otherwise dispatched in the TUI SHALL have a corresponding `Command` with a stable `<context>.<verb>` name. The name SHALL persist across releases and SHALL appear in the central `CommandRegistry`.

#### Scenario: Command names are dotted and stable
- **WHEN** the registry is built and `ft commands list` is invoked
- **THEN** every printed command name matches the pattern `[a-z][a-z0-9-]*\.[a-z][a-z0-9-]*` and the set of names is stable across rebuilds

#### Scenario: Every keymap binding resolves to a registered command
- **WHEN** the `KeyMap` lookup returns `Some(Command)` for any chord in any tab or modal keymap
- **THEN** that command's `name` is present in the central `CommandRegistry` with a `CommandDef`

### Requirement: `CommandDef` metadata covers description, parameter schema, modal opening, scope, and group

Each `CommandDef` SHALL include: `name`, `description`, `args_schema`, `opens_modal`, `scope`, and `group`. These fields SHALL be used by `ft commands list`, the `?` overlay, and the docs generator.

#### Scenario: Description shown by `?` overlay
- **WHEN** the user opens `?` and the active context has a keymap binding `c → graph.create-note`
- **THEN** the help row reads `c   graph.create-note   <description from CommandDef>`

#### Scenario: `opens_modal` gates `ft do`
- **WHEN** `CommandDef.opens_modal = true` and the user runs `ft do <command>`
- **THEN** `ft do` exits with a non-zero code and a clear message instructing the user to use `ft tui` for interactive flows

#### Scenario: `group` defines section ordering in `?` overlay
- **WHEN** the `?` overlay renders a tab's keymap
- **THEN** rows are grouped by `CommandDef.group` (e.g., "Navigation", "Mutations", "Modals", "View") and groups appear in a stable order

### Requirement: Mixed-granularity command model

Top-level commands SHALL be flow entry points (`graph.create-note`) and atomic actions (`tasks.complete-current`). Inside a modal, generic verbs (`modal.confirm`, `modal.cancel`, `modal.next`, `modal.prev`, `modal.toggle`, `modal.delete`, `modal.up`, `modal.down`) SHALL be interpreted by the active modal's `dispatch_command`. Step-specific commands inside a flow SHALL be the exception, not the default.

#### Scenario: Modal generic verb dispatches to active modal
- **WHEN** the section-move heading-multiselect modal is active and the user presses `Space`
- **THEN** the chord resolves to `modal.toggle`, the active modal's `dispatch_command("modal.toggle", _)` runs, and the focused heading is toggled

#### Scenario: Flow entry opens a modal
- **WHEN** no modal is active and the user presses `c` on the Graph tab
- **THEN** the chord resolves to `graph.create-note`, the Graph tab's `dispatch_command` returns `CommandOutcome::OpenModal(ActiveModal::Create(...))`, and the create modal becomes active

### Requirement: Tabs and modals expose `commands`, `keymap`, and `dispatch_command`

The `Tab` trait and the `Modal` trait SHALL each gain three methods: `commands() -> &'static [CommandDef]`, `keymap() -> KeyMap`, and `dispatch_command(&mut self, cmd: &Command, ctx: &mut TabCtx) -> CommandOutcome`. The default `handle_event` for both SHALL be implemented as `lookup chord → dispatch_command`.

#### Scenario: Tab handle_event delegates to keymap + dispatch_command
- **WHEN** a tab receives a key event with no modal active
- **THEN** the tab calls `self.keymap().lookup(chord)`, dispatches the returned command via `self.dispatch_command`, and returns the resulting `EventOutcome`

#### Scenario: Modal handle_event delegates to keymap + dispatch_command
- **WHEN** an active modal receives a key event
- **THEN** the modal calls `self.keymap().lookup(chord)` and dispatches via `self.dispatch_command`; a chord with no binding falls through (`ModalOutcome::NotHandled`) unless the modal owns raw input (e.g., text fields)

### Requirement: App-global keymap owns cross-cutting bindings

The App SHALL hold a `global_keymap()` that defines bindings reachable from any tab and any modal that returns `NotHandled`. The global keymap SHALL include at least: tab cycling, quit, help.

#### Scenario: Global binding fires from any tab
- **WHEN** the user presses `Tab` while on any tab and with any modal active (that returns `NotHandled` for `Tab`)
- **THEN** the global keymap dispatches `app.next-tab` and the active tab changes
