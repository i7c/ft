## REMOVED Requirements

### Requirement: Pulse tab registration
**Reason**: The Pulse tab and its `TabKind::Pulse` routing key are deleted;
the default lineup keeps Graph, Notes, and Search.
**Migration**: Use the Search tab (`3`); its input replaces the Pulse
handoff.

### Requirement: Default window on first focus
**Reason**: The tab no longer exists, so it has no first focus.
**Migration**: None.

### Requirement: Link list rendering
**Reason**: The Pulse row renderer is deleted with the tab.
**Migration**: Search results render as a feed of matching paragraphs.

### Requirement: Multi-select via space
**Reason**: There is no Pulse row list to select from; the Search tab owns
its own multi-select.
**Migration**: Use `Space` on the Search tab.

### Requirement: Window adjustment
**Reason**: `[` / `]` window widening/narrowing was pulse-only keybindings.
**Migration**: Narrow the period with a date predicate in the search query.

### Requirement: Handoff to Gather tab on enter
**Reason**: The source side of the handoff (the Pulse tab) is deleted;
`AppRequest::SearchWithQuery` survives for the Graph tab's `J` / `Ctrl+J`.
**Migration**: Type the query directly on the Search tab, or use `J` /
`Ctrl+J` on the Graph tab.

### Requirement: Help overlay via Tab::help_sections
**Reason**: `help_sections()` for the deleted tab goes with it.
**Migration**: None.

### Requirement: Background loading via mpsc
**Reason**: The per-focus `compute_pulse` call that motivated the worker is
gone.
**Migration**: None.
