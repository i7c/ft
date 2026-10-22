//! Newline-delimited JSON output: one task per line, each line a self-contained
//! JSON object. Pipeable into `jq`, `gron`, log processors, etc.

use anyhow::Result;
use ft_core::task::Task;
use std::io::Write;

pub fn render(tasks: &[&Task]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    for task in tasks {
        serde_json::to_writer(&mut out, task)?;
        writeln!(out)?;
    }
    Ok(())
}

/// A `--tree` row: the task plus its depth and the parent this occurrence
/// hangs under, so streaming consumers can rebuild the DAG (a task with
/// several parents appears once under each).
#[derive(serde::Serialize)]
struct TreeRow<'a> {
    #[serde(flatten)]
    task: &'a Task,
    depth: usize,
    via: Option<serde_json::Value>,
}

/// Pre-order `--tree` rows (`rows[i]` hangs under `via[i]`), one per line.
pub fn render_tree(rows: &[(usize, &Task)], via: &[Option<&Task>]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    for (&(depth, task), parent) in rows.iter().zip(via) {
        let row = TreeRow {
            task,
            depth,
            via: parent
                .map(|p| serde_json::json!({ "path": p.source_file, "line": p.source_line })),
        };
        serde_json::to_writer(&mut out, &row)?;
        writeln!(out)?;
    }
    Ok(())
}
