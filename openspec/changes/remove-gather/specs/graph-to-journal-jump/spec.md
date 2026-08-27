# graph-to-journal-jump

## REMOVED Requirements

### Requirement: Graph tab `J` keybinding opens the Gather tab
**Reason**: The Gather tab is deleted; the graph tab's `J` handoff is rewired to the Search tab.
**Migration**: `J` on a Note/Ghost row now opens the Search tab prefilled with the node as a `[[…]]` clause (see graph-to-search-jump).

### Requirement: App services the Journal-jump request via existing pending_request channel
**Reason**: The `AppRequest::GatherFor` variant and its servicing are deleted with the gather tab.
**Migration**: The App services `AppRequest::SearchWithQuery` for the graph handoff (see graph-to-search-jump).

### Requirement: Help overlay on graph tab lists `Shift+J`
**Reason**: The help row's target tab changed; the overlay text is updated to describe the Search jump.
**Migration**: The graph tab's `?` overlay lists `Shift+J` → "open Search for the selected note".
