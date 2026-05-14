//! Golden-snapshot test for the hand-ported templates under
//! `tests/fixtures/templates-ft/`.
//!
//! Each template is rendered against a representative title with a
//! fixed `today`/`now` (2026-05-13) and snapshotted. Snapshots land in
//! `tests/snapshots/`. To accept new output:
//!
//!   cargo insta accept --test templates

use std::path::PathBuf;

use chrono::{NaiveDate, NaiveTime};
use ft_core::notes::template::{render_path, TemplateContext};

/// All hand-ported templates and the representative title we render
/// them against. Sorted alphabetically — the test asserts the on-disk
/// fixture set matches.
const TEMPLATES: &[(&str, &str)] = &[
    ("daily", "2026-05-13"),
    ("father-watson", "Father Watson 2026-05-13"),
    ("goal-checkin", "Goal Checkin"),
    ("inbox", "Inbox 2026-05-13"),
    ("new", "Test Note"),
    ("progress-checkin", "Progress Checkin"),
    ("proj", "Test Project"),
    ("quarterly", "2026 Q2"),
    ("quick-add", "Quick"),
    ("restaurant", "Test Restaurant"),
    ("stry", "Test Story"),
    ("tags", "Tags"),
    ("tasks-in-this-path", "Tasks Here"),
    ("travel", "Travel 2026-05-13"),
    ("weeks", "2026 Week 19"),
    ("wrklg", "Test Worklog"),
];

fn fixtures_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("templates-ft");
    p
}

fn ctx_for(template_name: &str, title: &str) -> TemplateContext {
    let today = NaiveDate::from_ymd_opt(2026, 5, 13).unwrap();
    let now = today.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let mut ctx = TemplateContext::new(title, today, now);
    if template_name == "quick-add" {
        ctx.vars.insert("name".into(), title.into());
    }
    ctx
}

#[test]
fn fixture_set_matches_expectations() {
    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();

    let mut expected: Vec<String> = TEMPLATES.iter().map(|(n, _)| (*n).to_string()).collect();
    expected.sort();

    assert_eq!(
        on_disk, expected,
        "template fixture set drifted from TEMPLATES constant"
    );
}

#[test]
fn renders_all_fixture_templates() {
    for (name, title) in TEMPLATES {
        let path = fixtures_dir().join(format!("{name}.md"));
        let ctx = ctx_for(name, title);
        let rendered = render_path(&path, &ctx)
            .unwrap_or_else(|e| panic!("template `{name}` failed to render: {e}"));
        insta::assert_snapshot!(*name, rendered);
    }
}
