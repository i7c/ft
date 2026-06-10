//! Shared TUI widgets used across multiple tabs / views.

pub mod completion;
pub mod edit_buffer;
pub mod edit_keymap;
pub mod picker;

#[allow(unused_imports)] // public surface; consumers land in follow-ups
pub use completion::{
    CompletionContext, CompletionItem, CompletionKind, CompletionPopup, CompletionProvider,
    CompletionState, CompletionTrigger, PopupOutcome, TriggerSet,
};
pub use edit_buffer::EditBuffer;
pub use edit_keymap::{EDIT_COMMANDS, EDIT_KEYMAP};
// Re-exported eagerly so the picker is reachable as
// `crate::tui::widgets::FuzzyPicker` once plan-004 session 4 wires it in.
// `#[allow(unused_imports)]` keeps the re-exports legal until then.
#[allow(unused_imports)]
pub use picker::{
    FuzzyPicker, PathListPickerSource, PickerItem, PickerOutcome, PickerSource,
    VaultFilePickerSource,
};
