//! Relevance evaluation harness for paragraph search.
//!
//! Runs the graded `query → paragraph` judgments in
//! `tests/fixtures/search-eval/judgments.toml` against the fixture vault
//! next to it and scores the ranking with nDCG@10, MRR, and recall@10.
//! The fixture run fails when a mean drops below the thresholds in
//! `baseline.toml` (the ratchet); `--nocapture` prints the per-case
//! table.
//!
//! The real-vault run reuses the harness against `FT_VAULT` with a
//! private judgments file, report-only:
//!
//! ```sh
//! FT_REAL_VAULT_TESTS=1 FT_VAULT=~/git/fortytwo \
//!   FT_SEARCH_EVAL_JUDGMENTS=~/judgments.toml \
//!   cargo test -p ft-core --test search_eval -- --nocapture
//! ```
//!
//! `FT_SEARCH_EVAL_SHOW=1` also prints each case's top ten hits with
//! their grade and score. The real-vault run reports twice — recency
//! from file mtimes, then from the vault's blame cache;
//! `FT_SEARCH_EVAL_WARM=1` blames every uncached file first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ft_core::blame_cache::{BlameCache, BlameValidity};
use ft_core::config::SearchCfg;
use ft_core::graph::Graph;
use ft_core::search::{parse_query, search, warm_blame, RecencyTable, SearchCtx, SearchIndex};
use serde::Deserialize;

const CUTOFF: usize = 10;

// ── Judgments ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JudgmentFile {
    #[serde(default, rename = "case")]
    cases: Vec<RawCase>,
}

#[derive(Debug, Deserialize)]
struct RawCase {
    query: Option<String>,
    #[serde(default)]
    any: bool,
    #[serde(default)]
    note: String,
    #[serde(default)]
    relevant: Vec<RawRelevant>,
}

#[derive(Debug, Deserialize)]
struct RawRelevant {
    path: Option<String>,
    line: Option<u32>,
    grade: Option<u8>,
}

#[derive(Debug, Clone)]
struct Case {
    query: String,
    any: bool,
    note: String,
    relevant: Vec<Judged>,
}

#[derive(Debug, Clone)]
struct Judged {
    path: PathBuf,
    line: u32,
    grade: u8,
}

/// Parse a judgment file, naming the case index on any missing field
/// or out-of-range grade.
fn parse_judgments(text: &str) -> Result<Vec<Case>, String> {
    let file: JudgmentFile = toml::from_str(text).map_err(|e| format!("judgments: {e}"))?;
    let mut out = Vec::with_capacity(file.cases.len());
    for (i, raw) in file.cases.into_iter().enumerate() {
        let query = raw
            .query
            .ok_or_else(|| format!("case {i}: missing `query`"))?;
        if raw.relevant.is_empty() {
            return Err(format!("case {i}: needs at least one [[case.relevant]]"));
        }
        let mut relevant = Vec::with_capacity(raw.relevant.len());
        for (j, r) in raw.relevant.into_iter().enumerate() {
            let path = r
                .path
                .ok_or_else(|| format!("case {i}, relevant {j}: missing `path`"))?;
            let line = r
                .line
                .ok_or_else(|| format!("case {i}, relevant {j}: missing `line`"))?;
            let grade = r
                .grade
                .ok_or_else(|| format!("case {i}, relevant {j}: missing `grade`"))?;
            if !(1..=3).contains(&grade) {
                return Err(format!(
                    "case {i}, relevant {j}: grade {grade} is outside the allowed range 1–3"
                ));
            }
            relevant.push(Judged {
                path: PathBuf::from(path),
                line,
                grade,
            });
        }
        out.push(Case {
            query,
            any: raw.any,
            note: raw.note,
            relevant,
        });
    }
    Ok(out)
}

// ── Metrics ──────────────────────────────────────────────────────────

