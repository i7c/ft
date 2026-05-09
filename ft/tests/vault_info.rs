use assert_cmd::Command;
use predicates::prelude::*;

fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is ft/ft/, parent is workspace root ft/
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ft crate must have a parent (workspace root)")
        .to_path_buf()
}

#[test]
fn vault_info_tiny_fixture_succeeds() {
    let fixture = workspace_root().join("tests/fixtures/tiny");
    assert!(
        fixture.join(".obsidian").exists(),
        "tiny fixture must have .obsidian/ — is the path right?"
    );

    Command::cargo_bin("ft")
        .unwrap()
        .args(["--vault", fixture.to_str().unwrap(), "vault"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Vault:"))
        .stdout(predicate::str::contains("Config files"))
        .stdout(predicate::str::contains("Merged config"));
}

#[test]
fn vault_info_tiny_fixture_shows_vault_config_values() {
    let fixture = workspace_root().join("tests/fixtures/tiny");

    Command::cargo_bin("ft")
        .unwrap()
        .args(["--vault", fixture.to_str().unwrap(), "vault"])
        .assert()
        .success()
        // These come from tests/fixtures/tiny/.ft/config.toml
        .stdout(predicate::str::contains("Tasks.md"))
        .stdout(predicate::str::contains("Journal"));
}

#[test]
fn vault_info_missing_vault_exits_nonzero() {
    Command::cargo_bin("ft")
        .unwrap()
        .args(["--vault", "/tmp/definitely-not-a-vault-xyzzy", "vault"])
        .assert()
        .failure();
}

#[test]
fn help_works() {
    Command::cargo_bin("ft")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("vault"));
}

#[test]
fn version_works() {
    Command::cargo_bin("ft")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ft"));
}
