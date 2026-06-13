//! Markdown output: emit each task as a serialized source line so the result
//! can be piped back into a vault as a valid markdown task list.

use ft_core::task::emoji::EmojiFormat;
use ft_core::task::{format::TaskFormat, Task};

pub fn render(tasks: &[&Task]) -> String {
    let fmt = EmojiFormat;
    let mut out = String::new();
    for task in tasks {
        out.push_str(&fmt.serialize_line(task));
        out.push('\n');
    }
    out
}

/// Render a depth-annotated forest as a nested markdown task list. Each
/// depth adds two spaces of list indentation, so the output is valid nested
/// markdown that round-trips back into a vault. `rows` pairs each task with
/// its nesting depth (0 = top level).
pub fn render_tree(rows: &[(usize, &Task)]) -> String {
    let fmt = EmojiFormat;
    let mut out = String::new();
    for &(depth, task) in rows {
        out.push_str(&"  ".repeat(depth));
        out.push_str(&fmt.serialize_line(task));
        out.push('\n');
    }
    out
}
