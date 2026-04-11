#!/usr/bin/env bash
# Pre-fetch all context needed for /implement-issue
# Usage: load-issue-context.sh <issue-number>
#
# Outputs a single structured document with:
# - Issue details (title, body, labels, state)
# - All issue comments
# - Milestone issues
# - Dependency issue states
# - Open PRs on feat/ branches (for stacking decisions)
# - Current git branch state

set -euo pipefail

REPO="mpecan/tokf"
ISSUE_NUM="${1:?Usage: load-issue-context.sh <issue-number>}"

# Colors and symbols (disabled if not a terminal)
if [ -t 1 ]; then
  BOLD='\033[1m'
  DIM='\033[2m'
  CYAN='\033[36m'
  GREEN='\033[32m'
  YELLOW='\033[33m'
  RED='\033[31m'
  MAGENTA='\033[35m'
  RESET='\033[0m'
else
  BOLD='' DIM='' CYAN='' GREEN='' YELLOW='' RED='' MAGENTA='' RESET=''
fi

SYM_CHECK="${GREEN}✓${RESET}"
SYM_OPEN="${YELLOW}○${RESET}"
SYM_CLOSED="${DIM}●${RESET}"

header() { printf "\n${BOLD}${CYAN}━━━ %s ━━━${RESET}\n\n" "$1"; }
subheader() { printf "  ${BOLD}%s${RESET}\n" "$1"; }

state_icon() {
  case "$1" in
    closed) printf "${SYM_CHECK}" ;;
    open)   printf "${SYM_OPEN}" ;;
    *)      printf "?" ;;
  esac
}

# --- Issue details ---
ISSUE_JSON=$(gh api "repos/${REPO}/issues/${ISSUE_NUM}")
ISSUE_TITLE=$(echo "$ISSUE_JSON" | jq -r '.title')
ISSUE_STATE=$(echo "$ISSUE_JSON" | jq -r '.state')
ISSUE_BODY=$(echo "$ISSUE_JSON" | jq -r '.body // ""')
ISSUE_LABELS=$(echo "$ISSUE_JSON" | jq -r '[.labels[].name] | join(", ")')
ISSUE_MILESTONE=$(echo "$ISSUE_JSON" | jq -r '.milestone.title // "none"')

header "Issue #${ISSUE_NUM}: ${ISSUE_TITLE}"
printf "  ${DIM}State:${RESET}     %b %s\n" "$(state_icon "$ISSUE_STATE")" "$ISSUE_STATE"
printf "  ${DIM}Labels:${RESET}    %s\n" "${ISSUE_LABELS:-none}"
printf "  ${DIM}Milestone:${RESET} %s\n" "$ISSUE_MILESTONE"
echo ""
subheader "Description"
echo "$ISSUE_BODY" | sed 's/^/  /'
echo ""

# --- Issue comments ---
COMMENTS=$(gh api "repos/${REPO}/issues/${ISSUE_NUM}/comments" \
  --jq '[.[] | {author: .user.login, created_at: .created_at, body: .body}]')
COMMENT_COUNT=$(echo "$COMMENTS" | jq 'length')

header "Comments (${COMMENT_COUNT})"
if [ "$COMMENT_COUNT" -gt 0 ]; then
  echo "$COMMENTS" | jq -r '.[] | .author + " " + .created_at + "\n" + .body' | while IFS= read -r line; do
    # First line of each comment block is "author date"
    if echo "$line" | grep -qE '^[a-zA-Z0-9_-]+ [0-9]{4}-'; then
      AUTHOR=$(echo "$line" | cut -d' ' -f1)
      DATE=$(echo "$line" | cut -d' ' -f2 | cut -dT -f1)
      printf "\n  ${BOLD}${MAGENTA}@%s${RESET} ${DIM}(%s)${RESET}\n" "$AUTHOR" "$DATE"
    else
      printf "  %s\n" "$line"
    fi
  done
else
  printf "  ${DIM}(none)${RESET}\n"
fi
echo ""

