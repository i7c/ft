## ADDED Requirements

### Requirement: `Profile::Tasks` desugars bare predicates into a task-node query

When the graph DSL parser is invoked with `Profile::Tasks`, the parser SHALL apply two transformations:

1. If the source string does not begin with the keyword `node` (after optional whitespace), the parser SHALL inject `node where kind = Task and ` before the first predicate.
2. Attribute references without an explicit subject (`self`, `from`, `to`, `edge`) SHALL default to `Subject::SelfNode`.

The resulting AST SHALL be identical to what the verbose `Profile::Default` form would have produced.

#### Scenario: Bare predicate desugars
- **WHEN** the parser receives `priority = high and due < today` under `Profile::Tasks`
- **THEN** the resulting AST is identical to parsing `node where kind = Task and self.priority = High and self.due < today;` under `Profile::Default`

#### Scenario: Explicit node block preserved
- **WHEN** the parser receives `node where kind = Task and self.tags includes work` under `Profile::Tasks`
- **THEN** no injection occurs and the parser produces the same AST it would under `Profile::Default`

#### Scenario: Bare subject defaults to self
- **WHEN** the parser receives `path includes "Areas/"` under `Profile::Tasks`
- **THEN** `path` is interpreted as `self.path` and the expression matches `kind = Task` nodes whose source-file path includes `Areas/`

### Requirement: `ft tasks list` uses Tasks profile by default

The `ft tasks list <query>` CLI surface SHALL invoke the graph DSL parser with `Profile::Tasks`. The TUI Tasks tab's query bar SHALL also use `Profile::Tasks`.

#### Scenario: CLI accepts bare predicate
- **WHEN** the user runs `ft tasks list 'priority = High and not done'` (where `not done` is the `not-done` preset reference)
- **THEN** the CLI desugars and evaluates the query under `Profile::Tasks` and prints matching tasks

#### Scenario: CLI accepts explicit node form
- **WHEN** the user runs `ft tasks list 'node where kind = Task and self.priority = High'`
- **THEN** the CLI evaluates the explicit form unchanged

### Requirement: Built-in task presets are expressed in the unified DSL

Every built-in task preset SHALL be expressible as a `Profile::Tasks` graph DSL string. The `ft_core::query::preset::builtin` function SHALL return such strings.

#### Scenario: today preset
- **WHEN** `ft_core::query::preset::builtin("today")` is called
- **THEN** it returns a graph DSL string equivalent to `(status in {Open, InProgress}) and (due = today or scheduled = today)`

#### Scenario: not-done preset
- **WHEN** `ft_core::query::preset::builtin("not-done")` is called
- **THEN** it returns a graph DSL string equivalent to `status in {Open, InProgress}`

#### Scenario: Preset parses under Tasks profile
- **WHEN** each preset string is parsed under `Profile::Tasks`
- **THEN** parsing succeeds and the resulting query is equivalent to the old task DSL preset under the same `today` fixture
