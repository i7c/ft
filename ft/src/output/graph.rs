//! Output formatters for `ft graph query`.
//!
//! Five formats sit on top of `Vec<WalkNode>`:
//!
//! - [`Format::Tree`] / [`Format::Markdown`] for humans
//! - [`Format::Json`] / [`Format::Ndjson`] / [`Format::Edges`] for
//!   downstream tools
//!
//! All formatters consume `&[WalkNode]` directly — no format-specific
//! re-traversal of the graph — and emit cycle nodes (those with
//! [`WalkNode::cycle`] = `true`) with empty children regardless of
//! format, matching the cycle stop semantics of [`super::super::...`]
//! `GraphQuery::walk`.

use std::io::{self, Write};

use ft_core::graph::query::WalkNode;
use ft_core::graph::{EdgeKind, Graph, NodeKind};

/// Output shape. Mirrors the clap value enum the CLI exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    /// Indented ASCII tree. Default for TTY.
    Tree,
    /// One JSON document — pretty-printed array of root objects with
    /// nested `children`.
    Json,
    /// One JSON object per line in depth-first pre-order, each with
    /// `parent_id` instead of nesting. Pipeable into `jq`.
    Ndjson,
    /// Flat `src\tedge_kind\tdst` over every edge traversed,
    /// deduplicated, in pre-order discovery. Pipeable into graphviz
    /// or csvkit.
    Edges,
    /// Bulleted markdown list. Two-space indent per depth.
    Markdown,
}

pub fn render(
    out: &mut impl Write,
    tree: &[WalkNode],
    graph: &Graph,
    format: Format,
) -> io::Result<()> {
    match format {
        Format::Tree => render_tree(out, tree, graph),
        Format::Json => render_json(out, tree, graph),
        Format::Ndjson => render_ndjson(out, tree, graph),
        Format::Edges => render_edges(out, tree, graph),
        Format::Markdown => render_markdown(out, tree, graph),
    }
}

// ── Tree ──────────────────────────────────────────────────────────────

fn render_tree(out: &mut impl Write, tree: &[WalkNode], graph: &Graph) -> io::Result<()> {
    for root in tree {
        write_tree_node(out, root, graph)?;
    }
    Ok(())
}

fn write_tree_node(out: &mut impl Write, node: &WalkNode, graph: &Graph) -> io::Result<()> {
    let indent = "  ".repeat(node.depth);
    let glyph = if node.cycle {
        '↺'
    } else if !node.children.is_empty() {
        '▶'
    } else {
        '·'
    };
    let kind = kind_char(graph.node(node.id));
    let label = display_label(graph.node(node.id));
    writeln!(out, "{indent}{glyph} {kind} {label}")?;
    for child in &node.children {
        write_tree_node(out, child, graph)?;
    }
    Ok(())
}

// ── JSON / NDJSON ─────────────────────────────────────────────────────

fn render_json(out: &mut impl Write, tree: &[WalkNode], graph: &Graph) -> io::Result<()> {
    let roots: Vec<serde_json::Value> = tree.iter().map(|n| node_to_json(n, graph)).collect();
    let doc = serde_json::Value::Array(roots);
    let pretty = serde_json::to_string_pretty(&doc).expect("Value never fails to serialize");
    writeln!(out, "{pretty}")
}

fn node_to_json(node: &WalkNode, graph: &Graph) -> serde_json::Value {
    let kind = graph.node(node.id);
    serde_json::json!({
        "id": node.id.index(),
        "kind": kind_name(kind),
        "path": node_path(kind),
        "title": node_title(kind),
        "depth": node.depth,
        "cycle": node.cycle,
        "edge_to_parent": node.edge_to_parent.as_ref().map(edge_kind_label),
        "children": node.children.iter().map(|c| node_to_json(c, graph)).collect::<Vec<_>>(),
    })
}

fn render_ndjson(out: &mut impl Write, tree: &[WalkNode], graph: &Graph) -> io::Result<()> {
    for root in tree {
        write_ndjson_node(out, root, None, graph)?;
    }
    Ok(())
}

