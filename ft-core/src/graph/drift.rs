//! Drift detection: one concept silently split across several
//! `[[spellings]]` (`[[onboarding]]`, `[[onboarding-flow]]`, …).
//!
//! Three signals per candidate pair:
//!
//! - **Name similarity** gates the pair space: normalized-token
//!   containment/overlap plus a small edit distance. Cheap, so the
//!   O(n²) phase never touches the graph.
//! - **Neighborhood overlap** confirms: drifted siblings share
//!   co-occurrence neighbors (profiles from
//!   [`crate::related::score_related`], each side excluded from the
//!   other's profile).
//! - **Direct co-occurrence** vetoes: nobody writes both spellings in
//!   one sentence, so concepts that appear together in the same
//!   paragraph are related-but-distinct — the count divides the score.
//!
//! Ranking multiplies in the combined mention weight so high-stakes
//! splits surface first. Detection is read-only; the produced
//! suggestions are text for the user to run.

use std::collections::{HashMap, HashSet};

use crate::graph::{Graph, NodeKind, NoteId};
use crate::vault::Vault;

/// Minimum name similarity for a pair to be scored at all. Strictly
/// above 0.5 so two-token names sharing one generic token
/// (`beta-flow` / `gamma-flow` → containment 0.5) don't pair, while
/// compound drift (`onboarding` ⊂ `onboarding-flow` → 1.0) and typo
/// drift (high edit-similarity) do.
pub const NAME_SIMILARITY_GATE: f64 = 0.55;

/// One side of a drift pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftSide {
    pub id: NoteId,
    /// Display/rename target: the note's filename stem, or the ghost's
    /// raw target string.
    pub target: String,
    pub is_ghost: bool,
    /// Distinct paragraphs mentioning the concept.
    pub mentions: usize,
}

/// One ranked drift candidate. `keeper` is the side the suggestion
/// folds *into*: a real note always beats a ghost; otherwise the
/// higher-mentioned side wins (alphabetical tiebreak).
#[derive(Debug, Clone)]
pub struct DriftCandidate {
    pub keeper: DriftSide,
    pub lesser: DriftSide,
    pub name_similarity: f64,
    pub neighborhood_overlap: f64,
    /// Paragraphs mentioning both sides.
    pub direct_cooccurrence: usize,
    pub score: f64,
    /// Ready-to-run resolution text (never executed by ft).
    pub suggestion: String,
}

