## Context

`ft` already has the load-bearing pieces this change builds on: a graph with `Paragraph` nodes and `ParagraphLink` edges (`paragraph-graph` spec), a single-target journal (`notes-journal`) backed by `blame_cache` (`blame-cache` spec), a Journal TUI tab (`journal-tui-tab`), and atomic file writes via `fs::write_atomic`. The vault is expected to be a git repo (already assumed by blame). The user's workflow is "quick-capture + post-connection" and the missing tooling is the post-connecting ritual itself — surfacing recently-mentioned `[[wikilinks]]`, aggregating cross-vault context for selected ones, and producing synthesis notes whose excerpts are pinned to verifiable source locations.

The design is constrained by the load-bearing patterns in CLAUDE.md: plan/apply split for mutations, TUI single-threaded + mpsc, vault-relative paths in user-facing output, and explicit budgeting for signature changes on widely-called core APIs (`build_journal` is one such — see Risks).

## Goals / Non-Goals

**Goals:**
- One coherent ritual flow: review → multi-source journal → synthesize → editor handoff.
- Synth notes that remain readable in Obsidian/any markdown viewer with no `ft` tooling installed.
- Provenance that is verifiable offline (`ft synth verify`) — quoted text must match the git blob at the pinned commit, byte-for-byte after stripping the callout's `> ` prefix.
- Generalize the journal once, in `ft-core`, so the CLI and TUI inherit multi-target behavior in lock-step (per the "if TUI needs it, add to ft-core first" rule in CLAUDE.md).
- All four build invariants stay green: `cargo build --release`, `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, `cargo fmt --check`.

**Non-Goals:**
- A "rebase/refresh" affordance for drifted protected sections (intentionally deferred to keep v1 surface area small — `verify` reports drift, user fixes by hand).
- An insertion-point picker when extending a synth note (always-append for v1, can reuse the existing move-section picker later).
- Interactive TUI picker as a flag of `ft synth` (`--interactive`). The TUI path already covers interactive selection via Review → Journal → send-to-synth; a CLI-launched picker is duplicative for v1.
- Smarter dedup when a paragraph was rewritten between the pinned commit and HEAD (current paragraph-index lookup is HEAD-relative and acceptable).
- AI-driven summarization or any generative step. Synthesis is the user's prose around quoted excerpts.

## Decisions

### D1. Two-engine architecture
Engine 1 (graph + blame, generalized) drives the journal the user reads. Engine 2 (git-log scan) drives the link-review screen and the optional in-window filter.

**Alternative considered:** Use git-log everywhere — also build the journal from added lines. Rejected because the journal's purpose during synthesis is "what does the vault currently say about [[Foo]]," which is fundamentally a HEAD-state query. Forcing it through a diff loses the all-time scope and re-implements paragraph extraction from raw added lines instead of reusing the graph's existing paragraph index.

**Alternative considered:** Generalize only the journal and skip Engine 2 — derive new-link counts from a "first-seen blame" sweep. Rejected because blame's first-seen is the *file*'s first commit touching the line, not "when this `[[Link]]` was added," and rewrites would muddy the signal. A direct git-log scan is more honest.

### D2. Paragraph-level frequency for link review
`(link, paragraph)` pairs are deduped. Same `[[Foo]]` twice in one paragraph counts once; in two paragraphs of the same note counts twice. Matches the user's mental model ("how many distinct thoughts mentioned this") and aligns with the graph's `ParagraphLink` granularity.

### D3. HEAD-relative paragraph mapping for Engine 2
For each added line containing `[[Link]]` in `git log -p X..Y`, look up the containing paragraph in the **current HEAD paragraph index** (via `graph.paragraph_by_loc` or a fresh `extract_paragraphs` over the HEAD file content). The line numbers in the diff hunk header refer to the post-state of *each commit*, not HEAD, so line numbers may have shifted by HEAD time.

**Mapping strategy:** for each `(commit, path, added_line)` triple, read the path at HEAD, run `extract_paragraphs`, and find the paragraph whose `line_start..=line_end` contains `added_line`. If the file or paragraph has been deleted/rewritten such that no current paragraph contains that line, fall back to "synthetic paragraph" keyed by `(path, original_added_line)` so the link still counts but isn't dedup'd with current paragraphs. This preserves the count even when source content moves; precision loss is acceptable per the Non-Goals.

**Alternative considered:** Per-commit paragraph index reconstruction. Rejected as expensive (extract paragraphs at every commit in the window) for marginal precision gain.

### D4. Callout grammar
```
> [!ft-source] <vault-rel-path> L<a>-<b> @<sha7> #<hash6>
> <paragraph line 1>
> <paragraph line 2>
> ...
```

- Callout type literal: `ft-source`. Renders as a labeled callout in Obsidian and is the parsing anchor for `ft`.
- Header tokens in fixed order. Regex (approximate): `^>\s*\[!ft-source\]\s+(\S+)\s+L(\d+)-(\d+)\s+@([0-9a-f]{7,40})\s+#([0-9a-f]{6,})\s*$`. The regex tolerates longer hash prefixes if a user widens them by hand, but the scaffold writer emits the canonical 7/6 lengths.
- Body lines: each line is `> ` followed by the original paragraph line. Empty paragraph lines (cannot occur — paragraphs by definition exclude blanks) are not produced.
- One callout = one source paragraph. Adjacent paragraphs from the same source produce adjacent callouts, each with its own header — never fused.
- Verification: strip `> ` prefix from each body line, join with `\n`, compare to the git blob slice at `(@sha7, vault-rel-path, La..=Lb)` joined the same way. Independently compute blake3 of the stripped text and check the first 6 hex chars match `#hash6`. Both must match for `ok`; either failing yields `drifted`.

