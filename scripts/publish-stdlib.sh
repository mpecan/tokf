#!/usr/bin/env bash
# Publish all stdlib filters to the tokf registry.
#
# Required environment variables:
#   TOKF_SERVICE_TOKEN  — service token for the publish-stdlib endpoint
#   TOKF_REGISTRY_URL   — base URL of the registry (e.g. https://api.tokf.dev)
#
# Optional:
#   GITHUB_TOKEN        — GitHub token for resolving commit authors
#   DRY_RUN             — set to "true" to print the payload without posting
#
# Prerequisites: python3, curl, git, gh (optional)
#
# Usage:
#   bash scripts/publish-stdlib.sh
#   DRY_RUN=true bash scripts/publish-stdlib.sh

set -euo pipefail

: "${TOKF_SERVICE_TOKEN:?TOKF_SERVICE_TOKEN must be set}"
: "${TOKF_REGISTRY_URL:?TOKF_REGISTRY_URL must be set}"

FILTERS_DIR="crates/tokf-cli/filters"
FALLBACK_AUTHOR="mpecan"

# Resolve the GitHub username of the last author to touch a file.
# Falls back to FALLBACK_AUTHOR if git/GitHub resolution fails.
resolve_author() {
    local file="$1"
    local sha

    sha=$(git log -1 --format='%H' -- "$file" 2>/dev/null || true)
    if [[ -z "$sha" ]]; then
        echo "$FALLBACK_AUTHOR"
        return
    fi

    if [[ -n "${GITHUB_TOKEN:-}" ]]; then
        local repo
        repo=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)
        if [[ -n "$repo" ]]; then
            local login
            login=$(gh api "/repos/${repo}/commits/${sha}" \
                --jq '.author.login // empty' 2>/dev/null || true)
            if [[ -n "$login" ]]; then
                echo "$login"
                return
            fi
        fi
    fi

    echo "$FALLBACK_AUTHOR"
}

# JSON-escape a string (handles backslashes, quotes, newlines, tabs).
# Returns a quoted JSON string value, e.g. "hello\nworld".
json_escape() {
    python3 -c "import json,sys; print(json.dumps(sys.stdin.read()), end='')"
}

# JSON-escape a simple string passed as $1 (for filenames, usernames).
json_escape_str() {
    printf '%s' "$1" | json_escape
}

echo "[publish-stdlib] Enumerating filters in ${FILTERS_DIR}..."

# Build JSON payload
filters_json="["
first=true

while IFS= read -r -d '' toml_file; do
    filter_content=$(json_escape < "$toml_file")
    author=$(resolve_author "$toml_file")
    author_escaped=$(json_escape_str "$author")

    # Collect test files from adjacent _test/ directory
    stem="${toml_file%.toml}"
    test_dir="${stem}_test"
    test_files_json="["
    test_first=true

    if [[ -d "$test_dir" ]]; then
        for test_file in "$test_dir"/*.toml; do
            [[ -f "$test_file" ]] || continue
            test_filename=$(json_escape_str "$(basename "$test_file")")
            test_content=$(json_escape < "$test_file")

            if [[ "$test_first" == "true" ]]; then
                test_first=false
            else
                test_files_json+=","
            fi
            test_files_json+="{\"filename\":${test_filename},\"content\":${test_content}}"
        done
    fi
    test_files_json+="]"

    if [[ "$first" == "true" ]]; then
        first=false
    else
        filters_json+=","
    fi
    filters_json+="{\"filter_toml\":${filter_content},\"test_files\":${test_files_json},\"author_github_username\":${author_escaped}}"

    echo "[publish-stdlib]   ${toml_file} (author: ${author})"
done < <(find "$FILTERS_DIR" -name '*.toml' -not -path '*_test/*' -print0 | sort -z)

filters_json+="]"

payload="{\"filters\":${filters_json}}"

if [[ "${DRY_RUN:-}" == "true" ]]; then
    echo "[publish-stdlib] DRY RUN — payload:"
    echo "$payload" | python3 -m json.tool
    exit 0
fi

echo "[publish-stdlib] Publishing to ${TOKF_REGISTRY_URL}/api/filters/publish-stdlib..."

# Write payload to a temp file to avoid ARG_MAX limits and ps exposure.
payload_file=$(mktemp)
trap 'rm -f "$payload_file"' EXIT
printf '%s' "$payload" > "$payload_file"

# Pass auth header via stdin using curl --config to avoid token in ps output.
curl_config=$(mktemp)
trap 'rm -f "$payload_file" "$curl_config"' EXIT
printf -- '-H "Authorization: Bearer %s"\n' "$TOKF_SERVICE_TOKEN" > "$curl_config"

http_code=0
response=$(curl -s -w "\n%{http_code}" \
    -K "$curl_config" \
    -X POST \
    -H "Content-Type: application/json" \
    --data-binary "@${payload_file}" \
    "${TOKF_REGISTRY_URL}/api/filters/publish-stdlib") || {
    echo "[publish-stdlib] curl failed with exit code $?"
    exit 1
}

http_code=$(echo "$response" | tail -1)
body=$(echo "$response" | sed '$d')

# Validate http_code is numeric
if ! [[ "$http_code" =~ ^[0-9]+$ ]]; then
    echo "[publish-stdlib] Unexpected response (no HTTP status code)."
    echo "$body"
    exit 1
fi

echo "[publish-stdlib] HTTP ${http_code}"
echo "$body" | python3 -m json.tool 2>/dev/null || echo "$body"

# Check 207 before the general 2xx range — 207 means partial failure.
if [[ "$http_code" == "207" ]]; then
    echo "[publish-stdlib] Partial success (some filters failed)."
    exit 1
elif [[ "$http_code" -ge 200 && "$http_code" -lt 300 ]]; then
    echo "[publish-stdlib] Done."
    exit 0
else
    echo "[publish-stdlib] Failed with HTTP ${http_code}."
    exit 1
fi
