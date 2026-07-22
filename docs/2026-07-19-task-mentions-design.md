# Task → concept: `OwnsTask` edge + `mentions` attribute

*2026-07-19. Design note for closing the §2 gap from
[2026-07-19-premise-review.md](2026-07-19-premise-review.md).*

## The gap (recap)

The task thesis says tasks belong in `ft` because they arise *during*
note-taking — so a task should be queryable by the concept it arose
from. Today the task query surface (`Profile::Tasks` over the unified
DSL) can see task fields (`status`, `due`, …) and graph fields
(`indegree`, …) but has **no predicate for "the concept this task's
paragraph mentions."** A `Task` node has only `HasTask` (up to its note)
and `Subtask` (down to children) edges; it has no edge to its owning
paragraph, and `ParagraphLink` edges (paragraph → concept) originate
only from `Paragraph` nodes.

The path that exists today is a value-match lookup, not a graph walk:

```
Task  --(source_file + line ∈ [line_start, line_end])-->  owning Paragraph  →[ParagraphLink]→  concept
```

The fix is to make the first hop a real edge, in the same direction as
every other ownership edge in the graph.

## The change

### 1. Scanner skip (invariant fix, ships independently)

The `OwnsTask` edge is total only if **every task lands in exactly one
paragraph.** Today this invariant does *not* hold: `parse_file`
(`ft-core/src/vault.rs`) iterates every line with no skip logic, so a
`- [ ]` line inside a fenced code block or in frontmatter is parsed as
a task, while `extract_paragraphs` *does* skip those regions (via
`LineSkipState`). A task in a code block has no owning paragraph.

This is a latent bug independent of the edge question — `- [ ]` lines
in code blocks are examples, not tasks. Fix: make `parse_file` use the
same `LineSkipState` as `extract_paragraphs` / `extract_headings`:

```rust
// ft-core/src/vault.rs::parse_file
let mut tasks = Vec::new();
let mut state = LineSkipState::new();        // new
for (lineno, line) in content.lines().enumerate() {
    if state.skip_line(line) {               // new
        continue;
    }
    let ctx = ParseContext { source_file: rel.clone(), source_line: lineno + 1 };
    if let Some(task) = EmojiFormat.parse_line(line, ctx) {
        tasks.push(task);
    }
}
```

`LineSkipState` is `pub(crate)` in `markdown.rs`; already accessible.
After this, the two extractors and the task scanner share identical
skip rules, and the invariant holds by construction.

### 2. `OwnsTask` edge

New edge kind, mirroring the existing ownership family:

```rust
// ft-core/src/graph/mod.rs
/// A paragraph owns a task line within its `[line_start, line_end]`
/// range. Edge from paragraph → task. Total over tasks once the
/// scanner-skip invariant (§1) holds: every task lands in exactly one
/// paragraph. Distinct from `HasTask` (note → top-level task only)
/// and `Subtask` (task → task).
OwnsTask,
```

Add to the `EdgeKind::link()` `None` arm alongside the other unit
variants (`Contains`, `HasTask`, `Subtask`, `LinksInto`, `OwnsParagraph`,
`OwnsHeading`).

### 3. Edge creation

New private method, mirroring `insert_hastask_edges` / `insert_subtask_edges`:

```rust
fn insert_ownstask_edges(&mut self, tasks: &[Task]) {
    // Group tasks by source file so we walk each note's paragraphs
    // once. paragraph_index is keyed by (file, line_start); we can't
    // do a single lookup for "which paragraph contains line L" without
    // a range index, but a per-note scan over note_paragraphs(note)
    // (which are sorted by line_start) is O(tasks_in_note +
    // paragraphs_in_note) per note — fine for a build-time one-shot.
    let mut by_file: HashMap<PathBuf, Vec<&Task>> = HashMap::new();
    for task in tasks {
        by_file
            .entry(normalize_path(&task.source_file))
            .or_default()
            .push(task);
    }
    for (file, file_tasks) in by_file {
        let Some(&note_id) = self.path_index.get(&file) else { continue };
        let paragraphs = self.note_paragraphs(note_id); // sorted by line_start
        // For each paragraph, find tasks in its range.
        // (Implementation: since both lists are sorted by line, a
        // merge-style walk is O(n+m); a nested loop is also acceptable
        // for build-time cost.)
        for p_id in ¶graphs {
            let NodeKind::Paragraph(p) = self.node(*p_id) else { continue };
            for task in &file_tasks {
                let line = task.source_line as u32;
                if line >= p.line_start && line <= p.line_end {
                    if let Some(&task_id) = self.task_index
                        .get(&(file.clone(), task.source_line))
                    {
                        self.g.add_edge(p_id.0, task_id.0, EdgeKind::OwnsTask);
                    }
                }
            }
        }
    }
}
```

