## 1. BFS / candidate construction

- [ ] 1.1 Add `struct Candidate { id: NoteId, path: Vec<NoteId>, leaf: String, breadcrumb: String, kind_char: char }` inside `ft/src/tui/tabs/graph.rs`
- [ ] 1.2 Add `fn collect_search_candidates(graph: &Graph, query: &GraphQuery) -> Vec<Candidate>` that BFSes from `query.select(graph)` using `query.expand(graph, id)` as the successor function, with a `HashSet<NoteId>` visited set; pushes a `Candidate` for every visited node with the path captured in BFS order
- [ ] 1.3 Reuse the leaf-formatting logic from `TreeState::make_row` (factor into a `fn leaf_display(graph: &Graph, id: NoteId) -> (String, char)` shared by both call sites); breadcrumb is the leafs of `path[..len-1]` joined with `/`

## 2. Picker source + state

- [ ] 2.1 Add `GraphSearchPickerSource { candidates: Vec<Candidate>, matcher: nucleo_matcher::Matcher, buf: Vec<char> }` modelled on `PresetPickerSource` (graph.rs:61)
- [ ] 2.2 Implement `PickerSource for GraphSearchPickerSource`: `query` scores against `format!("{leaf} {breadcrumb}")`, returns top-N `PickerItem<Vec<NoteId>>` (data = the candidate's path); `initial_items` returns the first N candidates unfiltered for the open state
- [ ] 2.3 Each `PickerItem.label` is `format!("{leaf}  ·  {breadcrumb}")`; `match_indices` from nucleo are forwarded only when they fall within the leaf portion (clamp / drop indices > leaf char-count)
- [ ] 2.4 Add field `search_picker: Option<FuzzyPicker<GraphSearchPickerSource>>` to `GraphTab`; initialize `None` in `GraphTab::new`

## 3. Key dispatch

- [ ] 3.1 In `GraphTab::handle_event`, add a `if self.search_picker.is_some() { return self.handle_search_picker_key(k, ctx); }` branch *before* the `input_mode` capture and after the existing overlay branches (create / append / capture / rename / related / preset / move)
- [ ] 3.2 Add `fn handle_search_picker_key(&mut self, k: KeyEvent, ctx: &TabCtx) -> EventOutcome` that forwards the event to `FuzzyPicker::handle_event`; on `Selected(path)` calls `self.jump_to_path(path)` and clears the picker; on `Cancelled` clears the picker; on `StillOpen` / `NotHandled` consumes the event
- [ ] 3.3 Add the `(KeyCode::Char('f'), KeyModifiers::NONE)` arm in the tree-navigation keymap (after the empty-tree guard at graph.rs:1721): construct `GraphSearchPickerSource::new(graph, query)`, wrap in `FuzzyPicker::new`, store in `search_picker`

## 4. Jump implementation

- [ ] 4.1 Add `fn jump_to_path(&mut self, path: Vec<NoteId>)` on `GraphTab`: gets `graph`, looks up active view; if `path.len() > 1` calls `v.add_expansion_path(path[..path.len()-1].to_vec())`; sets `v.selected_path = Some(path)`; calls `v.restore_expansion(graph)`; calls `v.scroll_to_selection(vis)`
- [ ] 4.2 Verify `restore_expansion` lands `selected` on the leaf via `find_row_for_path`; if it doesn't (e.g. policy returned something weird) the fallback `selected = 0` is acceptable — no extra handling needed
- [ ] 4.3 The path stored in `selected_path` is the full `[root, …, target]` so the selection survives a subsequent graph refresh

## 5. Render

- [ ] 5.1 In `GraphTab::render`, after the `preset_picker` render block, add a render branch for `search_picker`: `centered_rect(60, 60, area)`, `Clear`, then `picker.render(frame, popup_area)`
- [ ] 5.2 Add a one-line footer in the popup: `"Enter: jump · Esc: cancel"`

## 6. Help

- [ ] 6.1 Add `("f", "search & jump to node in current view")` to the Navigation `HelpSection` in `help_sections()` (graph.rs:2305)

## 7. Tests

- [ ] 7.1 Unit test in graph.rs `mod search_tests`: `collect_search_candidates` over the `dirs` fixture with the canonical directory query produces a root candidate (path `[/]`) and at least one deeper candidate (`Areas/`, etc.), with shortest paths
- [ ] 7.2 Unit test: BFS terminates on a synthetic graph with a cycle (build a 2-node fixture programmatically, run BFS, assert ≤ 2 candidates and finite return)
- [ ] 7.3 Unit test: with no expand block in the query, `collect_search_candidates` returns exactly the roots, each with path of length 1
- [ ] 7.4 Unit test: leaf display matches `TreeState::make_row` output for all 5 node kinds (Note, Directory, Ghost, Task, Paragraph) — proves the factor-out in 1.3 didn't drift
- [ ] 7.5 Unit test: nucleo matcher ranks `bar` higher for a candidate whose haystack is `bar foo/bar` than for an unrelated candidate `quux foo/quux`
- [ ] 7.6 Unit test for `jump_to_path` against an `ExpandedView` seeded with the dirs fixture: after jumping to a depth-3 node, `selected` lands on that node, `tree.rows()[selected].depth == 3`, and ancestors are expanded
- [ ] 7.7 Integration test in `ft/src/tui/tests.rs`: open the Graph tab against the `dirs` fixture, press `f`, type a leaf name from a deep directory, press Enter, snapshot the resulting tree state — verifies end-to-end key path and `restore_expansion` integration
- [ ] 7.8 Integration test: `f` followed by `Esc` leaves the view unchanged (compare `expanded_paths` and `selected_path` before vs after)
- [ ] 7.9 Integration test: `f` on an empty tree (e.g. a freshly-created blank view) is a no-op
- [ ] 7.10 `TestBackend` snapshot test of the search picker overlay open over the dirs fixture (frames the popup chrome, label format, footer hint)

## 8. Build validation

- [ ] 8.1 `cargo build --release` — clean
- [ ] 8.2 `cargo test --workspace` — all tests pass
- [ ] 8.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [ ] 8.4 `cargo fmt --check` — clean
