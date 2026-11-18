#!/usr/bin/env bash
# Stage the third-party notices beside a distributed desktop build:
#   THIRD_PARTY_NOTICES.md (the single list, packaging/shared/)
#   NOTICE.txt and every OFL-*.txt (bundled font notices, packaging/shared/fonts/)
# The file list comes from the directories, not from a hand-kept list, so a
# new font licence is shipped by adding its file. All files land flat in
# DESTINATION with mode 644. Callers: bundle-macos (Resources/notices), the
# Linux packagers (usr/share/doc/openpencil/notices) and the release archives.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: stage-notices.sh DESTINATION" >&2
  exit 1
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
shared="$repo_root/packaging/shared"

shopt -s nullglob
files=("$shared/THIRD_PARTY_NOTICES.md" "$shared/fonts/NOTICE.txt" "$shared"/fonts/OFL-*.txt)
for file in "${files[@]}"; do
  if [ ! -f "$file" ]; then
    echo "stage-notices: missing notice file $file" >&2
    exit 1
  fi
done

mkdir -p "$1"
for file in "${files[@]}"; do
  cp "$file" "$1/$(basename "$file")"
  chmod 644 "$1/$(basename "$file")"
done
