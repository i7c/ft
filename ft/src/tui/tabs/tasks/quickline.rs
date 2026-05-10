//! Quickline parser used by the TUI's `c` (new task) flow.
//!
//! Walks a single line of typed input left-to-right and peels off any
//! whitespace-delimited token that matches a known prefix (`due:`,
//! `sched:`, `start:`, `pri:`, `in:`, `id:`, `#tag`, `every WORDS`). What
//! doesn't match becomes part of the task description, preserving the
//! user's original word order. Errors accumulate so the preview can show
//! the first one without halting the rest of the parse — the user types
//! progressively and we want the live preview to keep updating.
//!
//! The parser is pure: same input + `today` → same `QuicklineParse`. It
//! does not touch disk or the vault. Path safety for `in:` (must be
//! inside the vault root) is enforced by the caller because the parser
//! has no vault to check against.
//!
//! The UI surface that drives this parser lands in plan 004 session 2.
//! Module-level `allow(dead_code)` keeps the parser warning-free until
//! then; all behavior is exercised by unit tests already.

#![allow(dead_code)]

use std::path::PathBuf;

use chrono::NaiveDate;
use ft_core::task::Priority;

/// The result of parsing one quickline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuicklineParse {
    /// Free text after stripping prefix tokens. `#tag` words stay in the
    /// description as well — the parser is additive for tags, matching
    /// the CLI's `--tag` flag behavior.
    pub description: String,
    pub due: Option<NaiveDate>,
    pub scheduled: Option<NaiveDate>,
    pub start: Option<NaiveDate>,
    pub priority: Option<Priority>,
    pub tags: Vec<String>,
    pub recurrence: Option<String>,
    pub id: Option<String>,
    /// Vault-relative or absolute path supplied via `in:PATH`. The caller
    /// resolves and validates against the vault root.
    pub target: Option<PathBuf>,
    /// Human-readable errors encountered while parsing. Non-empty `errors`
    /// blocks the UI from accepting the input on Enter.
    pub errors: Vec<String>,
}

/// Parse a quickline into a structured form. `today` resolves DSL date
/// keywords (`today`, `tomorrow`, `+3d`, …).
pub fn parse_quickline(input: &str, today: NaiveDate) -> QuicklineParse {
    let mut out = QuicklineParse::default();
    let mut desc_parts: Vec<String> = Vec::new();

    // Tokenize on whitespace; the consumed/escaped-word distinction
    // happens inside the walk below.
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];

        // Backslash escape: `\due:foo` becomes literal `due:foo` in the
        // description. Same for `\every` etc. The escape only matters
        // for tokens that *would otherwise* be recognized; we always
        // strip the leading `\` to keep behavior predictable.
        if let Some(stripped) = tok.strip_prefix('\\') {
            desc_parts.push(stripped.to_string());
            i += 1;
            continue;
        }

        // `every WORDS...` — consume until end of input or the next
        // recognized prefix token. The whole span becomes the
        // recurrence string with `every` retained as a prefix.
        if tok.eq_ignore_ascii_case("every") {
            let mut j = i + 1;
            while j < tokens.len() && !is_prefix_token(tokens[j]) {
                j += 1;
            }
            // Preserve the original word case of `every` if the user
            // typed something other than lowercase — but in practice we
            // emit "every <words>" matching the Obsidian Tasks plugin's
            // canonical form.
            let body = tokens[i + 1..j].join(" ");
            let value = if body.is_empty() {
                "every".to_string()
            } else {
                format!("every {body}")
            };
            if out.recurrence.is_some() {
                out.errors.push("recurrence specified twice".into());
            } else {
                out.recurrence = Some(value);
            }
            i = j;
            continue;
        }

        if let Some(value) = strip_prefix_ci(tok, "due:") {
            assign_date(value, today, "due", &mut out.due, &mut out.errors);
        } else if let Some(value) = strip_prefix_ci(tok, "sched:") {
            assign_date(value, today, "sched", &mut out.scheduled, &mut out.errors);
        } else if let Some(value) = strip_prefix_ci(tok, "start:") {
            assign_date(value, today, "start", &mut out.start, &mut out.errors);
        } else if let Some(value) = strip_prefix_ci(tok, "pri:") {
            match parse_priority(value) {
                Ok(p) => out.priority = p,
                Err(e) => out.errors.push(e),
            }
        } else if let Some(value) = strip_prefix_ci(tok, "in:") {
            if value.is_empty() {
                out.errors.push("`in:` requires a path".into());
            } else {
                out.target = Some(PathBuf::from(value));
            }
        } else if let Some(value) = strip_prefix_ci(tok, "id:") {
            if value.is_empty() {
                out.errors.push("`id:` requires a value".into());
            } else {
                out.id = Some(value.to_string());
            }
        } else if let Some(tag) = as_tag(tok) {
            // Tags both populate `tags` and stay in the description so
            // the final markdown line preserves the inline `#tag` (per
            // the convention used by `ops::build_task`).
            if !out.tags.iter().any(|t| t == tag) {
                out.tags.push(tag.to_string());
            }
            desc_parts.push(tok.to_string());
        } else {
            desc_parts.push(tok.to_string());
        }
        i += 1;
    }

    out.description = desc_parts.join(" ");
    out
}

