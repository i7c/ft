## 1. Config model

- [x] 1.1 Add `presets: HashMap<String, String>` field to `GraphCfg` in `ft-core/src/config.rs`
- [x] 1.2 Update `GraphCfg` defaults and serde attributes; verify TOML round-trip with a unit test

## 2. Built-in preset table

- [x] 2.1 Create `ft-core/src/graph/query/preset.rs` with `builtin(name) -> Option<&'static str>` and `builtin_names() -> &[&str]`
- [x] 2.2 Add built-in presets: `orphans`, `tree`, `links`, `dangling`
- [x] 2.3 Add unit test asserting every built-in round-trips through `graph::query::parse`
- [x] 2.4 Register the module in `ft-core/src/graph/query/mod.rs`

## 3. CLI `--preset` flag

- [x] 3.1 Add `--preset <name>` flag to `QueryArgs` in `ft/src/cmd/graph.rs`, mutually exclusive with `query`, `query_opt`, `from_file`
- [x] 3.2 Implement preset resolution in `read_query_source`: user config → built-in, exit code 2 on unknown name
- [x] 3.3 Add integration test for `ft graph query --preset orphans` against a fixture vault
- [x] 3.4 Add integration test for unknown preset name (exit code 2)
- [x] 3.5 Add integration test for user preset shadowing built-in

## 4. TUI preset quick-pick

- [x] 4.1 Add preset list (user-defined + built-in, user first) to `GraphTab` state when `Ctrl+N` opens a new view
- [x] 4.2 Render preset names as selectable menu overlay; on select, pre-fill query input with resolved DSL string
- [x] 4.3 On dismiss without selection, fall back to default query (current behavior)
- [x] 4.4 Add TUI snapshot test for preset quick-pick overlay