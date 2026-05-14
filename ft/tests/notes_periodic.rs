//! Integration tests for `ft notes periodic <PERIOD>` and `ft notes
//! today` — the CLI surface added in plan 010 session 2.
//!
//! Each test builds a fresh `TempDir` vault (with a `.obsidian/` marker
//! and a minimal `.ft/config.toml`), invokes `ft` via `assert_cmd`, and
//! verifies the file-system state plus stdout / exit code. `FT_TODAY` is
//! pinned to `2026-05-13` (a Wednesday in ISO week 20 of 2026) so date
//! math is deterministic across machines.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

const TODAY: &str = "2026-05-13";

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn vault_with_config(toml: &str) -> assert_fs::TempDir {
    let dir = vault();
    dir.child(".ft").create_dir_all().unwrap();
    dir.child(".ft/config.toml").write_str(toml).unwrap();
    dir
}

/// Vault with `[periodic_notes.daily]` only — the most common shape.
fn vault_with_daily_config() -> assert_fs::TempDir {
    vault_with_config(
        r#"
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
"#,
    )
}

/// Vault with `[periodic_notes.daily]` pointing at a `daily` template
/// (the `templates-ft/daily.md` fixture is copied in).
fn vault_with_daily_template() -> assert_fs::TempDir {
    let dir = vault_with_config(
        r#"
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
template = "daily"
"#,
    );
    dir.child("templates-ft").create_dir_all().unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ft-core")
        .join("tests")
        .join("fixtures")
        .join("templates-ft")
        .join("daily.md");
    let src = src.canonicalize().unwrap();
    std::fs::copy(&src, dir.child("templates-ft").child("daily.md").path()).unwrap();
    dir
}

fn ft() -> Command {
    let mut cmd = Command::cargo_bin("ft").unwrap();
    cmd.env("FT_TODAY", TODAY);
    cmd
}

// ── happy paths: one test per period ────────────────────────────────────────

#[test]
fn daily_missing_file_is_created_then_subsequent_call_just_opens() {
    let dir = vault_with_daily_config();

    // First invocation: file doesn't exist → Created.
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "daily",
            "--no-open",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.starts_with("Created "),
        "expected `Created ...` prefix, got {stdout:?}"
    );
    assert!(stdout.contains("journal/2026/2026-05-13.md"), "{stdout:?}");

    let body = std::fs::read_to_string(dir.child("journal/2026/2026-05-13.md").path()).unwrap();
    assert_eq!(body, "# 2026-05-13\n\n");

    // Mutate the file so we can detect rewrites.
    std::fs::write(
        dir.child("journal/2026/2026-05-13.md").path(),
        "# manually edited\n",
    )
    .unwrap();

    // Second invocation: file exists → Opened, no rewrite.
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "daily",
            "--no-open",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.starts_with("Opened "),
        "expected `Opened ...` prefix on second call, got {stdout:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.child("journal/2026/2026-05-13.md").path()).unwrap(),
        "# manually edited\n",
        "second invocation must not rewrite an existing file"
    );
}

#[test]
fn daily_with_template_renders_against_invocation_today() {
    let dir = vault_with_daily_template();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "daily",
        "--no-open",
    ])
    .assert()
    .success();
    let body = std::fs::read_to_string(dir.child("journal/2026/2026-05-13.md").path()).unwrap();
    // The daily.md fixture uses `{{ today | date(format="%Y-%m-%d") }}`
    // for the heading and the `Created-...` tag, and `{{ title }}` for
    // the task query — all of which resolve to 2026-05-13 when FT_TODAY
    // is pinned to that date and no --offset is given.
    assert!(body.contains("tags: [Created-2026-05-13]"), "{body}");
    assert!(body.contains("# 2026-05-13"), "{body}");
    assert!(body.contains("(done 2026-05-13)"), "{body}");
}

#[test]
fn weekly_offset_negative_resolves_to_previous_iso_week() {
    // FT_TODAY=2026-05-13 → ISO W20. `--offset -1` → 2026-05-06 → W19.
    let dir = vault_with_config(
        r#"
[periodic_notes.weekly]
path = "journal/%Y"
format = "%G-W%V"
"#,
    );
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "weekly",
        "--offset",
        "-1",
        "--no-open",
    ])
    .assert()
    .success();
    assert!(
        dir.child("journal/2026/2026-W19.md").path().exists(),
        "expected journal/2026/2026-W19.md to exist"
    );
}

#[test]
fn monthly_offset_clamps_to_month_end() {
    // Jan 31 + 1 month → Feb 28 (2026 is not a leap year).
    let dir = vault_with_config(
        r#"
[periodic_notes.monthly]
path = "journal/%Y"
format = "%Y-%m"
"#,
    );
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "monthly",
            "--date",
            "2026-01-31",
            "--offset",
            "1",
            "--no-open",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains("journal/2026/2026-02.md"), "{stdout:?}");
    assert!(dir.child("journal/2026/2026-02.md").path().exists());
}

