//! `ft notes embed` — model and embedding management for semantic search.
//!
//! `fetch` downloads the configured Model2Vec model once (the only
//! network access semantic search ever makes); `status` reports where the
//! model lives and how much of the vault is embedded; `build` embeds new
//! or changed paragraphs into the vault's embedding cache.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Subcommand;
use ft_core::search::semantic::{
    cache::EmbeddingCache, fetch_model, load_embedder, model_dir, model_present, SemanticIndex,
    COMPILED,
};
use ft_core::search::SearchIndex;

#[derive(Subcommand, Debug)]
pub enum EmbedCommand {
    /// Download the embedding model (Hugging Face id from
    /// `[search.semantic] model`, or `--model`) into the user cache dir.
    Fetch {
        /// Model id or local directory (overrides the config).
        #[arg(long)]
        model: Option<String>,
    },
    /// Show the model location and how much of the vault is embedded.
    Status,
    /// Embed new or changed paragraphs into the vault's embedding cache.
    Build,
}

pub fn run(command: EmbedCommand, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let cfg = &vault.config.config.search;
    match command {
        EmbedCommand::Fetch { model } => {
            let model = model.unwrap_or_else(|| cfg.semantic.model.clone());
            match fetch_model(&model).with_context(|| format!("fetching {model}"))? {
                None => println!("{model} is a local directory — nothing to fetch"),
                Some(bytes) => println!(
                    "fetched {model} ({:.1} MB) into {}",
                    bytes as f64 / 1_048_576.0,
                    model_dir(&model).display()
                ),
            }
            Ok(ExitCode::SUCCESS)
        }
        EmbedCommand::Status => {
            let model = &cfg.semantic.model;
            let dir = model_dir(model);
            println!(
                "semantic search: {}",
                match (COMPILED, cfg.semantic.enabled) {
                    (false, _) => "not compiled in",
                    (true, true) => "enabled",
                    (true, false) => "disabled ([search.semantic] enabled = false)",
                }
            );
            println!("model: {model}");
            println!(
                "location: {} ({})",
                dir.display(),
                if model_present(&dir) {
                    "present"
                } else {
                    "missing — run `ft notes embed fetch`"
                }
            );
            if let Some(embedder) = load_embedder(model)? {
                let scan = vault.scan();
                let index = SearchIndex::build_with(
                    &scan,
                    &vault.config.config.synth.exclude_prefixes,
                    cfg,
                );
                let cache = EmbeddingCache::load(&vault.path, embedder.id(), embedder.dim());
                let covered = (0..index.paragraph_count() as u32)
                    .filter(|&d| {
                        cache.contains(&ft_core::search::semantic::cache::key_of(
                            &ft_core::search::semantic::embedded_text(&index, d),
                        ))
                    })
                    .count();
                println!("dimension: {}", embedder.dim());
                println!(
                    "embedded: {covered} / {} paragraphs ({})",
                    index.paragraph_count(),
                    EmbeddingCache::path(&vault.path, embedder.id()).display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        EmbedCommand::Build => {
            if !COMPILED {
                eprintln!("semantic search is not compiled into this ft build");
                return Ok(ExitCode::from(2));
            }
            let Some(embedder) = load_embedder(&cfg.semantic.model)? else {
                eprintln!(
                    "the model {} is not available — run `ft notes embed fetch`",
                    cfg.semantic.model
                );
                return Ok(ExitCode::from(2));
            };
            let scan = vault.scan();
            let index =
                SearchIndex::build_with(&scan, &vault.config.config.synth.exclude_prefixes, cfg);
            let mut cache = EmbeddingCache::load(&vault.path, embedder.id(), embedder.dim());
            let start = std::time::Instant::now();
            let (_, stats) = SemanticIndex::build(&index, embedder.as_ref(), &mut cache);
            cache
                .save(&vault.path)
                .context("saving the embedding cache")?;
            println!(
                "embedded {}, reused {}, pruned {} ({:.1}s)",
                stats.embedded,
                stats.reused,
                stats.pruned,
                start.elapsed().as_secs_f64()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}
