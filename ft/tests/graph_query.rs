//! Integration tests for `ft graph query`. Covers the five output
//! formats, the depth bound, the visit policy guard, parse-error exit
//! code, and `--from-file` parity with the inline form.

use assert_cmd::Command;
use assert_fs::prelude::*;
use assert_fs::TempDir;
use predicates::prelude::*;

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ft crate must have a parent (workspace root)")
        .to_path_buf()
}

fn dirs_vault() -> std::path::PathBuf {
    workspace_root().join("tests/fixtures/dirs")
}

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

const DIRS_FULL_QUERY: &str =
    "node where kind = Directory and path = \"\"; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};";

const DIRS_ROOT_QUERY: &str =
    "node where kind = Directory and path = \"\"; expand where from.kind = Directory and edge.kind = directory-contains and to.kind in {Note, Directory};";

// ── Tree (default) format ─────────────────────────────────────────────

#[test]
fn graph_query_tree_depth_one_prints_root_and_immediate_children() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_ROOT_QUERY,
            "--depth",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    // Root directory line
    assert!(
        stdout.contains("D /"),
        "missing root directory line:\n{stdout}"
    );
    // Three immediate children: Projects/, Areas/, root.md — and no
    // grandchildren at depth 1.
    assert!(stdout.contains("Projects/"), "missing Projects/:\n{stdout}");
    assert!(stdout.contains("Areas/"), "missing Areas/:\n{stdout}");
    assert!(stdout.contains("N root"), "missing root note:\n{stdout}");
    assert!(
        !stdout.contains("operations/"),
        "depth=1 must not include grandchildren:\n{stdout}"
    );
}

#[test]
fn graph_query_tree_unbounded_walks_full_subtree() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    // 4 directory glyphs + 4 note glyphs = 8 lines.
    let line_count = stdout.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(
        line_count, 8,
        "expected 8 lines for the dirs fixture full walk, got {line_count}:\n{stdout}"
    );
    // Deepest leaf must be present
    assert!(
        stdout.contains("shifts"),
        "missing deep leaf shifts:\n{stdout}"
    );
    // Notes show with `· N <stem>` glyph at their depths
    assert!(stdout.contains("· N shifts"));
    assert!(stdout.contains("· N finance"));
    assert!(stdout.contains("· N alpha"));
}

