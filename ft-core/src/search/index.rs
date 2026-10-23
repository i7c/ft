//! Paragraph search index built from a vault `Scan`.
//!
//! The index is a sibling of `Graph::build` over the same scan: no git,
//! no re-reads. It is immutable after build — the TUI holds one instance
//! in the shared snapshot and rebuilds it when the scan generation
//! changes.
//!
//! Every paragraph is a document with a **body** field plus **context
//! fields** it inherits: the note's title, aliases, and frontmatter tags
//! (stored once per note) and the enclosing heading path (stored once
//! per heading section). Matching structures:
//!
//! - a **token dictionary** (folded word tokens from every field) with
//!   per-token postings carrying `(field, unit, tf)`, where the unit is a
//!   doc, note, or section id depending on the field;
//! - **link postings** for `[[…]]` targets in paragraph bodies;
//! - a **trigram map** over the dictionary for fuzzy and substring
//!   candidate generation;
//! - one **stem map** per configured Snowball algorithm.
//!
//! Each note also carries a bounded ranking prior (in-degree and relative
//! recency) and its undirected note-link adjacency for `near:` boosts,
//! both derived from the scan's links with the graph's own resolution
//! rules ([`crate::graph::resolve`]) — no graph build needed.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rust_stemmers::Algorithm;

use crate::config::SearchCfg;
use crate::graph::resolve::{resolve_md, resolve_wiki, Resolution, ScanNoteIndex};
use crate::graph::LinkForm;
use crate::scan::Scan;
use crate::search::text::{self, TokenKind};
use crate::synth::callout::path_excluded;

/// Hash map with a fast, fixed (deterministic) hasher for the index's
/// hot paths: token lookups and per-query score accumulation.
pub(crate) type FastMap<K, V> = HashMap<K, V, foldhash::fast::FixedState>;

/// A field of a paragraph document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    /// The paragraph text itself.
    Body = 0,
    /// The note's file stem.
    Title = 1,
    /// The note's frontmatter aliases.
    Alias = 2,
    /// The headings enclosing the paragraph.
    Heading = 3,
    /// The note's frontmatter tags.
    Tag = 4,
}

pub(crate) const FIELDS: [Field; 5] = [
    Field::Body,
    Field::Title,
    Field::Alias,
    Field::Heading,
    Field::Tag,
];

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Body => "body",
            Field::Title => "title",
            Field::Alias => "alias",
            Field::Heading => "heading",
            Field::Tag => "tag",
        }
    }

    pub(crate) fn bit(self) -> u8 {
        1 << self as u8
    }
}

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
    /// Folded copy, the matching surface for multi-token substring and
    /// phrase clauses.
    pub(crate) folded: String,
    /// Owning note.
    pub(crate) note: u32,
    /// Owning heading section.
    pub(crate) section: u32,
    /// Body length in word tokens.
    pub(crate) len: u32,
    /// Inline `#tags` written in the paragraph (folded, no `#`).
    pub(crate) tags: Vec<String>,
}

/// Per-note context shared by the note's paragraphs.
#[derive(Debug, Clone)]
pub(crate) struct NoteCtx {
    pub(crate) folded_path: String,
    /// File stem, as written.
    pub(crate) title: String,
    pub(crate) folded_title: String,
    pub(crate) aliases: Vec<String>,
    /// Frontmatter tags (folded, no `#`).
    pub(crate) tags: Vec<String>,
    pub(crate) docs: Range<u32>,
    /// Word-token lengths of the title, alias, and tag fields.
    pub(crate) title_len: u32,
    pub(crate) alias_len: u32,
    pub(crate) tag_len: u32,
    /// Ranking prior in `[1.0, 1.25]`.
    pub(crate) prior: f64,
    pub(crate) mtime: SystemTime,
}

/// A run of consecutive paragraphs sharing one heading path.
#[derive(Debug, Clone)]
pub(crate) struct SectionCtx {
    /// Enclosing headings, outermost first, as written.
    pub(crate) heading_path: Vec<String>,
    /// The folded heading texts joined by spaces.
    pub(crate) folded: String,
    pub(crate) docs: Range<u32>,
    pub(crate) len: u32,
}

