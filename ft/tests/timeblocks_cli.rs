//! Integration tests for `ft timeblocks`. Uses temp vaults under
//! `assert_fs` so we never touch the real fortytwo vault.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

/// Build a temp vault with `[periodic_notes.daily]` configured so the
/// CLI's default daily-note resolution works.
fn vault_with_daily() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str("[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\n")
        .unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "timeblocks"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-16")
        .args(&full)
        .assert()
}

fn day_path(vault: &std::path::Path, date: &str) -> std::path::PathBuf {
    vault.join(format!("journal/{date}.md"))
}

fn seed_day(vault: &std::path::Path, date: &str, body: &str) {
    let p = day_path(vault, date);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

// ── list ─────────────────────────────────────────────────────────────────────

#[test]
fn list_empty_section_exits_1_by_default() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "# Day\n");
    run(dir.path(), &["list"]).failure().code(1);
}

#[test]
fn list_empty_with_allow_empty_succeeds() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "# Day\n");
    run(dir.path(), &["list", "--allow-empty"]).success();
}

#[test]
fn list_table_format_renders_columns() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 standup @work\n- 10:00 - 10:30 review @work/code\n",
    );
    run(dir.path(), &["list"])
        .success()
        .stdout(predicate::str::contains("Start"))
        .stdout(predicate::str::contains("standup"))
        .stdout(predicate::str::contains("09:00"));
}

#[test]
fn list_json_format_emits_array() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 standup @work\n",
    );
    let out = run(dir.path(), &["list", "--format", "json"]).success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["start"], "09:00");
    assert_eq!(arr[0]["end"], "10:00");
    assert_eq!(arr[0]["minutes"], 60);
    assert_eq!(arr[0]["desc"], "standup @work");
    let tags = arr[0]["tags"].as_array().unwrap();
    assert_eq!(tags[0], serde_json::json!(["work"]));
}

#[test]
fn list_ndjson_emits_one_per_line() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n- 10:00 - 11:00 b\n",
    );
    let out = run(dir.path(), &["list", "--format", "ndjson"]).success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let _: serde_json::Value = serde_json::from_str(line).unwrap();
    }
}

#[test]
fn list_markdown_emits_source_lines() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 standup\n",
    );
    run(dir.path(), &["list", "--format", "markdown"])
        .success()
        .stdout(predicate::str::contains("- 09:00 - 10:00 standup"));
}

#[test]
fn list_filter_by_tag_prefix_matches_subtags() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a @work/meeting\n- 10:00 - 11:00 b @personal\n",
    );
    let out = run(dir.path(), &["list", "--tag", "work", "--format", "json"]).success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["desc"], "a @work/meeting");
}

#[test]
fn list_filter_by_tag_repeated_composes_or() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a @work\n- 10:00 - 11:00 b @personal\n- 11:00 - 12:00 c @home\n",
    );
    let out = run(
        dir.path(),
        &["list", "--tag", "work", "--tag", "home", "--format", "json"],
    )
    .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 2);
}

#[test]
fn list_explicit_file_overrides_daily_resolution() {
    let dir = vault_with_daily();
    let f = dir.child("custom.md");
    f.write_str("## Time Blocks\n- 09:00 - 10:00 custom\n")
        .unwrap();
    run(dir.path(), &["list", "--file", "custom.md"])
        .success()
        .stdout(predicate::str::contains("custom"));
}

#[test]
fn list_with_date_keyword_tomorrow() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-17",
        "## Time Blocks\n- 09:00 - 10:00 tomorrow_block\n",
    );
    run(dir.path(), &["list", "--date", "tomorrow"])
        .success()
        .stdout(predicate::str::contains("tomorrow_block"));
}

// ── add ──────────────────────────────────────────────────────────────────────

#[test]
fn add_positional_blockstring_creates_section() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "# Day\n\n");
    run(dir.path(), &["add", "09:00 - 10:00 standup @work"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("## Time Blocks"));
    assert!(body.contains("- 09:00 - 10:00 standup @work"));
}

#[test]
fn add_short_form_derives_30m_end() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "");
    run(dir.path(), &["add", "10:00 quick check"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 10:00 - 10:30 quick check"));
}

#[test]
fn add_flag_form_with_tag() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "");
    run(
        dir.path(),
        &[
            "add", "--start", "09:00", "--end", "10:00", "--desc", "standup", "--tag", "work",
        ],
    )
    .success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 standup @work"));
}

