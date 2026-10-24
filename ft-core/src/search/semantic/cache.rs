//! On-disk paragraph embeddings: `<vault>/.ft/cache/embeddings/<model>.bin`.
//!
//! A versioned msgpack file of f16 vectors keyed by the first 16 bytes of
//! the blake3 hash of the embedded text. Keys and vectors are stored as
//! two flat byte blobs (msgpack `bin`), so loading is a copy rather than a
//! per-number parse. Loading a file written for
//! another model or dimension (or an older schema, or a corrupt file)
//! yields an empty cache, so the next build re-embeds.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use half::f16;
use serde::{Deserialize, Serialize};

use crate::error::Result;

const DIR_REL: &str = ".ft/cache/embeddings";
const SCHEMA_VERSION: u32 = 2;

/// Cache key: truncated blake3 of the embedded text.
pub type Key = [u8; 16];

pub fn key_of(text: &str) -> Key {
    let h = blake3::hash(text.as_bytes());
    let mut k = [0u8; 16];
    k.copy_from_slice(&h.as_bytes()[..16]);
    k
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    model: String,
    dim: usize,
    /// `n × 16` key bytes.
    keys: Bytes,
    /// `n × dim` little-endian f16 values, row per key.
    vectors: Bytes,
}

/// A byte blob serialized as msgpack `bin` (not an array of integers).
struct Bytes(Vec<u8>);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Bytes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a byte blob")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> std::result::Result<Bytes, E> {
                Ok(Bytes(v.to_vec()))
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> std::result::Result<Bytes, E> {
                Ok(Bytes(v))
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// Paragraph vectors for one model.
#[derive(Debug, Clone)]
pub struct EmbeddingCache {
    model: String,
    dim: usize,
    entries: HashMap<Key, Vec<u16>>,
}

impl EmbeddingCache {
    pub fn new(model: &str, dim: usize) -> Self {
        Self {
            model: model.to_string(),
            dim,
            entries: HashMap::new(),
        }
    }

    /// The cache file for `model` under `vault_root`.
    pub fn path(vault_root: &Path, model: &str) -> PathBuf {
        vault_root
            .join(DIR_REL)
            .join(format!("{}.bin", slug(model)))
    }

    /// Load the cache for `model` / `dim`; empty when missing, corrupt,
    /// or written for another model, dimension, or schema.
    pub fn load(vault_root: &Path, model: &str, dim: usize) -> Self {
        let empty = Self::new(model, dim);
        let Ok(bytes) = std::fs::read(Self::path(vault_root, model)) else {
            return empty;
        };
        let f = match rmp_serde::from_slice::<CacheFile>(&bytes) {
            Ok(f) if f.version == SCHEMA_VERSION && f.model == model && f.dim == dim => f,
            _ => return empty,
        };
        let n = f.keys.0.len() / 16;
        if f.keys.0.len() != n * 16 || f.vectors.0.len() != n * dim * 2 {
            return empty;
        }
        let entries = f
            .keys
            .0
            .chunks_exact(16)
            .zip(f.vectors.0.chunks_exact(dim * 2))
            .map(|(k, v)| {
                let key: Key = k.try_into().expect("16-byte chunk");
                let vals = v
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                (key, vals)
            })
            .collect();
        Self {
            model: f.model,
            dim,
            entries,
        }
    }

    /// Write atomically to [`Self::path`].
    pub fn save(&self, vault_root: &Path) -> Result<()> {
        let mut keys: Vec<&Key> = self.entries.keys().collect();
        keys.sort();
        let mut key_bytes = Vec::with_capacity(keys.len() * 16);
        let mut vec_bytes = Vec::with_capacity(keys.len() * self.dim * 2);
        for k in keys {
            key_bytes.extend_from_slice(k);
            for v in &self.entries[k] {
                vec_bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let file = CacheFile {
            version: SCHEMA_VERSION,
            model: self.model.clone(),
            dim: self.dim,
            keys: Bytes(key_bytes),
            vectors: Bytes(vec_bytes),
        };
        let bytes = rmp_serde::to_vec(&file)
            .map_err(|e| crate::error::Error::Notes(format!("serialize embeddings: {e}")))?;
        crate::fs::write_atomic_bytes(&Self::path(vault_root, &self.model), &bytes)
    }

    pub fn contains(&self, k: &Key) -> bool {
        self.entries.contains_key(k)
    }

    pub fn get(&self, k: &Key) -> Option<Vec<f32>> {
        self.entries
            .get(k)
            .map(|v| v.iter().map(|&b| f16::from_bits(b).to_f32()).collect())
    }

    pub fn insert(&mut self, k: Key, v: &[f32]) {
        self.entries
            .insert(k, v.iter().map(|&x| f16::from_f32(x).to_bits()).collect());
    }

    /// Keep only `keys`; returns how many entries were dropped.
    pub fn retain(&mut self, keys: &[Key]) -> usize {
        let keep: HashSet<&Key> = keys.iter().collect();
        let before = self.entries.len();
        self.entries.retain(|k, _| keep.contains(k));
        before - self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A file-name-safe form of a model id (`minishlab/potion-base-8M` →
/// `minishlab-potion-base-8M`).
fn slug(model: &str) -> String {
    model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_f16_precision() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let mut c = EmbeddingCache::new("m/x", 3);
        let k = key_of("text");
        c.insert(k, &[0.5, -0.25, 0.123_456]);
        c.save(tmp.path()).unwrap();
        let back = EmbeddingCache::load(tmp.path(), "m/x", 3).get(&k).unwrap();
        assert_eq!(back[0], 0.5);
        assert_eq!(back[1], -0.25);
        assert!((back[2] - 0.123_456).abs() < 1e-3);
        assert!(EmbeddingCache::path(tmp.path(), "m/x").ends_with("m-x.bin"));
    }

    #[test]
    fn model_or_dim_mismatch_loads_empty() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let mut c = EmbeddingCache::new("m/x", 2);
        c.insert(key_of("a"), &[1.0, 0.0]);
        c.save(tmp.path()).unwrap();
        assert!(EmbeddingCache::load(tmp.path(), "m/x", 4).is_empty());
        assert!(EmbeddingCache::load(tmp.path(), "m/y", 2).is_empty());
        assert_eq!(EmbeddingCache::load(tmp.path(), "m/x", 2).len(), 1);
    }

    #[test]
    fn corrupt_file_loads_empty() {
        let tmp = assert_fs::TempDir::new().unwrap();
        let p = EmbeddingCache::path(tmp.path(), "m/x");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"not msgpack").unwrap();
        assert!(EmbeddingCache::load(tmp.path(), "m/x", 2).is_empty());
    }

    #[test]
    fn retain_prunes() {
        let mut c = EmbeddingCache::new("m", 1);
        c.insert(key_of("a"), &[1.0]);
        c.insert(key_of("b"), &[1.0]);
        assert_eq!(c.retain(&[key_of("a")]), 1);
        assert!(c.contains(&key_of("a")) && !c.contains(&key_of("b")));
    }
}