/// One postings entry: `unit` is a doc id (body), note id (title, alias,
/// tag), or section id (heading).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Posting {
    pub(crate) field: Field,
    pub(crate) unit: u32,
    pub(crate) tf: u16,
}

/// The immutable search index.
#[derive(Debug, Default)]
pub struct SearchIndex {
    pub(crate) docs: Vec<ParagraphDoc>,
    pub(crate) notes: Vec<NoteCtx>,
    pub(crate) sections: Vec<SectionCtx>,
    /// Folded dictionary token → token id.
    pub(crate) token_ids: FastMap<String, u32>,
    /// Token id → folded token (parallel to `postings`).
    pub(crate) tokens: Vec<String>,
    /// Token id → postings sorted by `(field, unit)`.
    pub(crate) postings: Vec<Vec<Posting>>,
    /// Folded link target → sorted, deduped doc ids whose body contains
    /// a `[[…]]` span for it.
    pub(crate) link_postings: HashMap<String, Vec<u32>>,
    /// Trigram → token ids whose token contains the trigram.
    pub(crate) trigrams: FastMap<String, Vec<u32>>,
    /// Per configured algorithm: folded stem → token ids.
    pub(crate) stems: Vec<(Algorithm, HashMap<String, Vec<u32>>)>,
    /// Units per field (docs, notes, notes, sections, notes).
    pub(crate) field_units: [u32; 5],
    /// Average unit length per field, over units with a non-empty field.
    pub(crate) avg_len: [f64; 5],
    /// Folded title and folded extension-less path → notes, for `near:`.
    pub(crate) note_names: HashMap<String, Vec<u32>>,
    pub(crate) note_by_path: HashMap<PathBuf, u32>,
    /// `(path, line_start)` → doc id.
    pub(crate) doc_by_loc: HashMap<(PathBuf, u32), u32>,
    /// Undirected note-link adjacency between indexed notes.
    pub(crate) adjacency: Vec<Vec<u32>>,
}

impl SearchIndex {
    /// Build the index from a scan with the default `[search]` config,
    /// skipping files whose vault-relative path starts with any
    /// `exclude_prefixes` entry.
    pub fn build(scan: &Scan, exclude_prefixes: &[String]) -> SearchIndex {
        Self::build_with(scan, exclude_prefixes, &SearchCfg::default())
    }

    /// Build the index with an explicit `[search]` config.
    pub fn build_with(scan: &Scan, exclude_prefixes: &[String], cfg: &SearchCfg) -> SearchIndex {
        let algorithms: Vec<Algorithm> = cfg
            .stemmers
            .iter()
            .filter_map(|n| text::stemmer_algorithm(n))
            .collect();
        let mut b = Builder {
            index: SearchIndex {
                stems: algorithms.iter().map(|a| (*a, HashMap::new())).collect(),
                ..SearchIndex::default()
            },
            stemmed_tokens: Vec::new(),
            stemmed_surfaces: HashSet::new(),
        };

        let mut files: Vec<_> = scan
            .files
            .iter()
            .filter(|pf| !path_excluded(&pf.rel, exclude_prefixes))
            .collect();
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        for pf in files {
            b.add_file(pf);
        }
        let mut index = b.finish();
        index.apply_links(scan);
        index
    }

