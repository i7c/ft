//! Paragraph search index built from a vault `Scan`.
//!
//! The index is a sibling of `Graph::build` over the same scan: no
//! graph, no git, no re-reads. It is immutable after build — the TUI
//! holds one instance in the shared snapshot and rebuilds it when the
//! scan generation changes.
//!
//! Two structures back matching:
//!
//! - a **token dictionary** (case-folded alphanumeric runs plus
//!   `[[…]]` link targets) with sorted postings per token — the fast
//!   path for word, link, and fuzzy clauses;
//! - a **trigram map** over the dictionary for fuzzy candidate
//!   generation (a query-side trigram intersection narrows the
//!   levenshtein verification to plausible tokens).
//!
//! Substring and phrase clauses scan the folded paragraph text
//! directly; at vault scale that is sub-millisecond, so no extra
//! structure is built for them.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::scan::Scan;
use crate::synth::callout::path_excluded;

/// One searchable paragraph: the same grain the scaffold pins.
#[derive(Debug, Clone)]
pub struct ParagraphDoc {
    /// Vault-relative source path.
    pub path: PathBuf,
    /// 1-indexed line of the paragraph's first line.
    pub line_start: u32,
    /// 1-indexed line of the paragraph's last line.
    pub line_end: u32,
    /// Verbatim paragraph text.
    pub text: String,
    /// Case-folded copy, the matching surface for substring/phrase.
    pub(crate) folded: String,
}

/// The immutable search index.
#[derive(Debug, Default)]
pub struct SearchIndex {
    pub(crate) docs: Vec<ParagraphDoc>,
    /// Dictionary token → token id.
    pub(crate) token_ids: HashMap<String, u32>,
    /// Token id → folded dictionary token (parallel to `postings`).
    pub(crate) tokens: Vec<String>,
    /// Token id → sorted, deduped paragraph ids containing the token
    /// (word tokens, prose occurrences, and link targets alike).
    pub(crate) postings: Vec<Vec<u32>>,
    /// Link-target token → sorted, deduped paragraph ids containing a
    /// `[[…]]` span for that target. The link clause reads only this;
    /// word clauses read `postings`, so a prose-only occurrence never
    /// satisfies `[[foo]]`.
    pub(crate) link_postings: HashMap<String, Vec<u32>>,
    /// Trigram → token ids whose token contains the trigram.
    pub(crate) trigrams: HashMap<String, Vec<u32>>,
}

impl SearchIndex {
    /// Build the index from a scan, skipping files whose vault-relative
    /// path starts with any `exclude_prefixes` entry (the same
    /// `synth.exclude_prefixes` filter pulse uses).
    pub fn build(scan: &Scan, exclude_prefixes: &[String]) -> SearchIndex {
        let mut index = SearchIndex::default();
        for pf in &scan.files {
            if path_excluded(&pf.rel, exclude_prefixes) {
                continue;
            }
            for p in &pf.paragraphs {
                // Quoted material inside `[!ft-source]` protected
                // sections is not searchable: it is a copy of another
                // note's prose, and re-sourcing it would let a synth
                // note pin itself (pulse's recycling exclusion).
                if is_protected_callout(&p.text) {
                    continue;
                }
                let doc_id = index.docs.len() as u32;
                let folded = p.text.to_lowercase();
                index.docs.push(ParagraphDoc {
                    path: pf.rel.clone(),
                    line_start: p.line_start,
                    line_end: p.line_end,
                    text: p.text.clone(),
                    folded,
                });
                for (token, is_link) in tokenize(&index.docs[doc_id as usize].folded) {
                    index.add_token(doc_id, token, is_link);
                }
            }
        }
        index.finish_trigrams();
        index
    }

    /// Number of indexed paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.docs.len()
    }

    /// All indexed documents (iteration for substring scans).
    pub(crate) fn docs(&self) -> &[ParagraphDoc] {
        &self.docs
    }

    fn add_token(&mut self, doc_id: u32, token: String, is_link: bool) {
        let next_id = self.tokens.len() as u32;
        let id = *self.token_ids.entry(token.clone()).or_insert_with(|| {
            self.tokens.push(token.clone());
            self.postings.push(Vec::new());
            next_id
        });
        self.postings[id as usize].push(doc_id);
        if is_link {
            self.link_postings.entry(token).or_default().push(doc_id);
        }
    }

    /// Dedup + sort postings, build the trigram map. Postings are
    /// appended in doc order per token, so sorting is only needed for
    /// dedup (a paragraph mentions a token at most once per token).
    fn finish_trigrams(&mut self) {
        for p in self.postings.iter_mut() {
            p.sort_unstable();
            p.dedup();
        }
        for p in self.link_postings.values_mut() {
            p.sort_unstable();
            p.dedup();
        }
        for (id, token) in self.tokens.iter().enumerate() {
            for trig in trigrams_of(token) {
                self.trigrams.entry(trig).or_default().push(id as u32);
            }
        }
        for ids in self.trigrams.values_mut() {
            ids.sort_unstable();
            ids.dedup();
        }
    }
}

/// Tokenize folded paragraph text into `(folded token, is_link)` pairs.
/// Alphanumeric runs are words; `[[…]]` spans contribute their
/// anchor-stripped / alias-resolved target as a link token. Link tokens
/// are also added as plain dictionary entries, so a word clause
/// (`=foo`) matches `[[foo]]` mentions and a link clause restricts to
/// the bracket form.
fn tokenize(folded: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let bytes = folded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' && bytes.get(i + 1) == Some(&b'[') {
            if let Some(end) = folded[i + 2..].find("]]") {
                let content = &folded[i + 2..i + 2 + end];
                if let Some(target) = link_target(content) {
                    out.push((target, true));
                }
                i += 2 + end + 2;
                continue;
            }
        }
        if bytes[i].is_ascii_alphanumeric() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            out.push((folded[start..i].to_string(), false));
        } else {
            i += 1;
        }
    }
    out
}

