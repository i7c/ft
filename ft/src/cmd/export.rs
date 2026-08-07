//! `ft notes export` — read-only plumbing that renders a vault note
//! (or an original-file line range of it) as clean, portable markdown.
//!
//! This is the inverse of `ft notes quote` (which wraps a raw range
//! *into* a pinned `[!ft-source]` callout): it strips the vault-specific
//! structure — frontmatter, callout headers, wikilinks — so the output
//! is valid CommonMark for pasting/publishing/external tools. The
//! stripping rules live behind `ft_core::export::ExportTarget`;
//! `commonmark` is the v1 target, plain text and Slack planned. No git
//! interaction: the working tree is the source of truth.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Args, ValueEnum};
use ft_core::export::{export_content, CommonMarkExport, ExportError, ExportTarget, SlackExport};

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Source note to export (vault-relative; absolute paths are
    /// accepted and relativized). `.md` is not appended automatically.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// 1-indexed inclusive line range `A-B` in original-file lines.
    /// The start is clamped to the first line after the frontmatter
    /// block, so frontmatter is never exported. Omit for the whole
    /// file.
    #[arg(short = 'l', long, value_name = "A-B")]
    pub lines: Option<String>,

    /// Export target — the stripping rules applied. `commonmark` is
    /// clean CommonMark for pasting/publishing; `slack` rewrites to
    /// Slack's mrkdwn dialect (headings, emphasis, links, task
    /// checkboxes, callout markers and code fences converted; `& < >`
    /// left raw for the composer). Plain text is planned, not yet
    /// accepted.
    #[arg(long, value_enum, default_value_t = ExportFormat::CommonMark)]
    pub format: ExportFormat,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    /// Clean CommonMark: frontmatter dropped, `[!ft-source]` callout
    /// headers dropped (bodies stay as blockquotes), wikilinks
    /// converted to plain text / CommonMark images.
    #[default]
    #[value(name = "commonmark")]
    CommonMark,
    /// Slack mrkdwn: everything `commonmark` does, plus CommonMark
    /// syntax Slack renders as literal text converted to its dialect
    /// — headings → bold, `**bold**` → `*bold*`, `[text](url)` →
    /// `<url|text>`, images → text/URL, `- [ ]` checkboxes dropped,
    /// `[!type]` callout markers stripped, code-fence language tags
    /// dropped and `~~~` fences → ` ``` `. `& < >` stay raw.
    #[value(name = "slack")]
    Slack,
}

impl ExportFormat {
    fn target(self) -> &'static dyn ExportTarget {
        match self {
            ExportFormat::CommonMark => &CommonMarkExport,
            ExportFormat::Slack => &SlackExport,
        }
    }
}

pub fn run_export(args: ExportArgs, vault_flag: Option<PathBuf>) -> Result<ExitCode> {
    let range = match &args.lines {
        Some(spec) => Some(crate::cmd::common::parse_line_range(spec)?),
        None => None,
    };

    let vault = crate::cmd::common::discover_vault(vault_flag)?;
    let rel = vault.relativize(&args.file).to_path_buf();
    let absolute = vault.path.join(&rel);
    let content = std::fs::read_to_string(&absolute)
        .with_context(|| format!("cannot read source file `{}`", rel.display()))?;

    let outcome = export_content(&content, range, args.format.target()).map_err(|e| match e {
        ExportError::RangePastEnd {
            file_lines,
            requested_end,
        } => anyhow!(
            "line range L{}-{} outside file `{}` (file has {} lines)",
            range.map_or(0, |(a, _)| a),
            requested_end,
            rel.display(),
            file_lines
        ),
    })?;

    if !outcome.text.is_empty() {
        println!("{}", outcome.text);
    }
    Ok(ExitCode::SUCCESS)
}
