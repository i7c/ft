## 1. Graph DSL extensions — operators

- [x] 1.1 Add `Op::Lt`, `Op::Le`, `Op::Gt`, `Op::Ge`, `Op::IsNull`, `Op::IsNotNull` to the `Op` enum in `ft-core/src/graph/query.rs`
- [x] 1.2 Extend the tokenizer to recognize `<`, `<=`, `>`, `>=`; recognize `is null` / `is not null` as multi-token postfix sequences during parser
- [x] 1.3 Update the parser's op-and-value section to handle postfix operators (no rhs value for `is null`/`is not null`)
- [x] 1.4 Update the canonical serializer to round-trip new operators
- [x] 1.5 Unit tests: parse + roundtrip for each new operator

## 2. Graph DSL extensions — Date value

- [x] 2.1 Add `Literal::Date(NaiveDate)` to the value AST
- [x] 2.2 Public helper in `ft-core/src/dates.rs` (if not already): `parse_date_value(s: &str, today: NaiveDate) -> Option<NaiveDate>` that accepts `YYYY-MM-DD`, `today`, `tomorrow`, `yesterday`, `+Nd`, `-Nd`, `+Nw`, `-Nw`, `+Nm`, `-Nm`
- [x] 2.3 Parser: when the lhs attr is a date attr, parse the rhs in "date mode" using `parse_date_value`
- [x] 2.4 Unit tests: ISO, today, tomorrow, yesterday, relative offsets, error path for unknown form

## 3. Type and scope checking

- [x] 3.1 Add `Attr::value_type()` helper: returns `Int | Date | String | Enum | Set`
- [x] 3.2 Validate `op` × `value_type` at parse time: `<`/`<=`/`>`/`>=` requires `Int` or `Date`; `is null`/`is not null` requires optional attrs (`due`, `scheduled`, `created`, `start`, `completed`)
- [x] 3.3 Emit `TypeMismatch` errors with span pointing at the operator
- [x] 3.4 Unit tests: each rejection path

## 4. `Profile` mechanism

- [x] 4.1 Add `Profile { Default, Tasks }` enum next to `GraphQuery` in `ft-core/src/graph/query.rs`
- [x] 4.2 Change `parse(src)` to `parse(src, profile)`; existing callers pass `Profile::Default`
- [x] 4.3 In `Profile::Tasks`, before tokenization, detect whether the source begins with `node` (skipping whitespace + comments); if not, prepend `node where kind = Task and ` virtually (i.e., synthesize tokens, not string manipulation)
- [x] 4.4 In `Profile::Tasks`, bare attribute references default to `Subject::SelfNode` during parse
- [x] 4.5 Snapshot tests: identical AST for `priority = high` under Tasks and `node where kind = Task and self.priority = high` under Default

## 5. Evaluator updates

- [x] 5.1 Implement evaluation for new operators in `Condition::matches` against `NoteId` / `Task`
- [x] 5.2 Date attrs comparable to `Date` literals; integer attrs to `Integer` literals; `is null` / `is not null` over `Option<…>` fields
- [x] 5.3 Unit tests: evaluation on synthetic task fixtures

## 6. CLI wiring

- [x] 6.1 `ft/src/cmd/tasks.rs::run_list` switches from `query::dsl::parse` to `graph::query::parse(s, Profile::Tasks, today)`
- [x] 6.2 `Filter` (from CLI flags) compiles to `Vec<Condition>` AND-ed with parsed query expression
- [x] 6.3 `ft graph query` keeps `Profile::Default` as default; add `--profile tasks` opt-in
- [x] 6.4 Sort / limit remain CLI-flag-only (`--sort`, `--limit`); no DSL grammar change for them
- [x] 6.5 Update CLI tests covering query parsing in `ft/tests/`

## 7. TUI wiring

- [x] 7.1 TUI Tasks tab query bar uses `Profile::Tasks`
- [x] 7.2 Errors from the parser are surfaced in the existing error footer
- [x] 7.3 Snapshot test: typing a Tasks-profile query produces the expected result list

## 8. Built-in presets migration

- [x] 8.1 Rewrite `ft-core/src/query/preset.rs::builtin` to return graph DSL strings under Tasks profile semantics
- [x] 8.2 Update each preset's `every_builtin_parses` test to assert parse + equivalence to the old behaviour on a fixture vault
- [x] 8.3 Add `not-done` preset (was implicit in the old `not done` predicate)
- [x] 8.4 Update `builtin_names()` to include `not-done`

## 9. Deletions

- [x] 9.1 Delete `ft-core/src/query/dsl.rs` and its tests
- [x] 9.2 Delete `Atom` enum from `ft-core/src/query/expr.rs`; keep `Expr` if reused for AND/OR composition over `Condition` (otherwise delete)
- [x] 9.3 Update `ft-core/src/lib.rs` if `pub mod` lines change
- [x] 9.4 Delete any task-DSL-specific imports across the binary crate

## 10. Migration docs

- [x] 10.1 New `docs/migrating-task-queries.md` with the predicate translation table from the design doc
- [x] 10.2 Update `docs/query-dsl.md` → renamed/redirect-noted; primary docs are now `docs/graph-query-dsl.md` and the new migration doc
- [x] 10.3 README and CLAUDE.md updated to reference the unified DSL
- [x] 10.4 CHANGELOG entry naming the hard break and pointing at the migration doc

## 11. Tests

- [x] 11.1 Port every existing task-DSL test in `query/dsl.rs` (deleted) to the new syntax; place under `graph/query.rs` tests with a `mod tasks_profile` submodule
- [x] 11.2 Proptest round-trip for `<`/`<=`/`>`/`>=` operators on Integer and Date values
- [x] 11.3 Proptest round-trip for `Date` literal forms
- [x] 11.4 Integration test: each built-in preset matches the expected tasks on the `realistic/` fixture vault
- [x] 11.5 Integration test: `--sort` and `--limit` CLI flags work in combination with the new query syntax

## 12. Build validation

- [x] 12.1 `cargo build --release` — clean
- [x] 12.2 `cargo test --workspace` — all tests pass
- [x] 12.3 `cargo clippy --workspace --tests -- -D warnings` — clean
- [x] 12.4 `cargo fmt --check` — clean
