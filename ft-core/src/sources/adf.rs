//! Atlassian Document Format (Jira Cloud rich text) → markdown, for the
//! node types issue descriptions and comments actually use. Unknown nodes
//! degrade to their text content; conversion never fails.

use serde_json::Value;

/// Render an ADF document (or any ADF node) as markdown.
pub fn to_markdown(doc: &Value) -> String {
    let out = block(doc, 0);
    out.trim_end().to_string()
}

fn children(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn attr<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
    node.get("attrs").and_then(|a| a.get(key))
}

fn kind(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

/// Render a sequence of block nodes separated by blank lines.
fn blocks(nodes: &[Value], depth: usize) -> String {
    nodes
        .iter()
        .map(|n| block(n, depth))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn block(node: &Value, depth: usize) -> String {
    match kind(node) {
        "doc" | "panel" | "expand" | "nestedExpand" | "layoutSection" | "layoutColumn"
        | "mediaSingle" | "mediaGroup" => blocks(children(node), depth),
        "paragraph" => inline(children(node)),
        "heading" => {
            let level = attr(node, "level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            format!("{} {}", "#".repeat(level), inline(children(node)))
        }
        "bulletList" => list(node, depth, |_| "- ".to_string()),
        "orderedList" => {
            let start = attr(node, "order").and_then(Value::as_u64).unwrap_or(1);
            list(node, depth, |i| format!("{}. ", start + i as u64))
        }
        "taskList" | "decisionList" => children(node)
            .iter()
            .map(|item| {
                let done = attr(item, "state").and_then(Value::as_str) == Some("DONE")
                    || attr(item, "state").and_then(Value::as_str) == Some("DECIDED");
                let mark = if done { "☑" } else { "☐" };
                format!("{}- {mark} {}", "  ".repeat(depth), inline(children(item)))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "codeBlock" => {
            let lang = attr(node, "language").and_then(Value::as_str).unwrap_or("");
            let text: String = children(node)
                .iter()
                .filter_map(|t| t.get("text").and_then(Value::as_str))
                .collect();
            format!("```{lang}\n{}\n```", text.trim_end_matches('\n'))
        }
        "blockquote" => blocks(children(node), depth)
            .lines()
            .map(|l| {
                if l.is_empty() {
                    ">".to_string()
                } else {
                    format!("> {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "rule" => "***".to_string(),
        "table" => table(node),
        "blockCard" | "embedCard" => attr(node, "url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "media" => String::new(),
        _ => {
            // Unknown block: inline content if it looks inline, else blocks.
            if children(node).iter().any(|c| kind(c) == "text") {
                inline(children(node))
            } else if let Some(t) = node.get("text").and_then(Value::as_str) {
                t.to_string()
            } else {
                blocks(children(node), depth)
            }
        }
    }
}

fn list(node: &Value, depth: usize, marker: impl Fn(usize) -> String) -> String {
    let pad = "  ".repeat(depth);
    children(node)
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut parts = children(item).iter();
            let first = parts
                .next()
                .map(|b| block(b, depth + 1))
                .unwrap_or_default();
            let mut s = format!("{pad}{}{first}", marker(i));
            for rest in parts {
                let r = block(rest, depth + 1);
                if r.is_empty() {
                    continue;
                }
                s.push('\n');
                if matches!(kind(rest), "bulletList" | "orderedList" | "taskList") {
                    s.push_str(&r);
                } else {
                    let inner = "  ".repeat(depth + 1);
                    s.push_str(
                        &r.lines()
                            .map(|l| format!("{inner}{l}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn table(node: &Value) -> String {
    let rows: Vec<Vec<String>> = children(node)
        .iter()
        .map(|row| {
            children(row)
                .iter()
                .map(|cell| {
                    blocks(children(cell), 0)
                        .replace('\n', " ")
                        .replace('|', "\\|")
                })
                .collect()
        })
        .collect();
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        out.push(format!("| {} |", row.join(" | ")));
        if i == 0 {
            out.push(format!("|{}|", vec![" --- "; row.len()].join("|")));
        }
    }
    out.join("\n")
}

fn inline(nodes: &[Value]) -> String {
    nodes.iter().map(inline_node).collect()
}

fn inline_node(node: &Value) -> String {
    match kind(node) {
        "text" => {
            let mut s = node
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let marks = node
                .get("marks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for m in &marks {
                s = match kind(m) {
                    "strong" => format!("**{s}**"),
                    "em" => format!("*{s}*"),
                    "code" => format!("`{s}`"),
                    "strike" => format!("~~{s}~~"),
                    "link" => {
                        let href = attr(m, "href").and_then(Value::as_str).unwrap_or("");
                        format!("[{s}]({href})")
                    }
                    _ => s,
                };
            }
            s
        }
        "hardBreak" => "\n".to_string(),
        "mention" => {
            let t = attr(node, "text")
                .and_then(Value::as_str)
                .unwrap_or("someone");
            if t.starts_with('@') {
                t.to_string()
            } else {
                format!("@{t}")
            }
        }
        "emoji" => attr(node, "text")
            .or_else(|| attr(node, "shortName"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "inlineCard" => attr(node, "url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "date" => attr(node, "timestamp")
            .and_then(Value::as_str)
            .and_then(|ms| ms.parse::<i64>().ok())
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| d.date_naive().to_string())
            .unwrap_or_default(),
        "status" => attr(node, "text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => node
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| inline(children(node))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(text: &str) -> Value {
        json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
    }

    #[test]
    fn mixed_document() {
        let doc = json!({"type": "doc", "version": 1, "content": [
            {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Heading"}]},
            {"type": "bulletList", "content": [
                {"type": "listItem", "content": [{"type": "paragraph", "content": [
                    {"type": "text", "text": "item", "marks": [{"type": "strong"}]}]}]},
                {"type": "listItem", "content": [p("two"), {"type": "bulletList", "content": [
                    {"type": "listItem", "content": [p("nested")]}]}]}
            ]},
            {"type": "codeBlock", "attrs": {"language": "rust"}, "content": [{"type": "text", "text": "fn main() {}"}]},
            {"type": "paragraph", "content": [
                {"type": "mention", "attrs": {"id": "x", "text": "@Jane Doe"}},
                {"type": "text", "text": " see "},
                {"type": "text", "text": "docs", "marks": [{"type": "link", "attrs": {"href": "https://example.test"}}]},
                {"type": "hardBreak"},
                {"type": "text", "text": "x", "marks": [{"type": "code"}, {"type": "em"}]}
            ]},
            {"type": "taskList", "attrs": {"localId": "t"}, "content": [
                {"type": "taskItem", "attrs": {"state": "TODO"}, "content": [{"type": "text", "text": "task text"}]},
                {"type": "taskItem", "attrs": {"state": "DONE"}, "content": [{"type": "text", "text": "done text"}]}
            ]},
            {"type": "rule"},
            {"type": "blockquote", "content": [p("quoted")]},
        ]});
        assert_eq!(
            to_markdown(&doc),
            "# Heading\n\n- **item**\n- two\n  - nested\n\n```rust\nfn main() {}\n```\n\n@Jane Doe see [docs](https://example.test)\n*`x`*\n\n- ☐ task text\n- ☑ done text\n\n***\n\n> quoted"
        );
    }

    #[test]
    fn ordered_list_table_and_unknown_nodes() {
        let doc = json!({"type": "doc", "content": [
            {"type": "orderedList", "attrs": {"order": 3}, "content": [
                {"type": "listItem", "content": [p("a")]}, {"type": "listItem", "content": [p("b")]}]},
            {"type": "table", "content": [
                {"type": "tableRow", "content": [
                    {"type": "tableHeader", "content": [p("h1")]}, {"type": "tableHeader", "content": [p("h|2")]}]},
                {"type": "tableRow", "content": [
                    {"type": "tableCell", "content": [p("c1")]}, {"type": "tableCell", "content": [p("c2")]}]}
            ]},
            {"type": "panel", "content": [p("in panel")]},
            {"type": "somethingNew", "content": [{"type": "text", "text": "still text"}]},
            {"type": "paragraph", "content": [{"type": "inlineCard", "attrs": {"url": "https://example.test/x"}}]}
        ]});
        assert_eq!(
            to_markdown(&doc),
            "3. a\n4. b\n\n| h1 | h\\|2 |\n| --- | --- |\n| c1 | c2 |\n\nin panel\n\nstill text\n\nhttps://example.test/x"
        );
    }

    #[test]
    fn null_or_empty_is_empty() {
        assert_eq!(to_markdown(&Value::Null), "");
        assert_eq!(to_markdown(&json!({"type": "doc", "content": []})), "");
    }
}
