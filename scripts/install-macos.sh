#!/usr/bin/env bash
# Package, install and roll back the macOS OpenPencil app.
#
#   scripts/install-macos.sh package            zip + dmg + sha256 of the built bundle
#   scripts/install-macos.sh install [APP]      back up the installed app, then install APP
#   scripts/install-macos.sh list               list the backups
#   scripts/install-macos.sh revert [BACKUP]    put a backup back (default: the app the last install replaced)
#
# `install` never overwrites without a backup: the app that is installed now is
# copied to the backup directory first, and `revert` swaps it back the same way
# (so reverting is itself reversible). Nothing runs while the app is open.
#
# Environment (all optional):
#   OPENPENCIL_INSTALL_DIR  where the app lives          (default /Applications)
#   OPENPENCIL_BACKUP_DIR   where backups are kept       (default ~/OpenPencil-backups)
#   OPENPENCIL_BUNDLE       built bundle for package/install
#                           (default target/release/bundle/osx/OpenPencil.app)
#   OPENPENCIL_DIST_DIR     where `package` writes       (default target/dist)

set -euo pipefail

WS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="${OPENPENCIL_INSTALL_DIR:-/Applications}"
BACKUP_DIR="${OPENPENCIL_BACKUP_DIR:-$HOME/OpenPencil-backups}"
BUNDLE="${OPENPENCIL_BUNDLE:-$WS_ROOT/target/release/bundle/osx/OpenPencil.app}"
DIST_DIR="${OPENPENCIL_DIST_DIR:-$WS_ROOT/target/dist}"
APP_NAME="OpenPencil.app"
INSTALLED="$INSTALL_DIR/$APP_NAME"

fail() {
  printf 'install-macos: error: %s\n' "$*" >&2
  exit 1
}

plist_value() {
  /usr/libexec/PlistBuddy -c "Print :$2" "$1/Contents/Info.plist" 2>/dev/null || echo unknown
}

# The app must not be running: replacing a running bundle corrupts it.
refuse_if_running() {
  if pgrep -f "$INSTALLED/Contents/MacOS/" >/dev/null 2>&1 && [[ -z "${OPENPENCIL_SKIP_RUNNING_CHECK:-}" ]]; then
    fail "OpenPencil is running from $INSTALLED; quit it first (Cmd+Q)"
  fi
}

require_bundle() {
  [[ -d "$1" && -f "$1/Contents/Info.plist" && -x "$1/Contents/MacOS/openpencil-desktop" ]] \
    || fail "not an OpenPencil app bundle: $1"
}

arch_of() {
  lipo -archs "$1/Contents/MacOS/openpencil-desktop" 2>/dev/null || echo unknown
}

# Name a backup after the installed app's version and its binary's mtime, so the
# listing says what each one is.
backup_name_for() {
  local version stamp
  version="$(plist_value "$1" CFBundleShortVersionString)"
  stamp="$(stat -f '%Sm' -t '%Y%m%d-%H%M%S' "$1/Contents/MacOS/openpencil-desktop")"
  printf 'OpenPencil-%s-%s.app' "$version" "$stamp"
}

backup_installed() {
  [[ -d "$INSTALLED" ]] || return 0
  mkdir -p "$BACKUP_DIR"
  local name target
  name="$(backup_name_for "$INSTALLED")"
  target="$BACKUP_DIR/$name"
  if [[ -e "$target" ]]; then
    # Same build already backed up: keep that copy, do not duplicate.
    printf 'backup: %s already exists, reusing it\n' "$target" >&2
  else
    ditto "$INSTALLED" "$target"
    printf 'backup: %s\n' "$target" >&2
  fi
  # `revert` with no argument restores exactly the app this install replaced.
  printf '%s' "$target" >"$BACKUP_DIR/.last-backup"
  printf '%s' "$target"
}

cmd_package() {
  require_bundle "$BUNDLE"
  local version arch commit base
  version="$(plist_value "$BUNDLE" CFBundleShortVersionString)"
  arch="$(arch_of "$BUNDLE")"
  commit="$(git -C "$WS_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
  base="OpenPencil-$version-$commit-$arch"
  mkdir -p "$DIST_DIR"
  rm -f "$DIST_DIR/$base.zip" "$DIST_DIR/$base.dmg"
  ditto -c -k --sequesterRsrc --keepParent "$BUNDLE" "$DIST_DIR/$base.zip"
  local staging
  staging="$(mktemp -d)"
  ditto "$BUNDLE" "$staging/$APP_NAME"
  ln -s /Applications "$staging/Applications"
  hdiutil create -quiet -volname "OpenPencil $version" -srcfolder "$staging" \
    -ov -format UDZO "$DIST_DIR/$base.dmg"
  rm -rf "$staging"
  (cd "$DIST_DIR" && shasum -a 256 "$base.zip" "$base.dmg" >"$base.sha256")
  printf 'packaged:\n  %s\n  %s\n  %s\n' \
    "$DIST_DIR/$base.zip" "$DIST_DIR/$base.dmg" "$DIST_DIR/$base.sha256"
}

cmd_install() {
  local source="${1:-$BUNDLE}"
  require_bundle "$source"
  refuse_if_running
  local backup
  backup="$(backup_installed)"
  mkdir -p "$INSTALL_DIR"
  # Install through a sibling temp name, then swap, so a failed copy never
  # leaves a half-written app at the real path.
  local staging="$INSTALL_DIR/.OpenPencil-installing.app"
  rm -rf "$staging"
  ditto "$source" "$staging"
  rm -rf "$INSTALLED"
  mv "$staging" "$INSTALLED"
  xattr -dr com.apple.quarantine "$INSTALLED" 2>/dev/null || true
  printf 'installed: %s (version %s, %s)\n' "$INSTALLED" \
    "$(plist_value "$INSTALLED" CFBundleShortVersionString)" "$(arch_of "$INSTALLED")"
  if [[ -n "$backup" ]]; then
    printf 'revert any time with: scripts/install-macos.sh revert\n'
  fi
}

list_backups() {
  [[ -d "$BACKUP_DIR" ]] || return 0
  # Newest first by modification time of the backup directory entry.
  ls -1t "$BACKUP_DIR" 2>/dev/null | grep '\.app$' || true
}

cmd_list() {
  local any=""
  while IFS= read -r name; do
    any=1
    printf '%s  (version %s, %s)\n' "$BACKUP_DIR/$name" \
      "$(plist_value "$BACKUP_DIR/$name" CFBundleShortVersionString)" \
      "$(arch_of "$BACKUP_DIR/$name")"
  done < <(list_backups)
  [[ -n "$any" ]] || printf 'no backups in %s\n' "$BACKUP_DIR"
}

cmd_revert() {
  local choice="${1:-}"
  local target
  if [[ -n "$choice" ]]; then
    # A bare backup name is resolved inside the backup directory.
    if [[ -d "$choice" ]]; then target="$choice"; else target="$BACKUP_DIR/$choice"; fi
  else
    [[ -f "$BACKUP_DIR/.last-backup" ]] || fail "no backup recorded in $BACKUP_DIR; run 'list' and pass one"
    target="$(cat "$BACKUP_DIR/.last-backup")"
  fi
  require_bundle "$target"
  # Reverting swaps: the app being replaced is backed up first, so you can
  # undo the revert with another `revert`.
  cmd_install "$target"
}

case "${1:-}" in
  package) cmd_package ;;
  install) shift; cmd_install "${1:-}" ;;
  list) cmd_list ;;
  revert) shift; cmd_revert "${1:-}" ;;
  *)
    sed -n '2,19p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