#[test]
fn graph_query_depth_zero_returns_roots_only_for_every_format() {
    let v = dirs_vault();
    for format in ["tree", "json", "ndjson", "edges", "markdown"] {
        let out = ft()
            .args([
                "--vault",
                v.to_str().unwrap(),
                "graph",
                "query",
                DIRS_ROOT_QUERY,
                "--depth",
                "0",
                "--format",
                format,
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(out).unwrap();
        match format {
            "tree" | "markdown" => {
                // Exactly one body line — the root.
                let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
                assert_eq!(
                    lines.len(),
                    1,
                    "{format}: depth=0 should produce one line, got:\n{stdout}"
                );
            }
            "ndjson" => {
                let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
                assert_eq!(lines.len(), 1, "ndjson: depth=0 should produce one row");
            }
            "json" => {
                let v: serde_json::Value =
                    serde_json::from_str(&stdout).expect("json output must parse");
                let arr = v.as_array().expect("top-level array");
                assert_eq!(arr.len(), 1, "json: one root");
                let children = arr[0]["children"].as_array().unwrap();
                assert!(children.is_empty(), "json depth=0: children must be empty");
            }
            "edges" => {
                // No descents → no edges.
                assert!(
                    stdout.trim().is_empty(),
                    "edges: depth=0 must emit nothing, got:\n{stdout}"
                );
            }
            _ => unreachable!(),
        }
    }
}

// ── JSON format ───────────────────────────────────────────────────────

#[test]
fn graph_query_json_produces_valid_document() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json must parse");
    let arr = v.as_array().expect("top-level array");
    assert_eq!(arr.len(), 1, "exactly one root");
    let root = &arr[0];
    assert_eq!(root["kind"], "Directory");
    assert_eq!(root["path"], "");
    assert_eq!(root["depth"], 0);
    assert_eq!(root["closure"], "open");
    assert!(root["edge_to_parent"].is_null());

    // Count nodes via JSON walk: 8 total (4 dirs + 4 notes).
    fn count(n: &serde_json::Value) -> usize {
        let children = n["children"].as_array().unwrap();
        1 + children.iter().map(count).sum::<usize>()
    }
    assert_eq!(count(root), 8);
}

#[test]
fn graph_query_json_closure_marker_is_serialized() {
    let tmp = TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("[[b]]\n").unwrap();
    tmp.child("b.md").write_str("[[a]]\n").unwrap();

    // Default dedup policy: the re-entered `a` is a reference leaf.
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "graph",
            "query",
            "node where path = \"a.md\"; expand where edge.kind = note-link;",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let root = &v[0];
    let b = &root["children"][0];
    let a_ref = &b["children"][0];
    assert_eq!(
        a_ref["closure"], "reference",
        "under dedup the re-entered a is a reference"
    );
    assert!(
        a_ref["children"].as_array().unwrap().is_empty(),
        "reference nodes have no children"
    );

    // Tree policy (depth-bounded) reports the ancestor re-entry as a cycle.
    let out = ft()
        .args([
            "--vault",
            tmp.path().to_str().unwrap(),
            "graph",
            "query",
            "node where path = \"a.md\"; expand where edge.kind = note-link;",
            "--visit-policy",
            "tree",
            "--depth",
            "5",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let a_cycle = &v[0]["children"][0]["children"][0];
    assert_eq!(a_cycle["closure"], "cycle", "tree mode reports the cycle");
}

// ── NDJSON format ─────────────────────────────────────────────────────

#[test]
fn graph_query_ndjson_emits_pre_order_with_parent_ids() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
            "--format",
            "ndjson",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 8, "8 rows in pre-order");
    let rows: Vec<serde_json::Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).expect("each line must be JSON"))
        .collect();
    // First row is the root (parent_id null, depth 0).
    assert!(rows[0]["parent_id"].is_null());
    assert_eq!(rows[0]["depth"], 0);
    // Every non-root row has a parent_id matching some earlier row's id
    // (pre-order property).
    for (i, r) in rows.iter().enumerate().skip(1) {
        let parent = r["parent_id"]
            .as_u64()
            .unwrap_or_else(|| panic!("row {i} missing parent_id: {r}"));
        let parent_seen = rows[..i]
            .iter()
            .any(|prev| prev["id"].as_u64() == Some(parent));
        assert!(parent_seen, "row {i} parent_id {parent} not seen earlier");
    }
}

// ── Edges format ──────────────────────────────────────────────────────

#[test]
fn graph_query_edges_format_emits_unique_tsv_rows() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
            "--format",
            "edges",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    // 7 contains-edges descending from root → covers 7 of 8 nodes.
    assert_eq!(lines.len(), 7, "expected 7 edges:\n{stdout}");
    for line in &lines {
        let parts: Vec<&str> = line.split('\t').collect();
        assert_eq!(parts.len(), 3, "each row is src\\tlabel\\tdst, got: {line}");
        assert_eq!(parts[1], "directory-contains");
    }
}

// ── Markdown format ───────────────────────────────────────────────────

#[test]
fn graph_query_markdown_format_uses_bullets_and_indents() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
            "--format",
            "markdown",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    // Root line at indent 0.
    assert!(
        stdout.starts_with("- "),
        "first line must be a bullet:\n{stdout}"
    );
    // Deepest leaf at indent 6 (three levels deep × two spaces).
    assert!(
        stdout.contains("      - shifts ("),
        "shifts must be at depth 3 (6-space indent):\n{stdout}"
    );
}

// ── Parse-error path ──────────────────────────────────────────────────

#[test]
fn graph_query_parse_error_exits_two_with_message_on_stderr() {
    let v = dirs_vault();
    let assertion = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            "expand where edge.kind = note-link;",
        ])
        .assert()
        .failure()
        .code(2);
    let err = assertion.get_output().stderr.clone();
    let stderr = String::from_utf8(err).unwrap();
    assert!(
        stderr.contains("no `node` block")
            || stderr.contains("query has no")
            || stderr.contains("at least one"),
        "stderr should explain the parse error:\n{stderr}"
    );
}

