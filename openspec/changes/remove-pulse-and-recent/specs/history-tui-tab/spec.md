## REMOVED Requirements

### Requirement: Recent tab registration
**Reason**: `ft/src/tui/tabs/recent.rs`, `RECENT_COMMANDS`/`RECENT_KEYMAP`,
the `tab/recent` keymap scope, and `TabKind::Recent` are deleted; the default
lineup renumbers to `1 Graph  2 Notes  3 Search`.
**Migration**: Use the Search tab for a recency-ordered feed
(`--sort date`).

### Requirement: Shared-snapshot participation
**Reason**: The tab no longer participates in the shared graph snapshot; the
`GraphSnapshot::citations` field it consumed is removed with it.
**Migration**: None.

### Requirement: Windowed feed rendering
**Reason**: The split-pane history renderer call is deleted; the shared
`render_feed_split` widget stays for the Search tab.
**Migration**: None.

### Requirement: Row selection for synth
**Reason**: The Recent tab's selection/send-to-synth flow is deleted; the
Search tab keeps the same shared `synth_send` machinery.
**Migration**: Select rows on the Search tab and press `s` / `S`.

### Requirement: Seeded section-move action (TUI only)
**Reason**: The Recent tab's `m` binding seeds a section-move modal; the
modal itself and its other entry points (Notes tab, Graph tab) are
unchanged.
**Migration**: Use the Notes or Graph tab's section-move action.

### Requirement: Citation badge on history rows
**Reason**: `CitationIndex` is deleted with the feed.
**Migration**: None.

### Requirement: history.toggle-uncited command
**Reason**: The `u` toggle and its command are deleted.
**Migration**: None.
