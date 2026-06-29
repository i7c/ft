//! Data-driven graph DSL fixture matrix.
//!
//! Scans `ft-core/tests/fixtures/graph_queries/` for pairs of files:
//!
//! - `<NN>-<slug>.dsl`      — query source
//! - `<NN>-<slug>.expected` — expected result lines (`<kind-char> <path>`)
//!
//! For each pair, parse the query, run `select()` against the
//! `tests/fixtures/dirs` vault graph, render the resulting nodes in the
//! same `<kind-char> <path>` format, sort both sides, and diff.
//! Mismatches print a unified-style diff in the test output.
//!
//! See `ft-core/tests/fixtures/graph_queries/README.md` for the file
//! format and how to add a case.

use std::path::{Path, PathBuf};

use ft_core::graph::query::parse;
use ft_core::graph::{Graph, NodeKind, NoteId};
use ft_core::vault::{Scan, Vault};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graph_queries")
}

fn dirs_vault_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/dirs")
}

/// Format one node as `<kind-char> <path>`. Directory with empty path
/// becomes `<root>`; ghosts use their raw unresolved string.
fn fmt_node(graph: &Graph, id: NoteId) -> String {
    match graph.node(id) {
        NodeKind::Note(n) => format!("N {}", n.path.display()),
        NodeKind::Directory(d) => {
            if d.path.as_os_str().is_empty() {
                "D <root>".to_string()
            } else {
                format!("D {}", d.path.display())
            }
        }
        NodeKind::Ghost(g) => format!("G {}", g.raw),
        NodeKind::Task(t) => format!("T {}", t.description),
        NodeKind::Paragraph(p) => format!("P {}:{}", p.source_file.display(), p.line_start),
        NodeKind::Heading(h) => format!("H {}:{}", h.source_file.display(), h.line),
    }
}

/// Parse a `.expected` file into a sorted Vec of result lines. Blank
/// lines and `#`-prefixed comments are ignored.
fn read_expected(path: &Path) -> Vec<String> {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut lines: Vec<String> = src
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    lines.sort();
    lines
}

/// Diff two sorted Vec<String> slices, returning a unified-ish view
/// suitable for `assert!` messages. Empty string if identical.
fn diff(expected: &[String], actual: &[String]) -> String {
    if expected == actual {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("\n--- expected\n+++ actual\n");
    let mut i = 0;
    let mut j = 0;
    while i < expected.len() || j < actual.len() {
        match (expected.get(i), actual.get(j)) {
            (Some(e), Some(a)) if e == a => {
                out.push_str(&format!("  {e}\n"));
                i += 1;
                j += 1;
            }
            (Some(e), Some(a)) if e < a => {
                out.push_str(&format!("- {e}\n"));
                i += 1;
            }
            (Some(_), Some(a)) => {
                out.push_str(&format!("+ {a}\n"));
                j += 1;
            }
            (Some(e), None) => {
                out.push_str(&format!("- {e}\n"));
                i += 1;
            }
            (None, Some(a)) => {
                out.push_str(&format!("+ {a}\n"));
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}

#[test]
fn graph_query_fixture_matrix() {
    let dir = fixtures_dir();
    let vault = Vault::discover(Some(dirs_vault_path())).expect("dirs fixture vault must exist");
    let graph = Graph::build(&vault, &Scan::default()).expect("build graph");

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read fixtures dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "dsl"))
        .collect();
    entries.sort();

    assert!(
        entries.len() >= 15,
        "expected ≥15 .dsl fixtures, found {}",
        entries.len()
    );

    let mut failures: Vec<String> = Vec::new();
    for dsl_path in &entries {
        let expected_path = dsl_path.with_extension("expected");
        let src = std::fs::read_to_string(dsl_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", dsl_path.display()));
        let query = parse(src.trim())
            .unwrap_or_else(|e| panic!("parse {} failed: {e}", dsl_path.display()));

        let ids = query.select(&graph);
        let mut actual: Vec<String> = ids.iter().map(|id| fmt_node(&graph, *id)).collect();
        actual.sort();

        let expected = read_expected(&expected_path);
        let d = diff(&expected, &actual);
        if !d.is_empty() {
            failures.push(format!(
                "CASE {}{d}",
                dsl_path.file_name().unwrap().to_string_lossy()
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} fixture case(s) failed:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
