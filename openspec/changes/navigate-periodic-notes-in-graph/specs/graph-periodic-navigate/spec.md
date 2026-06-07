## ADDED Requirements

### Requirement: Periodic-note keybindings navigate within the graph tree

When the user presses a periodic-note keybinding on the Graph tab (`t` for today, or `p` followed by `d`/`w`/`m`/`q`/`y` for a specific period), and the target periodic note is reachable in the active view's subgraph under the current query's `select` and `expand` policy, the system SHALL expand the shortest root-to-target path in the tree and place the cursor on the target note.

#### Scenario: Today navigates to daily note when reachable
- **WHEN** the active view's query reaches the current daily note (e.g., via directory-contains from root)
- **AND** the user presses `t`
- **THEN** the tree expands ancestors along the shortest path to the daily note and the cursor lands on it

#### Scenario: Periodic leader opens and navigates on period key
- **WHEN** the active view's query reaches the target periodic note
- **AND** the user presses `p` then `w` (weekly leader chord)
- **THEN** the periodic-leader modal closes and the tree navigates to the weekly note, expanding ancestors

#### Scenario: Periodic leader closes on Esc without navigation
- **WHEN** the periodic-leader modal is open
- **AND** the user presses `Esc`
- **THEN** the modal closes and no tree navigation occurs

#### Scenario: Any non-period key closes the leader without navigation
- **WHEN** the periodic-leader modal is open
- **AND** the user presses a key other than `d`, `w`, `m`, `q`, `y`, or `Esc`
- **THEN** the modal closes and no tree navigation occurs

### Requirement: Unreachable periodic note queues an informational toast

When the target periodic note is not reachable in the active view's subgraph (not present in the result set of `select` + `expand`), the system SHALL queue an informational toast indicating the note is not in the current query's results and SHALL NOT navigate or open an editor.

#### Scenario: Daily note not in current query
- **WHEN** the active view's query does not reach today's daily note (e.g., the view shows a different subtree)
- **AND** the user presses `t`
- **THEN** an informational toast is displayed (e.g., "daily note not in current graph results")
- **AND** the tree state (selection, expansion, scroll) is unchanged

#### Scenario: Periodic note file does not exist on disk
- **WHEN** the periodic note file has not been created yet (no file on disk at the expected path)
- **THEN** it is not tracked in the graph
- **AND** pressing the periodic-note keybinding SHALL queue an informational toast without navigating

### Requirement: Shortest path is used for navigation

When the target periodic note is reachable via multiple paths from the active view's roots, the system SHALL select the shortest path (fewest edges). Tie-breaking is deterministic (BFS visit order, which depends on sorted expand outputs).

#### Scenario: Multi-parent target uses shortest path
- **WHEN** the periodic note is reachable via paths of length 2 and 4 from different roots
- **THEN** the path of length 2 is used for expansion

### Requirement: Target note is left collapsed after jump

After navigating to the periodic note, the target node SHALL be shown collapsed (its children are not expanded), matching the behaviour of `GraphJumpToNodes` / `jump_to_path`.

#### Scenario: Target stays collapsed
- **WHEN** the tree navigates to a periodic note that has children (e.g., linked notes, paragraphs, tasks)
- **THEN** the target row is selected and collapsed; only ancestors are expanded

### Requirement: Graph tab help overlay lists changed keybindings

The Graph tab's `?` help overlay SHALL describe the periodic-note keybindings as navigating within the graph, not opening in the editor.

#### Scenario: Help overlay reflects navigation behaviour
- **WHEN** the user presses `?` on the Graph tab
- **THEN** the help overlay mentions `t` and `p` + period keys in the Periodic notes section with descriptions indicating in-graph navigation
