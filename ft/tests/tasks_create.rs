use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

/// Build a temp vault with a `[periodic_notes.daily]` config in
/// `.ft/config.toml` so the default `--file` resolution works.
///
/// `format` accepts moment-style placeholders (`Some("YYYY-MM-DD")`) for
/// call-site readability — they're translated to the chrono `%`-tokens
/// the new resolver expects. `None` defaults to `%Y-%m-%d`.
fn vault_with_daily(folder: &str, format: Option<&str>) -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    let chrono_fmt = match format {
        Some("YYYY-MM-DD") | None => "%Y-%m-%d",
        Some(other) => panic!("unexpected daily-format token in tests: {other:?}"),
    };
    dir.child(".ft/config.toml")
        .write_str(&format!(
            "[periodic_notes.daily]\npath = \"{folder}\"\nformat = \"{chrono_fmt}\"\n"
        ))
        .unwrap();
    dir
}

fn run(vault: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut full = vec!["--vault", vault.to_str().unwrap(), "tasks", "create"];
    full.extend(args);
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-09")
        .args(&full)
        .assert()
}

#[test]
fn create_simple_task_in_daily_note() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Buy milk", "--due", "tomorrow"]).success();

    // The daily note didn't exist, so it's created from the daily stub
    // (`# <date>`) first — matching what `ft notes today` would produce.
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "# 2026-05-09\n\n- [ ] Buy milk ➕ 2026-05-09 📅 2026-05-10\n"
    );
}

#[test]
fn create_with_priority_and_tags() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(
        dir.path(),
        &[
            "Read book",
            "--priority",
            "high",
            "--tag",
            "work",
            "--tag",
            "books",
        ],
    )
    .success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "# 2026-05-09\n\n- [ ] Read book #work #books ⏫ ➕ 2026-05-09\n"
    );
}

#[test]
fn create_with_explicit_file_relative_to_vault() {
    let dir = vault_with_daily("journal", None);
    run(
        dir.path(),
        &["Take call", "--file", "inbox/calls.md", "--due", "+1w"],
    )
    .success();
    let content = std::fs::read_to_string(dir.path().join("inbox/calls.md")).unwrap();
    assert_eq!(content, "- [ ] Take call ➕ 2026-05-09 📅 2026-05-16\n");
}

#[test]
fn create_under_heading_existing() {
    let dir = vault_with_daily("journal", None);
    let f = dir.child("notes.md");
    f.write_str("# Notes\n\n## Tasks\n- [ ] existing\n\n## Other\n")
        .unwrap();
    run(
        dir.path(),
        &["New task", "--file", "notes.md", "--under-heading", "Tasks"],
    )
    .success();
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert!(content.contains("- [ ] existing\n- [ ] New task"));
}

#[test]
fn create_under_heading_creates_missing_heading() {
    let dir = vault_with_daily("journal", None);
    let f = dir.child("notes.md");
    f.write_str("# Notes\n").unwrap();
    run(
        dir.path(),
        &["New task", "--file", "notes.md", "--under-heading", "Tasks"],
    )
    .success();
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert!(content.contains("## Tasks"));
    assert!(content.contains("- [ ] New task"));
}

#[test]
fn create_at_line_inserts_at_position() {
    let dir = vault_with_daily("journal", None);
    let f = dir.child("notes.md");
    f.write_str("a\nb\nc\n").unwrap();
    run(
        dir.path(),
        &["New task", "--file", "notes.md", "--at-line", "2"],
    )
    .success();
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert_eq!(content, "a\n- [ ] New task ➕ 2026-05-09\nb\nc\n");
}

#[test]
fn duplicate_refused() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Buy milk", "--due", "tomorrow"]).success();
    run(dir.path(), &["Buy milk", "--due", "tomorrow"])
        .failure()
        .stderr(predicate::str::contains("duplicate task"));
}

#[test]
fn duplicate_inserted_with_force() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Buy milk", "--due", "tomorrow"]).success();
    run(dir.path(), &["Buy milk", "--due", "tomorrow", "--force"]).success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content
            .matches("- [ ] Buy milk ➕ 2026-05-09 📅 2026-05-10")
            .count(),
        2
    );
}

#[test]
fn invalid_date_rejected() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Bad", "--due", "zzznotadate"])
        .failure()
        .stderr(predicate::str::contains("--due"));
}

#[test]
fn duplicate_error_uses_relative_path() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Buy milk", "--due", "tomorrow"]).success();
    let assert = run(dir.path(), &["Buy milk", "--due", "tomorrow"]).failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("journal/2026-05-09.md"),
        "expected vault-relative path; got: {stderr}"
    );
    assert!(
        !stderr.contains(dir.path().to_str().unwrap()),
        "stderr should not contain absolute path; got: {stderr}"
    );
}

#[test]
fn round_trip_create_then_list() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Buy milk", "--due", "tomorrow"]).success();

    let assert = Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-09")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "tasks",
            "list",
            "--format",
            "json",
            "--no-color",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = v.as_array().unwrap();
    let descs: Vec<&str> = arr
        .iter()
        .map(|t| t["description"].as_str().unwrap())
        .collect();
    assert!(descs.contains(&"Buy milk"));
}

#[test]
fn missing_periodic_daily_config_explains_remedy() {
    // Vault with no [periodic_notes.daily] block and no --file should
    // fail with a hint pointing at the config key.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-09")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "tasks",
            "create",
            "Stuff",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("periodic_notes.daily"));
}