#[test]
fn graph_query_missing_query_arg_errors_out() {
    let v = dirs_vault();
    ft().args(["--vault", v.to_str().unwrap(), "graph", "query"])
        .assert()
        .failure();
}

// ── --from-file ───────────────────────────────────────────────────────

#[test]
fn graph_query_from_file_matches_inline_form() {
    let v = dirs_vault();
    let tmp = TempDir::new().unwrap();
    let qfile = tmp.child("q.dsl");
    qfile.write_str(DIRS_FULL_QUERY).unwrap();

    let inline = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let from_file = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            "--from-file",
            qfile.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(
        String::from_utf8(inline).unwrap(),
        String::from_utf8(from_file).unwrap(),
        "--from-file output must match inline output byte-for-byte"
    );
}

#[test]
fn graph_query_from_file_missing_path_errors_out() {
    let v = dirs_vault();
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("does-not-exist.dsl");
    ft().args([
        "--vault",
        v.to_str().unwrap(),
        "graph",
        "query",
        "--from-file",
        missing.to_str().unwrap(),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("could not read query"));
}

// ── Visit policy ──────────────────────────────────────────────────────

#[test]
fn graph_query_visit_allow_without_depth_is_rejected() {
    let v = dirs_vault();
    ft().args([
        "--vault",
        v.to_str().unwrap(),
        "graph",
        "query",
        DIRS_FULL_QUERY,
        "--visit-policy",
        "allow",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("--visit-policy {tree,allow}"));
}

#[test]
fn graph_query_visit_tree_without_depth_is_rejected() {
    let v = dirs_vault();
    ft().args([
        "--vault",
        v.to_str().unwrap(),
        "graph",
        "query",
        DIRS_FULL_QUERY,
        "--visit-policy",
        "tree",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("--visit-policy {tree,allow}"));
}

// ── Tree snapshot for the dirs fixture ────────────────────────────────

#[test]
fn graph_query_tree_snapshot_for_dirs_fixture() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            DIRS_FULL_QUERY,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    insta::assert_snapshot!("graph_query_dirs_tree", stdout);
}

// ── --preset ──────────────────────────────────────────────────────────

#[test]
fn graph_query_preset_resolves_builtin_tree() {
    let v = dirs_vault();
    let out = ft()
        .args([
            "--vault",
            v.to_str().unwrap(),
            "graph",
            "query",
            "--preset",
            "tree",
            "--depth",
            "0",
            "--format",
            "ndjson",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let rows: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson row must parse"))
        .collect();
    assert!(!rows.is_empty(), "tree preset should return rows");
    assert_eq!(rows[0]["kind"], "Directory");
}

#[test]
fn graph_query_preset_unknown_exits_two() {
    let v = dirs_vault();
    ft().args([
        "--vault",
        v.to_str().unwrap(),
        "graph",
        "query",
        "--preset",
        "nonexistent",
    ])
    .assert()
    .code(2)
    .stderr(predicate::str::contains("unknown preset: nonexistent"));
}

#[test]
fn graph_query_preset_user_shadows_builtin() {
    let tmp = TempDir::new().unwrap();
    let vault_dir = tmp.child("v");
    vault_dir.create_dir_all().unwrap();
    vault_dir.child(".obsidian").create_dir_all().unwrap();
    vault_dir.child(".ft").create_dir_all().unwrap();
    vault_dir
        .child(".ft/config.toml")
        .write_str(
            r#"
[graph.presets]
orphans = "node where kind = Note;"
"#,
        )
        .unwrap();
    vault_dir.child("a.md").write_str("# A\n").unwrap();
    vault_dir.child("b.md").write_str("# B\n").unwrap();

    let out = ft()
        .args([
            "--vault",
            vault_dir.path().to_str().unwrap(),
            "graph",
            "query",
            "--preset",
            "orphans",
            "--format",
            "ndjson",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(out).unwrap();
    let rows: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson row must parse"))
        .collect();
    assert!(
        !rows.is_empty(),
        "user-overridden orphans preset should return rows"
    );
}