/// One ranked result as the harness sees it.
#[derive(Debug, Clone)]
struct Hit {
    path: PathBuf,
    line_start: u32,
    line_end: u32,
    score: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct Scores {
    ndcg: f64,
    mrr: f64,
    recall: f64,
}

/// Per-rank gains: each judged paragraph counts once, at the first
/// result whose range contains its line.
fn gains(hits: &[Hit], relevant: &[Judged]) -> Vec<u8> {
    let mut used = vec![false; relevant.len()];
    hits.iter()
        .map(|h| {
            let mut best: Option<(usize, u8)> = None;
            for (k, j) in relevant.iter().enumerate() {
                if used[k] || j.path != h.path || j.line < h.line_start || j.line > h.line_end {
                    continue;
                }
                if best.is_none_or(|(_, g)| j.grade > g) {
                    best = Some((k, j.grade));
                }
            }
            match best {
                Some((k, g)) => {
                    used[k] = true;
                    g
                }
                None => 0,
            }
        })
        .collect()
}

fn dcg(grades: impl Iterator<Item = u8>) -> f64 {
    grades
        .enumerate()
        .map(|(i, g)| (2f64.powi(g as i32) - 1.0) / ((i + 2) as f64).log2())
        .fold(0.0, |acc, x| acc + x)
}

fn score(hits: &[Hit], relevant: &[Judged]) -> Scores {
    let g = gains(hits, relevant);
    let top = &g[..g.len().min(CUTOFF)];

    let mut ideal: Vec<u8> = relevant.iter().map(|j| j.grade).collect();
    ideal.sort_unstable_by(|a, b| b.cmp(a));
    ideal.truncate(CUTOFF);
    let idcg = dcg(ideal.into_iter());
    let ndcg = if idcg == 0.0 {
        0.0
    } else {
        dcg(top.iter().copied()) / idcg
    };

    let mrr = g
        .iter()
        .position(|&x| x > 0)
        .map(|p| 1.0 / (p + 1) as f64)
        .unwrap_or(0.0);

    let found = top.iter().filter(|&&x| x > 0).count();
    let recall = found as f64 / relevant.len() as f64;

    Scores { ndcg, mrr, recall }
}

// ── Running ──────────────────────────────────────────────────────────

/// The engine under evaluation, built once per vault.
struct Engine {
    index: SearchIndex,
    graph: Graph,
    presets: HashMap<String, String>,
    /// Paragraph blame recency; `None` ranks by file mtime (the fixture
    /// always runs without, so its in-repo commit times never leak in).
    recency: Option<RecencyTable>,
}

impl Engine {
    fn build(
        vault: &Path,
        exclude: &[String],
        cfg: &SearchCfg,
        presets: HashMap<String, String>,
    ) -> Engine {
        let scan = ft_core::scan::scan_vault(vault, &[]);
        let graph = Graph::build(&scan).unwrap();
        Engine {
            index: SearchIndex::build_with(&scan, exclude, cfg),
            graph,
            presets,
            recency: None,
        }
    }

