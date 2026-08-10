# Tooling — language-agnostic data gathering

Everything here uses only `rg`/`grep`, `git`, `find`, and standard shell/python — no language toolchains required. Where a language's doc tool exists (`cargo doc`, `godoc`, `sphinx`, `typedoc`, `javadoc`), it is a fine supplement, but the greps below are the baseline and work everywhere.

## 1. Module inventory

```bash
# Module declarations (adapt the pattern to the language):
rg -n '^\s*pub mod |^\s*mod |^module |^from |^import ' src/ lib/ 2>/dev/null | head -100

# Per-file LOC, sorted (largest first) — the size table:
find src/ -name '*.rs' -o -name '*.ts' -o -name '*.py' | xargs wc -l | sort -rn | head -40

# File counts by directory — where the gravity is:
find src/ -type d | while read d; do echo "$(find $d -name '*.rs' | wc -l) $d"; done | sort -rn | head -15
```

The universal script is `scripts/inventory.sh`; run it, adapt the extension list.

## 2. Public surface extraction

Grep for declaration patterns, per language family. Collect the surface per module into the map table.

```bash
# Rust
rg -n '^\s*pub (fn|struct|enum|trait|type|const|mod|use)' <file>

# Go (exported = capitalized)
rg -n '^\s*func [A-Z]|^\s*type [A-Z]'

# TypeScript / JavaScript (exports)
rg -n '^\s*export (function|class|interface|type|const|enum)'

# Python (module-level defs/classes; skip underscore-prefixed)
rg -n '^(def |class |async def )' <file> | grep -v '  _\|def _'

# Java / C# (public members)
rg -n '^\s*public (static )?(final )?(class|interface|enum|void|[A-Za-z<>\[\], ]+ [a-z][A-Za-z0-9]*\()'
```

Language doc tools give a cleaner surface: `cargo doc --no-deps` (Rust), `godoc` (Go), `typedoc` (TS), `sphinx`/`pydoc` (Python). Use them when available, but the greps are enough for the size/shape signal.

## 3. Dependency mapping

Extract imports per module, collapse to module level, then build the edge table.

```bash
# Rust: top-level crate/module references per module
rg -hoE 'crate::[a-z_]+' src/<module>/ --glob '*.rs' | sort -u

# Go: package imports per file
rg -ho '^\s*"[^"]+"' <file> | sort -u

# TS: relative + package imports
rg -ho "from '[./][^']+'" <file> | sort -u

# Python: import statements
rg -ho '^from [a-z_]+|^import [a-z_]+' <file> | sort -u
```

Then classify: **hubs** (imported by most), **leaves** (import nothing), **cycles** (A imports B and B imports A — grep the edges both ways). Note that test-only imports often hide inside `#[cfg(test)]`; distinguish production edges from test edges, they tell different stories.

## 4. Dead / internal-only surface measurement

The most load-bearing number in the review. Quick python pattern (language-agnostic — adapt the declaration regex):

```python
import re, subprocess, os, sys

root, decl_pat, ref_suffix = sys.argv[1], sys.argv[2], sys.argv[3]
# decl_pat: e.g. r'^\s*pub (fn|struct|enum|trait|const|type) ([a-zA-Z_0-9]+)'
# ref_suffix: file extension, e.g. '.rs'

items = {}  # name -> [(file, line)]
for dirpath, _, files in os.walk(root):
    for f in files:
        if not f.endswith(ref_suffix): continue
        p = os.path.join(dirpath, f)
        for i, line in enumerate(open(p), 1):
            m = re.search(decl_pat, line)
            if m: items.setdefault(m.group(2), []).append((p, i))

def refs(name):
    r = subprocess.run(['rg', '-l', r'\b'+re.escape(name)+r'\b', root],
                       capture_output=True, text=True)
    return set(r.stdout.splitlines())

dead, internal_only, external = [], [], []
for name, locs in sorted(items.items()):
    outside = refs(name) - set(p for p, _ in locs)
    if not outside: dead.append(name)
    elif not any(l.startswith(sys.argv[4]) for l in outside): internal_only.append(name)
    else: external.append(name)

print(f"total {len(items)}; dead {len(dead)}; internal-only {len(internal_only)}; external {len(external)}")
print("DEAD:", ', '.join(dead))
```

`sys.argv[4]` is the external-consumer directory (e.g. the binary crate). Verify a sample of the dead list by hand — "returned but never named" types (callers infer the type) are technically dead as *named* API but may be load-bearing; say so in the report.

## 5. Call-site counts

Before proposing any signature change:

```bash
rg -c '\bGraph::build\b' . -g '*.rs' | awk -F: '{s+=$2} END {print s}'   # total
rg -l '\bGraph::build\b' . -g '*.rs'                                     # which files
```

Classify: production glue vs tests, uniform patterns (codemoddable) vs irregular (hand-edit). If the count is huge, the skill's Phase 10 guidance applies: keep a delegator, or use struct-params.

## 6. Audit greps (Phase 11)

Prove the *reason* for the change, not just that it compiles:

```bash
# The cycle is gone:
rg -n 'crate::a' b/                                   # should be empty (production code)
# The coupling is gone:
rg -n 'is_synth_note' recent.rs pulse.rs              # should be empty
# Single read pass:
rg -n 'read_to_string' hot_path.rs                    # only the intended sites
# Dead surface deleted:
rg -rn 'walk_markdown_files|markdown_files_with_mtime' . -g '*.rs'   # empty
```

Each audit maps to a finding from Phase 7. List the audits in the change's task list so they're part of the definition of done.
