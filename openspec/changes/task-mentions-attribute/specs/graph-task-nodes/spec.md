## MODIFIED Requirements

### Requirement: Task attribute queries in graph DSL
The graph query DSL SHALL support attribute-based filtering on task nodes. The following attributes SHALL be recognized for `NodeKind::Task` nodes: `status`, `priority`, `due`, `scheduled`, `tags`, `description`, and `mentions`. String attributes (`status`, `priority`, `due`, `scheduled`, `description`) SHALL support equality, inequality, `in`, `includes`, `starts_with`, and `ends_with` operators. The `tags` attribute SHALL support `in` and `includes` operators. The `mentions` attribute SHALL support `=`, `!=`, `includes`, and `in`, and SHALL evaluate by walking the task's owning paragraph's `ParagraphLink` edges (via the `OwnsTask` edge); its semantics are normatively defined in the `task-mentions-attribute` capability. Any other attribute name evaluated against a task node — including `path` and `title` — SHALL yield no value, causing the condition to evaluate to false (consistent with unknown attributes on any other node kind). The DSL strings projected for `status` SHALL be exactly `"Open"`, `"Done"`, `"InProgress"`, `"Cancelled"`; the DSL strings projected for `priority` (when present) SHALL be exactly `"Highest"`, `"High"`, `"Medium"`, `"Low"`, `"Lowest"`. These spellings are a stable contract and SHALL NOT be coupled to the Rust `Debug` representation of the underlying enum variants.

#### Scenario: Filter tasks by status
- **WHEN** a user writes `node where kind = "Task" and status = "Open"`
- **THEN** the query SHALL return only task nodes whose `TaskData.status` is `"Open"`

#### Scenario: Filter tasks by priority
- **WHEN** a user writes `node where kind = "Task" and priority = "High"`
- **THEN** the query SHALL return only task nodes whose `TaskData.priority` is `Some("High")`

#### Scenario: Filter tasks by due date
- **WHEN** a user writes `node where kind = "Task" and due = "2025-01-15"`
- **THEN** the query SHALL return only task nodes whose `TaskData.due` is `Some("2025-01-15")`

#### Scenario: Filter tasks by tag
- **WHEN** a user writes `node where kind = "Task" and tags includes "work"`
- **THEN** the query SHALL return only task nodes whose `TaskData.tags` contain `"work"`

#### Scenario: Filter tasks by mentioned concept
- **WHEN** a task's owning paragraph contains `[[onboarding]]` resolving to note `onboarding.md` (title `"onboarding"`)
- **AND** the user writes `node where kind = "Task" and mentions = "onboarding"`
- **THEN** the query SHALL return that task node
