//! Integration tests for `ft notes search`.

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;
use serde_json::Value;

fn ft() -> Command {
    Command::cargo_bin("ft").unwrap()
}

/// Fixture vault with a spread of paragraphs exercising every mode.
fn make_vault() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/a.md")
        .write_str(
            "The eigen decomposition is central.\n\
             \n\
             We mention [[Eigen Decomposition]] in passing.\n\
             \n\
             Memoization strategy pays off here.\n\
             \n\
             prose about memoization only\n",
        )
        .unwrap();
    tmp.child("notes/b.md")
        .write_str("Eigenvalue bounds and trigram candidate filtering.\n")
        .unwrap();
    tmp.child("journal/2026-05-08.md")
        .write_str("Eigen daily note, noisy.\n")
        .unwrap();
    tmp
}

#[test]
fn substring_default_matches_fragments() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Eigenvalue bounds"));
    assert!(stdout.contains("eigen decomposition is central"));
    // The `[[Eigen Decomposition]]` mention contains the substring too.
    assert!(stdout.contains("in passing"));
}

#[test]
fn word_mode_requires_whole_token() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "=eigen", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("eigen decomposition is central"));
    assert!(
        !stdout.contains("Eigenvalue"),
        "=eigen must not match eigenvalue"
    );
}

#[test]
fn link_clause_with_spaces() {
    let tmp = make_vault();
    let out = ft()
        .args([
            "notes",
            "search",
            "[[Eigen Decomposition]]",
            "--flat",
            "--vault",
        ])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("in passing"));
    assert!(!stdout.contains("Memoization"));
    // One result only: the paragraph carrying the link.
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn and_requires_all_terms_in_same_paragraph() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen memoization", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.is_empty(), "no paragraph contains both: {stdout}");
}

#[test]
fn any_mode_unions() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen memoization", "--any", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Memoization strategy"));
    assert!(stdout.contains("eigen decomposition is central"));
}

#[test]
fn fuzzy_tolerates_typos() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "~memoizaton", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Memoization strategy"));
}

#[test]
fn phrase_requires_contiguous() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "\"trigram candidate\"", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Eigenvalue bounds"));
}

#[test]
fn exclude_filters_after_matching() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen -decomposition", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Eigenvalue bounds"));
    assert!(!stdout.contains("eigen decomposition is central"));
}