#[test]
fn quarterly_format_uses_q_token() {
    // FT_TODAY=2026-05-13 → May → Q2 → `2026-Q2.md`.
    let dir = vault_with_config(
        r#"
[periodic_notes.quarterly]
path = "journal/%Y"
format = "%Y-Q%q"
"#,
    );
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "quarterly",
        "--no-open",
    ])
    .assert()
    .success();
    assert!(dir.child("journal/2026/2026-Q2.md").path().exists());
}

#[test]
fn yearly_minimal_config_writes_at_configured_folder() {
    let dir = vault_with_config(
        r#"
[periodic_notes.yearly]
path = "journal"
format = "%Y"
"#,
    );
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "yearly",
        "--no-open",
    ])
    .assert()
    .success();
    let body = std::fs::read_to_string(dir.child("journal/2026.md").path()).unwrap();
    assert_eq!(body, "# 2026\n\n");
}

// ── short-form periods ───────────────────────────────────────────────────────

#[test]
fn short_form_period_letter_accepted() {
    // `d` → Period::Daily — same effect as `daily`.
    let dir = vault_with_daily_config();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "d",
        "--no-open",
    ])
    .assert()
    .success();
    assert!(dir.child("journal/2026/2026-05-13.md").path().exists());
}

// ── ft notes today ───────────────────────────────────────────────────────────

#[test]
fn today_alias_creates_same_file_as_periodic_daily() {
    let dir = vault_with_daily_config();
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "today",
            "--no-open",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.starts_with("Created "), "{stdout:?}");
    assert!(stdout.contains("journal/2026/2026-05-13.md"), "{stdout:?}");
    assert!(dir.child("journal/2026/2026-05-13.md").path().exists());
}

#[test]
fn today_with_date_override_resolves_explicit_day() {
    let dir = vault_with_daily_config();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "today",
        "--date",
        "2026-05-20",
        "--no-open",
    ])
    .assert()
    .success();
    assert!(dir.child("journal/2026/2026-05-20.md").path().exists());
}

// ── error handling ───────────────────────────────────────────────────────────

#[test]
fn missing_period_config_exits_2_with_hint() {
    let dir = vault_with_daily_config(); // weekly NOT configured
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "weekly",
        "--no-open",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("weekly not configured"))
    .stderr(predicate::str::contains("[periodic_notes.weekly]"));
}

#[test]
fn unparseable_date_exits_2() {
    let dir = vault_with_daily_config();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "daily",
        "--date",
        "garbage",
        "--no-open",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("YYYY-MM-DD"));
}

#[test]
fn unknown_period_exits_2() {
    let dir = vault_with_daily_config();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "periodic",
        "fortnightly",
        "--no-open",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("unknown period"));
}

// ── --no-open / editor dispatch ──────────────────────────────────────────────

#[test]
fn no_open_skips_editor() {
    let dir = vault_with_daily_config();
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "daily",
            "--editor",
            "echo",
            "--no-open",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    // With `--editor echo --no-open`, no editor invocation happens — so
    // the `+1 <path>` echo signature must not appear.
    assert!(
        !stdout.contains("+1"),
        "expected no editor invocation, got {stdout:?}"
    );
}

#[test]
fn editor_invoked_by_default() {
    let dir = vault_with_daily_config();
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "daily",
            "--editor",
            "echo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(
        stdout.contains("+1") && stdout.contains("2026-05-13.md"),
        "expected `+1 ...2026-05-13.md` in echo output, got {stdout:?}"
    );
}

// ── --obsidian dispatch ──────────────────────────────────────────────────────

#[test]
fn obsidian_dry_run_prints_url_and_skips_editor() {
    let dir = vault_with_daily_config();
    let out = ft()
        .env("FT_OBSIDIAN_DRY_RUN", "1")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "daily",
            "--obsidian",
            "--editor",
            "echo",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains("obsidian://open?"), "{stdout:?}");
    assert!(stdout.contains("2026-05-13.md"), "{stdout:?}");
    // The obsidian branch returns before the editor would have run.
    assert!(
        !stdout.contains("+1"),
        "editor should not run under --obsidian, got {stdout:?}"
    );
}

#[test]
fn custom_vault_name_in_obsidian_url() {
    let dir = vault_with_daily_config();
    let out = ft()
        .env("FT_OBSIDIAN_DRY_RUN", "1")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "periodic",
            "daily",
            "--obsidian",
            "--vault-name",
            "MyVault",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains("vault=MyVault"), "{stdout:?}");
}