    /// Number of indexed paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.docs.len()
    }

    /// Length of `unit` in `field`, in word tokens.
    pub(crate) fn unit_len(&self, field: Field, unit: u32) -> u32 {
        match field {
            Field::Body => self.docs[unit as usize].len,
            Field::Title => self.notes[unit as usize].title_len,
            Field::Alias => self.notes[unit as usize].alias_len,
            Field::Tag => self.notes[unit as usize].tag_len,
            Field::Heading => self.sections[unit as usize].len,
        }
    }

    /// The docs a unit of `field` covers.
    pub(crate) fn unit_docs(&self, field: Field, unit: u32) -> Range<u32> {
        match field {
            Field::Body => unit..unit + 1,
            Field::Title | Field::Alias | Field::Tag => self.notes[unit as usize].docs.clone(),
            Field::Heading => self.sections[unit as usize].docs.clone(),
        }
    }

    /// The doc at `(path, line_start)`, if indexed.
    pub(crate) fn doc_at(&self, path: &Path, line_start: u32) -> Option<u32> {
        self.doc_by_loc
            .get(&(path.to_path_buf(), line_start))
            .copied()
    }

    /// Notes a `near:` target names: by folded title, or by folded
    /// vault-relative path without the `.md` extension.
    pub(crate) fn notes_named(&self, folded: &str) -> &[u32] {
        self.note_names
            .get(folded)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Priors from in-degree and relative recency, and the undirected
    /// note-link adjacency. Links resolve against every scanned file
    /// (links from excluded files still count as backlinks); only links
    /// between distinct notes count, once per source note.
    fn apply_links(&mut self, scan: &Scan) {
        let n = self.notes.len();
        let lookup = ScanNoteIndex::new(scan.files.iter().map(|f| f.rel.as_path()));
        let note_of: Vec<Option<u32>> = scan
            .files
            .iter()
            .map(|f| self.note_by_path.get(&f.rel).copied())
            .collect();
        let mut indegree = vec![0usize; n];
        let mut adjacency: Vec<HashSet<u32>> = vec![HashSet::new(); n];
        for (src, pf) in scan.files.iter().enumerate() {
            let mut targets: HashSet<usize> = HashSet::new();
            for link in &pf.links {
                let r = match link.form {
                    LinkForm::WikiLink => resolve_wiki(&link.target_text, &lookup),
                    LinkForm::MdLink => resolve_md(&link.target_text, &pf.rel, &lookup),
                };
                if let Resolution::Resolved(t) = r {
                    if t != src {
                        targets.insert(t);
                    }
                }
            }
            for t in targets {
                let Some(tn) = note_of[t] else { continue };
                indegree[tn as usize] += 1;
                if let Some(sn) = note_of[src] {
                    adjacency[sn as usize].insert(tn);
                    adjacency[tn as usize].insert(sn);
                }
            }
        }

        let max_in = indegree.iter().copied().max().unwrap_or(0);
        let newest = self.mtimes_newest();
        for (i, note) in self.notes.iter_mut().enumerate() {
            let link_part = if max_in == 0 {
                0.0
            } else {
                PRIOR_INDEGREE * (1.0 + indegree[i] as f64).ln() / (1.0 + max_in as f64).ln()
            };
            // Whole days: files saved moments apart must not reorder ties.
            let age_days = newest
                .and_then(|newest| newest.duration_since(note.mtime).ok())
                .map_or(0.0, |d| (d.as_secs() / 86_400) as f64);
            let recency_part = PRIOR_RECENCY * (-age_days / PRIOR_RECENCY_DAYS).exp();
            note.prior = 1.0 + link_part.min(PRIOR_INDEGREE) + recency_part.min(PRIOR_RECENCY);
        }
        self.adjacency = adjacency
            .into_iter()
            .map(|s| {
                let mut v: Vec<u32> = s.into_iter().collect();
                v.sort_unstable();
                v
            })
            .collect();
    }

    fn mtimes_newest(&self) -> Option<SystemTime> {
        self.notes.iter().map(|n| n.mtime).max()
    }
}

/// Max prior share from in-degree (log-scaled against the vault max).
pub(crate) const PRIOR_INDEGREE: f64 = 0.15;
/// Max prior share from recency (exponential decay of relative age).
pub(crate) const PRIOR_RECENCY: f64 = 0.10;
/// Recency decay constant, in days.
const PRIOR_RECENCY_DAYS: f64 = 90.0;

/// Accumulates the index file by file.
struct Builder {
    index: SearchIndex,
    /// Per token id: its ASCII surface (= the token itself) was stemmed.
    stemmed_tokens: Vec<bool>,
    /// Non-ASCII lowercase surfaces already stemmed.
    stemmed_surfaces: HashSet<String>,
}

impl Builder {
    fn token_id(&mut self, folded: &str) -> u32 {
        if let Some(&id) = self.index.token_ids.get(folded) {
            return id;
        }
        let id = self.index.tokens.len() as u32;
        self.index.token_ids.insert(folded.to_string(), id);
        self.index.tokens.push(folded.to_string());
        self.index.postings.push(Vec::new());
        id
    }