/// Strip `prefix` (case-insensitive on the prefix itself) and return the
/// remainder. The value after the prefix is returned verbatim. Uses
/// `str::get` so a multi-byte char straddling the prefix boundary
/// returns `None` instead of panicking (think Cyrillic tokens in the
/// description that happen to be the same byte length as `due:`).
fn strip_prefix_ci<'a>(tok: &'a str, prefix: &str) -> Option<&'a str> {
    let head = tok.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        Some(&tok[prefix.len()..])
    } else {
        None
    }
}

/// Identify whether `tok` is a prefix-bearing token that would terminate
/// `every`'s greedy WORDS consumption.
fn is_prefix_token(tok: &str) -> bool {
    if tok.starts_with('\\') {
        return false;
    }
    if as_tag(tok).is_some() {
        return true;
    }
    for prefix in ["due:", "sched:", "start:", "pri:", "in:", "id:"] {
        if strip_prefix_ci(tok, prefix).is_some() {
            return true;
        }
    }
    tok.eq_ignore_ascii_case("every")
}

/// Parse `value` as a date, accumulating an error if invalid and
/// rejecting double-assignments so users can spot bad inputs quickly.
fn assign_date(
    value: &str,
    today: NaiveDate,
    field: &str,
    slot: &mut Option<NaiveDate>,
    errors: &mut Vec<String>,
) {
    if value.is_empty() {
        errors.push(format!("`{field}:` requires a value"));
        return;
    }
    if slot.is_some() {
        errors.push(format!("`{field}:` specified twice"));
        return;
    }
    match ft_core::dates::parse(value, today) {
        Ok(d) => *slot = Some(d),
        Err(e) => errors.push(format!("{field}: {e}")),
    }
}

/// Same priority vocabulary as the edit popup; lifted here so quickline
/// behavior matches what the user sees in the form.
fn parse_priority(s: &str) -> Result<Option<Priority>, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "lowest" => Ok(Some(Priority::Lowest)),
        "low" => Ok(Some(Priority::Low)),
        "medium" | "med" => Ok(Some(Priority::Medium)),
        "high" => Ok(Some(Priority::High)),
        "highest" => Ok(Some(Priority::Highest)),
        other => Err(format!(
            "priority `{other}` not recognized (try none / low / med / high)"
        )),
    }
}

/// Return the tag body (without the `#`) if `tok` is a tag token. A tag
/// token starts with `#` and is followed by one or more chars from
/// `[A-Za-z0-9_/-]` — the same vocabulary the markdown parser uses to
/// `extract_tags` from descriptions.
fn as_tag(tok: &str) -> Option<&str> {
    let rest = tok.strip_prefix('#')?;
    if rest.is_empty() {
        return None;
    }
    if rest
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
    {
        Some(rest)
    } else {
        None
    }
}

// ── tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 10).unwrap()
    }

    #[test]
    fn plain_text_is_description_only() {
        let p = parse_quickline("buy milk and eggs", today());
        assert_eq!(p.description, "buy milk and eggs");
        assert!(p.errors.is_empty());
        assert!(p.due.is_none());
        assert!(p.tags.is_empty());
    }

    #[test]
    fn empty_input_yields_empty_parse() {
        let p = parse_quickline("", today());
        assert_eq!(p, QuicklineParse::default());
    }

    // ── token-by-token coverage ────────────────────────────────────

    #[test]
    fn due_token_iso_date() {
        let p = parse_quickline("pay rent due:2026-06-01", today());
        assert_eq!(p.description, "pay rent");
        assert_eq!(p.due, Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap()));
        assert!(p.errors.is_empty());
    }

    #[test]
    fn due_token_relative_date() {
        let p = parse_quickline("write report due:+3d", today());
        assert_eq!(p.due, Some(NaiveDate::from_ymd_opt(2026, 5, 13).unwrap()));
        assert!(p.errors.is_empty());
    }

    #[test]
    fn due_token_keyword_today() {
        let p = parse_quickline("call dentist due:today", today());
        assert_eq!(p.due, Some(today()));
    }

    #[test]
    fn sched_token() {
        let p = parse_quickline("draft proposal sched:tomorrow", today());
        assert_eq!(
            p.scheduled,
            Some(NaiveDate::from_ymd_opt(2026, 5, 11).unwrap())
        );
    }

    #[test]
    fn start_token() {
        let p = parse_quickline("kickoff start:2026-05-15", today());
        assert_eq!(p.start, Some(NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()));
    }

    #[test]
    fn pri_token_each_level() {
        for (val, expected) in [
            ("none", None),
            ("low", Some(Priority::Low)),
            ("med", Some(Priority::Medium)),
            ("medium", Some(Priority::Medium)),
            ("high", Some(Priority::High)),
            ("highest", Some(Priority::Highest)),
            ("lowest", Some(Priority::Lowest)),
        ] {
            let line = format!("foo pri:{val}");
            let p = parse_quickline(&line, today());
            assert_eq!(p.priority, expected, "pri:{val}");
            assert!(p.errors.is_empty(), "pri:{val} should not error");
        }
    }

    #[test]
    fn in_token_relative_path() {
        let p = parse_quickline("note in:Inbox.md", today());
        assert_eq!(p.target, Some(PathBuf::from("Inbox.md")));
    }

    #[test]
    fn in_token_nested_path() {
        let p = parse_quickline("note in:Daily/2026-05-10.md", today());
        assert_eq!(p.target, Some(PathBuf::from("Daily/2026-05-10.md")));
    }

    #[test]
    fn id_token() {
        let p = parse_quickline("note id:abc123", today());
        assert_eq!(p.id, Some("abc123".to_string()));
    }

    #[test]
    fn tag_token_adds_to_list_and_stays_in_description() {
        let p = parse_quickline("buy gift for mom #birthday #urgent", today());
        assert_eq!(p.tags, vec!["birthday", "urgent"]);
        assert!(
            p.description.contains("#birthday"),
            "tags must stay inline in description: {}",
            p.description
        );
        assert!(p.description.contains("#urgent"));
        assert_eq!(p.description, "buy gift for mom #birthday #urgent");
    }

    #[test]
    fn duplicate_tags_collapse() {
        let p = parse_quickline("foo #work #work", today());
        assert_eq!(p.tags, vec!["work"]);
    }

    #[test]
    fn tag_with_slash_and_dash_accepted() {
        let p = parse_quickline("ship #area/projects #follow-up", today());
        assert_eq!(p.tags, vec!["area/projects", "follow-up"]);
    }

    #[test]
    fn bare_hash_is_description() {
        // A solitary `#` is not a tag — it's literal description text.
        let p = parse_quickline("update the # symbol", today());
        assert!(p.tags.is_empty());
        assert!(p.description.contains("# symbol"));
    }

    // ── `every` recurrence ─────────────────────────────────────────

    #[test]
    fn every_consumes_until_end() {
        let p = parse_quickline("water plants every week", today());
        assert_eq!(p.recurrence, Some("every week".into()));
        assert_eq!(p.description, "water plants");
    }

    #[test]
    fn every_consumes_multiple_words() {
        let p = parse_quickline("status report every other monday", today());
        assert_eq!(p.recurrence, Some("every other monday".into()));
        assert_eq!(p.description, "status report");
    }

    #[test]
    fn every_stops_at_next_prefix_token() {
        let p = parse_quickline("standup every weekday due:tomorrow", today());
        assert_eq!(p.recurrence, Some("every weekday".into()));
        assert_eq!(p.due, Some(NaiveDate::from_ymd_opt(2026, 5, 11).unwrap()));
        assert_eq!(p.description, "standup");
    }

    #[test]
    fn every_at_end_of_line_alone_is_recurrence_keyword_only() {
        let p = parse_quickline("foo every", today());
        assert_eq!(p.recurrence, Some("every".into()));
        assert_eq!(p.description, "foo");
    }

    // ── escapes ─────────────────────────────────────────────────────

    #[test]
    fn backslash_escapes_due_prefix() {
        let p = parse_quickline(r"send mail \due:tomorrow", today());
        assert!(p.due.is_none());
        assert_eq!(p.description, "send mail due:tomorrow");
    }

    #[test]
    fn backslash_escapes_every_keyword() {
        let p = parse_quickline(r"the \every-day grind", today());
        assert!(p.recurrence.is_none());
        assert!(p.description.contains("every-day grind"));
    }

    // ── ordering / mixed ───────────────────────────────────────────

    #[test]
    fn description_word_order_preserved() {
        let p = parse_quickline(
            "send the budget review pri:high due:friday #finance to Alice",
            today(),
        );
        assert_eq!(p.description, "send the budget review #finance to Alice");
        assert_eq!(p.priority, Some(Priority::High));
        assert!(p.due.is_some());
        assert_eq!(p.tags, vec!["finance"]);
    }

    #[test]
    fn unicode_in_description() {
        let p = parse_quickline("обзор кода due:tomorrow #дом", today());
        assert!(p.description.contains("обзор кода"));
        // Russian word chars are alphanumeric in Unicode — the tag token
        // should be accepted.
        assert_eq!(p.tags, vec!["дом"]);
        assert!(p.due.is_some());
    }

    #[test]
    fn only_tokens_no_description() {
        let p = parse_quickline("due:tomorrow pri:high #urgent", today());
        assert_eq!(p.description, "#urgent");
        assert_eq!(p.priority, Some(Priority::High));
        assert!(p.due.is_some());
    }

    #[test]
    fn case_insensitive_prefix() {
        let p = parse_quickline("foo DUE:tomorrow Pri:HIGH", today());
        assert_eq!(p.priority, Some(Priority::High));
        assert!(p.due.is_some());
    }

    // ── error accumulation ─────────────────────────────────────────

    #[test]
    fn invalid_date_emits_error() {
        let p = parse_quickline("foo due:not-a-date", today());
        assert!(!p.errors.is_empty());
        assert!(p.errors[0].contains("due"));
        assert!(p.due.is_none());
    }

    #[test]
    fn invalid_priority_emits_error() {
        let p = parse_quickline("foo pri:bogus", today());
        assert!(!p.errors.is_empty());
        assert!(p.errors[0].contains("priority"));
        assert!(p.priority.is_none());
    }

    #[test]
    fn empty_value_after_prefix_errors() {
        let p = parse_quickline("foo due: bar", today());
        assert!(p.errors.iter().any(|e| e.contains("requires a value")));
    }

    #[test]
    fn double_due_is_an_error() {
        let p = parse_quickline("foo due:tomorrow due:friday", today());
        assert!(p.errors.iter().any(|e| e.contains("specified twice")));
        // First wins.
        assert_eq!(p.due, Some(NaiveDate::from_ymd_opt(2026, 5, 11).unwrap()));
    }

    #[test]
    fn unknown_colon_token_stays_in_description() {
        // `re:invoice` isn't one of our prefixes — it must end up
        // verbatim in the description.
        let p = parse_quickline("email Bob re:invoice", today());
        assert!(p.errors.is_empty());
        assert!(p.description.contains("re:invoice"));
    }
}