/// Detect likely drift pairs across every mentioned concept (notes and
/// ghosts), ranked most-severe first.
pub fn detect_drift(graph: &Graph, vault: &Vault) -> Vec<DriftCandidate> {
    // Concept universe: notes + ghosts with ≥1 distinct-paragraph
    // mention, with their mention-paragraph sets (needed again for the
    // direct-co-occurrence count).
    struct Concept {
        id: NoteId,
        target: String,
        is_ghost: bool,
        tokens: Vec<String>,
        joined: String,
        paragraphs: HashSet<NoteId>,
    }
    let mut concepts: Vec<Concept> = Vec::new();
    for (id, node) in graph.nodes() {
        let (target, is_ghost) = match node {
            NodeKind::Note(n) => (
                n.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| n.path.to_string_lossy().into_owned()),
                false,
            ),
            NodeKind::Ghost(g) => (g.raw.clone(), true),
            _ => continue,
        };
        let paragraphs: HashSet<NoteId> = graph
            .mentions_of(id)
            .into_iter()
            .filter(|(src, _)| matches!(graph.node(*src), NodeKind::Paragraph(_)))
            .map(|(src, _)| src)
            .collect();
        if paragraphs.is_empty() {
            continue;
        }
        let tokens = normalize_tokens(&target);
        if tokens.is_empty() {
            continue;
        }
        let joined = tokens.concat();
        concepts.push(Concept {
            id,
            target,
            is_ghost,
            tokens,
            joined,
            paragraphs,
        });
    }

    // Lazily-memoized co-occurrence profiles for gated pairs only.
    let mut profiles: HashMap<NoteId, HashMap<NoteId, u32>> = HashMap::new();
    let mut profile_of = |graph: &Graph, id: NoteId| -> HashMap<NoteId, u32> {
        profiles
            .entry(id)
            .or_insert_with(|| {
                crate::related::score_related(graph, id, vault)
                    .map(|rows| rows.into_iter().map(|r| (r.note_id, r.score)).collect())
                    .unwrap_or_default()
            })
            .clone()
    };

    let mut out: Vec<DriftCandidate> = Vec::new();
    for i in 0..concepts.len() {
        for j in (i + 1)..concepts.len() {
            let (a, b) = (&concepts[i], &concepts[j]);
            let name_similarity = name_similarity(&a.tokens, &a.joined, &b.tokens, &b.joined);
            if name_similarity < NAME_SIMILARITY_GATE {
                continue;
            }

            let mut pa = profile_of(graph, a.id);
            let mut pb = profile_of(graph, b.id);
            pa.remove(&b.id);
            pb.remove(&a.id);
            let neighborhood_overlap = weighted_jaccard(&pa, &pb);
            let direct_cooccurrence = a.paragraphs.intersection(&b.paragraphs).count();
            let weight = a.paragraphs.len() + b.paragraphs.len();

            // Signal product × stakes, divided by the co-occurrence
            // veto. The 0.25 floor keeps strong-name/no-neighbor pairs
            // visible (young vaults have thin profiles) while ranking
            // them below neighbor-confirmed ones.
            let score =
                name_similarity * (0.25 + neighborhood_overlap) * (1.0 + (weight as f64)).ln()
                    / (1.0 + direct_cooccurrence as f64);

            let side = |c: &Concept| DriftSide {
                id: c.id,
                target: c.target.clone(),
                is_ghost: c.is_ghost,
                mentions: c.paragraphs.len(),
            };
            // Keeper: a real note beats a ghost; otherwise more
            // mentions win; alphabetical tiebreak for determinism.
            let a_keeps = match (a.is_ghost, b.is_ghost) {
                (false, true) => true,
                (true, false) => false,
                _ => match a.paragraphs.len().cmp(&b.paragraphs.len()) {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Less => false,
                    std::cmp::Ordering::Equal => a.target <= b.target,
                },
            };
            let (keeper, lesser) = if a_keeps {
                (side(a), side(b))
            } else {
                (side(b), side(a))
            };
            let suggestion = if keeper.is_ghost || lesser.is_ghost {
                format!(
                    "merge: ft notes rename \"[[{}]]\" \"{}\"",
                    lesser.target, keeper.target
                )
            } else {
                format!(
                    "alias: list [[{}]] under {}.md → ## Related (content merge is manual)",
                    lesser.target, keeper.target
                )
            };

            out.push(DriftCandidate {
                keeper,
                lesser,
                name_similarity,
                neighborhood_overlap,
                direct_cooccurrence,
                score,
                suggestion,
            });
        }
    }

    out.sort_by(|x, y| {
        y.score
            .partial_cmp(&x.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.keeper.target.cmp(&y.keeper.target))
            .then_with(|| x.lesser.target.cmp(&y.lesser.target))
    });
    out
}

/// Lowercase, strip `.md`, split on whitespace / `-` / `_` / `/`, trim
/// a trailing plural `s` from tokens long enough for that to be safe.
fn normalize_tokens(name: &str) -> Vec<String> {
    let lowered = name.to_lowercase();
    let stem = lowered.strip_suffix(".md").unwrap_or(&lowered);
    stem.split([' ', '\t', '-', '_', '/'])
        .filter(|t| !t.is_empty())
        .map(|t| {
            if t.len() > 3 && t.ends_with('s') {
                t[..t.len() - 1].to_string()
            } else {
                t.to_string()
            }
        })
        .collect()
}

