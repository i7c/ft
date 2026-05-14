//! Integration tests for `ft notes create` — blank/template note
//! creation, path normalization, collision handling, editor/obsidian
//! dispatch.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn vault() -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child(".obsidian").create_dir_all().unwrap();
    dir
}

fn vault_with_template(template_name: &str) -> assert_fs::TempDir {
    let dir = vault();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ft-core")
        .join("tests")
        .join("fixtures")
        .join("templates-ft")
        .join(template_name);
    let src = src.canonicalize().unwrap();
    dir.child("templates-ft").create_dir_all().unwrap();
    std::fs::copy(&src, dir.child("templates-ft").child(template_name).path()).unwrap();
    dir
}

fn ft() -> Command {
    let mut cmd = Command::cargo_bin("ft").unwrap();
    cmd.env("FT_TODAY", "2026-05-13");
    cmd
}

#[test]
fn blank_create_writes_title_heading() {
    let dir = vault();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "scratch",
        "--no-open",
    ])
    .assert()
    .success()
    .stderr(predicate::str::contains("created scratch.md"));

    let content = std::fs::read_to_string(dir.child("scratch.md").path()).unwrap();
    assert_eq!(content, "# scratch\n");
}

#[test]
fn md_extension_auto_appended() {
    let dir = vault();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "foo",
        "--no-open",
    ])
    .assert()
    .success();
    assert!(dir.child("foo.md").path().exists());
}

#[test]
fn intermediate_directories_created() {
    let dir = vault();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "deep/nested/path/file",
        "--no-open",
    ])
    .assert()
    .success();
    let content = std::fs::read_to_string(dir.child("deep/nested/path/file.md").path()).unwrap();
    assert_eq!(content, "# file\n");
}

#[test]
fn create_with_template_renders_against_title() {
    let dir = vault_with_template("proj.md");
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "proj/My Project",
        "--template",
        "proj",
        "--no-open",
    ])
    .assert()
    .success();
    let content = std::fs::read_to_string(dir.child("proj/My Project.md").path()).unwrap();
    assert!(content.contains("# My Project"));
    assert!(content.contains("Created-2026-05-13"));
    assert!(content.contains("[[{title}]]"));
}

#[test]
fn create_with_template_and_explicit_title_override() {
    let dir = vault_with_template("proj.md");
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "proj/short-slug",
        "--template",
        "proj",
        "--title",
        "Long Project Name",
        "--no-open",
    ])
    .assert()
    .success();
    let content = std::fs::read_to_string(dir.child("proj/short-slug.md").path()).unwrap();
    assert!(content.contains("# Long Project Name"));
}

#[test]
fn create_with_var_populates_template() {
    let dir = vault_with_template("quick-add.md");
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "inbox/2026-05-13-quick",
        "--template",
        "quick-add",
        "--var",
        "name=Lunch sandwich",
        "--no-open",
    ])
    .assert()
    .success();
    let content = std::fs::read_to_string(dir.child("inbox/2026-05-13-quick.md").path()).unwrap();
    assert!(content.contains("# Lunch sandwich"));
}

#[test]
fn missing_required_var_errors_with_exit_2() {
    let dir = vault_with_template("quick-add.md");
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "x",
        "--template",
        "quick-add",
        "--no-open",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("template render failed"));
}

#[test]
fn var_flag_without_equals_rejected_by_clap() {
    let dir = vault();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "x",
        "--var",
        "no_equals_sign",
        "--no-open",
    ])
    .assert()
    .code(2);
}

#[test]
fn collision_without_force_exits_2() {
    let dir = vault();
    dir.child("conflict.md").write_str("existing\n").unwrap();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "conflict",
        "--no-open",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("already exists"));
    // File untouched.
    assert_eq!(
        std::fs::read_to_string(dir.child("conflict.md").path()).unwrap(),
        "existing\n"
    );
}

#[test]
fn collision_with_force_overwrites() {
    let dir = vault();
    dir.child("conflict.md").write_str("existing\n").unwrap();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "conflict",
        "--force",
        "--no-open",
    ])
    .assert()
    .success();
    assert_eq!(
        std::fs::read_to_string(dir.child("conflict.md").path()).unwrap(),
        "# conflict\n"
    );
}

#[test]
fn missing_template_exits_2() {
    let dir = vault();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "x",
        "--template",
        "nope-not-here",
        "--no-open",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("template not found"));
}

#[test]
fn no_open_skips_editor() {
    let dir = vault();
    // If `--no-open` didn't work, this would attempt to spawn `echo`
    // and the test would still pass — but stdout wouldn't include
    // anything editor-shaped. Just assert success here; the
    // `editor_invoked_by_default` test below proves the inverse.
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "create",
            "noopen",
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
    assert!(
        !stdout.contains("+1"),
        "expected no editor invocation under --no-open, got stdout {stdout:?}"
    );
}

#[test]
fn editor_invoked_by_default() {
    let dir = vault();
    // Use `echo` as $EDITOR: it prints the args (`+1 <path>`) to stdout.
    let out = ft()
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "create",
            "auto-open",
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
        stdout.contains("+1") && stdout.contains("auto-open.md"),
        "expected `+1 ...auto-open.md` in editor invocation, got {stdout:?}"
    );
}

#[test]
fn obsidian_dry_run_prints_url_and_skips_editor() {
    let dir = vault();
    let out = ft()
        .env("FT_OBSIDIAN_DRY_RUN", "1")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "create",
            "ob-test",
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
    assert!(stdout.contains("&file=ob-test.md"), "{stdout:?}");
    // --editor echo would print `+1 path` to stdout if it ran; the
    // obsidian branch returns before spawning the editor.
    assert!(
        !stdout.contains("+1"),
        "editor should not run under --obsidian: {stdout:?}"
    );
}

#[test]
fn custom_vault_name_in_obsidian_url() {
    let dir = vault();
    let out = ft()
        .env("FT_OBSIDIAN_DRY_RUN", "1")
        .args([
            "--vault",
            dir.path().to_str().unwrap(),
            "notes",
            "create",
            "named",
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

#[test]
fn templates_dir_config_override_resolves_template() {
    let dir = vault();
    dir.child(".ft").create_dir_all().unwrap();
    dir.child(".ft/config.toml")
        .write_str(
            r#"
[notes]
templates_dir = "custom-templates"
"#,
        )
        .unwrap();
    dir.child("custom-templates").create_dir_all().unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ft-core")
        .join("tests")
        .join("fixtures")
        .join("templates-ft")
        .join("new.md")
        .canonicalize()
        .unwrap();
    std::fs::copy(&src, dir.child("custom-templates/new.md").path()).unwrap();

    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "via-custom-dir",
        "--template",
        "new",
        "--no-open",
    ])
    .assert()
    .success();
    let content = std::fs::read_to_string(dir.child("via-custom-dir.md").path()).unwrap();
    assert!(content.contains("# via-custom-dir"));
    assert!(content.contains("Created-2026-05-13"));
}

#[test]
fn absolute_template_path_used_as_is() {
    let dir = vault();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ft-core")
        .join("tests")
        .join("fixtures")
        .join("templates-ft")
        .join("new.md")
        .canonicalize()
        .unwrap();
    ft().args([
        "--vault",
        dir.path().to_str().unwrap(),
        "notes",
        "create",
        "from-abs",
        "--template",
        src.to_str().unwrap(),
        "--no-open",
    ])
    .assert()
    .success();
    let content = std::fs::read_to_string(dir.child("from-abs.md").path()).unwrap();
    assert!(content.contains("# from-abs"));
}
