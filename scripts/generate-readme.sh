#!/usr/bin/env bash
set -euo pipefail

# Generate README.md from docs/_readme/header.md + docs/*.md (sorted by order) + docs/_readme/footer.md
# Usage:
#   scripts/generate-readme.sh          # write README.md
#   scripts/generate-readme.sh --check  # verify README.md is up-to-date (exit 1 if stale)

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DOCS_DIR="$REPO_ROOT/docs"
HEADER="$DOCS_DIR/_readme/header.md"
FOOTER="$DOCS_DIR/_readme/footer.md"
README="$REPO_ROOT/README.md"

# Strip YAML frontmatter (the --- delimited block at the start of the file)
strip_frontmatter() {
    awk '
        BEGIN { in_fm = 0; seen_fm = 0 }
        /^---$/ && !seen_fm { in_fm = 1; seen_fm = 1; next }
        /^---$/ && in_fm    { in_fm = 0; next }
        !in_fm              { print }
    ' "$1"
}

# Get the order value from frontmatter
get_order() {
    awk '
        /^---$/ && !started { started = 1; next }
        /^---$/ && started  { exit }
        /^order:/ { print $2; exit }
    ' "$1"
}

# Collect doc files with their order values, then sort
sorted_docs() {
    for f in "$DOCS_DIR"/*.md; do
        [ -f "$f" ] || continue
        order=$(get_order "$f")
        echo "${order:-99} $f"
    done | sort -n | awk '{ print $2 }'
}

# Build the README content
generate() {
    cat "$HEADER"

    first=true
    for doc in $(sorted_docs); do
        echo ""
        echo "---"
        echo ""
        strip_frontmatter "$doc"
    done

    echo ""
    echo "---"
    echo ""
    cat "$FOOTER"
}

if [ "${1:-}" = "--check" ]; then
    generated=$(generate)
    if diff -q <(echo "$generated") "$README" > /dev/null 2>&1; then
        echo "README.md is up-to-date."
        exit 0
    else
        echo "README.md is out of date. Run 'just readme' to regenerate." >&2
        diff --unified <(echo "$generated") "$README" >&2 || true
        exit 1
    fi
else
    generate > "$README"
    echo "README.md generated."
fi