    /// Map every configured stem of `surface` to `token`. An ASCII
    /// surface lowercases to the token itself, so it is stemmed once per
    /// token; accented surfaces are stemmed unfolded, once per surface.
    fn stem(&mut self, surface: &str, token: u32) {
        if self.index.stems.is_empty() {
            return;
        }
        let lower = if surface.is_ascii() {
            let i = token as usize;
            if self.stemmed_tokens.len() <= i {
                self.stemmed_tokens.resize(i + 1, false);
            }
            if std::mem::replace(&mut self.stemmed_tokens[i], true) {
                return;
            }
            self.index.tokens[i].clone()
        } else {
            let lower = surface.to_lowercase();
            if !self.stemmed_surfaces.insert(lower.clone()) {
                return;
            }
            lower
        };
        for (algo, map) in self.index.stems.iter_mut() {
            let ids = map.entry(text::stem(*algo, &lower)).or_default();
            if !ids.contains(&token) {
                ids.push(token);
            }
        }
    }

    /// Count the word tokens of `s` into `counts`; returns the length.
    fn count_words(&mut self, s: &str, counts: &mut FastMap<u32, u16>) -> u32 {
        let mut toks = Vec::new();
        text::words(s, 0, &mut toks);
        for t in &toks {
            let id = self.token_id(&t.text);
            self.stem(&s[t.range.clone()], id);
            let c = counts.entry(id).or_insert(0);
            *c = c.saturating_add(1);
        }
        toks.len() as u32
    }

    fn post(&mut self, field: Field, unit: u32, counts: FastMap<u32, u16>) {
        for (id, tf) in counts {
            self.index.postings[id as usize].push(Posting { field, unit, tf });
        }
    }

