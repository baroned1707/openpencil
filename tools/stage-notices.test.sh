#!/usr/bin/env bash
# Offline test for scripts/stage-notices.sh and the places that depend on its
# output: every notice file in the repo arrives in the destination (mode 644),
# the old stage-font-notices.sh wrapper still works, every packaging script
# agrees on the destination directory name, and the NSIS installer lists the
# same files (by name or wildcard) and removes them again.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/openpencil-stage-notices.XXXXXX")"
FAKE_OFL="$ROOT/packaging/shared/fonts/OFL-zz-stage-test.txt"
trap 'rm -rf "$TMP_DIR"; rm -f "$FAKE_OFL"' EXIT

fail() {
  printf 'stage-notices.test: %s\n' "$*" >&2
  exit 1
}

# Mode of a file, portable across BSD and GNU stat.
mode_of() {
  stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"
}

# stage_notices_stages_every_file_in_the_notices_dir
expected=("$ROOT/packaging/shared/THIRD_PARTY_NOTICES.md" "$ROOT/packaging/shared/fonts/NOTICE.txt")
for ofl in "$ROOT"/packaging/shared/fonts/OFL-*.txt; do
  expected+=("$ofl")
done
[ "${#expected[@]}" -ge 5 ] || fail "expected at least 5 notice files, found ${#expected[@]}"

bash "$ROOT/scripts/stage-notices.sh" "$TMP_DIR/staged"
for source in "${expected[@]}"; do
  name="$(basename "$source")"
  [ -f "$TMP_DIR/staged/$name" ] || fail "$name was not staged"
  cmp -s "$source" "$TMP_DIR/staged/$name" || fail "$name differs from its source"
  [ "$(mode_of "$TMP_DIR/staged/$name")" = "644" ] || fail "$name is not mode 644"
done
staged_count="$(find "$TMP_DIR/staged" -type f | wc -l | tr -d ' ')"
[ "$staged_count" = "${#expected[@]}" ] || fail "staged $staged_count files, expected ${#expected[@]}"

# A new OFL file is picked up without touching the script.
fake="$FAKE_OFL"
[ ! -e "$fake" ] || fail "leftover $fake"
printf 'test\n' >"$fake"
bash "$ROOT/scripts/stage-notices.sh" "$TMP_DIR/staged-new"
rm -f "$fake"
[ -f "$TMP_DIR/staged-new/OFL-zz-stage-test.txt" ] || fail "a newly added OFL file was not staged"

# The old script name still works for existing callers.
bash "$ROOT/scripts/stage-font-notices.sh" "$TMP_DIR/staged-wrapper"
[ -f "$TMP_DIR/staged-wrapper/THIRD_PARTY_NOTICES.md" ] || fail "wrapper did not stage THIRD_PARTY_NOTICES.md"
[ -f "$TMP_DIR/staged-wrapper/NOTICE.txt" ] || fail "wrapper did not stage NOTICE.txt"

# Usage errors are refused.
if bash "$ROOT/scripts/stage-notices.sh" >/dev/null 2>&1; then
  fail "missing destination was accepted"
fi

# Every caller stages into a directory named "notices" and none still uses the
# old name.
grep -Fq 'stage-notices.sh" "$APP/Contents/Resources/notices"' "$ROOT/scripts/bundle-macos.sh" \
  || fail "scripts/bundle-macos.sh does not stage into Resources/notices"
grep -Fq 'stage-notices.sh" "$APP/Contents/Resources/notices"' "$ROOT/tools/bundle-macos.sh" \
  || fail "tools/bundle-macos.sh does not stage into Resources/notices"
grep -Fq 'stage-notices.sh" "$root/usr/share/doc/openpencil/notices"' "$ROOT/scripts/package-linux-common.sh" \
  || fail "scripts/package-linux-common.sh does not stage into the doc dir"
workflow="$ROOT/.github/workflows/rust-release.yml"
grep -Fq 'stage-notices.sh "target/${{ matrix.target }}/release/notices"' "$workflow" \
  || fail "release workflow does not stage notices"
grep -Fq 'openpencil-desktop notices' "$workflow" || fail "release tar does not include notices"
grep -Fq 'release\notices' "$workflow" || fail "release zip does not include notices"
if grep -rn 'font-notices' "$workflow" "$ROOT/scripts/bundle-macos.sh" "$ROOT/tools/bundle-macos.sh" \
  "$ROOT/scripts/package-linux-common.sh" >/dev/null; then
  fail "a packaging script still names font-notices"
fi

# NSIS: installs the same set (file or wildcard), uninstalls it, same dir.
nsi="$ROOT/scripts/package-windows.nsi"
grep -Fq 'SetOutPath "$INSTDIR\notices"' "$nsi" || fail "NSIS does not install into notices"
grep -Fq 'File "..\packaging\shared\THIRD_PARTY_NOTICES.md"' "$nsi" || fail "NSIS lacks THIRD_PARTY_NOTICES.md"
grep -Fq 'File "..\packaging\shared\fonts\NOTICE.txt"' "$nsi" || fail "NSIS lacks NOTICE.txt"
grep -Fq 'File "..\packaging\shared\fonts\OFL-*.txt"' "$nsi" || fail "NSIS lacks the OFL-*.txt wildcard"
grep -Fq 'Delete "$INSTDIR\notices\*.md"' "$nsi" || fail "NSIS uninstall keeps the .md notice"
grep -Fq 'Delete "$INSTDIR\notices\*.txt"' "$nsi" || fail "NSIS uninstall keeps the .txt notices"
grep -Fq 'RMDir "$INSTDIR\notices"' "$nsi" || fail "NSIS uninstall keeps the notices dir"

printf 'stage-notices.test: ok (%s files)\n' "${#expected[@]}"
