//! Integration tests for `ft timeblocks spent`. Uses a multi-day temp
//! vault so the period-walk + per-tag aggregation is exercised end-to-end.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault_with_daily() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str("[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\n")
        .unwrap();
    dir
}

fn run(vault: &std::path::Path, today: &str, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "timeblocks", "spent"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", today)
        .args(&full)
        .assert()
}

fn seed(vault: &std::path::Path, date: &str, body: &str) {
    let p = vault.join(format!("journal/{date}.md"));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// Sun-Sat of the 2026-05-13 (Wednesday) week is 2026-05-11..2026-05-17.
fn seed_a_week(vault: &std::path::Path) {
    seed(
        vault,
        "2026-05-11",
        "## Time Blocks\n- 09:00 - 10:00 mon-work @work\n- 10:00 - 10:30 mon-pause @break\n",
    );
    seed(
        vault,
        "2026-05-12",
        "## Time Blocks\n- 09:00 - 11:00 tue-work @work/meeting\n",
    );
    seed(
        vault,
        "2026-05-13",
        "## Time Blocks\n- 14:00 - 15:30 wed-personal @personal\n",
    );
    // 05-14, 05-15 skipped (no file)
    seed(
        vault,
        "2026-05-17",
        "## Time Blocks\n- 09:00 - 09:30 sun-x @work\n",
    );
}

// ── today (default preset) ──────────────────────────────────────────────────

#[test]
fn today_default_with_no_data_exits_1() {
    let dir = vault_with_daily();
    run(dir.path(), "2026-05-13", &[]).failure().code(1);
}

#[test]
fn today_default_aggregates_single_day() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-05-13",
        "## Time Blocks\n- 09:00 - 10:00 a @work\n- 10:00 - 10:30 b @break\n",
    );
    run(dir.path(), "2026-05-13", &["--format", "json"])
        .success()
        .stdout(predicate::str::contains("\"from\": \"2026-05-13\""))
        .stdout(predicate::str::contains("\"to\": \"2026-05-13\""))
        .stdout(predicate::str::contains("\"total_minutes\": 60"));
}

// ── this-week ───────────────────────────────────────────────────────────────

#[test]
fn this_week_aggregates_across_days_skipping_missing_files() {
    let dir = vault_with_daily();
    seed_a_week(dir.path());
    let out = run(dir.path(), "2026-05-13", &["this-week", "--format", "json"]).success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["from"], "2026-05-11");
    assert_eq!(v["to"], "2026-05-17");
    // total: 60 (mon-work) + 120 (tue-work) + 90 (wed-personal) + 30 (sun-x) = 300
    // @break excluded.
    assert_eq!(v["total_minutes"], 300);
    let tags = v["tags"].as_array().unwrap();
    // Top-level @work and @personal and @break should all appear in the
    // tree (break is shown but excluded from total).
    let names: Vec<&str> = tags.iter().map(|t| t["tag"].as_str().unwrap()).collect();
    assert!(names.contains(&"work"));
    assert!(names.contains(&"personal"));
    assert!(names.contains(&"break"));
}

// ── last-week ───────────────────────────────────────────────────────────────

#[test]
fn last_week_returns_previous_mon_sun_range() {
    let dir = vault_with_daily();
    // Seed a block in the week before our reference.
    seed(
        dir.path(),
        "2026-05-04",
        "## Time Blocks\n- 09:00 - 10:00 prev @work\n",
    );
    // Reference day inside the 05-11..05-17 week.
    let out = run(dir.path(), "2026-05-13", &["last-week", "--format", "json"]).success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["from"], "2026-05-04");
    assert_eq!(v["to"], "2026-05-10");
    assert_eq!(v["total_minutes"], 60);
}

// ── this-month / this-year ──────────────────────────────────────────────────

#[test]
fn this_month_walks_calendar_month() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-05-01",
        "## Time Blocks\n- 09:00 - 10:00 first @work\n",
    );
    seed(
        dir.path(),
        "2026-05-31",
        "## Time Blocks\n- 09:00 - 10:00 last @work\n",
    );
    // Reference day mid-month.
    let out = run(
        dir.path(),
        "2026-05-15",
        &["this-month", "--format", "json"],
    )
    .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["from"], "2026-05-01");
    assert_eq!(v["to"], "2026-05-31");
    assert_eq!(v["total_minutes"], 120);
}