# --- Milestone from GitHub metadata ---
MILESTONE_NUMBER=$(echo "$ISSUE_JSON" | jq -r '.milestone.number // empty')
MILESTONE_TITLE=$(echo "$ISSUE_JSON" | jq -r '.milestone.title // empty')

if [ -n "$MILESTONE_NUMBER" ]; then
  header "Milestone: ${MILESTONE_TITLE}"
  gh api "repos/${REPO}/issues?milestone=${MILESTONE_NUMBER}&state=all&per_page=100&sort=created&direction=asc" \
    --jq '.[] | "\(.state) #\(.number) \(.title)"' | while IFS= read -r line; do
      STATE=$(echo "$line" | cut -d' ' -f1)
      REST=$(echo "$line" | cut -d' ' -f2-)
      NUM=$(echo "$REST" | cut -d' ' -f1)
      TITLE=$(echo "$REST" | cut -d' ' -f2-)
      if [ "$NUM" = "#${ISSUE_NUM}" ]; then
        printf "  %b %s ${BOLD}%s${RESET} ${YELLOW}<-- this issue${RESET}\n" "$(state_icon "$STATE")" "$NUM" "$TITLE"
      else
        printf "  %b %s %s\n" "$(state_icon "$STATE")" "$NUM" "$TITLE"
      fi
    done
  echo ""
else
  header "Milestone"
  printf "  ${DIM}(no milestone assigned)${RESET}\n\n"
fi

# --- Dependency states ---
DEP_NUMS=$(echo "$ISSUE_BODY" | grep -oE '#[0-9]+' | grep -oE '[0-9]+' | sort -un || true)
if [ -n "$DEP_NUMS" ]; then
  header "Dependencies"
  ALL_MET=true
  for DN in $DEP_NUMS; do
    if [ "$DN" != "$ISSUE_NUM" ]; then
      DEP_JSON=$(gh api "repos/${REPO}/issues/${DN}" 2>/dev/null || echo '{}')
      DEP_STATE=$(echo "$DEP_JSON" | jq -r '.state // "unknown"')
      DEP_TITLE=$(echo "$DEP_JSON" | jq -r '.title // "(failed to fetch)"')
      printf "  %b #%-3s %s\n" "$(state_icon "$DEP_STATE")" "$DN" "$DEP_TITLE"
      if [ "$DEP_STATE" != "closed" ]; then ALL_MET=false; fi
    fi
  done
  echo ""
  if $ALL_MET; then
    printf "  ${GREEN}All dependencies met.${RESET}\n\n"
  else
    printf "  ${RED}Warning: some dependencies are still open.${RESET}\n\n"
  fi
fi

# --- Open PRs on feat/ branches ---
header "Open Feature Branches"
PR_OUTPUT=$(gh pr list --repo "$REPO" --state open \
  --json number,headRefName,baseRefName,title \
  --jq '.[] | select(.headRefName | startswith("feat/")) | "#\(.number)|\(.headRefName)|\(.baseRefName)|\(.title)"')

if [ -n "$PR_OUTPUT" ]; then
  echo "$PR_OUTPUT" | while IFS='|' read -r NUM HEAD BASE TITLE; do
    printf "  ${BOLD}%-6s${RESET} %s ${DIM}→ %s${RESET}\n" "$NUM" "$HEAD" "$BASE"
    printf "         ${DIM}%s${RESET}\n" "$TITLE"
  done
else
  printf "  ${DIM}(none)${RESET}\n"
fi
echo ""

# --- Current git state ---
header "Git State"
BRANCH=$(git branch --show-current)
DIRTY_COUNT=$(git status --porcelain | wc -l | tr -d ' ')
printf "  ${DIM}Branch:${RESET}    %s\n" "$BRANCH"
if [ "$DIRTY_COUNT" -eq 0 ]; then
  printf "  ${DIM}Working tree:${RESET} ${GREEN}clean${RESET}\n"
else
  printf "  ${DIM}Working tree:${RESET} ${YELLOW}%s uncommitted file(s)${RESET}\n" "$DIRTY_COUNT"
fi
echo ""
