//! Age-band classification for tasks.

use chrono::NaiveDate;

/// A task's staleness bucket, derived from `created` relative to `today`.
///
/// Bands are absolute (fixed day thresholds), not cohort-relative: the same
/// task classifies into the same band regardless of what else is in the view.
/// This keeps shading stable under filtering, deterministic in snapshots,
/// and well-behaved when `created` is missing (the common case).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeBand {
    /// 0–3 days since `created`.
    Fresh,
    /// 4–10 days since `created`.
    Aging,
    /// 11–30 days since `created`.
    Stale,
    /// More than 30 days since `created`.
    Rotten,
    /// `created` is `None` — age unknown, renders no shade.
    Unknown,
}

/// Absolute age thresholds (in days, inclusive of `created`'s day).
const FRESH_MAX: i64 = 3;
const AGING_MAX: i64 = 10;
const STALE_MAX: i64 = 30;

/// Classify a task's age into a band.
///
/// `created == today` is `Fresh` (0 days). A `created` date in the future
/// (possible for imported data) also classifies as `Fresh`: the task isn't
/// stale, and clamping to the freshest band is the least surprising choice.
pub fn age_band(created: Option<NaiveDate>, today: NaiveDate) -> AgeBand {
    let Some(created) = created else {
        return AgeBand::Unknown;
    };
    let days = today.signed_duration_since(created).num_days();
    match days {
        d if d <= FRESH_MAX => AgeBand::Fresh,
        d if d <= AGING_MAX => AgeBand::Aging,
        d if d <= STALE_MAX => AgeBand::Stale,
        _ => AgeBand::Rotten,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn created_today_is_fresh() {
        let today = date(2026, 7, 12);
        assert_eq!(age_band(Some(today), today), AgeBand::Fresh);
    }

    #[test]
    fn boundaries() {
        let today = date(2026, 7, 31);
        // 0, 3 → Fresh; 4, 10 → Aging; 11, 30 → Stale; 31 → Rotten.
        assert_eq!(age_band(Some(today), today), AgeBand::Fresh); // 0d
        assert_eq!(age_band(Some(date(2026, 7, 28)), today), AgeBand::Fresh); // 3d
        assert_eq!(age_band(Some(date(2026, 7, 27)), today), AgeBand::Aging); // 4d
        assert_eq!(age_band(Some(date(2026, 7, 21)), today), AgeBand::Aging); // 10d
        assert_eq!(age_band(Some(date(2026, 7, 20)), today), AgeBand::Stale); // 11d
        assert_eq!(age_band(Some(date(2026, 7, 1)), today), AgeBand::Stale); // 30d
        assert_eq!(age_band(Some(date(2026, 6, 30)), today), AgeBand::Rotten); // 31d
    }

    #[test]
    fn well_past_stale_is_rotten() {
        let today = date(2026, 7, 12);
        assert_eq!(age_band(Some(date(2026, 5, 28)), today), AgeBand::Rotten); // 45d
    }

    #[test]
    fn none_is_unknown() {
        let today = date(2026, 7, 12);
        assert_eq!(age_band(None, today), AgeBand::Unknown);
    }

    #[test]
    fn future_created_clamps_to_fresh() {
        let today = date(2026, 7, 12);
        assert_eq!(age_band(Some(date(2026, 7, 14)), today), AgeBand::Fresh);
    }

    #[test]
    fn cohort_independent() {
        // Same task, same today → same band regardless of call context.
        let created = date(2026, 6, 22);
        let today = date(2026, 7, 12);
        assert_eq!(age_band(Some(created), today), AgeBand::Stale);
        // Re-derive in a "different view" — pure function, identical result.
        assert_eq!(age_band(Some(created), today), AgeBand::Stale);
    }
}
