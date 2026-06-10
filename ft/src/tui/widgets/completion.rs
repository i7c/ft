// Most of this module is the public API that concrete providers (graph
// DSL, file paths, tags) will consume in follow-up changes. Until then
// only the test stub and the buffer's `handle_event` integration use
// the types. Drop this attribute when the first real provider lands.
#![allow(dead_code)]

//! Autocompletion scaffold for the shared [`EditBuffer`] widget.
//!
//! This module ships the *plumbing* — a [`CompletionProvider`] trait,
//! a [`CompletionItem`] value type, a [`CompletionPopup`] widget, and
//! a [`CompletionState`] bundle the buffer holds. No concrete providers
//! ship in `text-input-ux`; the graph DSL, file-path, and tag
//! providers are explicit follow-ups against this scaffold.
//!
//! ## Trigger model
//!
//! A provider declares a [`TriggerSet`] (e.g. "any printable", "only
//! `.` and `:`", "manual only"). The buffer consults the provider on
//! each input mutation; if the trigger matches, it calls
//! [`CompletionProvider::complete`] and opens / refreshes / closes the
//! popup based on the returned items.
//!
//! ## Char vs byte
//!
//! [`CompletionContext::cursor_byte`] is a *byte* offset (so providers
//! can index `text` directly). Items emit byte ranges in
//! [`CompletionItem::replace_span`]. The buffer converts between byte
//! and char positions at the integration boundary; provider code never
//! has to deal with the buffer's internal char-count cursor.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::tui::palette;

// ── Trait + value types ──────────────────────────────────────────────

/// Why the provider is being consulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionTrigger {
    /// User explicitly invoked completion (e.g. via `Tab` when no
    /// popup was open).
    Manual,
    /// An input mutation auto-triggered the popup.
    OnInput,
}

/// The set of conditions under which a provider wants the buffer to
/// query it. Built via the constructors (`printable`, `manual`,
/// `on_chars`) and combined as needed.
#[derive(Debug, Clone, Default)]
pub struct TriggerSet {
    /// Fire on every printable character insertion.
    pub on_any_printable: bool,
    /// Fire on these specific characters (e.g. `'.'`, `':'`).
    pub on_chars: Vec<char>,
    /// Fire only on a manual `Tab`-to-complete invocation. Mutually
    /// exclusive with the above two; if set, the buffer never
    /// auto-fires the provider.
    pub manual_only: bool,
}

impl TriggerSet {
    /// Fire on a manual `Tab` invocation only.
    pub fn manual() -> Self {
        Self {
            manual_only: true,
            ..Default::default()
        }
    }

    /// Fire on every printable character.
    pub fn printable() -> Self {
        Self {
            on_any_printable: true,
            ..Default::default()
        }
    }

    /// Fire only on these specific characters.
    pub fn on_chars(chars: impl IntoIterator<Item = char>) -> Self {
        Self {
            on_chars: chars.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Does this trigger set match `(trigger, ch)`?
    pub fn matches(&self, trigger: CompletionTrigger, ch: Option<char>) -> bool {
        match trigger {
            CompletionTrigger::Manual => true,
            CompletionTrigger::OnInput => {
                if self.manual_only {
                    return false;
                }
                if let Some(c) = ch {
                    if self.on_any_printable {
                        return true;
                    }
                    if self.on_chars.contains(&c) {
                        return true;
                    }
                }
                false
            }
        }
    }
}

/// Context passed to a provider on each query.
#[derive(Debug)]
pub struct CompletionContext<'a> {
    /// Current buffer text.
    pub text: &'a str,
    /// Byte offset of the cursor inside `text`.
    pub cursor_byte: usize,
    /// Why the provider is being consulted.
    pub trigger: CompletionTrigger,
}

/// Kind of a completion item — drives the popup glyph and (eventually)
/// per-kind styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Attribute,
    Operator,
    Value,
    Keyword,
    Path,
    Tag,
    Other,
}

impl CompletionKind {
    /// One-char glyph rendered in the popup's left gutter.
    pub fn glyph(self) -> char {
        match self {
            CompletionKind::Attribute => 'A',
            CompletionKind::Operator => 'O',
            CompletionKind::Value => 'V',
            CompletionKind::Keyword => 'K',
            CompletionKind::Path => 'P',
            CompletionKind::Tag => 'T',
            CompletionKind::Other => '·',
        }
    }
}

