## Context

`ft` already supports template-driven note creation via `ft notes create --template` (CLI) and the `C` keybinding (TUI). Both reuse `TemplateContext` (title, today, now, vars) and the MiniJinja rendering pipeline in `ft-core/src/notes/template.rs`. Append operations exist only for section-move (`ft notes move-section`), which moves existing sections between files — there is no way to render a template into an existing note.

The TUI's create flow (`ft/src/tui/notes_actions/create.rs`) is a multi-step state machine: template picker → folder picker → filename prompt → var prompt → commit. Append and quick capture need similar step sequences but with different starting points and fewer interactive steps.

## Goals / Non-Goals

**Goals:**
- Append a rendered template to an existing markdown note, at end-of-file or under a specific section heading (configured via per-note frontmatter `ft-append-section`).
- Quick capture presets: a config-driven shortcut that bundles action + target + template into a single keypress. No interactive template prompt or (when derivable) target prompt.
- From the graph tab: selected note drives the target for append, selected folder for create. From the notes tab: pickers resolve the target.
- After append or quick-capture create, the editor opens at the line where new content was inserted (not just the top of the file).

**Non-Goals:**
- CLI support for quick capture presets (TUI-only for now; CLI can use `ft notes append` directly).
- Template variables (`vars.KEY`) prompted during quick capture. Quick capture presets are zero-interaction beyond possibly picking the target file.
- Quick capture with frontmatter-defined vars or per-preset var defaults.
- Batch quick capture (one preset → multiple notes).
- Append-to-section without an exact heading text match (no regex or fuzzy section matching).

## Decisions

### 1. Core append function lives in `ft-core::notes::append`

**Rationale:** The existing `ft_core::notes` module already handles section extraction (`extract_sections`), heading parsing (`extract_headings`), and section-level operations. Append-to-section is a natural sibling. Keeping it in `ft-core` lets the CLI and TUI share the same logic.

**Signature:**
```rust
pub fn append_template(
    file_content: &str,
    template_rendered: &str,
    section_heading: Option<&str>,
) -> Result<(String, usize /* inserted line */)>
```

**Behavior:**
- `section_heading = None` → append to end of file. Prepend a `\n` separator if the file doesn't already end with one.
- `section_heading = Some("Sessions")` → find the heading `## Sessions` (any ATX level, trimmed case-insensitive match) via `extract_headings`, determine the section's end (next same-or-higher-level heading or EOF), insert after that boundary. If the heading is not found, error.
- Returns `(new_content, line_number)` where `line_number` is the 1-indexed line where the first byte of `template_rendered` lands in `new_content`.

**Alternative considered:** Make append part of the template module. Rejected because section targeting requires `extract_headings` from `notes`, creating a circular dependency. A new sibling module under `notes/` avoids this.

### 2. Frontmatter key: `ft-append-section`

**Rationale:** Obsidian frontmatter is the natural per-note configuration surface. The `ft-` prefix avoids collisions with user keys and with Obsidian plugin keys. The value is a plain string: the exact heading text to append under.

**Format:**
```yaml
---
ft-append-section: Sessions
---
```

**Parsing:** Reuse the existing frontmatter detection in `ft_core::markdown::extract_headings` (which skips frontmatter-delimited regions). Parse the YAML frontmatter block with `serde_yaml` (already a dependency). `ft-append-section` is optional; absence means append-to-end.

**Alternative considered:** TOML config in `[notes]` section. Rejected because section targets are per-note, not vault-global. A vault-global default could be added later but is not needed for v1.

### 3. Quick capture preset config shape

**Rationale:** TOML matches the existing config format. A dedicated `[capture_presets.<name>]` table keeps presets namespaced and discoverable. Each preset is a self-contained bundle.

**Schema:**
```toml
[capture_presets.session]
action = "append"                   # required: "append" | "create"
template = "session.md"             # required: template name from templates dir
note = "Areas/therapy.md"           # optional: hardcoded target for append
section = "Sessions"                # optional: section heading for append (overrides frontmatter)

[capture_presets.meeting-note]
action = "create"
template = "meeting.md"
path = "%Y-%m-%d meeting"           # optional: filename pattern (strftime tokens)
folder = "Meetings"                 # optional: target folder (vault-relative)
```

