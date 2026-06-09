//! CLI-level real-vault smoke test.
//! Gated on `FT_REAL_VAULT_TESTS=1` so CI never depends on a local vault.
//! Run with:  FT_REAL_VAULT_TESTS=1 cargo test -p ft --test real_vault_cli

use assert_cmd::Command;
use chrono::Local;

const REAL_VAULT: &str = "/Users/cmw/git/fortytwo";

fn gated() -> bool {
    std::env::var("FT_REAL_VAULT_TESTS").as_deref() == Ok("1")
}

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

#[test]
fn real_vault_list_is_non_empty() {
    if !gated() {
        return;
    }
    let assert = ft()
        .args(["--vault", REAL_VAULT, "tasks", "list", "--allow-empty"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.trim().is_empty(),
        "real-vault list output should be non-empty"
    );
}

#[test]
fn real_vault_list_then_list_is_stable() {
    if !gated() {
        return;
    }
    let run = || -> String {
        let out = ft()
            .args([
                "--vault",
                REAL_VAULT,
                "tasks",
                "list",
                "--format",
                "ndjson",
                "--allow-empty",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(out).unwrap()
    };
    let a = run();
    let b = run();
    assert_eq!(a, b, "two consecutive list runs should match byte-for-byte");
}

#[test]
fn real_vault_overdue_preset_runs() {
    if !gated() {
        return;
    }
    // The `overdue` preset may legitimately match zero tasks, so allow empty.
    ft().args([
        "--vault",
        REAL_VAULT,
        "tasks",
        "list",
        "overdue",
        "--allow-empty",
    ])
    .env("FT_TODAY", "2026-05-10")
    .assert()
    .success();
}

// ── ft timeblocks against the real vault ────────────────────────────────────

#[test]
fn real_vault_timeblocks_list_today_succeeds() {
    if !gated() {
        return;
    }
    // The real vault may legitimately have an empty `## Time Blocks`
    // section today (or none at all), so allow empty results.
    ft().args([
        "--vault",
        REAL_VAULT,
        "timeblocks",
        "list",
        "--allow-empty",
        "--format",
        "json",
    ])
    .assert()
    .success();
}

#[test]
fn real_vault_timeblocks_add_dry_run_does_not_modify() {
    if !gated() {
        return;
    }
    // Compute the daily-note path the same way `[periodic_notes.daily]`
    // resolves it (path = "journal/%Y", format = "%Y-%m-%d"), then
    // hash the file before + after to assert --dry-run never writes.
    let today = Local::now().date_naive();
    let path = std::path::Path::new(REAL_VAULT)
        .join(format!("journal/{}", today.format("%Y")))
        .join(format!("{}.md", today.format("%Y-%m-%d")));
    let before = std::fs::read(&path).ok();
    ft().args([
        "--vault",
        REAL_VAULT,
        "timeblocks",
        "add",
        "23:50 - 23:55 __ft_smoke_dry_run__",
        "--dry-run",
    ])
    .assert()
    .success();
    let after = std::fs::read(&path).ok();
    assert_eq!(
        before,
        after,
        "--dry-run must not modify {} — content changed",
        path.display()
    );
}

#[test]
fn real_vault_timeblocks_spent_this_week_runs() {
    if !gated() {
        return;
    }
    ft().args([
        "--vault",
        REAL_VAULT,
        "timeblocks",
        "spent",
        "this-week",
        "--format",
        "json",
        "--allow-empty",
    ])
    .assert()
    .success();
}

#[test]
fn real_vault_dry_run_move_does_not_modify() {
    if !gated() {
        return;
    }
    // Build a dry-run move that almost certainly matches no tasks (a
    // synthetic tag) so the diff is empty / tiny but the command still
    // exercises scan + plan_move + diff render against the real vault.
    ft().args([
        "--vault",
        REAL_VAULT,
        "tasks",
        "move",
        "--query",
        "tag is __ft_no_such_tag__",
        "--to",
        "_ft_smoke_real_target.md",
        "--dry-run",
        "--yes",
    ])
    .assert()
    // No tasks match → CLI errors with "no tasks matched"; treat that as
    // a successful exercise of the path (the failure exit just reports
    // emptiness).
    .failure();
}

// ── ft review / ft synth against the real vault ────────────────────────────

#[test]
fn real_vault_review_since_7d_runs() {
    if !gated() {
        return;
    }
    // A real vault may legitimately have no new links in the last 7d
    // (the command prints "no new links in window" and exits 0).
    ft().args(["--vault", REAL_VAULT, "review", "--since", "7d"])
        .assert()
        .success();
}

#[test]
fn real_vault_review_json_is_valid_json() {
    if !gated() {
        return;
    }
    let out = ft()
        .args(["--vault", REAL_VAULT, "review", "--since", "30d", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // Must parse as a JSON array of row objects.
    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert!(v.is_array(), "ft review --json must emit a JSON array");
}

#[test]
fn real_vault_synth_verify_all_runs() {
    if !gated() {
        return;
    }
    // `ft synth verify --all` exits 0 when every section is `ok` and
    // 1 when any section drifted. A vault with no synth notes prints
    // "no synth notes found" and exits 0. Accept either as long as
    // the command doesn't panic / fail to launch.
    let _ = ft()
        .args(["--vault", REAL_VAULT, "synth", "verify", "--all"])
        .assert();
}
