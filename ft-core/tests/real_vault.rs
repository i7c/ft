/// Real-vault round-trip smoke test.
/// Gated on `FT_REAL_VAULT_TESTS=1` so CI never depends on a local vault.
/// Run with:  FT_REAL_VAULT_TESTS=1 cargo test -p ft-core --test real_vault
use ft_core::task::{
    emoji::EmojiFormat,
    format::{ParseContext, TaskFormat},
};
use std::path::{Path, PathBuf};

fn walk_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') {
            continue; // skip .obsidian, .git, etc.
        }
        if path.is_dir() {
            walk_md(&path, out);
        } else if path.extension().map(|e| e == "md").unwrap_or(false) {
            out.push(path);
        }
    }
}

#[test]
fn real_vault_round_trip() {
    if std::env::var("FT_REAL_VAULT_TESTS").as_deref() != Ok("1") {
        return;
    }

    let vault = PathBuf::from("/Users/cmw/git/fortytwo");
    let mut files = Vec::new();
    walk_md(&vault, &mut files);

    let mut parsed = 0usize;
    let mut skipped = 0usize;
    let mut mismatches: Vec<(String, String, PathBuf, usize)> = Vec::new();

    for file in &files {
        let content = match std::fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (lineno, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("- [") {
                continue;
            }
            // Skip Templater template variables (not real task lines).
            if line.contains("<%") {
                skipped += 1;
                continue;
            }
            let ctx = ParseContext {
                source_file: file.clone(),
                source_line: lineno + 1,
            };
            let Some(task) = EmojiFormat.parse_line(line, ctx) else {
                skipped += 1;
                continue;
            };
            let serialized = EmojiFormat.serialize_line(&task);
            if serialized != line {
                // Known acceptable mismatches:
                // 1. Trailing whitespace: editing artifacts in the source file;
                //    the parser trims them (insignificant).
                // 2. Unknown status markers (e.g. `[!]`): parsed as Open per
                //    spec; the original marker is not preserved.
                let trailing_space_only = line.trim_end() == serialized;
                let status_mismatch = {
                    let marker = line.trim_start().get(3..4).unwrap_or("");
                    !matches!(marker, " " | "x" | "X" | "/" | "-")
                };
                if !trailing_space_only && !status_mismatch {
                    mismatches.push((line.to_string(), serialized, file.clone(), lineno + 1));
                }
            }
            parsed += 1;
        }
    }

    if !mismatches.is_empty() {
        let mut msg = format!(
            "{} round-trip mismatches out of {} tasks ({} skipped):\n",
            mismatches.len(),
            parsed,
            skipped
        );
        for (orig, got, file, line) in mismatches.iter().take(20) {
            msg.push_str(&format!("  {}:{line}\n", file.display()));
            msg.push_str(&format!("    orig: {orig:?}\n"));
            msg.push_str(&format!("    got:  {got:?}\n"));
        }
        panic!("{msg}");
    }

    println!("real_vault_round_trip: {parsed} tasks OK, {skipped} skipped, 0 mismatches");
}

// ── ft-core synth ritual against the real vault ─────────────────────────

#[test]
fn real_vault_link_review_runs() {
    use chrono::Duration;
    use ft_core::link_review::{compute_link_review, WindowRange};
    use ft_core::vault::Vault;

    if std::env::var("FT_REAL_VAULT_TESTS").as_deref() != Ok("1") {
        return;
    }
    let vault_path = PathBuf::from("/Users/cmw/git/fortytwo");
    let vault = match Vault::discover(Some(vault_path.clone())) {
        Ok(v) => v,
        Err(_) => return, // vault not present on this machine
    };
    if ft_core::git::discover_repo(&vault.path).is_none() {
        return;
    }
    let scan = vault.scan();
    let graph = ft_core::graph::Graph::build(&vault, &scan).expect("build graph");
    let cfg = vault.config.config.synth.clone();
    let window = WindowRange::Since(Duration::days(7));
    let review = compute_link_review(&graph, &vault, &window, &cfg)
        .expect("compute_link_review on real vault");
    // Sanity: rows are sorted by count desc, alphabetical tiebreak.
    for w in review.rows.windows(2) {
        assert!(
            w[0].count > w[1].count || (w[0].count == w[1].count && w[0].target <= w[1].target),
            "rows must be sorted by count desc, target asc"
        );
    }
    println!(
        "real_vault_link_review: {} rows ({} ghosts) over since-7d",
        review.rows.len(),
        review.rows.iter().filter(|r| r.is_ghost).count()
    );
}

#[test]
fn real_vault_synth_verify_all_runs() {
    use ft_core::synth::verify::verify_all;
    use ft_core::vault::Vault;

    if std::env::var("FT_REAL_VAULT_TESTS").as_deref() != Ok("1") {
        return;
    }
    let vault_path = PathBuf::from("/Users/cmw/git/fortytwo");
    let vault = match Vault::discover(Some(vault_path.clone())) {
        Ok(v) => v,
        Err(_) => return,
    };
    if ft_core::git::discover_repo(&vault.path).is_none() {
        return;
    }
    let groups = verify_all(&vault).expect("verify_all on real vault");
    let total_sections: usize = groups.iter().map(|(_, rs)| rs.len()).sum();
    let drifted: usize = groups
        .iter()
        .flat_map(|(_, rs)| rs.iter())
        .filter(|r| !matches!(r.status, ft_core::synth::verify::SectionStatus::Ok))
        .count();
    println!(
        "real_vault_synth_verify_all: {} synth notes, {} sections, {} non-ok",
        groups.len(),
        total_sections,
        drifted
    );
}
