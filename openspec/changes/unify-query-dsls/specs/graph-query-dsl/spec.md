## MODIFIED Requirements

### Requirement: Comparison operators on integer and date attributes

The graph DSL SHALL support the operators `<`, `<=`, `>`, `>=` on attributes whose value type is `Integer` (`indegree`, `outdegree`) or `Date` (`due`, `scheduled`, `created`, `start`, `completed`). Applying a comparison operator to a non-comparable attribute SHALL produce a parse-time `TypeMismatch` error pointing at the operator span.

#### Scenario: Integer comparison on indegree
- **WHEN** the user runs `ft graph query 'node where self.indegree > 5'`
- **THEN** the parser accepts the query and returns notes whose incoming edge count exceeds 5

#### Scenario: Date comparison on due
- **WHEN** the user runs `ft tasks list 'due < today'`
- **THEN** the parser accepts the query (under `Profile::Tasks`) and returns tasks whose `due` is strictly before today's date

#### Scenario: Type mismatch on string attribute
- **WHEN** the user runs `ft graph query 'node where self.title < "x"'`
- **THEN** the parser exits with a `TypeMismatch` error naming `title` and the operator `<`

### Requirement: `Date` value type with literal and keyword forms

The graph DSL SHALL accept `Date` values written as:

- ISO literal `YYYY-MM-DD`
- Keywords `today`, `tomorrow`, `yesterday`
- Relative offsets `+Nd`, `-Nd`, `+Nw`, `-Nw`, `+Nm`, `-Nm`

Date values SHALL resolve against `FT_TODAY` when set, otherwise the system date. Date values SHALL be valid only on the right-hand side of date attributes.

#### Scenario: ISO date literal
- **WHEN** the user runs `ft tasks list 'due = 2026-12-31'`
- **THEN** the parser produces a `Value::Single(Literal::Date(2026-12-31))`

#### Scenario: today keyword resolves against FT_TODAY
- **WHEN** `FT_TODAY=2026-06-02` is set and the user runs `ft tasks list 'due = today'`
- **THEN** the query matches tasks whose `due` equals `2026-06-02`

#### Scenario: Relative offset
- **WHEN** `FT_TODAY=2026-06-02` is set and the user runs `ft tasks list 'due < +7d'`
- **THEN** the query matches tasks whose `due` is strictly before `2026-06-09`

#### Scenario: Date keyword outside date context errors
- **WHEN** the user runs `ft graph query 'node where self.title = today'`
- **THEN** the parser exits with an error stating that `today` is a date keyword and `title` is a string attribute

### Requirement: Nullability operators on optional attributes

The graph DSL SHALL support `is null` and `is not null` as postfix operators (no right-hand-side value). These operators SHALL be valid only on attributes that are optional in the underlying model: `due`, `scheduled`, `created`, `start`, `completed`.

#### Scenario: due is null
- **WHEN** the user runs `ft tasks list 'due is null'`
- **THEN** the query matches every task whose `due` field is unset

#### Scenario: due is not null
- **WHEN** the user runs `ft tasks list 'due is not null'`
- **THEN** the query matches every task whose `due` field is set, regardless of value

#### Scenario: null op on non-optional attribute errors
- **WHEN** the user runs `ft graph query 'node where self.kind is null'`
- **THEN** the parser exits with an error stating that `kind` is a required attribute and null operators are not applicable

### Requirement: `sort` and `limit` are removed from the DSL grammar

The graph DSL grammar SHALL NOT accept `sort by …` or `limit N` clauses. Existing CLI flags `--sort` and `--limit` SHALL be the only way to order and truncate results.

#### Scenario: Old sort clause errors
- **WHEN** the user runs `ft tasks list 'status = Open sort by due'`
- **THEN** the parser exits with a clear error pointing at the `sort` token and naming `--sort` as the replacement

#### Scenario: Old limit clause errors
- **WHEN** the user runs `ft tasks list 'status = Open limit 5'`
- **THEN** the parser exits with a clear error pointing at the `limit` token and naming `--limit` as the replacement

#### Scenario: CLI flags continue to work
- **WHEN** the user runs `ft tasks list 'status = Open' --sort -due --limit 5`
- **THEN** the parser accepts the query, the CLI sorts by descending due and emits up to 5 rows
