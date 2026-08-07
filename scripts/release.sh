#!/usr/bin/env bash
#
# release.sh — cut a new `ft` release.
#
#   scripts/release.sh            # patch bump: v0.1.6 -> v0.1.7
#   scripts/release.sh --minor    #             v0.1.6 -> v0.2.0
#   scripts/release.sh --major    #             v0.1.6 -> v1.0.0
#
# Steps (preview everything with --dry-run):
#   1. Next version. Base = the higher of the latest v* tag and the
#      [workspace.package] version in Cargo.toml (they can drift);
#      bumped patch / minor / major per the flag.
#   2. CHANGELOG.md gets a new "## <version>" section generated from
#      the commit subjects between the last release tag and HEAD,
#      grouped under "### Added" / "### Fixed" / "### Internal cleanup".
#      openspec/chore/ci/docs/build commits are process noise and are
#      skipped (counted in the summary).
#   3. [workspace.package] version in Cargo.toml is bumped and
#      Cargo.lock refreshed — the release workflow builds with --locked.
#   4. The result is committed as "chore(release): bump workspace
#      version to X.Y.Z" and tagged vX.Y.Z (lightweight, matching the
#      existing tags). Pushing is opt-in; tag pushes trigger CI.
#
# Options:
#   --minor    bump the minor version (X.(Y+1).0)
#   --major    bump the major version ((X+1).0.0)
#   --dry-run  print the plan and the generated changelog; change nothing
#   --check    run the five build invariants from AGENTS.md first
#   --push     push the release commit and tag to origin
#   --help
set -euo pipefail

BUMPS="patch"
DRY_RUN=0
PUSH=0
DO_CHECK=0

usage() {
  # Print the comment header (lines 2.. up to the first non-comment line)
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --minor) BUMPS="minor" ;;
    --major) BUMPS="major" ;;
    --dry-run) DRY_RUN=1 ;;
    --check) DO_CHECK=1 ;;
    --push) PUSH=1 ;;
    --help|-h) usage; exit 0 ;;
    *)
      echo "release.sh: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree is not clean; commit or stash before releasing" >&2
  exit 1
fi

# --- version arithmetic -----------------------------------------------------

is_semver() { [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; }

ver_max() {
  [[ "$1" == "$2" ]] && { echo "$1"; return; }
  printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1
}

bump() {
  local v="$1" mode="$2" major minor patch
  IFS=. read -r major minor patch <<<"$v"
  case "$mode" in
    patch) echo "$major.$minor.$((patch + 1))" ;;
    minor) echo "$major.$((minor + 1)).0" ;;
    major) echo "$((major + 1)).0.0" ;;
  esac
}

CARGO_VERSION="$(sed -n 's/^version = "\([0-9][0-9.]*\)".*/\1/p' Cargo.toml | head -n1)"
[[ -n "$CARGO_VERSION" ]] || { echo "error: no version line in [workspace.package]" >&2; exit 1; }
is_semver "$CARGO_VERSION" || { echo "error: Cargo.toml version '$CARGO_VERSION' is not semver" >&2; exit 1; }

LATEST_TAG="$(git tag --list 'v[0-9]*' --sort=-v:refname | head -n1 || true)"
if [[ -n "$LATEST_TAG" ]]; then
  TAG_VERSION="${LATEST_TAG#v}"
  is_semver "$TAG_VERSION" || { echo "error: tag $LATEST_TAG is not semver" >&2; exit 1; }
  BASE_VERSION="$(ver_max "$CARGO_VERSION" "$TAG_VERSION")"
  if [[ "$CARGO_VERSION" != "$TAG_VERSION" ]]; then
    echo "note: Cargo.toml ($CARGO_VERSION) differs from latest tag ($LATEST_TAG); releasing from $BASE_VERSION" >&2
  fi
else
  echo "note: no v* tags found; releasing from the Cargo.toml version" >&2
  BASE_VERSION="$CARGO_VERSION"
fi

NEW_VERSION="$(bump "$BASE_VERSION" "$BUMPS")"
NEW_TAG="v$NEW_VERSION"

if git rev-parse -q --verify "refs/tags/$NEW_TAG" >/dev/null; then
  echo "error: tag $NEW_TAG already exists" >&2
  exit 1
