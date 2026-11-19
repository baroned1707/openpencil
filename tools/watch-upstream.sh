#!/usr/bin/env bash
# Watch the upstream project's releases and print what is new. Read-only.
#
#   tools/watch-upstream.sh              show releases newer than the last one seen
#   tools/watch-upstream.sh --since TAG  show releases newer than TAG (does not save)
#   tools/watch-upstream.sh --reset TAG  record TAG as seen without printing
#   tools/watch-upstream.sh --all        list every release (does not save)
#
# This fork never merges upstream. The script only READS the public GitHub API
# through `gh` (release list, release notes, a compare summary) so a person can
# decide which ideas are worth re-implementing. It does not fetch, checkout,
# merge, rebase or push anything, and it sends nothing about this repository.
#
# State: the last release tag seen lives in `<git dir>/upstream-last-seen`
# (inside .git, never committed). The first run records the newest release as
# the baseline and prints nothing else, so old history is not replayed.
#
# Environment: UPSTREAM_REPO overrides the watched repository (default
# ZSeven-W/openpencil).

set -euo pipefail

UPSTREAM_REPO="${UPSTREAM_REPO:-ZSeven-W/openpencil}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GIT_DIR_PATH="$(git -C "$ROOT" rev-parse --absolute-git-dir)"
STATE="$GIT_DIR_PATH/upstream-last-seen"

fail() { printf 'watch-upstream: %s\n' "$*" >&2; exit 1; }
command -v gh >/dev/null 2>&1 || fail "the GitHub CLI (gh) is required"
command -v jq >/dev/null 2>&1 || fail "jq is required"

mode=new
arg=""
case "${1:-}" in
  "") ;;
  --since) mode=since; arg="${2:-}"; [ -n "$arg" ] || fail "--since needs a tag" ;;
  --reset) mode=reset; arg="${2:-}"; [ -n "$arg" ] || fail "--reset needs a tag" ;;
  --all) mode=all ;;
  -h|--help) sed -n '2,22p' "${BASH_SOURCE[0]}"; exit 0 ;;
  *) fail "unknown option: $1 (see --help)" ;;
esac

# Tags are written into a state file and into API paths: accept only a plain
# version-like tag.
valid_tag() { [[ "$1" =~ ^[A-Za-z0-9._-]{1,64}$ ]]; }
[ -z "$arg" ] || valid_tag "$arg" || fail "unsupported tag: $arg"

if [ "$mode" = reset ]; then
  printf '%s\n' "$arg" > "$STATE"
  printf 'recorded %s as the last seen release\n' "$arg"
  exit 0
fi

# Newest first, drafts excluded (prereleases are included on purpose: this
# project publishes its versions as prereleases).
releases_json="$(gh release list -R "$UPSTREAM_REPO" --limit 30 --json tagName,publishedAt,isPrerelease,isDraft 2>&1)" \
  || fail "could not read releases of $UPSTREAM_REPO: $releases_json"
tags="$(printf '%s' "$releases_json" | jq -r '[.[] | select(.isDraft | not)] | sort_by(.publishedAt) | reverse | .[].tagName')"
[ -n "$tags" ] || fail "no releases found for $UPSTREAM_REPO"
newest="$(printf '%s\n' "$tags" | head -1)"

last=""
case "$mode" in
  since) last="$arg" ;;
  new)
    if [ -f "$STATE" ]; then
      last="$(tr -d '[:space:]' < "$STATE")"
    else
      printf '%s\n' "$newest" > "$STATE"
      printf 'first run: recorded %s as the baseline. Nothing to report yet.\n' "$newest"
      exit 0
    fi
    ;;
esac

if [ "$mode" != all ] && [ -n "$last" ]; then
  valid_tag "$last" || fail "the saved tag is not valid: $last"
fi

# Releases newer than $last: everything listed before $last in the newest-first list.
fresh=""
if [ "$mode" = all ]; then
  fresh="$tags"
else
  fresh="$(printf '%s\n' "$tags" | awk -v last="$last" '$0==last {exit} {print}')"
  if ! printf '%s\n' "$tags" | grep -qx -- "$last"; then
    printf 'note: %s is not among the 30 newest releases; showing all of them.\n' "$last" >&2
  fi
fi

if [ -z "$fresh" ]; then
  printf 'no new upstream release since %s (newest: %s)\n' "${last:-none}" "$newest"
  exit 0
fi

count="$(printf '%s\n' "$fresh" | wc -l | tr -d ' ')"
printf '%s new upstream release(s) in %s\n' "$count" "$UPSTREAM_REPO"
prev="$last"
# Oldest first so the story reads in order.
for tag in $(printf '%s\n' "$fresh" | sed '1!G;h;$!d'); do
  printf '\n=== %s ===\n' "$tag"
  gh release view "$tag" -R "$UPSTREAM_REPO" --json publishedAt,url,body \
    --jq '"published: \(.publishedAt)\n\(.url)\n\n\(.body)"' 2>/dev/null | head -80 || true
  if [ -n "$prev" ] && [ "$mode" != all ]; then
    summary="$(gh api "repos/$UPSTREAM_REPO/compare/$prev...$tag" \
      --jq '"commits: \(.total_commits), files changed: \(.files | length)"' 2>/dev/null || true)"
    [ -z "$summary" ] || printf '\nsince %s: %s\n' "$prev" "$summary"
    printf 'compare: https://github.com/%s/compare/%s...%s\n' "$UPSTREAM_REPO" "$prev" "$tag"
  fi
  prev="$tag"
done

if [ "$mode" = new ]; then
  printf '%s\n' "$newest" > "$STATE"
  printf '\nrecorded %s as the last seen release\n' "$newest"
fi
printf 'Nothing was merged or fetched. Re-implement ideas by hand.\n'