/// Max of token containment (|∩| / min — compound drift scores 1.0),
/// token Jaccard, and normalized edit similarity on the joined tokens
/// (typo drift).
fn name_similarity(
    tokens_a: &[String],
    joined_a: &str,
    tokens_b: &[String],
    joined_b: &str,
) -> f64 {
    let sa: HashSet<&String> = tokens_a.iter().collect();
    let sb: HashSet<&String> = tokens_b.iter().collect();
    let inter = sa.intersection(&sb).count() as f64;
    let containment = inter / (sa.len().min(sb.len()) as f64);
    let jaccard = inter / (sa.union(&sb).count() as f64);
    let lev = levenshtein(joined_a, joined_b) as f64;
    let max_len = joined_a.chars().count().max(joined_b.chars().count()) as f64;
    let edit_sim = if max_len == 0.0 {
        0.0
    } else {
        1.0 - lev / max_len
    };
    containment.max(jaccard).max(edit_sim)
}

/// Classic two-row Levenshtein over chars — small inputs only (concept
/// names), so no need for a dependency.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Weighted Jaccard over two score maps: Σ min / Σ max across the key
/// union. 0.0 when both are empty.
fn weighted_jaccard(a: &HashMap<NoteId, u32>, b: &HashMap<NoteId, u32>) -> f64 {
    let keys: HashSet<&NoteId> = a.keys().chain(b.keys()).collect();
    if keys.is_empty() {
        return 0.0;
    }
    let (mut num, mut den) = (0u64, 0u64);
    for k in keys {
        let va = u64::from(*a.get(k).unwrap_or(&0));
        let vb = u64::from(*b.get(k).unwrap_or(&0));
        num += va.min(vb);
        den += va.max(vb);
    }
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;

    fn vault_graph(files: &[(&str, &str)]) -> (assert_fs::TempDir, Vault, Graph) {
        let tmp = assert_fs::TempDir::new().unwrap();
        tmp.child(".obsidian").create_dir_all().unwrap();
        for (name, content) in files {
            tmp.child(name).write_str(content).unwrap();
        }
        let v = Vault::discover(Some(tmp.path().to_path_buf())).unwrap();
        let g = Graph::build(&v, &v.scan()).unwrap();
        (tmp, v, g)
    }

    fn pair<'a>(cands: &'a [DriftCandidate], x: &str, y: &str) -> Option<&'a DriftCandidate> {
        cands.iter().find(|c| {
            (c.keeper.target == x && c.lesser.target == y)
                || (c.keeper.target == y && c.lesser.target == x)
        })
    }

    #[test]
    fn compound_name_drift_detected_dissimilar_not() {
        let (_t, v, g) = vault_graph(&[(
            "a.md",
            "[[onboarding]] and [[activation]] together.\n\n\
             [[onboarding-flow]] and [[activation]] together.\n",
        )]);
        let cands = detect_drift(&g, &v);
        let c = pair(&cands, "onboarding", "onboarding-flow")
            .expect("compound-name pair must be detected");
        assert!(c.name_similarity >= NAME_SIMILARITY_GATE);
        assert_eq!(c.direct_cooccurrence, 0);
        assert!(
            c.neighborhood_overlap > 0.0,
            "shared [[activation]] neighbor should overlap: {c:?}"
        );
        assert!(
            pair(&cands, "onboarding", "activation").is_none(),
            "dissimilar names must never pair"
        );
    }

    #[test]
    fn cooccurring_near_names_rank_below_true_drift() {
        let (_t, v, g) = vault_graph(&[(
            "a.md",
            "[[beta]] with [[koala]].\n\n\
             [[beta-max]] with [[koala]].\n\n\
             [[gamma]] and [[gamma-max]] with [[koala]].\n\n\
             [[gamma]] and [[gamma-max]] again with [[koala]].\n",
        )]);
        let cands = detect_drift(&g, &v);
        let a = pair(&cands, "beta", "beta-max").expect("true drift pair");
        let b = pair(&cands, "gamma", "gamma-max").expect("co-occurring pair");
        assert_eq!(b.direct_cooccurrence, 2);
        assert!(
            a.score > b.score,
            "never-co-occurring pair must outrank the co-occurring one:\nA={a:?}\nB={b:?}"
        );
    }

    #[test]
    fn stakes_raise_rank() {
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("[[delta]] mention {i} with [[zebra]].\n\n"));
            content.push_str(&format!("[[delta-max]] mention {i} with [[zebra]].\n\n"));
        }
        content.push_str("[[eps]] once with [[zebra]].\n\n[[eps-max]] once with [[zebra]].\n");
        let (_t, v, g) = vault_graph(&[("a.md", &content)]);
        let cands = detect_drift(&g, &v);
        let heavy = pair(&cands, "delta", "delta-max").expect("heavy pair");
        let light = pair(&cands, "eps", "eps-max").expect("light pair");
        assert!(
            heavy.score > light.score,
            "heavier split must rank first:\nheavy={heavy:?}\nlight={light:?}"
        );
    }

    #[test]
    fn note_is_keeper_over_ghost_with_merge_suggestion() {
        let (_t, v, g) = vault_graph(&[
            ("onboarding.md", "# onboarding\n"),
            (
                "a.md",
                "[[onboarding]] here with [[zebra]].\n\n\
                 [[onboarding-flow]] there with [[zebra]].\n",
            ),
        ]);
        let cands = detect_drift(&g, &v);
        let c = pair(&cands, "onboarding", "onboarding-flow").expect("pair");
        assert!(!c.keeper.is_ghost, "note must keep: {c:?}");
        assert_eq!(c.keeper.target, "onboarding");
        assert!(c.lesser.is_ghost);
        assert_eq!(
            c.suggestion,
            "merge: ft notes rename \"[[onboarding-flow]]\" \"onboarding\""
        );
    }

    #[test]
    fn heavier_ghost_is_keeper() {
        let (_t, v, g) = vault_graph(&[(
            "a.md",
            "[[busy]] one with [[zebra]].\n\n[[busy]] two with [[zebra]].\n\n\
             [[busy-max]] once with [[zebra]].\n",
        )]);
        let cands = detect_drift(&g, &v);
        let c = pair(&cands, "busy", "busy-max").expect("pair");
        assert_eq!(c.keeper.target, "busy");
        assert_eq!(c.keeper.mentions, 2);
        assert!(c
            .suggestion
            .starts_with("merge: ft notes rename \"[[busy-max]]\""));
    }

    #[test]
    fn note_pair_gets_alias_advice() {
        let (_t, v, g) = vault_graph(&[
            ("onboarding.md", "# onboarding\n"),
            ("onboarding-flow.md", "# onboarding-flow\n"),
            (
                "a.md",
                "[[onboarding]] and [[zebra]].\n\n[[onboarding-flow]] and [[zebra]].\n\n\
                 [[onboarding]] again with [[zebra]].\n",
            ),
        ]);
        let cands = detect_drift(&g, &v);
        let c = pair(&cands, "onboarding", "onboarding-flow").expect("pair");
        assert!(!c.keeper.is_ghost && !c.lesser.is_ghost);
        assert!(
            c.suggestion
                .starts_with("alias: list [[onboarding-flow]] under onboarding.md"),
            "{}",
            c.suggestion
        );
        assert!(!c.suggestion.contains("rename"), "{}", c.suggestion);
    }

    #[test]
    fn clean_vault_is_empty() {
        let (_t, v, g) =
            vault_graph(&[("a.md", "[[onboarding]] here.\n\n[[activation]] there.\n")]);
        assert!(detect_drift(&g, &v).is_empty());
    }
}