#[test]
fn exclude_prefixes_skip_noisy_folders() {
    let tmp = make_vault();
    tmp.child(".ft/config.toml")
        .write_str("[synth]\nexclude_prefixes = [\"journal/\"]\n")
        .unwrap();
    let out = ft()
        .args(["notes", "search", "eigen", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(!stdout.contains("daily note"), "journal/ must be excluded");
}

#[test]
fn limit_caps_results() {
    let tmp = make_vault();
    let out = ft()
        .args([
            "notes", "search", "eigen", "--limit", "1", "--flat", "--vault",
        ])
        .arg(tmp.path())
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn json_shape() {
    let tmp = make_vault();
    let out = ft()
        .args(["notes", "search", "eigen", "--json", "--vault"])
        .arg(tmp.path())
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert!(first.get("path").unwrap().is_string());
    assert_eq!(first.get("line_start").unwrap().as_u64(), Some(1));
    assert!(first.get("body").unwrap().is_string());
    assert!(first.get("matched").unwrap().is_array());
    assert!(first.get("score").unwrap().is_number());
    assert!(first.get("date").is_none(), "date only with --sort date");
}

#[test]
fn json_includes_date_with_sort_date() {
    let tmp = make_vault();
    // git repo so blame can resolve dates.
    let repo = tmp.path();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .current_dir(repo)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.name", "T"]);
    run(&["config", "user.email", "t@e.com"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["config", "maintenance.auto", "false"]);
    run(&["add", "."]);
    run(&["commit", "-m", "c1"]);

    let out = ft()
        .args([
            "notes", "search", "eigen", "--sort", "date", "--json", "--vault",
        ])
        .arg(tmp.path())
        .assert()
        .success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    for row in v.as_array().unwrap() {
        assert!(row.get("date").unwrap().is_string(), "date present per row");
    }
}

#[test]
fn empty_query_prints_nothing() {
    let tmp = make_vault();
    ft().args(["notes", "search", "", "--vault"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout("");
}

#[test]
fn no_match_prints_nothing_exit_zero() {
    let tmp = make_vault();
    ft().args(["notes", "search", "zebra", "--vault"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout("");
}

#[test]
fn removed_gather_subcommand_is_unknown() {
    // `ft notes gather` (and its `journal` alias) were removed with the
    // deprecated gather feed; the subcommand must not exist anymore.
    let tmp = make_vault();
    ft().args(["notes", "gather", "a", "--vault"])
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
    ft().args(["notes", "journal", "a", "--vault"])
        .arg(tmp.path())
        .assert()
        .failure();
}

fn stdout_of(tmp: &assert_fs::TempDir, args: &[&str]) -> String {
    let out = ft()
        .args(["notes", "search"])
        .args(args)
        .arg("--vault")
        .arg(tmp.path())
        .assert()
        .success();
    String::from_utf8(out.get_output().stdout.clone()).unwrap()
}

/// Vault for the grouped/snippet/filter scenarios.
fn make_rich_vault() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("notes/linear.md")
        .write_str(
            "# Linear\n\n\
             ## Spectra\n\n\
             An eigenvalue scales its eigenvector.\n\n\
             Every eigenvalue of a symmetric matrix is real.\n",
        )
        .unwrap();
    tmp.child("proj/busao/Ônibus.md")
        .write_str("---\ntags: [proj/busao]\n---\n\nO Ônibus sai da garagem às seis.\n")
        .unwrap();
    tmp.child("archive/old.md")
        .write_str("An old eigenvalue note.\n")
        .unwrap();
    tmp
}

#[test]
fn grouped_output_has_note_headers_and_snippets() {
    let tmp = make_rich_vault();
    let stdout = stdout_of(&tmp, &["eigenvalue -path:archive/"]);
    insta::assert_snapshot!("search_grouped", stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "notes/linear.md");
    assert!(lines[1].starts_with("  L5-5  Linear › Spectra  eigenvalue  "));
    assert!(lines[2].starts_with("  L7-7  Linear › Spectra  eigenvalue  "));
    assert_eq!(lines.len(), 3);
}

#[test]
fn flat_output_keeps_the_one_line_format() {
    let tmp = make_rich_vault();
    let stdout = stdout_of(&tmp, &["eigenvalue", "path:notes/", "--flat"]);
    insta::assert_snapshot!("search_flat", stdout);
    for line in stdout.lines() {
        assert!(line.starts_with("notes/linear.md L"), "{line}");
    }
}

#[test]
fn json_carries_presentation_fields() {
    let tmp = make_rich_vault();
    let out = stdout_of(&tmp, &["ônibus", "--json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let row = &v.as_array().unwrap()[0];
    let body = row["body"].as_str().unwrap();
    let h = row["highlights"].as_array().unwrap();
    assert_eq!(h.len(), 1);
    let (s, e) = (
        h[0][0].as_u64().unwrap() as usize,
        h[0][1].as_u64().unwrap() as usize,
    );
    assert_eq!(&body[s..e], "Ônibus");
    assert_eq!(row["title"], "Ônibus");
    assert!(row["heading_path"].as_array().unwrap().is_empty());
    let reasons = row["reasons"].as_array().unwrap();
    assert!(reasons
        .iter()
        .any(|r| r["clause"] == "ônibus" && r["field"] == "body"));
    assert!(reasons.iter().any(|r| r["field"] == "title"));
}

#[test]
fn json_snapshot() {
    let tmp = make_rich_vault();
    let out = stdout_of(&tmp, &["eigenvalue", "path:notes/", "--json"]);
    insta::assert_snapshot!("search_json", out);
}

#[test]
fn tag_filter_from_frontmatter() {
    let tmp = make_rich_vault();
    let stdout = stdout_of(&tmp, &["garagem", "tag:proj", "--flat"]);
    assert!(stdout.contains("proj/busao/Ônibus.md"));
    let stdout = stdout_of(&tmp, &["garagem", "tag:other", "--flat"]);
    assert!(stdout.is_empty());
}

#[test]
fn unknown_scope_exits_two_and_lists_presets() {
    let tmp = make_rich_vault();
    ft().args(["notes", "search", "eigenvalue scope:nope", "--vault"])
        .arg(tmp.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("nope").and(predicate::str::contains("links")));
}

#[test]
fn scope_filter_uses_graph_presets() {
    let tmp = make_rich_vault();
    tmp.child(".ft/config.toml")
        .write_str("[graph.presets]\nnotes-only = 'node where path starts_with \"notes/\";'\n")
        .unwrap();
    let stdout = stdout_of(&tmp, &["eigenvalue", "scope:notes-only", "--flat"]);
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
    assert!(!stdout.contains("archive/"));
}

#[test]
fn date_filter_without_git_warns_and_prints_nothing() {
    let tmp = make_rich_vault();
    ft().args(["notes", "search", "eigenvalue after:2026-01-01", "--vault"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains(
            "3 matching paragraph(s) have no blame date",
        ));
}

fn git_commit_all(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}");
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.name", "T"]);
    run(&["config", "user.email", "t@e.com"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "c1"]);
}

#[test]
fn plain_search_never_blames_but_date_sort_fills_the_cache() {
    let tmp = make_rich_vault();
    git_commit_all(tmp.path());
    let cache = tmp.path().join(".ft/cache/blame.msgpack");

    let stdout = stdout_of(&tmp, &["eigenvalue", "--flat"]);
    assert_eq!(stdout.lines().count(), 3);
    assert!(!cache.exists(), "a relevance search must not blame");

    stdout_of(&tmp, &["eigenvalue", "--sort", "date", "--flat"]);
    assert!(cache.exists(), "the date sort populates the cache");
    let first = std::fs::read(&cache).unwrap();

    // A relevance search reads the warm cache without rewriting it.
    stdout_of(&tmp, &["eigenvalue", "--flat"]);
    assert_eq!(std::fs::read(&cache).unwrap(), first);
}

// ── Semantic (hybrid) search ────────────────────────────────────────

/// Vault for hybrid tests. `b.md` shares only `alpha` with the query
/// `alpha beta`, so AND excludes it lexically while the (hash) embedding
/// still finds it similar.
fn make_semantic_vault(config: &str) -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().unwrap();
    tmp.child(".obsidian").create_dir_all().unwrap();
    tmp.child("a.md").write_str("alpha beta gamma.\n").unwrap();
    tmp.child("b.md")
        .write_str("alpha delta epsilon.\n")
        .unwrap();
    tmp.child("c.md")
        .write_str("unrelated zeta words.\n")
        .unwrap();
    if !config.is_empty() {
        tmp.child(".ft/config.toml").write_str(config).unwrap();
    }
    tmp
}

fn hashed(tmp: &assert_fs::TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    ft().env("FT_SEMANTIC_TEST_EMBEDDER", "hash")
        .args(["--vault"])
        .arg(tmp.path())
        .args(args)
        .assert()
}

#[test]
fn semantic_flag_adds_meaning_only_matches() {
    let tmp = make_semantic_vault("");
    let out = hashed(&tmp, &["notes", "search", "alpha beta", "--flat"]).success();
    let lexical = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(lexical.lines().count(), 1, "AND: only a.md\n{lexical}");

    let out = hashed(
        &tmp,
        &["notes", "search", "alpha beta", "--semantic", "--flat"],
    )
    .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("search_semantic_flat", stdout);
    assert!(
        stdout.lines().next().unwrap().starts_with("a.md"),
        "{stdout}"
    );
    assert!(
        stdout.lines().any(|l| l.starts_with("b.md L1-1  ≈  ")),
        "b.md joins as a meaning-only match:\n{stdout}"
    );
    assert!(!stdout.contains("c.md"));
    assert!(
        tmp.child(".ft/cache/embeddings/test-hash.bin")
            .path()
            .exists(),
        "embeddings are cached"
    );
}

#[test]
fn semantic_json_reason_and_no_semantic_override() {
    let tmp = make_semantic_vault("[search.semantic]\nenabled = true\n");
    let out = hashed(&tmp, &["notes", "search", "alpha beta", "--json"]).success();
    let v: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    let b = v
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == "b.md")
        .expect("enabled in config");
    assert_eq!(b["matched"].as_array().unwrap().len(), 0);
    assert!(b["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["clause"] == "≈" && r["field"] == "semantic"));

    let out = hashed(
        &tmp,
        &["notes", "search", "alpha beta", "--no-semantic", "--flat"],
    )
    .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1, "--no-semantic wins:\n{stdout}");
}

#[test]
fn like_ranks_by_similarity_and_excludes_the_source() {
    let tmp = make_semantic_vault("");
    let out = hashed(&tmp, &["notes", "search", "--like", "a.md:1", "--flat"]).success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.starts_with("b.md"), "{stdout}");
    assert!(!stdout.contains("a.md"), "source excluded:\n{stdout}");
    hashed(&tmp, &["notes", "search", "--like", "a.md:7"])
        .code(2)
        .stderr(predicate::str::contains(
            "no indexed paragraph starts at a.md:7",
        ));
}

#[test]
fn missing_model_degrades_or_exits_two() {
    let tmp = make_semantic_vault("[search.semantic]\nmodel = \"none/absent-model\"\n");
    ft().args([
        "notes",
        "search",
        "alpha beta",
        "--semantic",
        "--flat",
        "--vault",
    ])
    .arg(tmp.path())
    .assert()
    .success()
    .stdout(predicate::str::contains("a.md"))
    .stderr(predicate::str::contains("ft notes embed fetch"));
    ft().args(["notes", "search", "--like", "a.md:1", "--vault"])
        .arg(tmp.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("ft notes embed fetch"));
}

#[test]
fn embed_build_and_status() {
    let tmp = make_semantic_vault("");
    hashed(&tmp, &["notes", "embed", "build"])
        .success()
        .stdout(predicate::str::starts_with(
            "embedded 3, reused 0, pruned 0",
        ));
    hashed(&tmp, &["notes", "embed", "build"])
        .success()
        .stdout(predicate::str::starts_with(
            "embedded 0, reused 3, pruned 0",
        ));
    tmp.child("b.md")
        .write_str("alpha changed text.\n")
        .unwrap();
    hashed(&tmp, &["notes", "embed", "build"])
        .success()
        .stdout(predicate::str::starts_with(
            "embedded 1, reused 2, pruned 1",
        ));
    hashed(&tmp, &["notes", "embed", "status"])
        .success()
        .stdout(predicate::str::contains("embedded: 3 / 3 paragraphs"));
}

#[test]
fn embed_fetch_of_a_local_directory_is_a_no_op() {
    let tmp = make_semantic_vault("");
    let dir = tmp.child("models/local");
    dir.create_dir_all().unwrap();
    ft().args(["notes", "embed", "fetch", "--model"])
        .arg(dir.path())
        .args(["--vault"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("nothing to fetch"));
}
