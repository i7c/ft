use anyhow::Result;
use ft_core::task::Task;
use serde::Serialize;

/// Pretty-print all tasks as a JSON array on stdout.
pub fn render(tasks: &[&Task]) -> Result<()> {
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, tasks)?;
    println!();
    Ok(())
}

/// A task plus its nested subtasks, for `--tree` JSON output.
#[derive(Serialize)]
struct TreeNode<'a> {
    #[serde(flatten)]
    task: &'a Task,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subtasks: Vec<TreeNode<'a>>,
}

/// Pretty-print a depth-annotated forest as a nested JSON array. `rows` is in
/// pre-order (each task immediately followed by its subtree); subtasks land in
/// a `subtasks` array on their parent.
pub fn render_tree(rows: &[(usize, &Task)]) -> Result<()> {
    let mut pos = 0;
    let forest = build_nodes(rows, &mut pos, 0);
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, &forest)?;
    println!();
    Ok(())
}

/// Reconstruct nested nodes from a pre-order, depth-annotated row slice.
fn build_nodes<'a>(rows: &[(usize, &'a Task)], pos: &mut usize, depth: usize) -> Vec<TreeNode<'a>> {
    let mut out = Vec::new();
    while let Some(&(d, task)) = rows.get(*pos) {
        if d < depth {
            break;
        }
        *pos += 1;
        let subtasks = build_nodes(rows, pos, depth + 1);
        out.push(TreeNode { task, subtasks });
    }
    out
}