    fn add_file(&mut self, pf: &crate::scan::ParsedFile) {
        let note_id = self.index.notes.len() as u32;
        let title = pf
            .rel
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (aliases, tags) = pf
            .frontmatter
            .as_deref()
            .map(|fm| {
                (
                    crate::frontmatter::aliases_in(fm),
                    crate::frontmatter::tags_in(fm),
                )
            })
            .unwrap_or_default();

        let mut counts = FastMap::default();
        let title_len = self.count_words(&title, &mut counts);
        self.post(Field::Title, note_id, counts);
        let mut counts = FastMap::default();
        let alias_len = aliases
            .iter()
            .map(|a| self.count_words(a, &mut counts))
            .sum();
        self.post(Field::Alias, note_id, counts);
        let mut counts = FastMap::default();
        let tag_len = tags.iter().map(|t| self.count_words(t, &mut counts)).sum();
        self.post(Field::Tag, note_id, counts);

        let folded_title = text::fold(&title);
        let rel = pf.rel.to_string_lossy().replace('\\', "/");
        let folded_path = text::fold(&rel);
        let stem_path = folded_path
            .strip_suffix(".md")
            .unwrap_or(&folded_path)
            .to_string();
        for name in [folded_title.clone(), stem_path] {
            let v = self.index.note_names.entry(name).or_default();
            if !v.contains(&note_id) {
                v.push(note_id);
            }
        }
        self.index.note_by_path.insert(pf.rel.clone(), note_id);

        let doc_start = self.index.docs.len() as u32;
        let section_start = self.index.sections.len();
        let mut headings = pf.headings.clone();
        headings.sort_by_key(|h| h.line);
        let mut next_heading = 0;
        let mut stack: Vec<(u8, String)> = Vec::new();

        for p in &pf.paragraphs {
            // Headings strictly above the paragraph enclose it; a heading
            // on the paragraph's own first line is its body, not its path.
            while let Some(h) = headings.get(next_heading) {
                if h.line >= p.line_start as usize {
                    break;
                }
                while stack.last().is_some_and(|(lvl, _)| *lvl >= h.level) {
                    stack.pop();
                }
                stack.push((h.level, h.text.clone()));
                next_heading += 1;
            }
            // Quoted material inside `[!ft-source]` protected sections is
            // not searchable: it is a copy of another note's prose, and
            // re-sourcing it would let a synth note pin itself.
            if is_protected_callout(&p.text) {
                continue;
            }

            let doc_id = self.index.docs.len() as u32;
            let path: Vec<String> = stack.iter().map(|(_, t)| t.clone()).collect();
            let new_section = self.index.sections.len() == section_start
                || self.index.sections.last().map(|s| &s.heading_path) != Some(&path);
            if new_section {
                let section_id = self.index.sections.len() as u32;
                let joined = path.join(" ");
                let mut counts = FastMap::default();
                let len = self.count_words(&joined, &mut counts);
                self.post(Field::Heading, section_id, counts);
                self.index.sections.push(SectionCtx {
                    folded: text::fold(&joined),
                    heading_path: path,
                    docs: doc_id..doc_id,
                    len,
                });
            }
            let section = self.index.sections.len() as u32 - 1;
            self.index.sections[section as usize].docs.end = doc_id + 1;

            let mut counts: FastMap<u32, u16> = FastMap::default();
            let mut len = 0u32;
            let mut doc_tags = Vec::new();
            for tok in text::tokenize(&p.text) {
                match tok.kind {
                    TokenKind::Word => {
                        let id = self.token_id(&tok.text);
                        self.stem(&p.text[tok.range.clone()], id);
                        let c = counts.entry(id).or_insert(0);
                        *c = c.saturating_add(1);
                        len += 1;
                    }
                    TokenKind::Link => {
                        self.index
                            .link_postings
                            .entry(tok.text)
                            .or_default()
                            .push(doc_id);
                    }
                    TokenKind::Tag => {
                        if !doc_tags.contains(&tok.text) {
                            doc_tags.push(tok.text);
                        }
                    }
                }
            }
            self.post(Field::Body, doc_id, counts);
            self.index
                .doc_by_loc
                .insert((pf.rel.clone(), p.line_start), doc_id);
            self.index.docs.push(ParagraphDoc {
                path: pf.rel.clone(),
                line_start: p.line_start,
                line_end: p.line_end,
                text: p.text.clone(),
                folded: text::fold(&p.text),
                note: note_id,
                section,
                len,
                tags: doc_tags,
            });
        }

        self.index.notes.push(NoteCtx {
            folded_path,
            title,
            folded_title,
            aliases: aliases.iter().map(|a| text::fold(a)).collect(),
            tags: tags.iter().map(|t| text::fold(t)).collect(),
            docs: doc_start..self.index.docs.len() as u32,
            title_len,
            alias_len,
            tag_len,
            prior: 1.0,
            mtime: pf.mtime,
        });
    }

    fn finish(mut self) -> SearchIndex {
        let ix = &mut self.index;
        for p in ix.postings.iter_mut() {
            p.sort_unstable_by_key(|p| (p.field, p.unit));
        }
        for p in ix.link_postings.values_mut() {
            p.sort_unstable();
            p.dedup();
        }
        for (id, token) in ix.tokens.iter().enumerate() {
            for trig in text::trigrams_of(token) {
                ix.trigrams.entry(trig).or_default().push(id as u32);
            }
        }
        for ids in ix.trigrams.values_mut() {
            ids.sort_unstable();
            ids.dedup();
        }
        for (_, map) in ix.stems.iter_mut() {
            for ids in map.values_mut() {
                ids.sort_unstable();
            }
        }

        let notes = ix.notes.len() as u32;
        ix.field_units = [
            ix.docs.len() as u32,
            notes,
            notes,
            ix.sections.len() as u32,
            notes,
        ];
        let mean = |lens: &mut dyn Iterator<Item = u32>| {
            let (sum, n) = lens
                .filter(|&l| l > 0)
                .fold((0u64, 0u64), |(s, n), l| (s + l as u64, n + 1));
            if n == 0 {
                1.0
            } else {
                sum as f64 / n as f64
            }
        };
        ix.avg_len = [
            mean(&mut ix.docs.iter().map(|d| d.len)),
            mean(&mut ix.notes.iter().map(|n| n.title_len)),
            mean(&mut ix.notes.iter().map(|n| n.alias_len)),
            mean(&mut ix.sections.iter().map(|s| s.len)),
            mean(&mut ix.notes.iter().map(|n| n.tag_len)),
        ];
        self.index
    }
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