Called from `Graph::build` after `insert_task_node` and
`insert_hastask_edges` / `insert_subtask_edges` (needs both
`paragraph_index` and `task_index` populated):

```rust
// Graph::build, after the existing task-edge insertions:
graph.insert_ownstask_edges(&scan.tasks);
```

### 4. `mentions` attribute

New `Attr` variant, generalized across node kinds:

```rust
// ft-core/src/graph/query/mod.rs
pub enum Attr {
    // ... existing ...
    /// Does this node (or, for `Task`, its owning paragraph) link to a
    /// target whose title/raw matches the value? Walks the link edges
    /// originating from the node — `ParagraphLink` for `Paragraph`,
    /// `HeadingLink` for `Heading`, `NoteLink` for `Note`, and the
    /// owning paragraph's `ParagraphLink` edges for `Task`.
    Mentions,
}
```

`parse_attr`: `"mentions" => Ok(Attr::Mentions)`.
`value_type`: `ValueType::Str` (compares against target title/raw).
`is_optional`: `false`.
`validate_attr_subject`: node-only (reject `Subject::Edge`), like
`Path`/`Title`.

Eval arm in `eval.rs::eval_cond_on_node`:

```rust
Attr::Mentions => {
    let mentioned: HashSet<String> = mentioned_targets(graph, id);
    match c.op {
        Op::Eq | Op::Includes => {
            // `mentions = "onboarding"` / `mentions includes "onboarding"`
            let want = match &c.value {
                Value::Single(lit) => literal_as_str(lit),
                _ => return false,
            };
            mentioned.iter().any(|m| m == want)
        }
        Op::In => {
            // `mentions in {"onboarding", "analytics"}`
            let wants: Vec<&str> = match &c.value {
                Value::Set(lits) => lits.iter().map(literal_as_str).collect(),
                _ => return false,
            };
            mentioned.iter().any(|m| wants.contains(&m.as_str()))
        }
        Op::NotEq => {
            let want = match &c.value {
                Value::Single(lit) => literal_as_str(lit),
                _ => return false,
            };
            !mentioned.iter().any(|m| m == want)
        }
        _ => false,
    }
}
```

Helper:

```rust
/// The set of concept target strings this node mentions. For `Task`,
/// walks the owning paragraph's `ParagraphLink` edges; for `Paragraph`
/// and `Heading`, walks their own link edges; for `Note`, walks its
/// `NoteLink` edges. Empty for `Ghost` / `Directory`.
fn mentioned_targets(graph: &Graph, id: NoteId) -> HashSet<String> {
    let mut out = HashSet::new();
    match graph.node(id) {
        NodeKind::Task(_) => {
            // Up to owning paragraph via incoming OwnsTask, then out
            // via ParagraphLink.
            for (owner, _) in graph.incoming(id) {
                if !matches!(graph.node(owner), NodeKind::Paragraph(_)) { continue; }
                if !graph.edge_kind(owner, id).is_some_and(|e| matches!(e, EdgeKind::OwnsTask)) {
                    continue;
                }
                collect_link_targets(graph, owner, &mut out);
            }
        }
        NodeKind::Paragraph(_) => collect_link_targets(graph, id, &mut out),
        NodeKind::Heading(_) => {
            // HeadingLink originates from the heading; also
            // ParagraphLink from the paragraph at the same line
            // (Fork A2). Union both for "mentions" completeness.
            collect_link_targets(graph, id, &mut out);
        }
        NodeKind::Note(_) => collect_note_link_targets(graph, id, &mut out),
        NodeKind::Ghost(_) | NodeKind::Directory(_) => {}
    }
    out
}

fn collect_link_targets(graph: &Graph, id: NoteId, out: &mut HashSet<String>) {
    for (target, edge) in graph.outgoing(id) {
        if let Some(link) = edge.link() {
            match graph.node(target) {
                NodeKind::Note(n) => { out.insert(n.title.clone()); }
                NodeKind::Ghost(g) => { out.insert(g.raw.clone()); }
                NodeKind::Heading(h) => { out.insert(h.text.clone()); }
                _ => {}
            }
        }
    }
}
```

