## 1. Add color mapping function

- [ ] 1.1 Add `fn node_kind_color(kind: &NodeKind) -> Color` to `ft/src/tui/tabs/graph.rs`, mapping Note→Cyan, Directory→Blue, Ghost→DarkGray, Task→Yellow, Paragraph→Gray

## 2. Update tree row rendering

- [ ] 2.1 In the `render` method, replace the single-`Span::styled(line, style)` approach with a `Line` built from multiple `Span`s: indent/indicator/marker in `base_style`, kind-char and display in `kind_style` (base_style with type fg color)
- [ ] 2.2 Ensure selected-row highlight applies correctly: base_style gets `bg(Color::White)` + `fg(Color::Black)`, kind_style inherits the white background but keeps the type-specific foreground

## 3. Build verification

- [ ] 3.1 Run `cargo build --release` and verify no compile errors
- [ ] 3.2 Run `cargo clippy --workspace --tests -- -D warnings` and fix any warnings
- [ ] 3.3 Run `cargo fmt --check` and apply formatting if needed

## 4. Update snapshot tests

- [ ] 4.1 Update TUI snapshot tests with `INSTA_UPDATE=always cargo test` to regenerate all graph-tab test snapshots
- [ ] 4.2 Review the updated snapshots to verify the color styles appear correctly (Span styled with Cyan, Blue, Yellow, Gray, DarkGray as expected)
- [ ] 4.3 Run `cargo test --workspace` with snapshots committed and verify all tests pass
