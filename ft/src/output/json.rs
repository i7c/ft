use anyhow::Result;
use ft_core::task::context::{ContextIndex, TaskContext};
use ft_core::task::Task;
use serde::Serialize;

/// A listed task: every [`Task`] key, then the [`TaskContext`] keys
/// (`headings`, `mentions`, `line_mentions`, `note`). Only `tasks list`
/// prints these; the rows writes print are bare `Task`s.
#[derive(Serialize)]
pub struct ListRow<'a> {
    #[serde(flatten)]
    pub task: &'a Task,
    #[serde(flatten)]
    pub ctx: TaskContext,
}

impl<'a> ListRow<'a> {
    pub fn new(task: &'a Task, index: &ContextIndex) -> Self {
        Self {
            task,
            ctx: index.for_task(task),
        }
    }
}

/// Pretty-print all tasks as a JSON array on stdout.
pub fn render(tasks: &[&Task], index: &ContextIndex) -> Result<()> {
    let rows: Vec<ListRow> = tasks.iter().map(|t| ListRow::new(t, index)).collect();
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, &rows)?;
    println!();
    Ok(())
}

/// A task plus its nested subtasks, for `--tree` JSON output.
#[derive(Serialize)]
struct TreeNode<'a> {
    #[serde(flatten)]
    row: ListRow<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subtasks: Vec<TreeNode<'a>>,
}

/// Pretty-print a depth-annotated forest as a nested JSON array. `rows` is in
/// pre-order (each task immediately followed by its subtree); subtasks land in
/// a `subtasks` array on their parent.
pub fn render_tree(rows: &[(usize, &Task)], index: &ContextIndex) -> Result<()> {
    let mut pos = 0;
    let forest = build_nodes(rows, &mut pos, 0, index);
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, &forest)?;
    println!();
    Ok(())
}

/// Reconstruct nested nodes from a pre-order, depth-annotated row slice.
fn build_nodes<'a>(
    rows: &[(usize, &'a Task)],
    pos: &mut usize,
    depth: usize,
    index: &ContextIndex,
) -> Vec<TreeNode<'a>> {
    let mut out = Vec::new();
    while let Some(&(d, task)) = rows.get(*pos) {
        if d < depth {
            break;
        }
        *pos += 1;
        let subtasks = build_nodes(rows, pos, depth + 1, index);
        out.push(TreeNode {
            row: ListRow::new(task, index),
            subtasks,
        });
    }
    out
}
