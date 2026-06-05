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

/// White (unchanged).
pub const WHITE: Color = Color::White;

/// Black — modal / overlay backgrounds.
pub const BLACK: Color = Color::Black;

// ── Status colors (kept distinct from warm palette) ────────────────

/// Success toasts, positive feedback.
pub const SUCCESS: Color = Color::Green;

/// Error toasts, parse error text, invalid input feedback.
pub const ERROR: Color = Color::Red;