fn write_ndjson_node(
    out: &mut impl Write,
    node: &WalkNode,
    parent_id: Option<usize>,
    graph: &Graph,
) -> io::Result<()> {
    let kind = graph.node(node.id);
    let row = serde_json::json!({
        "id": node.id.index(),
        "parent_id": parent_id,
        "kind": kind_name(kind),
        "path": node_path(kind),
        "title": node_title(kind),
        "depth": node.depth,
        "cycle": node.cycle,
        "edge_to_parent": node.edge_to_parent.as_ref().map(edge_kind_label),
    });
    writeln!(out, "{row}")?;
    for child in &node.children {
        write_ndjson_node(out, child, Some(node.id.index()), graph)?;
    }
    Ok(())
}

// ── Edges ─────────────────────────────────────────────────────────────

fn render_edges(out: &mut impl Write, tree: &[WalkNode], _graph: &Graph) -> io::Result<()> {
    use std::collections::HashSet;

    let mut seen: HashSet<(usize, &'static str, usize)> = HashSet::new();
    for root in tree {
        walk_edges(out, root, &mut seen)?;
    }
    Ok(())
}

fn walk_edges(
    out: &mut impl Write,
    node: &WalkNode,
    seen: &mut std::collections::HashSet<(usize, &'static str, usize)>,
) -> io::Result<()> {
    for child in &node.children {
        // child.edge_to_parent is always Some for non-roots — walk
        // sets it on every descent.
        if let Some(ref ek) = child.edge_to_parent {
            let label = edge_kind_label(ek);
            let key = (node.id.index(), label, child.id.index());
            if seen.insert(key) {
                writeln!(
                    out,
                    "{src}\t{label}\t{dst}",
                    src = node.id.index(),
                    dst = child.id.index(),
                )?;
            }
        }
        walk_edges(out, child, seen)?;
    }
    Ok(())
}

// ── Markdown ──────────────────────────────────────────────────────────

fn render_markdown(out: &mut impl Write, tree: &[WalkNode], graph: &Graph) -> io::Result<()> {
    for root in tree {
        write_markdown_node(out, root, graph)?;
    }
    Ok(())
}

fn write_markdown_node(out: &mut impl Write, node: &WalkNode, graph: &Graph) -> io::Result<()> {
    let indent = "  ".repeat(node.depth);
    let kind = graph.node(node.id);
    let title = node_title(kind);
    let path = node_path(kind);
    let cycle = if node.cycle { " (↺)" } else { "" };
    writeln!(out, "{indent}- {title} ({path}){cycle}")?;
    for child in &node.children {
        write_markdown_node(out, child, graph)?;
    }
    Ok(())
}

// ── Node / edge helpers ───────────────────────────────────────────────

fn kind_char(node: &NodeKind) -> char {
    match node {
        NodeKind::Note(_) => 'N',
        NodeKind::Directory(_) => 'D',
        NodeKind::Ghost(_) => 'G',
    }
}

fn kind_name(node: &NodeKind) -> &'static str {
    match node {
        NodeKind::Note(_) => "Note",
        NodeKind::Directory(_) => "Directory",
        NodeKind::Ghost(_) => "Ghost",
    }
}

fn display_label(node: &NodeKind) -> String {
    match node {
        NodeKind::Note(n) => n
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| n.path.to_string_lossy().into_owned()),
        NodeKind::Directory(d) => {
            if d.path.as_os_str().is_empty() {
                "/".to_string()
            } else {
                format!("{}/", d.name)
            }
        }
        NodeKind::Ghost(g) => g.raw.clone(),
    }
}

fn node_path(node: &NodeKind) -> String {
    match node {
        NodeKind::Note(n) => n.path.to_string_lossy().into_owned(),
        NodeKind::Directory(d) => d.path.to_string_lossy().into_owned(),
        NodeKind::Ghost(g) => g.raw.clone(),
    }
}

fn node_title(node: &NodeKind) -> String {
    match node {
        NodeKind::Note(n) => n.title.clone(),
        NodeKind::Directory(d) => {
            if d.name.is_empty() {
                "/".to_string()
            } else {
                d.name.clone()
            }
        }
        NodeKind::Ghost(g) => g.raw.clone(),
    }
}

fn edge_kind_label(e: &EdgeKind) -> &'static str {
    match e {
        EdgeKind::Link(_) => "link",
        EdgeKind::Embed(_) => "embed",
        EdgeKind::Contains => "directory-contains",
    }
}
