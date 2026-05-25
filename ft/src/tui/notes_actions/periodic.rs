//! Tab-agnostic "open the periodic note for period X" flow.
//!
//! Used by both the Notes tab (`t` shortcut + `p` leader) and the
//! Graph tab (same bindings). Resolves the configured per-period
//! template, creates the dated file if missing, and queues an
//! [`AppRequest::OpenInEditor`] for the App to handle. Any failure
//! (missing config, template render error, write error) surfaces as
//! an error toast and skips the editor handoff.

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use ft_core::periodic::{create_or_get_periodic_path, Period};

use crate::tui::{
    notes_actions::queue_toast,
    tab::{AppRequest, TabCtx, ToastStyle},
};

/// Open today's periodic note for `period`, creating the dated file
/// from the configured template if it doesn't exist yet.
pub fn run_periodic_open(ctx: &TabCtx, period: Period) {
    let pn = &ctx.vault.config.config.periodic_notes;
    let cfg_opt = match period {
        Period::Daily => pn.daily.as_ref(),
        Period::Weekly => pn.weekly.as_ref(),
        Period::Monthly => pn.monthly.as_ref(),
        Period::Quarterly => pn.quarterly.as_ref(),
        Period::Yearly => pn.yearly.as_ref(),
    };
    let Some(cfg) = cfg_opt else {
        queue_toast(
            ctx,
            &format!("{} not configured", period.as_str()),
            ToastStyle::Error,
        );
        return;
    };

    // `today` is the App-wide ctx.today (FT_TODAY-aware via the test
    // clock); `now` pins to FT_TODAY at 00:00 when set, else local
    // wall clock — keeps periodic-note timestamps deterministic in
    // tests without diverging from real-time creation in production.
    let today = ctx.today;
    let now: NaiveDateTime = std::env::var("FT_TODAY")
        .ok()
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .map(|d| d.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()))
        .unwrap_or_else(|| Local::now().naive_local());

    let templates_dir = ctx.vault.templates_dir();
    let (abs_path, _created) = match create_or_get_periodic_path(
        &ctx.vault.path,
        &templates_dir,
        cfg,
        today,
        today,
        now,
    ) {
        Ok(pair) => pair,
        Err(e) => {
            queue_toast(ctx, &format!("{e}"), ToastStyle::Error);
            return;
        }
    };

    if let Ok(rel) = abs_path.strip_prefix(&ctx.vault.path) {
        ctx.recents.record_open(rel);
    }
    *ctx.pending_request.borrow_mut() = Some(AppRequest::OpenInEditor {
        path: abs_path,
        line: 1,
    });
}