    fn index_of(files: &[(&str, &str)]) -> SearchIndex {
        let tmp = assert_fs::TempDir::new().unwrap();
        SearchIndex::build(&build_scan(tmp.path(), files), &[])
    }

    fn doc<'a>(idx: &'a SearchIndex, path: &str, line: u32) -> &'a ParagraphDoc {
        idx.docs
            .iter()
            .find(|d| d.path == *path && d.line_start == line)
            .unwrap_or_else(|| panic!("no doc {path}:{line}"))
    }

    #[test]
    fn index_covers_every_paragraph() {
        let idx = index_of(&[
            ("a.md", "One paragraph.\n\nSecond paragraph.\n"),
            ("b.md", "Solo.\n"),
        ]);
        assert_eq!(idx.paragraph_count(), 3);
        assert_eq!(doc(&idx, "a.md", 1).text, "One paragraph.");
        assert_eq!(doc(&idx, "a.md", 3).text, "Second paragraph.");
        assert_eq!(idx.notes.len(), 2);
        let a = &idx.notes[doc(&idx, "a.md", 1).note as usize];
        assert_eq!(a.title, "a");
        assert_eq!(a.docs.len(), 2);
    }

    #[test]
    fn link_targets_are_link_postings() {
        let idx = index_of(&[(
            "a.md",
            "See [[Foo Bar]], [[Baz#Section]], and [[Qux|Alias]].\n",
        )]);
        for tok in ["foo bar", "baz", "alias"] {
            assert!(
                idx.link_postings.contains_key(tok),
                "missing link postings for {tok:?}"
            );
        }
        // The words inside a link are body words.
        assert!(idx.token_ids.contains_key("foo"));
        assert!(idx.token_ids.contains_key("qux"));
    }

    #[test]
    fn prose_occurrence_is_not_a_link() {
        let idx = index_of(&[("a.md", "Prose mentions [[memoization]].\n")]);
        assert!(idx.token_ids.contains_key("memoization"));
        assert_eq!(idx.link_postings["memoization"], vec![0]);
        let idx = index_of(&[("a.md", "prose memoization only\n")]);
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
    fn postings_carry_term_frequency() {
        let idx = index_of(&[("a.md", "foo foo foo.\n")]);
        let id = idx.token_ids["foo"];
        let body: Vec<_> = idx.postings[id as usize]
            .iter()
            .filter(|p| p.field == Field::Body)
            .collect();
        assert_eq!(body.len(), 1);
        assert_eq!(body[0].tf, 3);
        assert_eq!(idx.docs[0].len, 3);
    }

    #[test]
    fn trigram_map_has_candidates() {
        let idx = index_of(&[("a.md", "memoization helps.\n")]);
        assert!(idx.trigrams.contains_key("mem"));
        assert!(idx.trigrams.contains_key("zat"));
    }

    #[test]
    fn indexes_non_ascii_link_target_folded() {
        let idx = index_of(&[("a.md", "See [[problems with móveis ii]] for context.\n")]);
        assert!(idx.link_postings.contains_key("problems with moveis ii"));
        assert!(idx.token_ids.contains_key("moveis"));
        assert!(idx.trigrams.contains_key("ove"));
    }

    #[test]
    fn accented_words_are_whole_tokens() {
        let idx = index_of(&[("a.md", "o ônibus da manhã\n")]);
        assert!(idx.token_ids.contains_key("onibus"));
        assert!(idx.token_ids.contains_key("manha"));
        assert!(!idx.token_ids.contains_key("nibus"));
        assert!(!idx.token_ids.contains_key("manh"));
    }

    #[test]
    fn heading_path_is_captured() {
        let idx = index_of(&[(
            "a.md",
            "# Busão\n\nIntro.\n\n## Design\n\nUnder design.\n\n# Other\n\nElsewhere.\n",
        )]);
        let path_of = |line| {
            let d = doc(&idx, "a.md", line);
            idx.sections[d.section as usize].heading_path.clone()
        };
        // A heading paragraph's own heading is body, not path.
        assert!(path_of(1).is_empty());
        assert_eq!(path_of(3), vec!["Busão"]);
        assert_eq!(path_of(5), vec!["Busão"]);
        assert_eq!(path_of(7), vec!["Busão", "Design"]);
        assert_eq!(path_of(11), vec!["Other"]);
        let design = idx.token_ids["design"];
        assert!(idx.postings[design as usize]
            .iter()
            .any(|p| p.field == Field::Heading));
        let section = &idx.sections[doc(&idx, "a.md", 7).section as usize];
        assert_eq!(section.folded, "busao design");
    }

    #[test]
    fn tag_set_combines_frontmatter_and_inline() {
        let idx = index_of(&[(
            "a.md",
            "---\ntags: [proj/busao]\n---\n\nFirst #idea here.\n\nSecond.\n",
        )]);
        let first = doc(&idx, "a.md", 5);
        let second = doc(&idx, "a.md", 7);
        let note = &idx.notes[first.note as usize];
        assert_eq!(note.tags, vec!["proj/busao"]);
        assert_eq!(first.tags, vec!["idea"]);
        assert!(second.tags.is_empty());
    }

    #[test]
    fn context_fields_are_posted_per_note() {
        let idx = index_of(&[(
            "Memoization.md",
            "---\naliases: [memo cache]\n---\n\nOne.\n\nTwo.\n",
        )]);
        let field_of = |tok: &str| -> Vec<Field> {
            idx.postings[idx.token_ids[tok] as usize]
                .iter()
                .map(|p| p.field)
                .collect()
        };
        assert_eq!(field_of("memoization"), vec![Field::Title]);
        assert_eq!(field_of("cache"), vec![Field::Alias]);
        let note = &idx.notes[0];
        assert_eq!(idx.unit_docs(Field::Title, 0), note.docs.clone());
        assert_eq!(note.docs.len(), 2);
        assert_eq!(idx.notes_named("memoization"), &[0]);
    }

    #[test]
    fn stems_map_inflections_to_tokens() {
        let idx = index_of(&[("a.md", "Garden memories.\n")]);
        let (_, map) = &idx.stems[0];
        let stem = text::stem(Algorithm::English, "memory");
        let ids = &map[&stem];
        assert!(ids.contains(&idx.token_ids["memories"]));
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
        let idx = index_of(&[(
            "Synthesis/topic.md",
            "---\nft:\n  synth:\n    enabled: true\n---\n\n\
             User prose between callouts is searchable.\n\n\
             > [!ft-source] \"notes/source.md\" L1-1 @abc1234 #7f3a91\n\
             > Quoted material must not be indexed.\n",
        )]);
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

    #[test]
    fn link_priors_and_adjacency() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let scan = build_scan(
            tmp.path(),
            &[
                ("hub.md", "The hub links [[hub]] itself.\n"),
                ("a.md", "Links [[hub]] and [[hub]] again.\n"),
                ("b.md", "Links [Hub](hub.md) too.\n"),
                ("journal/day.md", "Excluded but links [[hub]].\n"),
                ("orphan.md", "Alone.\n"),
            ],
        );
        let idx = SearchIndex::build(&scan, &["journal/".to_string()]);
        let prior = |name: &str| idx.notes[idx.note_by_path[&PathBuf::from(name)] as usize].prior;
        assert!(prior("hub.md") > prior("orphan.md"));
        assert!(prior("hub.md") > prior("a.md"));
        // Three distinct sources (a, b, the excluded journal note); no self link.
        assert!(
            (prior("hub.md") - 1.25).abs() < 1e-9,
            "max in-degree + newest"
        );
        for n in &idx.notes {
            assert!((1.0..=1.25).contains(&n.prior), "{} {}", n.title, n.prior);
        }
        let hub = idx.note_by_path[&PathBuf::from("hub.md")];
        let a = idx.note_by_path[&PathBuf::from("a.md")];
        assert!(idx.adjacency[hub as usize].contains(&a));
        assert!(idx.adjacency[a as usize].contains(&hub));
    }
}