/// One completion candidate.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// Display string in the popup.
    pub label: String,
    /// Text inserted into the buffer when this item is accepted.
    pub insert_text: String,
    /// Byte range in the buffer's text to replace with `insert_text`.
    /// `None` means "replace the current word" (boundary:
    /// `[A-Za-z0-9_]`, determined by the buffer).
    pub replace_span: Option<Range<usize>>,
    /// Item kind (drives glyph + future styling).
    pub kind: CompletionKind,
    /// Optional one-line description rendered below the label.
    pub description: Option<String>,
}

/// The provider trait. Implementations are domain-specific (graph DSL
/// completion, file paths, tags, …) and live in follow-up changes.
///
/// `Debug` is a supertrait so [`crate::tui::widgets::EditBuffer`] can
/// keep its `#[derive(Debug)]`.
pub trait CompletionProvider: std::fmt::Debug {
    /// Compute completion items for the given context. The popup
    /// renders items in the returned order — providers control
    /// ranking. Returning an empty `Vec` closes the popup.
    fn complete(&mut self, ctx: &CompletionContext) -> Vec<CompletionItem>;

    /// Which input events should auto-fire `complete`. Manual
    /// invocations (e.g. `Tab` when no popup is open) always fire,
    /// regardless of this set.
    fn trigger_on(&self) -> TriggerSet;
}

// ── State bundle ─────────────────────────────────────────────────────

/// Pair of provider + currently-open popup, held inside the buffer.
/// `popup` is `Some` while the user can see and navigate completions;
/// `None` between sessions (provider attached but nothing on screen).
pub struct CompletionState {
    pub provider: Box<dyn CompletionProvider>,
    pub popup: Option<CompletionPopup>,
}

impl std::fmt::Debug for CompletionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletionState")
            .field("provider", &self.provider)
            .field("popup", &self.popup)
            .finish()
    }
}

impl CompletionState {
    pub fn new(provider: Box<dyn CompletionProvider>) -> Self {
        Self {
            provider,
            popup: None,
        }
    }
}

// ── Popup widget ─────────────────────────────────────────────────────

/// Max items visible in the popup before scrolling. Matches the
/// fuzzy picker's row budget.
pub const MAX_VISIBLE_ITEMS: usize = 8;

/// Result of feeding a key event to the popup.
#[derive(Debug, Clone)]
pub enum PopupOutcome {
    /// Popup absorbed the key, popup stays open.
    Consumed,
    /// User accepted the highlighted item (`Tab` / `Enter`). Caller
    /// applies `item.replace_span` + `item.insert_text` and closes the
    /// popup.
    Accepted(CompletionItem),
    /// User pressed `Esc`. Caller closes the popup; buffer is
    /// unchanged.
    Dismissed,
    /// Popup didn't recognise the key. Caller falls through to its
    /// host (the buffer's normal dispatch).
    NotHandled,
}

/// Vertical list of completion candidates rendered near the host
/// edit buffer's cursor.
#[derive(Debug, Clone)]
pub struct CompletionPopup {
    pub items: Vec<CompletionItem>,
    pub selected: usize,
    pub scroll_offset: usize,
}

impl CompletionPopup {
    pub fn new(items: Vec<CompletionItem>) -> Self {
        Self {
            items,
            selected: 0,
            scroll_offset: 0,
        }
    }

