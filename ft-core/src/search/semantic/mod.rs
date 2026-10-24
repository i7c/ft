//! Meaning-based retrieval: paragraph embeddings fused with the lexical
//! ranking.
//!
//! The engine only sees the [`Embedder`] trait and a [`SemanticIndex`]
//! of L2-normalized vectors aligned with the [`SearchIndex`]'s doc ids,
//! so it compiles without any model. The Model2Vec implementation lives
//! behind the `semantic` Cargo feature ([`model`]); [`HashEmbedder`] is a
//! deterministic stand-in for tests.
//!
//! A paragraph is embedded together with its context — note title and
//! heading path — because that is where much of its meaning lives (the
//! same reason BM25F has context fields). Vectors persist in an
//! [`cache::EmbeddingCache`] keyed by a hash of that text, so rebuilding
//! embeds only new or changed paragraphs.

pub mod cache;
#[cfg(feature = "semantic")]
pub mod model;

use crate::search::index::SearchIndex;
use crate::search::text;

use cache::EmbeddingCache;

/// Paragraphs embedded per batch.
const BATCH: usize = 1024;

/// The files a Model2Vec model directory holds.
pub const MODEL_FILES: &[&str] = &["config.json", "tokenizer.json", "model.safetensors"];

/// Where the model named by `[search.semantic] model` lives: the value
/// itself when it names an existing directory, else
/// `<user cache dir>/ft/models/<model id>/`.
pub fn model_dir(model: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(model);
    if p.is_dir() {
        return p.to_path_buf();
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("ft")
        .join("models")
        .join(model)
}

/// Whether `dir` holds every [`MODEL_FILES`] entry.
pub fn model_present(dir: &std::path::Path) -> bool {
    MODEL_FILES.iter().all(|f| dir.join(f).is_file())
}

/// Turns texts into fixed-size vectors.
pub trait Embedder: Send + Sync {
    /// Identifies the model; caches are kept per id.
    fn id(&self) -> &str;
    /// Vector length.
    fn dim(&self) -> usize;
    /// One vector per text, in order.
    fn embed(&self, texts: &[String]) -> Vec<Vec<f32>>;
}

/// Deterministic hashed bag-of-words embedder for tests: each folded word
/// adds ±1 to a bucket. Texts sharing words are similar; synonyms are
/// not — it exercises the plumbing, not meaning.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }
}

impl Embedder for HashEmbedder {
    fn id(&self) -> &str {
        "test/hash"
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[String]) -> Vec<Vec<f32>> {
        texts
            .iter()
            .map(|t| {
                let mut v = vec![0f32; self.dim];
                for w in text::word_texts(t) {
                    let h = blake3::hash(w.as_bytes());
                    let b = h.as_bytes();
                    let i = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize % self.dim;
                    v[i] += if b[4] & 1 == 0 { 1.0 } else { -1.0 };
                }
                v
            })
            .collect()
    }
}

/// Scale `v` to unit length (zero vectors stay zero).
pub(crate) fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// How an incremental build went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildStats {
    pub embedded: usize,
    pub reused: usize,
    pub pruned: usize,
}

/// L2-normalized paragraph vectors aligned with one [`SearchIndex`].
#[derive(Debug, Clone, Default)]
pub struct SemanticIndex {
    dim: usize,
    /// Row-major `rows × dim`.
    rows: Vec<f32>,
    /// Doc id → row.
    doc_rows: Vec<Option<u32>>,
}

impl SemanticIndex {
    /// Embed every indexed paragraph, reusing `cache` hits and adding new
    /// vectors to it; entries for paragraphs no longer indexed are pruned.
    /// The caller saves the cache.
    pub fn build(
        index: &SearchIndex,
        embedder: &dyn Embedder,
        cache: &mut EmbeddingCache,
    ) -> (Self, BuildStats) {
        Self::build_with_progress(index, embedder, cache, |_, _| {})
    }

    /// [`Self::build`] reporting `(embedded so far, to embed)` after each
    /// batch.
    pub fn build_with_progress(
        index: &SearchIndex,
        embedder: &dyn Embedder,
        cache: &mut EmbeddingCache,
        mut progress: impl FnMut(usize, usize),
    ) -> (Self, BuildStats) {
        let dim = embedder.dim();
        let n = index.docs.len();
        let texts: Vec<String> = (0..n as u32).map(|d| embedded_text(index, d)).collect();
        let keys: Vec<cache::Key> = texts.iter().map(|t| cache::key_of(t)).collect();

        let missing: Vec<usize> = (0..n).filter(|&i| !cache.contains(&keys[i])).collect();
        let mut stats = BuildStats {
            reused: n - missing.len(),
            ..BuildStats::default()
        };
        for chunk in missing.chunks(BATCH) {
            let batch: Vec<String> = chunk.iter().map(|&i| texts[i].clone()).collect();
            for (&i, mut v) in chunk.iter().zip(embedder.embed(&batch)) {
                normalize(&mut v);
                cache.insert(keys[i], &v);
            }
            stats.embedded += chunk.len();
            progress(stats.embedded, missing.len());
        }
        stats.pruned = cache.retain(&keys);

        let mut rows = Vec::with_capacity(n * dim);
        let mut doc_rows = vec![None; n];
        for (i, k) in keys.iter().enumerate() {
            if let Some(v) = cache.get(k).filter(|v| v.len() == dim) {
                doc_rows[i] = Some((rows.len() / dim) as u32);
                rows.extend(v);
            }
        }
        (
            Self {
                dim,
                rows,
                doc_rows,
            },
            stats,
        )
    }