## Two open questions for the design

1. **`Note` granularity.** Should `mentions` on a `Note` walk `NoteLink`
   (note-level, one edge per occurrence) or the union of its paragraphs'
   `ParagraphLink` edges? For a pure existence check they're the same
   set — every `ParagraphLink` is also represented as a `NoteLink` at
   note level. Walking `NoteLink` is cheaper (one hop, no paragraph
   descent). **Recommendation: `NoteLink`**, since it's the natural
   note-level signal and `ParagraphLink` adds nothing for an existence
   check. If counting matters later, revisit.

2. **Target matching — title/raw/both.** A `ParagraphLink` edge targets
   a `NoteId` that may point at a `Note` (resolved) or a `Ghost`
   (unresolved). `mentions = "onboarding"` should match either the
   note's `title` or the ghost's `raw`. **Recommendation: match both**,
   so an unresolved `[[onboarding]]` still counts. The helper above
   already does this.

   Sub-question: aliases. A wikilink `[[onboarding|onboarding-flow]]`
   has `display = Some("onboarding-flow")` but the target is still the
   `onboarding` note. Should `mentions = "onboarding-flow"` match? The
   edge target is the note, so matching against `note.title` says no —
   `mentions` is about *which concept*, not *which alias was used*.
   **Recommendation: match concept (title/raw), not alias.** If alias
   matching is wanted, it's a separate attribute (`alias = "..."`).

## What this unlocks

The §2 "impossible query" becomes possible:

```
ft tasks list --query 'mentions = "onboarding" and due < today'
```

Under `Profile::Tasks`, the prelude makes this
`node where kind = Task and mentions = "onboarding" and due < today`,
and the eval walks `Task →[OwnsTask]→ Paragraph →[ParagraphLink]→ concept`.

And because `mentions` is generalized (not task-specific), the same
attribute works on graph queries: `node where kind = Paragraph and
mentions = "onboarding"` — which is just a more explicit way of saying
the same thing the paragraph's `ParagraphLink` edges already encode,
but now expressible in the DSL.

## Out of scope (deliberately)

- **No new task-only edge to concepts.** The review's "option 2"
  (TaskMentions, a direct task→concept edge) is rejected: it
  duplicates `ParagraphLink` information and requires maintaining
  parity. `OwnsTask` + the existing `ParagraphLink` is the right
  shape — one source of truth for "what concepts does this text
  mention," reached from tasks via the ownership edge.
- **No `refresh_note` task updates.** `refresh_note` doesn't touch
  task nodes today (only `build` does — see `insert_task_node`,
  `insert_hastask_edges`, `insert_subtask_edges`). `OwnsTask` will
  have the same behavior: created at `build` time only. This is a
  pre-existing limitation, not introduced by this change, and
  fixing it is a separate concern.
- **No counting.** `mentions` is a boolean existence predicate. If
  "how many times does this paragraph mention X" becomes wanted,
  it's a separate attribute.

## Build invariants touched

- `cargo build --release` — new edge variant, new attr, new eval arm.
- `cargo test --workspace` — new tests: scanner skip, `OwnsTask` edge
  creation, `mentions` eval on each node kind.
- `cargo clippy --workspace --tests -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- `ft commands docs --check` — unaffected (no keymap/command changes).
