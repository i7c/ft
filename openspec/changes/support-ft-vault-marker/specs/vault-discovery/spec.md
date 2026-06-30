## ADDED Requirements

### Requirement: Vault root markers
A directory SHALL be recognized as a vault root when it contains **either** a
`.obsidian/` directory **or** a `.ft/` directory. The two markers are
equivalent: neither is preferred over the other, and a directory containing
both SHALL be treated as a single vault root with no warning. A path entry
(name `.obsidian` or `.ft`) that is a regular file rather than a directory
SHALL NOT qualify the parent as a vault root.

#### Scenario: `.ft/`-only directory is a vault root
- **WHEN** a directory contains a `.ft/` subdirectory and no `.obsidian/` subdirectory
- **THEN** that directory SHALL be recognized as a vault root

#### Scenario: `.obsidian/`-only directory is a vault root
- **WHEN** a directory contains a `.obsidian/` subdirectory and no `.ft/` subdirectory
- **THEN** that directory SHALL be recognized as a vault root

#### Scenario: directory with both markers is one vault root
- **WHEN** a directory contains both `.obsidian/` and `.ft/` subdirectories
- **THEN** that directory SHALL be recognized as a single vault root with no preference and no warning

#### Scenario: a regular file named `.ft` does not qualify
- **WHEN** a directory contains a regular file named `.ft` (not a directory) and no `.obsidian/` directory
- **THEN** that directory SHALL NOT be recognized as a vault root

### Requirement: Discovery precedence
The vault root SHALL be resolved by trying, in order, the first of these that
points at a recognized vault root: (1) the `--vault` CLI flag, (2) the
`FT_VAULT` environment variable, (3) a walk up from the current working
directory checking each directory and its parents for a vault-root marker,
(4) the `default_vault` key in `~/.config/ft/config.toml`. The first rung
whose path is a recognized vault root SHALL win. The `--vault`, `FT_VAULT`,
and `default_vault` paths SHALL be accepted when the path contains `.obsidian/`
or `.ft/`. The walk-up SHALL return the first directory (including the starting
directory itself) that contains `.obsidian/` or `.ft/`.

#### Scenario: `--vault` flag points at a `.ft/`-only directory
- **WHEN** `--vault` is passed a path containing `.ft/` but no `.obsidian/`
- **THEN** discovery SHALL succeed and resolve to that path

#### Scenario: `FT_VAULT` points at a `.ft/`-only directory
- **WHEN** `FT_VAULT` is set to a path containing `.ft/` but no `.obsidian/`
- **THEN** discovery SHALL succeed and resolve to that path

#### Scenario: walk-up finds a `.ft/`-only ancestor
- **WHEN** the current working directory is a descendant of a directory containing `.ft/` (and no `.obsidian/` above it)
- **THEN** the walk-up rung SHALL resolve to that ancestor directory

#### Scenario: `default_vault` points at a `.ft/`-only directory
- **WHEN** `default_vault` in `~/.config/ft/config.toml` is a path containing `.ft/` but no `.obsidian/`
- **THEN** discovery SHALL succeed and resolve to that path

#### Scenario: first matching rung wins
- **WHEN** the `--vault` flag points at a valid vault root and `FT_VAULT` is also set to a valid vault root
- **THEN** discovery SHALL resolve to the `--vault` flag path, not the `FT_VAULT` path

### Requirement: Debuggable discovery failure
When no discovery rung resolves a vault root, `ft` SHALL emit a
`VaultNotFound` error whose message lists every rung that was attempted. The
failure strings for each rung SHALL name both recognized markers
(`.obsidian/ or .ft/`) so the user knows either marker suffices. The error
message SHALL NOT assert that Obsidian is required.

#### Scenario: failure lists both markers for each rung
- **WHEN** no rung resolves a vault root
- **THEN** the error message SHALL mention `--vault`, `FT_VAULT`, the CWD walk, and `default_vault`, and each marker-specific line SHALL name both `.obsidian/` and `.ft/`

#### Scenario: failure does not mention Obsidian as required
- **WHEN** no rung resolves a vault root
- **THEN** the error message SHALL NOT contain the phrase "Obsidian vault"

### Requirement: `.ft/` excluded from scans
The vault scan SHALL exclude the `.ft/` directory (and its contents) from
markdown-file and directory enumeration, alongside `.obsidian/`, `.git/`, and
`attachments/`. `.ft/` MAY be excluded by the dotfile/hidden-file filter of
the directory walker rather than by an explicit entry in the default-ignored
list; the exclusion is a behavioral requirement, not a configuration
requirement.

#### Scenario: tasks inside `.ft/` are not scanned
- **WHEN** a vault root contains `.ft/notes.md` with task lines and no other markdown files
- **THEN** the scan SHALL return zero tasks from that file

#### Scenario: `.ft/` does not appear as a directory node
- **WHEN** the graph is built from a vault whose root contains a `.ft/` subdirectory
- **THEN** `.ft` SHALL NOT appear as a `Directory` node in the graph