#[test]
fn add_positional_and_flags_are_mutually_exclusive() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "");
    run(
        dir.path(),
        &["add", "09:00 - 10:00 foo", "--start", "10:00"],
    )
    .failure();
}

#[test]
fn add_duplicate_is_rejected_without_force() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 same\n",
    );
    run(dir.path(), &["add", "09:00 - 10:00 same"])
        .failure()
        .stderr(predicate::str::contains("duplicate"));
}

#[test]
fn add_duplicate_with_force_appends_second_copy() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 same\n",
    );
    run(dir.path(), &["add", "--force", "09:00 - 10:00 same"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert_eq!(body.matches("- 09:00 - 10:00 same").count(), 2);
}

#[test]
fn add_dry_run_does_not_modify_file() {
    let dir = vault_with_daily();
    let original = "# Day\n\n## Time Blocks\n- 09:00 - 10:00 a\n";
    seed_day(dir.path(), "2026-05-16", original);
    run(dir.path(), &["add", "--dry-run", "10:00 - 11:00 b"])
        .success()
        .stdout(predicate::str::contains("(before)"))
        .stdout(predicate::str::contains("(after)"));
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert_eq!(body, original);
}

#[test]
fn add_inserts_in_sort_order() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 10:00 - 11:00 second\n",
    );
    run(dir.path(), &["add", "09:00 - 10:00 first"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    let first = body.find("first").unwrap();
    let second = body.find("second").unwrap();
    assert!(first < second);
}

#[test]
fn add_with_invalid_tag_rejects() {
    let dir = vault_with_daily();
    seed_day(dir.path(), "2026-05-16", "");
    run(
        dir.path(),
        &[
            "add", "--start", "09:00", "--end", "10:00", "--desc", "x", "--tag", "a/b/c/d",
        ],
    )
    .failure();
}

#[test]
fn add_creates_daily_note_directory_when_missing() {
    let dir = vault_with_daily();
    // Don't seed — the daily note shouldn't exist yet.
    run(dir.path(), &["add", "09:00 - 10:00 first"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 first"));
}

#[test]
fn add_renders_daily_template_when_creating_new_file() {
    // When the daily note doesn't exist yet, `ft timeblocks add` must
    // first render the `[periodic_notes.daily].template` so the new
    // file matches what `ft notes today` would produce — not a bare
    // `## Time Blocks`-only file. Regression test for the plan 015
    // "spell out daily-note template behaviour" clarification.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child("templates-ft/daily.md")
        .write_str("# {{ title }}\n\n## Notes\n\n## Time Blocks\n")
        .unwrap();
    dir.child(".ft/config.toml")
        .write_str(
            "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\ntemplate = \"daily\"\n",
        )
        .unwrap();

    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-16")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "timeblocks",
            "add",
            "09:00 - 10:00 first",
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    // Template's other sections must survive…
    assert!(
        body.contains("# 2026-05-16"),
        "template title missing: {body}"
    );
    assert!(body.contains("## Notes"), "template Notes missing: {body}");
    // …and the new block must be inserted under the existing Time Blocks heading.
    assert!(body.contains("## Time Blocks"));
    assert!(body.contains("- 09:00 - 10:00 first"));
    // Exactly one "## Time Blocks" heading — the section-replace should
    // splice into the template's existing heading, not append a second one.
    assert_eq!(
        body.matches("## Time Blocks").count(),
        1,
        "should not duplicate the heading: {body}"
    );
}

#[test]
fn add_with_explicit_file_does_not_render_daily_template() {
    // `--file <PATH>` opts out of daily-note resolution, so the template
    // must NOT be applied — we just append blocks to whatever file the
    // user named.
    let dir = vault_with_daily();
    let custom = dir.child("custom.md");
    custom.write_str("# Custom\n\nprose\n").unwrap();
    run(
        dir.path(),
        &["add", "--file", "custom.md", "09:00 - 10:00 first"],
    )
    .success();
    let body = std::fs::read_to_string(custom.path()).unwrap();
    assert!(body.starts_with("# Custom\n\nprose\n"));
    assert!(body.contains("- 09:00 - 10:00 first"));
}

// ── edit ─────────────────────────────────────────────────────────────────────

#[test]
fn edit_by_line_changes_start_and_end_absolute() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    run(
        dir.path(),
        &["edit", "1", "--start", "09:30", "--end", "11:00"],
    )
    .success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:30 - 11:00 a"));
}

#[test]
fn edit_by_time_relative_shift_minutes() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    run(dir.path(), &["edit", "09:00", "--end", "+15m"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:15 a"));
}

#[test]
fn edit_relative_negative_shift() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    run(dir.path(), &["edit", "1", "--start", "-15m"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 08:45 - 10:00 a"));
}

#[test]
fn edit_change_desc() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 old\n",
    );
    run(dir.path(), &["edit", "1", "--desc", "new desc"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 new desc"));
    assert!(!body.contains("old"));
}

#[test]
fn edit_add_tag_appends_to_desc() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 standup\n",
    );
    run(dir.path(), &["edit", "1", "--add-tag", "work/meeting"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 standup @work/meeting"));
}

#[test]
fn edit_remove_tag_strips_token() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 standup @work @later\n",
    );
    run(dir.path(), &["edit", "1", "--remove-tag", "work"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 standup @later"));
}

#[test]
fn edit_fuzzy_selector_ambiguous_lists_candidates() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 review pr\n- 10:00 - 11:00 review issues\n",
    );
    run(dir.path(), &["edit", "review", "--desc", "x"])
        .failure()
        .stderr(predicate::str::contains("ambiguous"))
        .stderr(predicate::str::contains("review pr"));
}

#[test]
fn edit_no_match_errors() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    run(dir.path(), &["edit", "99", "--desc", "x"]).failure();
}

#[test]
fn edit_dry_run_does_not_modify_file() {
    let dir = vault_with_daily();
    let original = "## Time Blocks\n- 09:00 - 10:00 a\n";
    seed_day(dir.path(), "2026-05-16", original);
    run(dir.path(), &["edit", "--dry-run", "1", "--desc", "new"])
        .success()
        .stdout(predicate::str::contains("(after)"));
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert_eq!(body, original);
}

#[test]
fn edit_rejects_end_at_or_before_start() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    run(dir.path(), &["edit", "1", "--end", "09:00"]).failure();
}

