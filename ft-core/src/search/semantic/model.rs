//! The Model2Vec embedder (`semantic` feature).
//!
//! Loads a static embedding model from a local directory holding
//! `config.json`, `tokenizer.json`, and `model.safetensors` (what
//! `ft notes embed fetch` downloads). The crate is built `local-only`:
//! it never touches the network.

use std::path::Path;

use model2vec_rs::model::StaticModel;

use super::Embedder;
use crate::error::{Error, Result};

/// A loaded Model2Vec model.
pub struct Model2VecEmbedder {
    id: String,
    dim: usize,
    model: StaticModel,
}

impl Model2VecEmbedder {
    /// Load the model files in `dir`; `id` names the model (for caches).
    pub fn load(dir: &Path, id: &str) -> Result<Self> {
        let model = StaticModel::from_pretrained(dir, None, None, None).map_err(|e| {
            Error::Notes(format!(
                "loading embedding model from {}: {e}",
                dir.display()
            ))
        })?;
        let dim = model
            .encode(&["dimension probe".to_string()])
            .first()
            .map_or(0, Vec::len);
        if dim == 0 {
            return Err(Error::Notes(format!(
                "embedding model at {} produced empty vectors",
                dir.display()
            )));
        }
        Ok(Self {
            id: id.to_string(),
            dim,
            model,
        })
    }
}

impl Embedder for Model2VecEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[String]) -> Vec<Vec<f32>> {
        self.model.encode(texts)
    }
}