**Alternative considered:** HTML-comment delimited blockquotes. Rejected because comments leak visibly in some viewers (GitHub) and the structured comment markup is more verbose without a parsing benefit.

**Alternative considered:** Fenced block with magic info string. Rejected because excerpts render as code blocks, losing the prose feel of a quoted paragraph.

### D5. Content hash = blake3 prefix
6 hex chars (24 bits). False-positive probability negligible at vault scale (synth notes have ~10s of sections, not millions). Storage cost trivial; verification cost is comparing a 6-char string. blake3 chosen for speed and existing-in-ecosystem reasons; not security-critical so any modern hash would work.

### D6. Synth-note identification via frontmatter marker
`ft-synth: true` in YAML frontmatter. `ft` parses frontmatter (already required for periodic notes) and treats this as a tag on the note. Used for:
- `ft synth verify --all` enumeration.
- Filtering links inside `[!ft-source]` callouts from the link-review count (next ritual doesn't see recycled material).
- Future filters in TUI views (graph tab, etc.) — not required for v1 but the marker is the seam.

**Alternative considered:** Quarantine into a separate directory excluded from `scan()`. Rejected because synth notes *are* notes — they should participate in the graph so backlinks and links inside them (user's own prose) work normally.

### D7. Two-layer link exclusion
Two scopes, complementary:
- **Whole-note level:** the `synth.exclude_prefixes` config filter excludes path prefixes from the link-review entirely (periodic notes by default).
- **Section level:** when computing the link-review count, links inside `[!ft-source]` callouts in synth notes are skipped. Links in the user's prose between callouts ARE counted (genuine new connections worth surfacing next ritual).

The graph itself doesn't need to know about callouts; the link-review pass tags each `[[Link]]` occurrence with whether it falls within the line range of a callout in a synth-marked note and skips those.

### D8. Multi-target journal API
Change signature:
```rust
pub fn build_journal(
    graph: &Graph,
    targets: &[NoteId],
    vault: &Vault,
    repo: &Path,
    cache: &mut BlameCache,
) -> Result<JournalReport>
```

Behavior changes:
- `targets.len() == 1` → preserves current semantics including Related-aliases resolution and "exclude N's own paragraphs."
- `targets.len() > 1` → no Related-aliases resolution (the multi-target use case is "I picked these links explicitly, don't expand further"); no self-exclusion (multiple targets have no single "self"). Caller can pre-filter.
- `JournalEntry` gains `matched: Vec<NoteId>` — the subset of `targets` that the paragraph linked to. Single-target case sets `matched = vec![targets[0]]` and renderers can ignore the field.
- Sort is unchanged: date desc, title asc tiebreak.

Every current caller passes a one-element slice. The CLAUDE.md "signature changes ripple" caveat applies here: this is one of those changes. Mitigation: ripple is mechanical (`build_journal(g, &[id], ...)`) and the only existing callers are `ft notes journal` CLI, the Journal TUI tab, and the journal's own test module. We accept the ripple over a struct-params shim — the new param is the central abstraction, not an option.

### D9. In-window filter as a post-pass
After `build_journal` returns the all-time set, apply an optional in-window filter: keep entries whose `(source_file, line_start..=line_end)` overlaps any added-line tuple recorded by Engine 2 for the same window. Implementation: Engine 2 already builds a `HashMap<PathBuf, BTreeSet<u32>>` of added lines per file for the window; the filter is a per-entry lookup. CLI flag `--in-window`; TUI key (proposed `w` on the Journal tab).

### D10. Plan/apply split for synth mutations
Per CLAUDE.md, mutations go through a planner returning a plan struct and an applier that calls `write_atomic`. New module `ft-core/src/synth/scaffold.rs`:

```rust
pub struct SynthScaffoldPlan {
    pub target: PathBuf,                 // vault-relative
    pub create: bool,                    // true if note doesn't exist
    pub frontmatter_to_add: Option<String>, // None if file exists with marker
    pub sections: Vec<ProtectedSection>, // in scaffold order (date desc)
    pub append_offset: Option<usize>,    // byte offset for append; None when create=true
}

pub fn plan_synth_scaffold(
    graph: &Graph,
    vault: &Vault,
    repo: &Path,
    target: &Path,
    entries: &[JournalEntry],
) -> Result<SynthScaffoldPlan>;

pub fn apply_synth_scaffold(vault: &Vault, plan: &SynthScaffoldPlan) -> Result<()>;
```

The applier:
- Creates parent dirs as needed.
- If `create`, writes the full file (frontmatter + sections) atomically.
- If extending, appends `\n\n` + serialized sections at end of file via `write_atomic` (read-modify-write into a same-dir tempfile).
- Returns the path that `$EDITOR` should be opened at (caller responsibility) with the line number = first line after appended content.

### D11. CLI surfaces
- `ft review` is a new top-level command (parallel to `ft journal`-style verbs). Default output is a table: `count | link | ghost?`. `--json` for scripting.
- `ft journal` already exists as `ft notes journal <note>`. Add a top-level alias OR keep it as `ft notes journal` and add the new `--link` flag there. **Decision: keep at `ft notes journal`** for consistency with the existing structure; `--link` is mutually exclusive with positional `<note>`. Per CLAUDE.md "Where to add things," new flags on existing commands stay in the same command file. Update `ft/src/cmd/notes.rs` accordingly.
- `ft synth` is a new top-level command with subcommands: `ft synth <target.md> ...` (default = scaffold) and `ft synth verify ...`. Use clap subcommands.

### D12. TUI Review tab
New tab `ft/src/tui/tabs/review.rs`. Slots after Journal in the tab strip (so position order is Graph, Tasks, Notes, Timeblocks, Journal, Review). On focus, runs Engine 2 over the default window (configurable; default `--since 7d` equivalent). Body shows a list:

```
(3) [[Foo]]
(2) [[Bar]]?
(1) [[Baz]]
```

- `<space>` toggles selection.
- `<enter>` queues selected link targets to the Journal tab and switches focus.
- `<` / `>` adjusts window (e.g., `-7d` / `+7d` increments) — exact key binding TBD during implementation.
- `?` help overlay via `Tab::help_sections`.
- Like the Journal tab, holds its own data + cache; loads via a background worker on focus (same single-threaded + mpsc pattern, see existing journal tab for reference).

### D13. TUI Journal tab additions
- `queued_targets: Option<Vec<NoteId>>` alongside the existing `queued_journal_for_path`.
- Renderer detects multi-target mode when `targets.len() > 1` and shows a `matched: Foo, Bar` badge per entry whose `matched` field has >1 element.
- Key `w` toggles in-window-only (no-op when no window context is present; the Review tab's handoff carries the window range).
- `<space>` selects entries; new key (proposed `s`) opens a small inline prompt for target note (fuzzy picker over existing synth-marked notes + "new note" option). On confirm, calls `plan_synth_scaffold` + `apply_synth_scaffold` and triggers editor handoff (reuse existing editor-launch path).

### D14. Config additions
Two fields under a new `synth: Synth` sub-struct in `ft-core/src/config.rs`:
```rust
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Synth {
    #[serde(default = "default_synth_folder")]
    pub folder: String,                  // default "Synthesis/"
    #[serde(default = "default_exclude_prefixes")]
    pub exclude_prefixes: Vec<String>,   // default ["Periodic/"] or wherever
}
```
Wired via `#[serde(default)] pub synth: Synth` on `Config`.

## Risks / Trade-offs

- **`build_journal` signature change ripples** → CLAUDE.md flags this explicitly. Mitigation: the ripple is mechanical (`&[id]` instead of `id`) and the caller count is small (3 known: CLI subcommand, TUI tab, internal tests). Budgeted in tasks.md as a single chore step.

- **HEAD-relative paragraph mapping is imperfect** → if a source file has been substantially edited since a window commit, an added-line in that commit may not map cleanly to a current paragraph. Mitigation: fall back to a synthetic per-`(path, line)` key so the count is still produced, even if dedup is less aggressive. Documented as acceptable per the Non-Goals.

- **Callout grammar mistakes break verification** → if a user hand-edits inside a protected section but leaves the header intact, `verify` will report `drifted` correctly. But if a user deletes a `> ` prefix or modifies the header, parsing may fail entirely. Mitigation: `verify` distinguishes `drifted` (header parsed, content differs) from `malformed` (header didn't parse) and reports the path:line so the user can locate the damage.

- **blake3 dependency** → adds a crate to `ft-core`. blake3 is small, fast, has no transitive surprises. Acceptable cost for the verification ergonomics.

- **Vault must be a git repo (already assumed) and the window must reach valid commits** → if a user runs `ft review --since 7d` in a vault with no commits in that period, output is empty. Surface this clearly with a friendly "no commits in window" message rather than a blank table.

- **TUI tab count grows to 6** → already nontrivial; adding Review pushes it. Mitigation: Review tab is intentionally the rightmost tab so existing muscle memory for digit-keys 1–5 is preserved. The Tab trait's `help_sections` keeps discoverability adequate.

- **Path-prefix exclude can over-shoot or under-shoot** → `synth.exclude_prefixes` is a literal prefix match against vault-relative paths. Easy to reason about, easy to misconfigure. Mitigation: clear default (periodic-notes folder from `periodic_notes` config), documented behavior. Glob support is out of scope for v1.

- **Synth notes are graph citizens** → the user's prose between callouts may itself contain `[[wikilinks]]`. Those count in the next link-review (intended). But it means the link-review will tend to show concepts the user just synthesized — recursive surfacing. This is a feature, not a bug: it surfaces concepts the user actively thinks about. Worth flagging in docs so the behavior isn't surprising.

## Migration Plan

No data migration. Net-new commands, net-new TUI tab, net-new config fields with defaults. Existing `ft notes journal <note>` continues to work unchanged (passes one-element slice internally). No file-format changes for existing notes; synth notes are an opt-in convention.

Rollback: this change is additive. To roll back, remove the new modules, the `Synth` config sub-struct (or leave it — `deny_unknown_fields` is at `Config` level, but `synth:` would simply be unrecognized after removal, which IS an error; pre-removal users must drop the key from config). Practically, the change is non-destructive once shipped; the only externally-visible artifact (synth notes) is plain markdown that survives without `ft`.

## Open Questions

- Exact TUI keybindings for window adjustment in the Review tab (`<` / `>` vs. `[` / `]` vs. an input prompt). Resolve during implementation against existing keymap to avoid collisions.
- Should `ft synth verify --all` walk the configured `synth.folder` only, or every `.md` in the vault checking for the frontmatter marker? **Tentative answer:** every `.md` — the marker, not the folder, is authoritative. Folder is convenience for users, not enforcement.
- For the multi-target journal: should the `matched` badge render link targets by display title or by raw `[[wikilink]]` text? **Tentative answer:** display title (`Foo`, not `[[Foo]]`) since the badge is prose-adjacent.
