//! Integration tests for `@`-sigil query interpolation: `@today`,
//! `@daily` (with offsets), dynamic presets, and the error paths.
//!
//! Uses temp vaults under `assert_fs` so nothing touches a real vault.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

/// Temp vault with `[periodic_notes.daily]` configured
/// (`journal/%Y-%m-%d.md`).
fn vault_with_daily() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str("[periodic_notes.daily]\npath = \"journal/%Y\"\nformat = \"%Y-%m-%d\"\n")
        .unwrap();
    dir
}

/// Temp vault with daily config **and** a `[tasks.presets]` entry that
/// uses `@daily`.
fn vault_with_daily_preset() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(
            "[periodic_notes.daily]\npath = \"journal/%Y\"\nformat = \"%Y-%m-%d\"\n\
             [tasks.presets]\n\
             daily-open = \"path includes @daily and status in {Open, InProgress}\"\n",
        )
        .unwrap();
    dir
}

fn seed_day(vault: &std::path::Path, date: &str, body: &str) {
    let (year, _m, _d) = parse_iso(date);
    let p = vault.join(format!("journal/{year}/{date}.md"));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn parse_iso(s: &str) -> (&str, &str, &str) {
    let mut it = s.split('-');
    (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
}

fn run_tasks(vault: &std::path::Path, today: &str, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks", "list"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", today)
        .args(&full)
        .assert()
}

fn json_tasks(vault: &std::path::Path, today: &str, args: &[&str]) -> serde_json::Value {
    let mut full: Vec<&str> = vec!["--format", "json", "--no-color"];
    full.extend(args);
    let assert = run_tasks(vault, today, &full).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    serde_json::from_str(&stdout).expect("ft tasks list --format json must produce valid JSON")
}

fn descriptions(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| {
            t.get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

// ── @daily / @today expansion ───────────────────────────────────────────────

#[test]
fn at_daily_matches_tasks_in_todays_daily_note() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-07-29",
        "# 2026-07-29\n- [ ] review PR\n- [x] done yesterday\n",
    );
    seed_day(
        dir.path(),
        "2026-07-30",
        "# 2026-07-30\n- [ ] tomorrow task\n",
    );

    // `@daily` with FT_TODAY=2026-07-29 should match only the 07-29 note's
    // tasks, identical to the spelled-out path literal.
    let via_sigil = json_tasks(
        dir.path(),
        "2026-07-29",
        &["path includes @daily and status = Open", "--allow-empty"],
    );
    let via_literal = json_tasks(
        dir.path(),
        "2026-07-29",
        &[
            r#"path includes "journal/2026/2026-07-29.md" and status = Open"#,
            "--allow-empty",
        ],
    );
    assert_eq!(descriptions(&via_sigil), descriptions(&via_literal));
    assert!(descriptions(&via_sigil).contains(&"review PR".to_string()));
    assert!(!descriptions(&via_sigil).contains(&"tomorrow task".to_string()));
}

#[test]
fn at_today_matches_iso_date_substring() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-07-29",
        "# 2026-07-29\n- [ ] today thing\n",
    );
    let via_sigil = json_tasks(
        dir.path(),
        "2026-07-29",
        &["path includes @today and status = Open", "--allow-empty"],
    );
    let via_literal = json_tasks(
        dir.path(),
        "2026-07-29",
        &[
            r#"path includes "2026-07-29" and status = Open"#,
            "--allow-empty",
        ],
    );
    assert_eq!(descriptions(&via_sigil), descriptions(&via_literal));
    assert!(descriptions(&via_sigil).contains(&"today thing".to_string()));
}

#[test]
fn at_daily_offset_resolves_neighbor_day() {
    let dir = vault_with_daily();
    seed_day(
        dir.path(),
        "2026-07-28",
        "# 2026-07-28\n- [ ] yesterday task\n",
    );
    seed_day(dir.path(), "2026-07-29", "# 2026-07-29\n- [ ] today task\n");

    // FT_TODAY=2026-07-29, @daily-1 → 2026-07-28's note.
    let v = json_tasks(
        dir.path(),
        "2026-07-29",
        &["path = @daily-1 and status = Open", "--allow-empty"],
    );
    assert_eq!(descriptions(&v), vec!["yesterday task".to_string()]);
}

// ── dynamic presets ─────────────────────────────────────────────────────────

#[test]
fn preset_with_at_daily_resolves_dynamically_per_run() {
    let dir = vault_with_daily_preset();
    seed_day(dir.path(), "2026-07-29", "# 2026-07-29\n- [ ] 29th task\n");
    seed_day(dir.path(), "2026-07-30", "# 2026-07-30\n- [ ] 30th task\n");

    let day1 = json_tasks(dir.path(), "2026-07-29", &["daily-open", "--allow-empty"]);
    assert_eq!(descriptions(&day1), vec!["29th task".to_string()]);

    // Same preset, next day → different note, different task.
    let day2 = json_tasks(dir.path(), "2026-07-30", &["daily-open", "--allow-empty"]);
    assert_eq!(descriptions(&day2), vec!["30th task".to_string()]);
}

// ── error paths ─────────────────────────────────────────────────────────────

#[test]
fn unknown_sigil_exits_2() {
    let dir = vault_with_daily();
    let assert = run_tasks(dir.path(), "2026-07-29", &["path includes @datly"]);
    assert
        .failure()
        .code(2)
        .stderr(predicate::str::contains("@datly"))
        .stderr(predicate::str::contains("today"));
}

#[test]
fn missing_periodic_config_exits_2() {
    // Vault with NO [periodic_notes.daily] block.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml").write_str("").unwrap();

    let assert = run_tasks(dir.path(), "2026-07-29", &["path = @daily"]);
    assert
        .failure()
        .code(2)
        .stderr(predicate::str::contains("[periodic_notes.daily]"));
}

#[test]
fn at_today_works_without_periodic_config() {
    // @today needs no periodic config.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml").write_str("").unwrap();
    seed_day(dir.path(), "2026-07-29", "# 2026-07-29\n- [ ] flat task\n");

    let v = json_tasks(
        dir.path(),
        "2026-07-29",
        &["path includes @today and status = Open", "--allow-empty"],
    );
    assert!(descriptions(&v).contains(&"flat task".to_string()));
}