#[test]
fn this_year_walks_calendar_year() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-01-01",
        "## Time Blocks\n- 09:00 - 10:00 a @work\n",
    );
    seed(
        dir.path(),
        "2026-12-31",
        "## Time Blocks\n- 09:00 - 10:00 b @work\n",
    );
    let out = run(dir.path(), "2026-06-15", &["this-year", "--format", "json"]).success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["from"], "2026-01-01");
    assert_eq!(v["to"], "2026-12-31");
    assert_eq!(v["total_minutes"], 120);
}

// ── --from / --to ───────────────────────────────────────────────────────────

#[test]
fn explicit_from_to_range() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-05-12",
        "## Time Blocks\n- 09:00 - 10:00 a @work\n",
    );
    seed(
        dir.path(),
        "2026-05-14",
        "## Time Blocks\n- 09:00 - 10:00 b @work\n",
    );
    let out = run(
        dir.path(),
        "2026-05-13",
        &[
            "--from",
            "2026-05-12",
            "--to",
            "2026-05-14",
            "--format",
            "json",
        ],
    )
    .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["from"], "2026-05-12");
    assert_eq!(v["to"], "2026-05-14");
    assert_eq!(v["total_minutes"], 120);
}

#[test]
fn from_without_to_errors() {
    let dir = vault_with_daily();
    run(dir.path(), "2026-05-13", &["--from", "2026-05-12"]).failure();
}

#[test]
fn from_and_period_are_mutually_exclusive() {
    let dir = vault_with_daily();
    run(
        dir.path(),
        "2026-05-13",
        &["this-week", "--from", "2026-05-12", "--to", "2026-05-14"],
    )
    .failure();
}

#[test]
fn from_after_to_errors() {
    let dir = vault_with_daily();
    run(
        dir.path(),
        "2026-05-13",
        &["--from", "2026-05-14", "--to", "2026-05-12"],
    )
    .failure();
}

// ── tag filter ──────────────────────────────────────────────────────────────

#[test]
fn tag_filter_restricts_aggregation() {
    let dir = vault_with_daily();
    seed_a_week(dir.path());
    let out = run(
        dir.path(),
        "2026-05-13",
        &["this-week", "--format", "json", "--tag", "personal"],
    )
    .success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["total_minutes"], 90);
    let tags = v["tags"].as_array().unwrap();
    let names: Vec<&str> = tags.iter().map(|t| t["tag"].as_str().unwrap()).collect();
    assert!(names.contains(&"personal"));
    assert!(!names.contains(&"work"));
}

// ── output formats ──────────────────────────────────────────────────────────

#[test]
fn text_format_prints_total_row() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-05-13",
        "## Time Blocks\n- 09:00 - 10:30 a @work\n",
    );
    run(dir.path(), "2026-05-13", &[])
        .success()
        .stdout(predicate::str::contains("01:30"))
        .stdout(predicate::str::contains("total"));
}

#[test]
fn text_format_renders_hierarchical_tag_rows() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-05-13",
        "## Time Blocks\n- 09:00 - 10:00 a @work/meeting\n- 10:00 - 11:00 b @work/code\n",
    );
    run(dir.path(), "2026-05-13", &[])
        .success()
        .stdout(predicate::str::contains("work"))
        .stdout(predicate::str::contains("meeting"))
        .stdout(predicate::str::contains("code"));
}

#[test]
fn json_format_shape() {
    let dir = vault_with_daily();
    seed(
        dir.path(),
        "2026-05-13",
        "## Time Blocks\n- 09:00 - 10:00 a @work/meeting\n",
    );
    let out = run(dir.path(), "2026-05-13", &["--format", "json"]).success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(v["from"].is_string());
    assert!(v["to"].is_string());
    assert!(v["total_minutes"].is_number());
    let tags = v["tags"].as_array().unwrap();
    assert_eq!(tags[0]["tag"], "work");
    assert_eq!(tags[0]["minutes"], 60);
    let children = tags[0]["children"].as_array().unwrap();
    assert_eq!(children[0]["tag"], "meeting");
    assert_eq!(children[0]["minutes"], 60);
}

// ── allow-empty ─────────────────────────────────────────────────────────────

#[test]
fn empty_with_allow_empty_succeeds() {
    let dir = vault_with_daily();
    run(dir.path(), "2026-05-13", &["--allow-empty"]).success();
}

// ── missing daily config ────────────────────────────────────────────────────

#[test]
fn missing_periodic_notes_daily_errors_with_hint() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml").write_str("").unwrap();
    Command::cargo_bin("ft")
        .unwrap()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "timeblocks",
            "spent",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("periodic_notes.daily"));
}
