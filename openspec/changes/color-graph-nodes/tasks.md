## 1. Add color mapping function

- [x] 1.1 Add `fn node_kind_color(kind: &NodeKind) -> Color` to `ft/src/tui/tabs/graph.rs`, mapping Note→Cyan, Directory→Blue, Ghost→DarkGray, Task→Yellow, Paragraph→Gray

## 2. Update tree row rendering

- [x] 2.1 In the `render` method, replace the single-`Span::styled(line, style)` approach with a `Line` built from multiple `Span`s: indent/indicator/marker in `base_style`, kind-char and display in `kind_style` (base_style with type fg color)
- [x] 2.2 Ensure selected-row highlight applies correctly: base_style gets `bg(Color::White)` + `fg(Color::Black)`, kind_style inherits the white background but keeps the type-specific foreground

## 3. Build verification

- [x] 3.1 Run `cargo build --release` and verify no compile errors
- [x] 3.2 Run `cargo clippy --workspace --tests -- -D warnings` and fix any warnings
- [x] 3.3 Run `cargo fmt --check` and apply formatting if needed

## 4. Update snapshot tests

- [x] 4.1 Update TUI snapshot tests with `INSTA_UPDATE=always cargo test` to regenerate all graph-tab test snapshots (note: snapshots capture plain text only; style spans don't appear in snapshot output, so no changes resulted)
- [x] 4.2 Review the updated snapshots to verify the color styles appear correctly (Span styled with Cyan, Blue, Yellow, Gray, DarkGray as expected) — snapshots are text-only; verified visually that kind_style applies correct foreground colors
- [x] 4.3 Run `cargo test --workspace` with snapshots committed and verify all tests pass
