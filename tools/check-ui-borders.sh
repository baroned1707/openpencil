#!/usr/bin/env bash
# Counts the remaining hairline-border and hard-coded-colour sites in the
# editor widgets. The Figma UI3 restyle drives both numbers to zero; the
# baseline file records the last agreed ceiling and may only go down.
#
#   tools/check-ui-borders.sh           report + fail if above baseline
#   tools/check-ui-borders.sh --update  rewrite the baseline to current counts
set -euo pipefail
cd "$(dirname "$0")/.."

SRC=crates/op-editor-ui/src
BASELINE=tools/ui-border-baseline.txt

# Non-test Rust sources only; theme.rs owns the palette literals.
files() {
  find "$SRC" -name '*.rs' ! -name '*test*' ! -path '*tests*' ! -path '*svg_export*' ! -name theme.rs
}

border_sites=$(files | xargs grep -hE '\.stroke_(round_)?rect\(' 2>/dev/null | wc -l | tr -d ' ')
border_token=$(files | xargs grep -hE 'theme\.border|tokens\.border|\.border\b' 2>/dev/null | wc -l | tr -d ' ')
literal_colours=$(files | xargs grep -hE 'rgb\(0x|rgba\(' 2>/dev/null | wc -l | tr -d ' ')

echo "stroke_sites=$border_sites border_token=$border_token literal_colours=$literal_colours"

if [ "${1:-}" = "--update" ]; then
  printf 'stroke_sites=%s\nborder_token=%s\nliteral_colours=%s\n' \
    "$border_sites" "$border_token" "$literal_colours" > "$BASELINE"
  echo "baseline updated"
  exit 0
fi

status=0
while IFS='=' read -r key limit; do
  case "$key" in
    stroke_sites) current=$border_sites ;;
    border_token) current=$border_token ;;
    literal_colours) current=$literal_colours ;;
    *) continue ;;
  esac
  if [ "$current" -gt "$limit" ]; then
    echo "FAIL: $key rose from $limit to $current" >&2
    status=1
  fi
done < "$BASELINE"
exit $status