    /// Whether this was built for an index with `docs` paragraphs.
    pub fn fits(&self, docs: usize) -> bool {
        self.doc_rows.len() == docs
    }

    /// Paragraphs with a vector.
    pub fn embedded(&self) -> usize {
        self.doc_rows.iter().filter(|r| r.is_some()).count()
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// The vector of `doc`, if embedded.
    pub fn vector(&self, doc: u32) -> Option<&[f32]> {
        let r = (*self.doc_rows.get(doc as usize)?)? as usize;
        Some(&self.rows[r * self.dim..(r + 1) * self.dim])
    }

    /// The `k` most similar docs to the normalized `q` with similarity ≥
    /// `min`, best first (ties by doc id), skipping `exclude`.
    pub fn top_k(&self, q: &[f32], k: usize, min: f32, exclude: Option<u32>) -> Vec<(u32, f32)> {
        if q.len() != self.dim || k == 0 {
            return Vec::new();
        }
        let mut hits: Vec<(u32, f32)> = self
            .doc_rows
            .iter()
            .enumerate()
            .filter_map(|(doc, r)| {
                let r = (*r)? as usize;
                if Some(doc as u32) == exclude {
                    return None;
                }
                let row = &self.rows[r * self.dim..(r + 1) * self.dim];
                let s: f32 = row.iter().zip(q).map(|(a, b)| a * b).sum();
                (s >= min).then_some((doc as u32, s))
            })
            .collect();
        hits.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        hits.truncate(k);
        hits
    }
}

/// The text a paragraph is embedded as: note title, heading path, body.
pub fn embedded_text(index: &SearchIndex, doc: u32) -> String {
    let d = &index.docs[doc as usize];
    let title = &index.notes[d.note as usize].title;
    let headings = index.sections[d.section as usize].heading_path.join(" ");
    if headings.is_empty() {
        format!("{title}. {}", d.text)
    } else {
        format!("{title}. {headings}. {}", d.text)
    }
}

/// Everything a query needs for the semantic side.
#[derive(Clone, Copy)]
pub struct SemanticQuery<'a> {
    pub index: &'a SemanticIndex,
    pub embedder: &'a dyn Embedder,
    /// Semantic candidates joining the result set.
    pub candidates: usize,
    /// Cosine-similarity floor for candidates.
    pub min_similarity: f32,
}

impl std::fmt::Debug for SemanticQuery<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticQuery")
            .field("embedder", &self.embedder.id())
            .field("candidates", &self.candidates)
            .field("min_similarity", &self.min_similarity)
            .finish()
    }
}

impl SemanticQuery<'_> {
    /// The normalized vector for a query text.
    pub fn embed_query(&self, text: &str) -> Vec<f32> {
        let mut v = self
            .embedder
            .embed(&[text.to_string()])
            .pop()
            .unwrap_or_default();
        normalize(&mut v);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(files: &[(&str, &str)]) -> SearchIndex {
        let tmp = assert_fs::TempDir::new().unwrap();
        for (name, content) in files {
            let path = tmp.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        SearchIndex::build(&crate::scan::scan_vault(tmp.path(), &[]), &[])
    }

    #[test]
    fn embedded_text_carries_context() {
        let idx = index_of(&[("Deep Work.md", "# Routine\n\nBlock two hours.\n")]);
        let doc = idx
            .docs
            .iter()
            .position(|d| d.text == "Block two hours.")
            .unwrap() as u32;
        assert_eq!(
            embedded_text(&idx, doc),
            "Deep Work. Routine. Block two hours."
        );
    }

    #[test]
    fn build_is_incremental_and_prunes() {
        let e = HashEmbedder::new(32);
        let idx = index_of(&[("a.md", "alpha one.\n\nbeta two.\n")]);
        let mut cache = EmbeddingCache::new(e.id(), e.dim());
        let (s, stats) = SemanticIndex::build(&idx, &e, &mut cache);
        assert_eq!((stats.embedded, stats.reused), (2, 0));
        assert_eq!(s.embedded(), 2);
        let (_, stats) = SemanticIndex::build(&idx, &e, &mut cache);
        assert_eq!((stats.embedded, stats.reused, stats.pruned), (0, 2, 0));

        let idx2 = index_of(&[("a.md", "alpha one.\n\ngamma three.\n")]);
        let (_, stats) = SemanticIndex::build(&idx2, &e, &mut cache);
        assert_eq!((stats.embedded, stats.reused, stats.pruned), (1, 1, 1));
    }

    #[test]
    fn top_k_ranks_by_cosine_and_respects_floor() {
        let e = HashEmbedder::new(64);
        let idx = index_of(&[
            ("a.md", "cache invalidation strategy.\n"),
            ("b.md", "garden tomatoes.\n"),
        ]);
        let mut cache = EmbeddingCache::new(e.id(), e.dim());
        let (s, _) = SemanticIndex::build(&idx, &e, &mut cache);
        let q = SemanticQuery {
            index: &s,
            embedder: &e,
            candidates: 5,
            min_similarity: 0.1,
        };
        let hits = s.top_k(&q.embed_query("cache invalidation"), 5, 0.1, None);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            idx.docs[hits[0].0 as usize].path,
            std::path::PathBuf::from("a.md")
        );
        let all = s.top_k(&q.embed_query("cache invalidation"), 5, -1.0, None);
        assert_eq!(all.len(), 2);
        let none = s.top_k(
            &q.embed_query("cache invalidation"),
            5,
            -1.0,
            Some(hits[0].0),
        );
        assert_eq!(none.len(), 1, "excluded doc skipped");
    }
}
