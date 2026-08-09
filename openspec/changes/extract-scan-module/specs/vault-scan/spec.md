# vault-scan

## ADDED Requirements

### Requirement: Single-read scan pass
The scan SHALL walk the vault once — collecting the markdown-file list and the directory list from the same walker pass — and SHALL read each markdown file exactly once, extracting from that single read the task lines (via the active `TaskFormat`), the raw link occurrences, the paragraph ranges, the heading ranges, and the raw `ft:` frontmatter block (the text between the YAML fences) into a per-file `ParsedFile`. The scan SHALL return one `Scan` containing all tasks, all per-file artifacts, the directory list (`dirs`), and per-file non-fatal errors (`errors`). The scan SHALL be a pure in-memory operation after the reads: no callbacks, no incremental mutation.

#### Scenario: Files and directories come from one walk
- **WHEN** a vault containing notes in nested directories is scanned
- **THEN** `Scan::files` lists every markdown file and `Scan::dirs` lists every directory, and both sets are produced by the same walker pass (no second filesystem walk for directories)

#### Scenario: Every file read exactly once
- **WHEN** a vault of N markdown files is scanned
- **THEN** each file's content is read exactly once, and every extracted artifact (tasks, links, headings, paragraphs, frontmatter block) for that file derives from that single read

#### Scenario: Frontmatter block captured
- **WHEN** a scanned file begins with a well-formed `---` … `---` YAML frontmatter block
- **THEN** `ParsedFile::frontmatter` is the raw text between the fences, and a file without frontmatter has `frontmatter == None`

#### Scenario: Non-fatal per-file errors collected
- **WHEN** one file fails to read (e.g. permissions) during a scan
- **THEN** the scan completes, the failing file is recorded in `Scan::errors`, and all other files' artifacts are present

### Requirement: Read-only snapshot consumed by graph and vault
The `Scan` SHALL be an immutable in-memory snapshot of the vault's parse artifacts. `Graph::build` SHALL construct the graph entirely from a `&Scan` — including directory nodes from `Scan::dirs` — and SHALL NOT require vault access or perform file I/O of its own. `Vault::scan()` SHALL remain the discovery-side convenience that produces the scan from the vault root and the configured `ignored_paths`.

#### Scenario: Graph builds from the scan alone
- **WHEN** `Graph::build(scan)` is called for a vault with notes and directories
- **THEN** the graph contains one note node per `Scan::files` entry and one directory node per `Scan::dirs` entry, and the build performs no vault access and no file reads

#### Scenario: Vault::scan delegates
- **WHEN** `Vault::scan()` is called
- **THEN** it returns the same `Scan` as `scan::scan_vault(&vault.path, &vault.config.config.ignored_paths)`

### Requirement: Walker semantics
The scan walker SHALL exclude hidden entries, git-ignored entries, the `DEFAULT_IGNORED` folders (`.obsidian`, `.git`, `attachments`), and the vault config's `ignored_paths`. `scan::markdown_files` and `scan::markdown_files_with_mtime` SHALL return vault-relative paths. The file set and the directory set SHALL be consistent: a directory excluded from the walk SHALL NOT appear in `Scan::dirs`, and a file inside an excluded directory SHALL NOT appear in `Scan::files`.

#### Scenario: Default exclusions honored
- **WHEN** a vault contains `attachments/pic.md` and `.obsidian/plugin.md`
- **THEN** neither file appears in `Scan::files` and neither directory appears in `Scan::dirs`

#### Scenario: Config ignored_paths honored
- **WHEN** the vault config lists `archive/` in `ignored_paths`
- **THEN** `archive/*.md` files are absent from `Scan::files` and the `archive` directory is absent from `Scan::dirs`

#### Scenario: Relative paths returned
- **WHEN** `scan::markdown_files` is called with a vault root
- **THEN** every returned path is relative to that root

### Requirement: Frontmatter block readers
`frontmatter::block(content)` SHALL return the raw frontmatter block text between the YAML fences, or `None`. The four readers (`ft_tasks_section`, `ft_append_section`, `ft_synth_enabled`, `ft_synth_targets`) SHALL have block-taking variants (`ft_tasks_section_in`, `ft_append_section_in`, `ft_synth_enabled_in`, `ft_synth_targets_in`) that resolve the same keys from a block, and the content-taking readers SHALL delegate to the block variants. A consumer holding a `ParsedFile` SHALL resolve any of the four frontmatter keys from `ParsedFile::frontmatter` without re-reading the file.

#### Scenario: Keys resolved from a captured block
- **WHEN** a consumer holds a `ParsedFile` whose `frontmatter` is the block of a file with `ft.synth.enabled: true`
- **THEN** `ft_synth_enabled_in(&frontmatter)` returns `Some(true)` without any disk access

#### Scenario: Content readers delegate
- **WHEN** `ft_tasks_section(content)` is called with full file content
- **THEN** it returns the same value as `ft_tasks_section_in(block(content))`

### Requirement: Vault walker methods removed
`Vault` SHALL NOT expose `markdown_files`, `markdown_files_with_mtime`, or `directories`; the walkers SHALL be functions of the scan module. `Vault::scan()` SHALL be the only vault-surface entry point for reading the vault.

#### Scenario: No walker methods on Vault
- **WHEN** code outside the scan module needs a file list or directory list
- **THEN** it uses `scan::markdown_files` / `Scan::dirs` rather than a `Vault` method
