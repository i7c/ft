//! Per-tab help data model + global (App-level) bindings.
//!
//! Phase 1 of plan 022. Each [`Tab`](crate::tui::tab::Tab) implements
//! [`Tab::help_sections`](crate::tui::tab::Tab::help_sections) to return its
//! own keybinding inventory; the `?` overlay (rendered by
//! [`crate::tui::ui::render_help_overlay`]) composes the active tab's
//! sections with [`global_section`] below.
//!
//! `keys` is a pre-rendered display string ("Ctrl+E", "Shift+C", "g s") so
//! Phase 2 can swap in a parsed `KeySpec` (driving both dispatch and help
//! from the same map) without changing the renderer or the tab API.

/// One row in the help overlay: the key combo on the left, the description
/// on the right.
#[derive(Debug, Clone)]
pub struct HelpEntry {
    pub keys: String,
    pub desc: String,
}

impl HelpEntry {
    pub fn new(keys: impl Into<String>, desc: impl Into<String>) -> Self {
        Self {
            keys: keys.into(),
            desc: desc.into(),
        }
    }
}

/// Named group of entries (e.g. "Navigation", "Mutations", "Modals"). The
/// renderer prints the title in cyan above its rows so tabs with many
/// bindings stay readable.
#[derive(Debug, Clone)]
pub struct HelpSection {
    pub title: String,
    pub entries: Vec<HelpEntry>,
}

impl HelpSection {
    pub fn new(title: impl Into<String>, entries: &[(&str, &str)]) -> Self {
        Self {
            title: title.into(),
            entries: entries
                .iter()
                .map(|(k, d)| HelpEntry::new(*k, *d))
                .collect(),
        }
    }
}

/// App-level bindings rendered first in every `?` overlay regardless of
/// active tab. Mirrors what `App::handle_global_key` actually handles.
pub fn global_section() -> HelpSection {
    HelpSection::new(
        "Global",
        &[
            ("q / Ctrl+C", "quit"),
            ("?", "toggle this help"),
            ("Tab / Shift+Tab", "next / previous tab"),
            ("1 / 2 / 3 / 4", "jump to tab N"),
            ("g s", "git sync"),
            ("Esc", "close overlay"),
        ],
    )
}