fi

# --- collect and classify commits -------------------------------------------

RANGE="$([ -n "$LATEST_TAG" ] && echo "$LATEST_TAG..HEAD" || echo "HEAD")"
mapfile -t SUBJECTS < <(git log --format=%s "$RANGE")
if [[ ${#SUBJECTS[@]} -eq 0 ]]; then
  echo "error: no commits since ${LATEST_TAG:-the beginning}; nothing to release" >&2
  exit 1
fi

# Regexes live in variables: bash 5.3's [[ ]] parser trips on ')' inside
# an unquoted character class (e.g. [^)]), so keep =~ patterns as words.
RE_PROC='^(openspec|chore|ci|docs|build|deps)(\([^)]*\))?:'
RE_MERGE='^Merge (branch|remote-tracking|tag|pull)'
RE_FIX='^fix(\([^)]*\))?:'
RE_CLEANUP='^(refactor|cleanup|internal)(\([^)]*\))?:'

is_process_commit() {
  [[ "$1" =~ $RE_PROC ]] || [[ "$1" =~ $RE_MERGE ]]
}

ADDED=()
FIXED=()
CLEANUP=()
SKIPPED=0
for s in "${SUBJECTS[@]}"; do
  if is_process_commit "$s"; then
    SKIPPED=$((SKIPPED + 1))
  elif [[ "$s" =~ $RE_FIX ]]; then
    FIXED+=("$s")
  elif [[ "$s" =~ $RE_CLEANUP ]]; then
    CLEANUP+=("$s")
  else
    ADDED+=("$s")
  fi
done

INCLUDED=$(( ${#ADDED[@]} + ${#FIXED[@]} + ${#CLEANUP[@]} ))
if [[ $INCLUDED -eq 0 ]]; then
  echo "error: all ${#SUBJECTS[@]} commits since $LATEST_TAG are process commits; nothing to changelog" >&2
  exit 1
fi

# --- build the changelog fragment -------------------------------------------

FRAG_FILE="$(mktemp)"
CHANGELOG_TMP="$(mktemp)"
trap 'rm -f "$FRAG_FILE" "$CHANGELOG_TMP"' EXIT
{
  [[ ${#ADDED[@]} -gt 0 ]] && { echo "### Added"; echo; printf -- '- %s\n' "${ADDED[@]}"; echo; }
  [[ ${#FIXED[@]} -gt 0 ]] && { echo "### Fixed"; echo; printf -- '- %s\n' "${FIXED[@]}"; echo; }
  [[ ${#CLEANUP[@]} -gt 0 ]] && { echo "### Internal cleanup"; echo; printf -- '- %s\n' "${CLEANUP[@]}"; echo; }
} > "$FRAG_FILE"

# --- optional pre-flight: build invariants ------------------------------------

if [[ $DO_CHECK -eq 1 ]]; then
  echo "== checking build invariants (AGENTS.md) =="
  cargo build --release
  cargo test --workspace
  cargo clippy --workspace --tests -- -D warnings
  cargo fmt --check
  cargo run --release -q -- commands docs --check
fi

# --- dry run -------------------------------------------------------------------

if [[ $DRY_RUN -eq 1 ]]; then
  echo "Release plan (dry run):"
  echo "  new version:  $NEW_VERSION (${BUMPS} bump from $BASE_VERSION)"
  echo "  release tag:  $NEW_TAG"
  echo "  commits:      ${#SUBJECTS[@]} since ${LATEST_TAG:-the beginning} — $INCLUDED into CHANGELOG, $SKIPPED skipped (process/docs)"
  echo
  echo "  Cargo.toml:   version = \"$CARGO_VERSION\" -> version = \"$NEW_VERSION\""
  echo "  Cargo.lock:   refreshed by 'cargo metadata'"
  UNRELEASED_NONEMPTY="$(awk '/^## Unreleased/{f=1; next} /^## /{f=0} f && NF' CHANGELOG.md | wc -l)"
  if [[ "$UNRELEASED_NONEMPTY" -gt 0 ]]; then
    echo "  note: Unreleased has $UNRELEASED_NONEMPTY non-blank line(s); they will be merged into the new section"
  fi
  echo
  echo "  CHANGELOG.md:"
  echo
  {
    echo "## $NEW_VERSION"
    echo
    cat "$FRAG_FILE"
  } | sed 's/^/    /'
  echo "  would commit 'chore(release): bump workspace version to $NEW_VERSION' and tag $NEW_TAG"
  exit 0
fi

# --- apply ----------------------------------------------------------------------

# Read from a copy: the '> CHANGELOG.md' redirect truncates the real file
# before the python process starts, so it must not be the read source.
cp CHANGELOG.md "$CHANGELOG_TMP"

python3 - "$NEW_VERSION" "$FRAG_FILE" "$CHANGELOG_TMP" <<'PY' > CHANGELOG.md
import re
import sys

version, frag_path, changelog_path = sys.argv[1], sys.argv[2], sys.argv[3]
with open(frag_path, encoding="utf-8") as f:
    frag = f.read().strip() + "\n\n"
with open(changelog_path, encoding="utf-8") as f:
    text = f.read()

marker = "## Unreleased"
idx = text.find(marker)
if idx == -1:
    sys.exit("release.sh: CHANGELOG.md has no '## Unreleased' section; add one before releasing")

rest = text[idx + len(marker):]
end = len(rest)
for m in re.finditer(r"(?m)^## ", rest):
    end = m.start()
    break
unreleased_body = rest[:end].strip()
after = rest[end:]


def split_into_blocks(text):
    """Split into [(heading_or_None, [body lines]), ...] on '### ' lines."""
    blocks = []
    heading = None
    body = []
    for ln in text.split("\n"):
        if ln.startswith("### "):
            if heading is not None or body:
                blocks.append((heading, body))
            heading = ln
            body = []
        else:
            body.append(ln)
    if heading is not None or body:
        blocks.append((heading, body))
    return blocks


def render(blocks):
    """Render blocks back to markdown, normalized to single blank separators."""
    parts = []
    for heading, body in blocks:
        lines = []
        if heading is not None:
            lines.append(heading)
            if body:
                lines.append("")
        lines.extend(body)
        # Collapse stray blank runs (frag blocks carry their own separators)
        joined = re.sub(r"\n{2,}", "\n\n", "\n".join(lines)).strip("\n")
        if joined:
            parts.append(joined)
    return "\n\n".join(parts) + "\n\n"


if unreleased_body:
    # Merge generated bullets into the existing Unreleased sections so we
    # don't end up with two '### Added' blocks; new categories append.
    blocks = split_into_blocks(unreleased_body)
    for frag_heading, frag_body in split_into_blocks(frag):
        if frag_heading is None:
            continue
        for i, (h, b) in enumerate(blocks):
            if h == frag_heading:
                existing = set(b)
                new = [ln for ln in frag_body if ln not in existing]
                if new:
                    blocks[i] = (h, b + new)
                break
        else:
            blocks.append((frag_heading, frag_body))
    body = render(blocks)
else:
    body = frag

version_section = "## " + version + "\n\n" + body
sys.stdout.write(text[:idx] + marker + "\n\n" + version_section + after)
PY

sed -i.bak "s/^version = \"[0-9][0-9.]*\"/version = \"$NEW_VERSION\"/" Cargo.toml
rm -f Cargo.toml.bak

cargo metadata --format-version 1 >/dev/null   # refresh Cargo.lock for the workspace version change

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -q -m "chore(release): bump workspace version to $NEW_VERSION"
git tag "$NEW_TAG"

COMMIT_SHA="$(git rev-parse --short HEAD)"
echo "Released $NEW_TAG ($COMMIT_SHA)"
echo "  commits: ${#SUBJECTS[@]} since ${LATEST_TAG:-the beginning} — $INCLUDED into CHANGELOG, $SKIPPED skipped (process/docs)"
if [[ $PUSH -eq 1 ]]; then
  if git remote get-url origin >/dev/null 2>&1; then
    git push origin HEAD
    git push origin "$NEW_TAG"
  else
    echo "  warning: no origin remote; push manually:"
    echo "    git push origin HEAD && git push origin $NEW_TAG"
  fi
else
  echo "  not pushed; when ready:"
  echo "    git push origin HEAD && git push origin $NEW_TAG"
fi
