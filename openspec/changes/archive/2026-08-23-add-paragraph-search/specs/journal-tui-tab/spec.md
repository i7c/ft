# journal-tui-tab

## MODIFIED Requirements

### Requirement: Gather tab registration

The TUI SHALL register the top-level tab titled `Gather` as DEPRECATED: the tab SHALL be absent from the default tab lineup, and SHALL be included only when the `[tui] show_gather = true` config flag is set (the same opt-in pattern as `tasks_tab` and `timeblocks_tab`). The tab SHALL implement the `Tab` trait alongside the existing tabs whenever included. The Search tab SHALL occupy the default slot the Gather tab previously held.

#### Scenario: Tab hidden by default

- **WHEN** the TUI starts with no `show_gather` config
- **THEN** the tab strip does NOT list Gather, and it lists Search

#### Scenario: Tab restored by config

- **WHEN** the TUI starts with `[tui] show_gather = true`
- **THEN** the tab strip lists Gather alongside the other tabs

#### Scenario: Tab can receive focus

- **WHEN** the Gather tab is present and the user presses the digit key for its position
- **THEN** focus switches to the Gather tab and `on_focus` runs

#### Scenario: Deprecation surfaced when restored

- **WHEN** the Gather tab is focused with `show_gather = true`
- **THEN** the tab shows a deprecation note directing the user to the Search tab
