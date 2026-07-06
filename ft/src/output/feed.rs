//! Shared renderer for paragraph-feed output — the reverse-chronological
//! date/title/separator/body form used by both `ft notes journal` and
//! `ft notes history`. Journal rows carry a non-empty `matched` list
//! (rendered as a `matched: …` badge when it has more than one target and
//! always serialized in JSON); history rows leave `matched` empty, which
//! suppresses the badge and omits the field from JSON. Both feeds carry
//! citation state (`cited_in`): a `cited:` / `cited*:` badge line in the
//! table form, always serialized (possibly empty) in JSON.

use std::path::Path;

use anyhow::{Context, Result};

/// One synth note citing a row's paragraph. `note` is the vault-relative
/// path; `stale` marks an edited-since-cited (line-overlap) match rather
/// than a byte-identical one.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CitedIn {
    pub note: String,
    pub stale: bool,
}

/// One rendered feed row, borrowed from the caller's entry type.
pub struct FeedRow<'a> {
    pub date: String,
    pub source_title: &'a str,
    pub source_path: &'a Path,
    pub section: &'a str,
    /// Display titles of the targets this row matched. Empty for history;
    /// one or more for journal (badge shown only when `len() > 1`).
    pub matched: Vec<String>,
    /// Synth notes citing this paragraph; empty when uncited.
    pub cited_in: Vec<CitedIn>,
}

/// Render the default (table) form. `empty_msg` is printed when there are
/// no rows (e.g. `"no journal entries"` / `"no history entries"`).
pub fn render_table(rows: &[FeedRow], use_color: bool, empty_msg: &str) {
    if rows.is_empty() {
        println!("{empty_msg}");
        return;
    }
    use owo_colors::OwoColorize;
    let mut first = true;
    for r in rows {
        if !first {
            println!();
        }
        first = false;
        let header = format!("{}  {}", r.date, r.source_title);
        if use_color {
            println!("{}", header.bold().cyan());
        } else {
            println!("{header}");
        }
        if r.matched.len() > 1 {
            let badge = format!("matched: {}", r.matched.join(", "));
            if use_color {
                println!("{}", badge.dimmed());
            } else {
                println!("{badge}");
            }
        }
        if let Some(badge) = cited_badge(&r.cited_in) {
            if use_color {
                println!("{}", badge.dimmed());
            } else {
                println!("{badge}");
            }
        }
        let sep_len = header.chars().count().clamp(20, 72);
        println!("{}", "─".repeat(sep_len));
        println!("{}", r.section);
    }
}

/// Badge text for a row's citation state: `cited: <first note stem>`
/// (`cited*:` when stale), with a `+N` overflow when more than one synth
/// note cites the paragraph. `None` when uncited.
pub fn cited_badge(cited_in: &[CitedIn]) -> Option<String> {
    let first = cited_in.first()?;
    let stem = Path::new(&first.note)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| first.note.clone());
    let label = if first.stale { "cited*" } else { "cited" };
    let overflow = match cited_in.len() {
        0 | 1 => String::new(),
        n => format!(" +{}", n - 1),
    };
    Some(format!("{label}: {stem}{overflow}"))
}

/// Render a JSON array. Each element has `date`, `source_title`,
/// `source_path`, `section`, `cited_in`, and — when non-empty — `matched`.
pub fn render_json(rows: &[FeedRow]) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Row<'a> {
        date: &'a str,
        source_title: &'a str,
        source_path: String,
        section: &'a str,
        #[serde(skip_serializing_if = "<[String]>::is_empty")]
        matched: &'a [String],
        cited_in: &'a [CitedIn],
    }
    let out: Vec<Row> = rows
        .iter()
        .map(|r| Row {
            date: &r.date,
            source_title: r.source_title,
            source_path: r.source_path.to_string_lossy().into_owned(),
            section: r.section,
            matched: &r.matched,
            cited_in: &r.cited_in,
        })
        .collect();
    let s = serde_json::to_string_pretty(&out).context("serialize feed json")?;
    println!("{s}");
    Ok(())
}
