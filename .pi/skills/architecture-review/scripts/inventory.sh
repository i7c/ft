#!/usr/bin/env bash
# Universal module inventory for an architecture review.
# Prints: source-tree layout, per-file LOC (largest first), and a
# rough public-symbol count per file. Language-agnostic — pass the
# source dir and, optionally, file extensions and a symbol regex.
#
# Usage:
#   inventory.sh <src-dir> [ext...] [symbol-regex]
# Examples:
#   ./inventory.sh src rs                          # Rust
#   ./inventory.sh src ts tsx                      # TypeScript
#   ./inventory.sh lib py                          # Python
#   ./inventory.sh src rs '^\s*pub (fn|struct|enum|trait|type)'   # custom surface

set -euo pipefail

SRC="${1:?usage: inventory.sh <src-dir> [ext...] [symbol-regex]}"
shift

EXTS=()
while [ $# -gt 0 ]; do
  case "$1" in
    *' '*) SYM_RE="$1" ;;  # last positional containing spaces = the regex
    *) EXTS+=("$1") ;;
  esac
  shift
done
[ ${#EXTS[@]} -eq 0 ] && EXTS=("rs" "ts" "js" "go" "py" "java" "rb")
SYM_RE="${SYM_RE:-^[[:space:]]*(pub |export |public |func [A-Z]|def |class |module )}"

echo "=== source tree (directories with file counts) ==="
find "$SRC" -type d | sort | while read -r d; do
  n=0
  for e in "${EXTS[@]}"; do
    c=$(find "$d" -maxdepth 1 -type f -name "*.$e" 2>/dev/null | wc -l)
    n=$((n + c))
  done
  [ "$n" -gt 0 ] && printf "%5d  %s\n" "$n" "${d#./}"
done

echo
echo "=== per-file LOC (largest first) ==="
FILES=()
for e in "${EXTS[@]}"; do
  while IFS= read -r f; do FILES+=("$f"); done < <(find "$SRC" -name "*.$e" -type f 2>/dev/null)
done
for f in "${FILES[@]}"; do printf "%8d  %s\n" "$(wc -l < "$f")" "$f"; done | sort -rn

echo
echo "=== per-file public-symbol count (top 30 by symbols) ==="
for f in "${FILES[@]}"; do
  n=$(grep -cE "$SYM_RE" "$f" || true)
  printf "%6d  %s\n" "$n" "$f"
done | sort -rn | head -30

echo
echo "=== totals ==="
total_lines=0
for f in "${FILES[@]}"; do total_lines=$((total_lines + $(wc -l < "$f"))); done
printf "files: %d  lines: %d\n" "${#FILES[@]}" "$total_lines"
