## ADDED Requirements

### Requirement: z key re-roots on selected Note node

When the user presses `z` and the selected row is a Note node, the system SHALL rewrite the active view's query to root the tree on that note, preserving the existing expand block.

#### Scenario: Note node with expand block
- **WHEN** the query is `node where kind = Directory and path = ""; expand where edge.kind = directory-contains;` and the selected row is the Note node for `Areas/finance.md`
- **THEN** the query becomes `node where kind = Note and path = "Areas/finance.md"; expand where edge.kind = directory-contains;`

#### Scenario: Note node without expand block
- **WHEN** the query is `node where kind = Ghost;` and the selected row is the Note node for `Index.md`
- **THEN** the query becomes `node where kind = Note and path = "Index.md";`

#### Scenario: Tree rebuilds from new root
- **WHEN** the query is rewritten as above and the graph is available
- **THEN** the tree is rebuilt with only that Note node as the root row, collapsed (no children auto-expanded)

### Requirement: z key re-roots on selected Directory node

When the user presses `z` and the selected row is a Directory node, the system SHALL rewrite the query to root on that directory.

#### Scenario: Root directory node
- **WHEN** the selected row is the vault-root Directory node (path `""`)
- **THEN** the query becomes `node where kind = Directory and path = "";` followed by the preserved expand block

#### Scenario: Subdirectory node
- **WHEN** the selected row is the Directory node for `Areas/finance`
- **THEN** the query becomes `node where kind = Directory and path = "Areas/finance";` followed by the preserved expand block

### Requirement: z key is a no-op for Ghost and Task nodes

Ghost and Task nodes have no `path` attribute in the DSL. Pressing `z` on these node types SHALL have no effect.

#### Scenario: Ghost node
- **WHEN** the selected row is a Ghost node (e.g., `[[Phantom]]`)
- **THEN** the query is unchanged

#### Scenario: Task node
- **WHEN** the selected row is a Task node
- **THEN** the query is unchanged

### Requirement: Query text visible in input bar

After `z` rewrites the query, the updated query text SHALL be visible in the input bar and the input cursor SHALL be placed at the end of the new text.

#### Scenario: Input bar reflects new query
- **WHEN** `z` rewrites the query to `node where kind = Note and path = "foo.md"; expand where edge.kind = link;`
- **THEN** the input bar displays `> node where kind = Note and path = "foo.md"; expand where edge.kind = link;` and the cursor is at the end

### Requirement: Successive z presses re-root to the new selection

Pressing `z` again on a different node SHALL overwrite the query to root on that new node, not stack or toggle back.

#### Scenario: z pressed twice on different nodes
- **WHEN** the user presses `z` on node A (query rewrites to root on A), navigates to node B, and presses `z` again
- **THEN** the query is rewritten to root on node B, with no trace of node A in the node block

### Requirement: z is shown in the help overlay

The `z` keybinding SHALL be listed in the graph tab's help overlay (`?`).

#### Scenario: Help overlay lists z
- **WHEN** the user presses `?` on the graph tab
- **THEN** the help overlay includes an entry for `z` describing "root view on selected node"
