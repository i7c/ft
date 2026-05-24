# Graph query DSL fixture matrix

Each test case is a pair:

- `<NN>-<slug>.dsl` — the query source
- `<NN>-<slug>.expected` — the expected result, one node per line

The runner (`ft-core/tests/graph_query_matrix.rs`) parses every `.dsl`
file in this directory, runs `select()` against the `tests/fixtures/dirs`
vault graph, and compares the sorted set of resulting nodes against the
matching `.expected` file. Mismatches show a unified diff.

## `.expected` line format

```
<kind-char> <path>
```

| Kind char | Node kind   | Path                                                |
|-----------|-------------|-----------------------------------------------------|
| `N`       | Note        | vault-relative file path (e.g. `Areas/finance.md`)  |
| `D`       | Directory   | vault-relative dir path; vault root uses `<root>`   |
| `G`       | Ghost       | the raw unresolved link string                      |

Lines are sorted (kind char first, then path) so the comparison is
order-independent. Blank lines and lines starting with `#` are ignored
in the `.expected` file.

## Adding a case

1. Drop a `.dsl` file with the next free `NN` prefix.
2. Run `cargo test -p ft-core --test graph_query_matrix` and copy the
   diff output into a fresh `.expected` file. (Or hand-write the
   expected lines — the format is small enough.)
3. Re-run; the test should pass.

The fixture graph (`tests/fixtures/dirs`) contains:

```
Directory: <root>
Directory: Areas
Directory: Areas/operations
Directory: Projects
Note:      root.md
Note:      Areas/finance.md
Note:      Areas/operations/shifts.md
Note:      Projects/alpha.md
```

with `directory-contains` edges from each parent directory to its
immediate children.