/// Fold `[[…]]` content (already case-folded) into its match target:
/// strip `#anchor`, use the alias after `|`. `None` for empty content
/// (`[[]]` or `[[#x]]`).
fn link_target(content: &str) -> Option<String> {
    let no_anchor = content.split('#').next().unwrap_or(content);
    let target = no_anchor.rsplit('|').next().unwrap_or(no_anchor);
    let target = target.trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// All 3-grams of a folded term (length ≥ 3).
fn trigrams_of(term: &str) -> Vec<String> {
    if term.len() < 3 {
        return Vec::new();
    }
    let bytes = term.as_bytes();
    (0..=bytes.len() - 3)
        .map(|i| term[i..i + 3].to_string())
        .collect()
}

/// True when the paragraph is a `[!ft-source]` protected-section
/// callout block (one contiguous `> ` block containing the marker
/// line). Such paragraphs are quoted source material, not user prose.
fn is_protected_callout(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with("> [!ft-source]"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn build_scan(dir: &std::path::Path, files: &[(&str, &str)]) -> Scan {
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
        crate::scan::scan_vault(dir, &[])
    }

    #[test]
    fn index_covers_every_paragraph() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(
            tmp.path(),
            &[
                ("a.md", "One paragraph.\n\nSecond paragraph.\n"),
                ("b.md", "Solo.\n"),
            ],
        );
        let idx = SearchIndex::build(&scan, &[]);
        assert_eq!(idx.paragraph_count(), 3);
        // Scan order is not guaranteed to be alphabetical; find by path.
        let a = idx.docs.iter().find(|d| d.path == *"a.md").unwrap();
        assert_eq!(a.line_start, 1);
        assert_eq!(a.line_end, 1);
        assert_eq!(a.text, "One paragraph.");
        let a2 = idx
            .docs
            .iter()
            .find(|d| d.path == *"a.md" && d.line_start == 3)
            .unwrap();
        assert_eq!(a2.text, "Second paragraph.");
    }

    #[test]
    fn link_tokens_extracted_with_anchor_and_alias() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(
            tmp.path(),
            &[(
                "a.md",
                "See [[Foo Bar]], [[Baz#Section]], and [[Qux|Alias]].\n",
            )],
        );
        let idx = SearchIndex::build(&scan, &[]);
        // The three link targets are dictionary tokens and link postings.
        for tok in ["foo bar", "baz", "alias"] {
            assert!(idx.token_ids.contains_key(tok), "missing token {tok:?}");
            assert!(
                idx.link_postings.contains_key(tok),
                "missing link postings for {tok:?}"
            );
        }
    }

    #[test]
    fn word_tokens_and_link_tokens_share_the_dictionary() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(tmp.path(), &[("a.md", "Prose mentions [[memoization]].\n")]);
        let idx = SearchIndex::build(&scan, &[]);
        // "memoization" has a dictionary entry (from the link target) —
        // reachable from a word clause via the shared token postings,
        // and from a link clause via the link postings.
        assert!(idx.token_ids.contains_key("memoization"));
        assert_eq!(idx.link_postings["memoization"], vec![0]);
        // Prose-only occurrence stays out of the link postings.
        let scan = build_scan(tmp.path(), &[("a.md", "prose memoization only\n")]);
        let idx = SearchIndex::build(&scan, &[]);
        assert!(idx.token_ids.contains_key("memoization"));
        assert!(!idx.link_postings.contains_key("memoization"));
    }

    #[test]
    fn exclude_prefixes_skip_whole_files() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(
            tmp.path(),
            &[
                ("notes/a.md", "Keep me.\n"),
                ("journal/2026-05-08.md", "Drop me.\n"),
            ],
        );
        let idx = SearchIndex::build(&scan, &["journal/".to_string()]);
        assert_eq!(idx.paragraph_count(), 1);
        assert_eq!(idx.docs[0].path, PathBuf::from("notes/a.md"));
    }

    #[test]
    fn postings_dedup_same_token_once_per_paragraph() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(tmp.path(), &[("a.md", "foo foo foo.\n")]);
        let idx = SearchIndex::build(&scan, &[]);
        let id = idx.token_ids["foo"];
        assert_eq!(idx.postings[id as usize], vec![0]);
    }

    #[test]
    fn trigram_map_has_candidates() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(tmp.path(), &[("a.md", "memoization helps.\n")]);
        let idx = SearchIndex::build(&scan, &[]);
        assert!(idx.trigrams.contains_key("mem"));
        assert!(idx.trigrams.contains_key("zat"));
    }

    #[test]
    fn no_paragraphs_in_empty_scan() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = crate::scan::scan_vault(tmp.path(), &[]);
        let idx = SearchIndex::build(&scan, &[]);
        assert_eq!(idx.paragraph_count(), 0);
    }

    #[test]
    fn protected_callout_bodies_are_not_indexed() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(
            tmp.path(),
            &[(
                "Synthesis/topic.md",
                "---\nft:\n  synth:\n    enabled: true\n---\n\n\
                 User prose between callouts is searchable.\n\n\
                 > [!ft-source] \"notes/source.md\" L1-1 @abc1234 #7f3a91\n\
                 > Quoted material must not be indexed.\n",
            )],
        );
        let idx = SearchIndex::build(&scan, &[]);
        assert_eq!(
            idx.paragraph_count(),
            1,
            "only the prose paragraph is indexed"
        );
        assert_eq!(
            idx.docs[0].text,
            "User prose between callouts is searchable."
        );
    }
}