    fn run(&self, case: &Case) -> Vec<Hit> {
        let query = parse_query(&case.query, case.any);
        let ctx = SearchCtx {
            graph: Some(&self.graph),
            presets: Some(&self.presets),
            recency: self.recency.as_ref(),
        };
        search(&self.index, &query, ctx)
            .unwrap_or_else(|e| panic!("query {:?}: {e}", case.query))
            .into_iter()
            .map(|r| Hit {
                path: r.path,
                line_start: r.line_start,
                line_end: r.line_end,
                score: r.score,
            })
            .collect()
    }
}

/// Run every case, print the per-case table, return the means.
fn evaluate(engine: &Engine, cases: &[Case]) -> Scores {
    let mut sum = Scores::default();
    println!(
        "{:<36} {:>8} {:>6} {:>9}  probe",
        "query", "nDCG@10", "MRR", "recall@10"
    );
    let show = std::env::var_os("FT_SEARCH_EVAL_SHOW").is_some();
    for case in cases {
        let hits = engine.run(case);
        let s = score(&hits, &case.relevant);
        let q = if case.any {
            format!("{} (any)", case.query)
        } else {
            case.query.clone()
        };
        println!(
            "{:<36} {:>8.3} {:>6.3} {:>9.3}  {}",
            q, s.ndcg, s.mrr, s.recall, case.note
        );
        if show {
            let g = gains(&hits, &case.relevant);
            for (h, grade) in hits.iter().zip(&g).take(CUTOFF) {
                println!(
                    "    [{grade}] {:>7.3}  {} L{}-{}",
                    h.score,
                    h.path.display(),
                    h.line_start,
                    h.line_end
                );
            }
        }
        sum.ndcg += s.ndcg;
        sum.mrr += s.mrr;
        sum.recall += s.recall;
    }
    let n = cases.len().max(1) as f64;
    let mean = Scores {
        ndcg: sum.ndcg / n,
        mrr: sum.mrr / n,
        recall: sum.recall / n,
    };
    println!(
        "{:<36} {:>8.3} {:>6.3} {:>9.3}  ({} cases)",
        "MEAN",
        mean.ndcg,
        mean.mrr,
        mean.recall,
        cases.len()
    );
    mean
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/search-eval")
}

#[derive(Debug, Deserialize)]
struct Baseline {
    ndcg10: f64,
    mrr: f64,
}

#[test]
fn fixture_relevance_ratchet() {
    std::env::set_var("FT_TODAY", "2026-10-23");
    let dir = fixture_dir();
    let cases = parse_judgments(&std::fs::read_to_string(dir.join("judgments.toml")).unwrap())
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(cases.len() >= 25, "the fixture needs at least 25 cases");

    let presets: HashMap<String, String> = [(
        "know-notes".to_string(),
        r#"node where path starts_with "know/";"#.to_string(),
    )]
    .into();
    let engine = Engine::build(&dir.join("vault"), &[], &SearchCfg::default(), presets);
    let mean = evaluate(&engine, &cases);

    let baseline: Baseline =
        toml::from_str(&std::fs::read_to_string(dir.join("baseline.toml")).unwrap()).unwrap();
    assert!(
        mean.ndcg + 1e-9 >= baseline.ndcg10,
        "mean nDCG@10 {:.4} fell below the baseline threshold {:.4}",
        mean.ndcg,
        baseline.ndcg10
    );
    assert!(
        mean.mrr + 1e-9 >= baseline.mrr,
        "mean MRR {:.4} fell below the baseline threshold {:.4}",
        mean.mrr,
        baseline.mrr
    );
}

#[test]
fn fixture_covers_every_probe_class() {
    let cases =
        parse_judgments(&std::fs::read_to_string(fixture_dir().join("judgments.toml")).unwrap())
            .unwrap();
    for class in [
        "length-norm",
        "idf",
        "context-title",
        "context-alias",
        "context-heading",
        "context-tag",
        "diacritics-pt",
        "diacritics-en",
        "stemming",
        "filter-path",
        "filter-tag",
        "filter-scope",
        "near",
        "regression-phrase",
        "regression-link",
        "regression-fuzzy",
        "regression-exclude",
    ] {
        assert!(
            cases.iter().any(|c| c.note.starts_with(class)),
            "no case probes {class}"
        );
    }
}

/// Report-only run against a private vault and judgments file.
#[test]
fn real_vault_relevance_report() {
    if std::env::var("FT_REAL_VAULT_TESTS").as_deref() != Ok("1") {
        return;
    }
    let (Some(vault), Some(judgments)) = (
        std::env::var_os("FT_VAULT"),
        std::env::var_os("FT_SEARCH_EVAL_JUDGMENTS"),
    ) else {
        return;
    };
    let vault = ft_core::vault::Vault::discover(Some(PathBuf::from(vault))).unwrap();
    let cases = parse_judgments(&std::fs::read_to_string(judgments).unwrap())
        .unwrap_or_else(|e| panic!("{e}"));
    let cfg = &vault.config.config;
    let mut engine = Engine::build(
        &vault.path,
        &cfg.synth.exclude_prefixes,
        &cfg.search,
        cfg.graph.presets.clone(),
    );
    println!("── recency from file mtimes");
    evaluate(&engine, &cases);

    // Blame recency from the vault's cache; `FT_SEARCH_EVAL_WARM=1`
    // blames every uncached file first (writes `.ft/cache/blame.msgpack`).
    if let Some(validity) = BlameValidity::snapshot(&vault.path).unwrap() {
        let mut cache = BlameCache::load(&vault.path).unwrap();
        if std::env::var("FT_SEARCH_EVAL_WARM").as_deref() == Ok("1") {
            let n = warm_blame(
                &engine.index,
                &vault.path,
                &validity,
                &mut cache,
                || true,
                |_, _| {},
            );
            println!("── warmed {n} files");
        }
        let table = RecencyTable::build(&engine.index, &cache, &validity);
        println!(
            "── recency from blame ({} of {} paragraphs covered)",
            table.covered(),
            engine.index.paragraph_count()
        );
        engine.recency = Some(table);
        evaluate(&engine, &cases);
    }
}

// ── Metric unit tests ────────────────────────────────────────────────

fn hit(path: &str, a: u32, b: u32) -> Hit {
    Hit {
        path: PathBuf::from(path),
        line_start: a,
        line_end: b,
        score: 0.0,
    }
}

fn judged(path: &str, line: u32, grade: u8) -> Judged {
    Judged {
        path: PathBuf::from(path),
        line,
        grade,
    }
}

#[test]
fn perfect_ranking_scores_one() {
    let rel = [judged("a.md", 3, 3), judged("b.md", 1, 1)];
    let s = score(&[hit("a.md", 3, 4), hit("b.md", 1, 1)], &rel);
    assert!((s.ndcg - 1.0).abs() < 1e-12);
    assert_eq!(s.mrr, 1.0);
    assert_eq!(s.recall, 1.0);
}

#[test]
fn swapped_ranking_matches_hand_computed_ndcg() {
    // Ideal: 7/log2(2) + 1/log2(3) = 7.6309; actual: 1 + 7/log2(3) = 5.4165.
    let rel = [judged("a.md", 3, 3), judged("b.md", 1, 1)];
    let s = score(&[hit("b.md", 1, 1), hit("a.md", 3, 4)], &rel);
    let expected = (1.0 + 7.0 / 3f64.log2()) / (7.0 + 1.0 / 3f64.log2());
    assert!((s.ndcg - expected).abs() < 1e-12);
    assert_eq!(s.mrr, 1.0);
}

#[test]
fn mrr_uses_first_judged_rank() {
    let rel = [judged("a.md", 1, 2)];
    let s = score(
        &[hit("x.md", 1, 1), hit("y.md", 1, 1), hit("a.md", 1, 1)],
        &rel,
    );
    assert!((s.mrr - 1.0 / 3.0).abs() < 1e-12);
}

#[test]
fn recall_counts_top_ten_only() {
    let rel = [judged("a.md", 1, 1), judged("b.md", 1, 1)];
    let mut hits: Vec<Hit> = (0..10).map(|i| hit(&format!("x{i}.md"), 1, 1)).collect();
    hits.insert(0, hit("a.md", 1, 1));
    hits.push(hit("b.md", 1, 1)); // rank 12: outside the cutoff
    let s = score(&hits, &rel);
    assert_eq!(s.recall, 0.5);
}

#[test]
fn range_containment_matches_judgment() {
    let rel = [judged("notes/a.md", 12, 3)];
    let s = score(&[hit("notes/a.md", 10, 14)], &rel);
    assert_eq!(s.recall, 1.0);
}

#[test]
fn judged_paragraph_counts_once() {
    let rel = [judged("a.md", 2, 3)];
    let g = gains(&[hit("a.md", 1, 3), hit("a.md", 1, 3)], &rel);
    assert_eq!(g, vec![3, 0]);
}

#[test]
fn no_results_scores_zero() {
    let s = score(&[], &[judged("a.md", 1, 2)]);
    assert_eq!(s.ndcg, 0.0);
    assert_eq!(s.mrr, 0.0);
    assert_eq!(s.recall, 0.0);
}

#[test]
fn invalid_grade_names_the_case() {
    let err = parse_judgments(
        "[[case]]\nquery = \"a\"\n[[case.relevant]]\npath = \"a.md\"\nline = 1\ngrade = 1\n\
         [[case]]\nquery = \"b\"\n[[case.relevant]]\npath = \"b.md\"\nline = 1\ngrade = 5\n",
    )
    .unwrap_err();
    assert!(err.contains("case 1"), "{err}");
    assert!(err.contains("1–3"), "{err}");
}

#[test]
fn missing_field_names_the_case() {
    let err =
        parse_judgments("[[case]]\n[[case.relevant]]\npath = \"a.md\"\nline = 1\ngrade = 1\n")
            .unwrap_err();
    assert!(err.contains("case 0") && err.contains("query"), "{err}");
}
