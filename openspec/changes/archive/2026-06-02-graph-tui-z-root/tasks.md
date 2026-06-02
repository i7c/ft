## 1. Core implementation

- [x] 1.1 Add `z` key handler in `handle_event` (tree-navigation section, alongside `j`/`k`/`Enter`): guard on graph and tree non-empty, get selected row's NoteId and node kind, rewrite query for Note/Directory nodes, no-op for Ghost/Task
- [x] 1.2 Implement the query-rewriting logic: extract the current parsed query's expand block via canonical serialization + `"; expand"` split, construct new query `node where kind = <K> and path = "<P>"; <expand>`, escape double-quote and backslash in paths
- [x] 1.3 After rewriting, set `query_text` and `input_cursor = query_text.len()`, call `apply_query(graph)` to re-parse and rebuild the tree
- [x] 1.4 Add `z` entry to `help_sections()` in the Navigation group

## 2. Tests

- [x] 2.1 Test: `z` on a Note node rewrites query with correct kind and path, preserving expand block
- [x] 2.2 Test: `z` on a Directory node rewrites query correctly
- [x] 2.3 Test: `z` on root Directory node (path `""`) rewrites query correctly
- [x] 2.4 Test: `z` on a Ghost node is a no-op (query unchanged)
- [x] 2.5 Test: `z` on a Task node is a no-op (query unchanged)
- [x] 2.6 Test: `z` preserves expand block when present
- [x] 2.7 Test: `z` produces correct query when no expand block exists
- [x] 2.8 Test: snapshot of TUI frame after `z` on a Note node

## 3. Build validation

- [x] 3.1 Run `cargo test --workspace` — all tests pass
- [x] 3.2 Run `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 3.3 Run `cargo fmt --check` — clean