    /// Replace the visible items, preserving the selection where
    /// possible. Used when the provider returns refreshed candidates
    /// after a keystroke.
    pub fn refresh(&mut self, items: Vec<CompletionItem>) {
        self.items = items;
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        let max_top = self.items.len().saturating_sub(MAX_VISIBLE_ITEMS);
        if self.scroll_offset > max_top {
            self.scroll_offset = max_top;
        }
        self.fix_scroll();
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.items.len();
        self.fix_scroll();
    }

    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.items.len() - 1
        } else {
            self.selected - 1
        };
        self.fix_scroll();
    }

    fn fix_scroll(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + MAX_VISIBLE_ITEMS {
            self.scroll_offset = self.selected + 1 - MAX_VISIBLE_ITEMS;
        }
    }

    /// Dispatch one key event. See [`PopupOutcome`] for the contract.
    pub fn handle_event(&mut self, key: KeyEvent) -> PopupOutcome {
        if self.items.is_empty() {
            return PopupOutcome::NotHandled;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.select_prev();
                PopupOutcome::Consumed
            }
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.select_next();
                PopupOutcome::Consumed
            }
            (KeyCode::Tab, _) | (KeyCode::Enter, _) => {
                let item = self.items[self.selected].clone();
                PopupOutcome::Accepted(item)
            }
            (KeyCode::Esc, _) => PopupOutcome::Dismissed,
            _ => PopupOutcome::NotHandled,
        }
    }

    /// Compute the popup's render rect given the host area and the
    /// cursor's screen position. The popup renders below the cursor
    /// when the cursor is in the upper half of `host`; above
    /// otherwise. The rect is clamped to fit inside `host`.
    pub fn compute_area(&self, host: Rect, cursor: (u16, u16), max_label_width: u16) -> Rect {
        let (cx, cy) = cursor;
        let n_visible = self.items.len().min(MAX_VISIBLE_ITEMS) as u16;
        // +2 for the top/bottom border lines.
        let height = (n_visible + 2).min(host.height);
        // +2 for left/right padding inside the border (1 cell each).
        let width = (max_label_width + 4).min(host.width.saturating_sub(1));

        let render_above = cy.saturating_sub(host.y) > host.height / 2;
        let y = if render_above {
            // Place the popup so its bottom row sits just above the
            // cursor.
            cy.saturating_sub(height)
        } else {
            cy.saturating_add(1)
        };
        // Clamp horizontally.
        let mut x = cx;
        let host_right = host.x + host.width;
        if x + width > host_right {
            x = host_right.saturating_sub(width);
        }
        if x < host.x {
            x = host.x;
        }
        // Clamp vertically.
        let y = y
            .max(host.y)
            .min(host.y + host.height.saturating_sub(height));
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Render the popup overlay. Caller is responsible for picking the
    /// area via [`Self::compute_area`].
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.items.is_empty() || area.width < 4 || area.height < 3 {
            return;
        }
        let visible_end = (self.scroll_offset + MAX_VISIBLE_ITEMS).min(self.items.len());
        let visible = &self.items[self.scroll_offset..visible_end];
        let local_selected = self.selected.saturating_sub(self.scroll_offset);

        let rows: Vec<ListItem> = visible
            .iter()
            .map(|item| {
                let mut spans: Vec<Span<'_>> = Vec::new();
                spans.push(Span::styled(
                    format!("{} ", item.kind.glyph()),
                    Style::default().fg(palette::DIM),
                ));
                spans.push(Span::raw(item.label.clone()));
                let line = Line::from(spans);
                ListItem::new(line)
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette::PRIMARY));
        let list = List::new(rows).block(block).highlight_style(
            Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::REVERSED),
        );
        let mut state = ListState::default();
        state.select(Some(local_selected));
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(list, area, &mut state);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Fixture provider used by buffer + popup tests. Returns a fixed
    /// list of items every time `complete` is called.
    #[derive(Debug, Clone)]
    pub(crate) struct StubProvider {
        pub items: Vec<CompletionItem>,
        pub trigger: TriggerSet,
        pub calls: usize,
    }

    impl StubProvider {
        pub fn new(labels: &[&str]) -> Self {
            let items = labels
                .iter()
                .map(|l| CompletionItem {
                    label: l.to_string(),
                    insert_text: l.to_string(),
                    replace_span: None,
                    kind: CompletionKind::Keyword,
                    description: None,
                })
                .collect();
            Self {
                items,
                trigger: TriggerSet::printable(),
                calls: 0,
            }
        }
    }

    impl CompletionProvider for StubProvider {
        fn complete(&mut self, _ctx: &CompletionContext) -> Vec<CompletionItem> {
            self.calls += 1;
            self.items.clone()
        }

        fn trigger_on(&self) -> TriggerSet {
            self.trigger.clone()
        }
    }

    #[test]
    fn trigger_set_manual_only_blocks_on_input() {
        let ts = TriggerSet::manual();
        assert!(ts.matches(CompletionTrigger::Manual, None));
        assert!(!ts.matches(CompletionTrigger::OnInput, Some('a')));
    }

    #[test]
    fn trigger_set_printable_matches_any_char() {
        let ts = TriggerSet::printable();
        assert!(ts.matches(CompletionTrigger::OnInput, Some('a')));
        assert!(ts.matches(CompletionTrigger::OnInput, Some('.')));
        assert!(!ts.matches(CompletionTrigger::OnInput, None));
    }

    #[test]
    fn trigger_set_on_chars_filters() {
        let ts = TriggerSet::on_chars(['.', ':']);
        assert!(ts.matches(CompletionTrigger::OnInput, Some('.')));
        assert!(ts.matches(CompletionTrigger::OnInput, Some(':')));
        assert!(!ts.matches(CompletionTrigger::OnInput, Some('a')));
    }

    #[test]
    fn popup_select_next_wraps() {
        let items = vec![
            CompletionItem {
                label: "a".into(),
                insert_text: "a".into(),
                replace_span: None,
                kind: CompletionKind::Keyword,
                description: None,
            },
            CompletionItem {
                label: "b".into(),
                insert_text: "b".into(),
                replace_span: None,
                kind: CompletionKind::Keyword,
                description: None,
            },
        ];
        let mut p = CompletionPopup::new(items);
        assert_eq!(p.selected, 0);
        p.select_next();
        assert_eq!(p.selected, 1);
        p.select_next();
        assert_eq!(p.selected, 0, "wraps to first");
        p.select_prev();
        assert_eq!(p.selected, 1, "wraps to last");
    }

    #[test]
    fn popup_tab_returns_accepted_item() {
        let p_items = vec![CompletionItem {
            label: "node".into(),
            insert_text: "node".into(),
            replace_span: None,
            kind: CompletionKind::Keyword,
            description: None,
        }];
        let mut p = CompletionPopup::new(p_items);
        let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        match p.handle_event(key) {
            PopupOutcome::Accepted(item) => assert_eq!(item.label, "node"),
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[test]
    fn popup_esc_dismisses() {
        let items = vec![CompletionItem {
            label: "x".into(),
            insert_text: "x".into(),
            replace_span: None,
            kind: CompletionKind::Other,
            description: None,
        }];
        let mut p = CompletionPopup::new(items);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        match p.handle_event(key) {
            PopupOutcome::Dismissed => {}
            other => panic!("expected Dismissed, got {other:?}"),
        }
    }

    #[test]
    fn popup_printable_char_not_handled_falls_through() {
        let items = vec![CompletionItem {
            label: "x".into(),
            insert_text: "x".into(),
            replace_span: None,
            kind: CompletionKind::Other,
            description: None,
        }];
        let mut p = CompletionPopup::new(items);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(matches!(p.handle_event(key), PopupOutcome::NotHandled));
    }

    #[test]
    fn popup_refresh_clamps_selection() {
        let items = vec![
            CompletionItem {
                label: "a".into(),
                insert_text: "a".into(),
                replace_span: None,
                kind: CompletionKind::Keyword,
                description: None,
            },
            CompletionItem {
                label: "b".into(),
                insert_text: "b".into(),
                replace_span: None,
                kind: CompletionKind::Keyword,
                description: None,
            },
            CompletionItem {
                label: "c".into(),
                insert_text: "c".into(),
                replace_span: None,
                kind: CompletionKind::Keyword,
                description: None,
            },
        ];
        let mut p = CompletionPopup::new(items);
        p.selected = 2;
        let smaller = vec![CompletionItem {
            label: "z".into(),
            insert_text: "z".into(),
            replace_span: None,
            kind: CompletionKind::Keyword,
            description: None,
        }];
        p.refresh(smaller);
        assert_eq!(p.selected, 0, "out-of-range selection clamps");
    }

    #[test]
    fn popup_compute_area_renders_below_cursor_in_upper_half() {
        let p = CompletionPopup::new(vec![CompletionItem {
            label: "x".into(),
            insert_text: "x".into(),
            replace_span: None,
            kind: CompletionKind::Other,
            description: None,
        }]);
        let host = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let area = p.compute_area(host, (5, 5), 10);
        assert!(area.y >= 6, "popup should render below cursor row");
    }

    #[test]
    fn popup_compute_area_renders_above_cursor_in_lower_half() {
        let p = CompletionPopup::new(vec![CompletionItem {
            label: "x".into(),
            insert_text: "x".into(),
            replace_span: None,
            kind: CompletionKind::Other,
            description: None,
        }]);
        let host = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let area = p.compute_area(host, (5, 20), 10);
        assert!(
            area.y + area.height <= 20,
            "popup should render above cursor row"
        );
    }
}