#[test]
fn daily_path_with_year_token_resolves_per_year() {
    // [periodic_notes.daily] with a %Y folder token keeps working as the
    // year rolls over without reconfiguring.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(
            r#"
[periodic_notes.daily]
path = "journal/%Y"
format = "%Y-%m-%d"
"#,
        )
        .unwrap();

    run(dir.path(), &["Buy milk", "--due", "tomorrow"]).success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "# 2026-05-09\n\n- [ ] Buy milk ➕ 2026-05-09 📅 2026-05-10\n"
    );
}

#[test]
fn old_daily_notes_block_rejected_with_config_error() {
    // The pre-010 `[daily_notes]` block is no longer accepted — the
    // figment loader rejects unknown top-level keys.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(
            r#"
[daily_notes]
source = "explicit"
path = "journal"
format = "YYYY-MM-DD"
"#,
        )
        .unwrap();
    Command::cargo_bin("ft")
        .unwrap()
        .env("FT_TODAY", "2026-05-09")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "tasks",
            "create",
            "Stuff",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("daily_notes"));
}

#[test]
fn description_collected_from_multiple_args() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(dir.path(), &["Buy", "milk", "and", "bread"]).success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "# 2026-05-09\n\n- [ ] Buy milk and bread ➕ 2026-05-09\n"
    );
}

#[test]
fn recurrence_id_and_depends_on() {
    let dir = vault_with_daily("journal", Some("YYYY-MM-DD"));
    run(
        dir.path(),
        &[
            "Pay tax",
            "--due",
            "2026-05-18",
            "--recurrence",
            "every month on the 18th",
            "--id",
            "tax42",
            "--depends-on",
            "abc",
            "--depends-on",
            "def",
        ],
    )
    .success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert!(content.contains("🔁 every month on the 18th"));
    assert!(content.contains("📅 2026-05-18"));
    assert!(content.contains("🆔 tax42"));
    assert!(content.contains("⛔ abc,def"));
}

/// Like [`vault_with_daily`] but also sets `[tasks] default_section`.
fn vault_with_daily_and_section(folder: &str, section: &str) -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(&format!(
            "[periodic_notes.daily]\npath = \"{folder}\"\nformat = \"%Y-%m-%d\"\n\n[tasks]\ndefault_section = \"{section}\"\n"
        ))
        .unwrap();
    dir
}

#[test]
fn default_section_lands_task_under_heading() {
    let dir = vault_with_daily_and_section("journal", "Tasks");
    dir.child("journal/2026-05-09.md")
        .write_str("# 2026-05-09\n\nNotes.\n")
        .unwrap();
    run(dir.path(), &["Buy milk"]).success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "# 2026-05-09\n\nNotes.\n\n## Tasks\n- [ ] Buy milk ➕ 2026-05-09\n"
    );
}

#[test]
fn frontmatter_section_overrides_config_default() {
    let dir = vault_with_daily_and_section("journal", "Tasks");
    let f = dir.child("notes.md");
    f.write_str("---\nft:\n  tasks:\n    section: Inbox\n---\n# Notes\n\n## Inbox\n")
        .unwrap();
    run(dir.path(), &["Ping bob", "--file", "notes.md"]).success();
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert!(content.contains("## Inbox\n- [ ] Ping bob"));
    assert!(!content.contains("## Tasks"));
}

#[test]
fn append_flag_overrides_default_section() {
    let dir = vault_with_daily_and_section("journal", "Tasks");
    dir.child("journal/2026-05-09.md")
        .write_str("# 2026-05-09\n\nNotes.\n")
        .unwrap();
    run(dir.path(), &["Raw append", "--append"]).success();
    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "# 2026-05-09\n\nNotes.\n- [ ] Raw append ➕ 2026-05-09\n"
    );
}

#[test]
fn under_heading_overrides_default_section() {
    let dir = vault_with_daily_and_section("journal", "Tasks");
    let f = dir.child("notes.md");
    f.write_str("# Notes\n\n## Later\n").unwrap();
    run(
        dir.path(),
        &[
            "Pick this",
            "--file",
            "notes.md",
            "--under-heading",
            "Later",
        ],
    )
    .success();
    let content = std::fs::read_to_string(f.path()).unwrap();
    assert!(content.contains("## Later\n- [ ] Pick this"));
    assert!(!content.contains("## Tasks"));
}

#[test]
fn missing_daily_note_is_created_from_template() {
    // A configured daily template is rendered when the note doesn't exist
    // yet, and the new task lands under the template's section (frontmatter
    // wins over config default). Proves ensure_target composes with the
    // section resolution.
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(
            "[periodic_notes.daily]\npath = \"journal\"\nformat = \"%Y-%m-%d\"\ntemplate = \"daily\"\n\n[notes]\ntemplates_dir = \"templates-ft\"\n",
        )
        .unwrap();
    dir.child("templates-ft/daily.md")
        .write_str(
            "---\nft:\n  tasks:\n    section: Tasks\n---\n# {{ title }}\n\n## Tasks\n\n## Log\n",
        )
        .unwrap();

    run(dir.path(), &["Buy milk"]).success();

    let content = std::fs::read_to_string(dir.path().join("journal/2026-05-09.md")).unwrap();
    assert_eq!(
        content,
        "---\nft:\n  tasks:\n    section: Tasks\n---\n# 2026-05-09\n\n## Tasks\n- [ ] Buy milk ➕ 2026-05-09\n\n## Log\n"
    );
}
