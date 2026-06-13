use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use ft_core::task::{Priority, Status, Task};

pub struct TableOpts {
    pub use_color: bool,
}

pub fn render(tasks: &[&Task], opts: TableOpts) -> String {
    let rows: Vec<TreeRow> = tasks.iter().map(|t| (0usize, *t)).collect();
    render_one(&rows, opts)
}

/// One rendered row: its nesting depth (0 = top level) and the task. The
/// depth indents the description column so subtasks read as a tree.
pub type TreeRow<'a> = (usize, &'a Task);

/// Render a depth-annotated forest (used by `tasks list --tree`). Identical
/// to [`render`] except the description column carries an indent per depth.
pub fn render_tree(rows: &[TreeRow], opts: TableOpts) -> String {
    render_one(rows, opts)
}

/// Render multiple labelled groups, separated by section headings.
pub fn render_grouped(groups: &[(String, Vec<&Task>)], opts: TableOpts) -> String {
    let mut out = String::new();
    for (i, (label, tasks)) in groups.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("## {label} ({})\n", tasks.len()));
        let rows: Vec<TreeRow> = tasks.iter().map(|t| (0usize, *t)).collect();
        out.push_str(&render_one(
            &rows,
            TableOpts {
                use_color: opts.use_color,
            },
        ));
        out.push('\n');
    }
    out
}

fn render_one(rows: &[TreeRow], opts: TableOpts) -> String {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Status", "Pri", "Due", "Description", "Path", "Tags"]);

    for &(depth, task) in rows {
        let status_cell = status_cell(task.status, opts.use_color);
        let pri_cell = priority_cell(task.priority, opts.use_color);
        let due_cell = match task.due {
            Some(d) => Cell::new(d.to_string()),
            None => Cell::new(""),
        };
        let mut path = task.source_file.display().to_string();
        path.push(':');
        path.push_str(&task.source_line.to_string());
        let tags = task.tags.join(", ");
        let desc = if depth > 0 {
            format!("{}↳ {}", "  ".repeat(depth - 1), task.description)
        } else {
            task.description.clone()
        };
        let mut row: Vec<Cell> = vec![
            status_cell,
            pri_cell,
            due_cell,
            Cell::new(desc),
            Cell::new(path),
            Cell::new(tags),
        ];
        if opts.use_color && task.status == Status::Done {
            for cell in &mut row {
                *cell = std::mem::replace(cell, Cell::new("")).fg(Color::DarkGrey);
            }
        }
        table.add_row(row);
    }
    table.to_string()
}

fn status_cell(status: Status, color: bool) -> Cell {
    let label = match status {
        Status::Open => "[ ]",
        Status::Done => "[x]",
        Status::InProgress => "[/]",
        Status::Cancelled => "[-]",
    };
    let mut cell = Cell::new(label);
    if color {
        cell = match status {
            Status::Done => cell.fg(Color::Green),
            Status::InProgress => cell.fg(Color::Yellow),
            Status::Cancelled => cell.fg(Color::DarkGrey),
            Status::Open => cell,
        };
    }
    cell
}

fn priority_cell(priority: Option<Priority>, color: bool) -> Cell {
    let label = match priority {
        Some(Priority::Highest) => "!!!",
        Some(Priority::High) => "!!",
        Some(Priority::Medium) => "!",
        Some(Priority::Low) => "v",
        Some(Priority::Lowest) => "vv",
        None => "",
    };
    let mut cell = Cell::new(label);
    if color {
        cell = match priority {
            Some(Priority::Highest) => cell.fg(Color::Red),
            Some(Priority::High) => cell.fg(Color::Red),
            Some(Priority::Medium) => cell.fg(Color::Yellow),
            _ => cell,
        };
    }
    cell
}
