//! Shared, tab-agnostic note-action state machines.
//!
//! These flows started life inside the Notes tab (`tabs/notes/mod.rs`)
//! where they were tightly coupled to `NotesState`. As more tabs (Graph,
//! eventually others) need the same UX — pick a folder, pick a template,
//! prompt for filename + vars, handle collisions — the underlying flow
//! belongs to neither tab in particular. Modules here own the state and
//! event-handling logic; each tab is responsible for owning a slot
//! (`Option<CreateState>`, or a variant in its own state enum) and for
//! invoking the flow's entry point.

pub mod create;
pub mod periodic;

use crate::tui::tab::{AppRequest, TabCtx, ToastStyle};

/// Helper used by the action flows in this crate to surface a status
/// toast via [`AppRequest::Toast`]. Centralised here so each flow
/// module doesn't reimplement the same three-line `RefCell` poke.
pub(crate) fn queue_toast(ctx: &TabCtx, text: &str, style: ToastStyle) {
    *ctx.pending_request.borrow_mut() = Some(AppRequest::Toast {
        text: text.to_string(),
        style,
    });
}