**Resolution at use time:**
- **Append with `note` set:** Use that note directly. No file picker.
- **Append without `note`:** From graph tab → use selected note. From notes tab → open vault file picker, then append.
- **Create with `path` set:** Resolve `path` with `chrono` strftime tokens → `folder/<resolved>.md`. Create if missing; collision → overwrite (quick capture is optimistic).
- **Create without `path`:** Open filename prompt (like `c` binding but without folder picker if `folder` is set).
- **`section` field:** If present, use for append-to-section. If absent, read `ft-append-section` from the target note's frontmatter. If neither, append to end.

**Alternative considered:** Reuse existing `[presets]` map (task query presets). Rejected because capture presets have a different schema (action + template + target vs. DSL string) and mixing them would complicate deserialization.

### 4. TUI keybindings

**Graph tab additions:**
- `A` (shift-a): Append with template. Opens the template picker (same as `C` but for append on selected note). After template selection, reads `ft-append-section` from the selected note's frontmatter, renders, appends, opens editor.
- `Q` (shift-q): Quick capture. Opens a fuzzy picker over `[capture_presets]` names. On selection, executes the preset immediately.

**Notes tab additions:**
- `a`: Append with template. Opens template picker, then vault file picker for target note, then appends.
- `Q` (shift-q): Quick capture. Same preset picker as graph tab. For append presets without `note`, opens vault file picker (or uses idle-selected note if `NotesState::OpenPicking` selection is available — but quick capture from Idle is the primary flow).

**Rationale:** `a`/`A` mirrors `c`/`C` (lowercase = simpler flow, uppercase = template-first). `Q` is unused on both tabs and easy to reach.

**Alternative considered:** Adding quick capture to idle leader (`p` then another key). Rejected because quick capture should be a single chord, not two, to minimize interruption.

### 5. Editor jump to insertion point

**Rationale:** When you append to a long file, opening at line 1 forces you to scroll. Jumping to the insertion line makes the capture feel instant.

**Implementation:** `AppRequest::OpenInEditor` already carries `line: usize`. For append, compute the insertion line from `append_template`'s return value. For quick-capture creates, open at the last line of the new file so the user lands at the inserted content. For `ft notes create`, continue using line 1 (existing behavior — not changed by this feature).

### 6. No new TUI state machine variants

**Rationale:** The existing `CreateState` state machine handles template picking, folder picking, filename prompt, and var prompt. Append reuses the template picker state (`TemplatePicking`) and then diverges into its own `AppendState`. Quick capture uses a simple preset picker (same `FuzzyPicker` pattern as preset selection in graph tab) and delegates to either the create flow or the append flow.

**Data flow:**
```
CreateState (existing, unchanged)
AppendState (new): TemplatePicking → commit_append
QuickCapturePresetPicker (new): pick preset → resolve target → commit_create or commit_append
```

**Alternative considered:** Extend `CreateState` with append variants. Rejected because append never goes through folder picking or filename prompting — the target already exists. Separate state enums keep each flow's invariants tight.

## Risks / Trade-offs

- **Frontmatter parsing cost:** Every append-with-template operation reads the target file twice (once for section lookup, once for the full content to splice). This is negligible for markdown files (typically <100KB) and the read is already needed for the final write through `write_atomic`. → Accept.
- **Quick capture overwrite on collision:** Create presets with `path` patterns can collide with existing files. The preset silently overwrites — appropriate for "quick capture" where friction is the enemy. → Document this behavior; if users want collision protection, they use `ft notes create` instead.
- **Section heading ambiguity:** If the target file has two `## Sessions` headings at different levels, `extract_headings` returns both. We take the first match. → Accept for v1; regex support could be added later if needed.
- **Quick capture preset picker has no preview:** Unlike the template picker, the preset picker shows only the preset name. Users need to know their preset names. → Presets are user-configured; they already defined the names. The help overlay can list configured preset names.

## Open Questions

- Should quick capture presets support `--var`-like default values (e.g., `vars = { topic = "" }` with interactive prompt)? → Out of scope for v1; the point of quick capture is zero prompts.
- Should append support a `--dry-run` flag to preview the rendered template? → Out of scope; users can test templates via `ft notes create --template` with a scratch file.