// ── delete ───────────────────────────────────────────────────────────────────

#[test]
fn delete_with_yes_removes_block() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n- 10:00 - 11:00 b\n",
    );
    run(dir.path(), &["delete", "1", "--yes"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(!body.contains("09:00 - 10:00 a"));
    assert!(body.contains("- 10:00 - 11:00 b"));
}

#[test]
fn delete_non_tty_without_yes_errors() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    // assert_cmd inherits stdin from the process which is not a tty
    // in the test runner, so this exercises the non-TTY branch.
    run(dir.path(), &["delete", "1"])
        .failure()
        .stderr(predicate::str::contains("--yes"));
}

#[test]
fn delete_dry_run_does_not_modify_file() {
    let dir = vault_with_daily();
    let original = "## Time Blocks\n- 09:00 - 10:00 a\n";
    seed_day(dir.path(), "2026-05-16", original);
    run(dir.path(), &["delete", "1", "--dry-run"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert_eq!(body, original);
}

#[test]
fn delete_no_match_errors() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n",
    );
    run(dir.path(), &["delete", "99", "--yes"]).failure();
}

#[test]
fn delete_by_time_selector() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-05-16",
        "## Time Blocks\n- 09:00 - 10:00 a\n- 10:00 - 11:00 b\n",
    );
    run(dir.path(), &["delete", "10:00", "--yes"]).success();
    let body = std::fs::read_to_string(day_path(dir.path(), "2026-05-16")).unwrap();
    assert!(body.contains("- 09:00 - 10:00 a"));
    assert!(!body.contains("- 10:00 - 11:00 b"));
}

// ── missing daily-config remedy hint ─────────────────────────────────────────

#[test]
fn missing_periodic_notes_daily_config_emits_hint() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    // No `[periodic_notes.daily]` block.
    dir.child(".ft/config.toml").write_str("").unwrap();
    Command::cargo_bin("ft")
        .unwrap()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "timeblocks",
            "list",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--file"));
}
