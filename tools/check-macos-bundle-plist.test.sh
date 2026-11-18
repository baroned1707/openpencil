#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHECKER="$ROOT/tools/check-macos-bundle-plist.sh"
FIXTURE="$ROOT/crates/op-host-desktop/Info.plist"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/openpencil-macos-plist.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

expect_rejected() {
  local plist=$1
  local expected=$2
  local output="$TMP_DIR/rejected.log"

  if bash "$CHECKER" "$plist" >"$output" 2>&1; then
    printf 'expected checker to reject %s\n' "$plist" >&2
    exit 1
  fi
  grep -Fq "$expected" "$output" || {
    printf 'checker rejected %s without expected diagnostic %s\n' \
      "$plist" "$expected" >&2
    cat "$output" >&2
    exit 1
  }
}

bash "$CHECKER"
bash "$CHECKER" "$FIXTURE"

python3 - "$FIXTURE" "$TMP_DIR" <<'PY'
import plistlib
import sys
from pathlib import Path

source = Path(sys.argv[1])
output = Path(sys.argv[2])
with source.open("rb") as handle:
    baseline = plistlib.load(handle)

cases = {
    "missing-description.plist": {
        **baseline,
        "NSLocalNetworkUsageDescription": None,
    },
    "wrong-description.plist": {
        **baseline,
        "NSLocalNetworkUsageDescription": "OpenPencil scans your network.",
    },
    "missing-service.plist": {
        **baseline,
        "NSBonjourServices": [],
    },
    "extra-service.plist": {
        **baseline,
        "NSBonjourServices": [
            "_openpencil-collab._tcp",
            "_unrelated._tcp",
        ],
    },
}

def document_types(op_icon=None, pen_icon=None, op_extensions=("op",)):
    def entry(extensions, icon):
        item = {"CFBundleTypeExtensions": list(extensions)}
        if icon is not None:
            item["CFBundleTypeIconFile"] = icon
        return item

    return [
        entry(op_extensions, op_icon),
        entry(("pen",), pen_icon),
        entry(("fig",), None),
    ]


cases["document-types-ok.plist"] = {
    **baseline,
    "CFBundleDocumentTypes": document_types(op_icon="document"),
}
cases["op-icon-missing.plist"] = {
    **baseline,
    "CFBundleDocumentTypes": document_types(),
}
cases["pen-icon.plist"] = {
    **baseline,
    "CFBundleDocumentTypes": document_types(op_icon="document", pen_icon="document"),
}
cases["op-pen-shared.plist"] = {
    **baseline,
    "CFBundleDocumentTypes": document_types(op_icon="document", op_extensions=("op", "pen")),
}

cases["missing-description.plist"].pop("NSLocalNetworkUsageDescription")
for name, value in cases.items():
    with (output / name).open("wb") as handle:
        plistlib.dump(value, handle)
PY

expect_rejected \
  "$TMP_DIR/missing-description.plist" \
  'NSLocalNetworkUsageDescription must equal'
expect_rejected \
  "$TMP_DIR/wrong-description.plist" \
  'NSLocalNetworkUsageDescription must equal'
expect_rejected \
  "$TMP_DIR/missing-service.plist" \
  'NSBonjourServices must equal'
expect_rejected \
  "$TMP_DIR/extra-service.plist" \
  'NSBonjourServices must equal'

bash "$CHECKER" "$TMP_DIR/document-types-ok.plist"
expect_rejected \
  "$TMP_DIR/op-icon-missing.plist" \
  'the .op document type must list only'
expect_rejected \
  "$TMP_DIR/pen-icon.plist" \
  'only the .op document type may set CFBundleTypeIconFile'
expect_rejected \
  "$TMP_DIR/op-pen-shared.plist" \
  'the .op document type must list only'

if [[ "$(uname -s)" == Darwin ]]; then
  # Exercise the same canonical patch helper used by both real bundle scripts.
  # Start from a plist whose values are deliberately absent.
  cp "$TMP_DIR/missing-description.plist" "$TMP_DIR/patched.plist"
  # shellcheck source=macos-local-network-plist.sh
  source "$ROOT/tools/macos-local-network-plist.sh"
  openpencil_apply_macos_local_network_plist "$TMP_DIR/patched.plist"
  bash "$CHECKER" "$TMP_DIR/patched.plist"
fi

printf 'check-macos-bundle-plist tests: PASS\n'
