//! Central color palette for the TUI — warm orange/red/yellow tones.
//!
//! Every render site SHALL reference these constants instead of
//! inlining raw `Color` values. This makes future theme work a
//! one-file change.

use ratatui::style::Color;

// ── Accent colors ──────────────────────────────────────────────────

/// Primary accent: used for active borders, highlighted tabs, selected
/// row backgrounds, focused pane borders, active query bar prompts.
pub const PRIMARY: Color = Color::Rgb(255, 165, 0); // orange

/// Secondary accent: used for keybinding labels in help overlay, status
/// bar chord hints, picker highlights.
pub const SECONDARY: Color = Color::Rgb(255, 200, 50); // gold/yellow

/// Tertiary accent: used for modal names, overdue task indicators,
/// delete confirmations.
pub const TERTIARY: Color = Color::Rgb(255, 80, 80); // warm red

// ── Semantic colors ────────────────────────────────────────────────

/// Dim / inactive text: status bar labels, dividers, hints, inactive
/// borders, unfocused pane borders, sidebar border.
pub const DIM: Color = Color::Rgb(100, 85, 70); // warm gray

/// Status bar background — slightly warmer than the old cool dark.
pub const STATUS_BG: Color = Color::Rgb(30, 28, 26);

/// Entry-header bar background — a warm dark band drawn full-width
/// behind each Journal entry's title line so entries read as blocks.
/// Slightly lighter than [`STATUS_BG`] so the band separates from the
/// terminal background.
pub const ENTRY_HEADER_BG: Color = Color::Rgb(48, 42, 36);

/// White (unchanged).
pub const WHITE: Color = Color::White;

/// Black — modal / overlay backgrounds.
pub const BLACK: Color = Color::Black;

// ── Status colors (kept distinct from warm palette) ────────────────

/// Success toasts, positive feedback.
pub const SUCCESS: Color = Color::Green;

/// Error toasts, parse error text, invalid input feedback.
pub const ERROR: Color = Color::Red;

// ── Age-band greys (Tasks SearchView age badge) ────────────────────
// Four absolute shades for task aging: lightest = freshest, darkest =
// most stale. Applied as a span-scoped background on the age badge
// column only, so they never collide with the selected-row brown or
// the done/cancelled DIM modifier.

/// `Fresh` band (0–3 days): lightest grey.
pub const AGE_FRESH: Color = Color::Rgb(70, 66, 60);
/// `Aging` band (4–10 days).
pub const AGE_AGING: Color = Color::Rgb(90, 84, 76);
/// `Stale` band (11–30 days).
pub const AGE_STALE: Color = Color::Rgb(120, 112, 100);
/// `Rotten` band (>30 days): darkest grey.
pub const AGE_ROTTEN: Color = Color::Rgb(150, 140, 125);
