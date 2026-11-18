#!/usr/bin/env bash
# Compile-check the optional feature combinations that a plain
# `cargo check --workspace --tests` does NOT build.
#
# Why this exists: adding a ChatDelta variant broke the mobile editor feature
# (`op-engine-ffi --features editor`) and nobody noticed, because the default
# workspace check never turns that feature on. Every host that has its own
# feature set is listed here, so a new enum variant or a changed signature
# fails here instead of on a device.
#
#   tools/check-feature-builds.sh            all combinations
#   tools/check-feature-builds.sh wasm       only combinations whose label contains "wasm"
#
# Uses the caller's CARGO_TARGET_DIR if set. The wasm combinations need the
# toolchain pinned in rust-toolchain.toml (rustup), so they run through it even
# when another `cargo` is first on PATH. A combination that cannot be checked
# (missing toolchain) is reported as SKIPPED and makes the script exit 2, so a
# skipped build is never mistaken for a passing one.

set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
FILTER="${1:-}"

PINNED="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml | head -1)"
PINNED_BIN=""
if command -v rustup >/dev/null 2>&1 && [ -n "$PINNED" ]; then
  # `dirname ""` is "." and would pass for a toolchain that is not installed,
  # so only take the directory when rustup really printed a cargo path.
  PINNED_CARGO="$(rustup which cargo --toolchain "$PINNED" 2>/dev/null || true)"
  if [ -n "$PINNED_CARGO" ] && [ -x "$PINNED_CARGO" ]; then
    PINNED_BIN="$(dirname "$PINNED_CARGO")"
  fi
fi

failures=()
skipped=()
ran=0

run() {
  local label="$1"
  shift
  if [ -n "$FILTER" ] && [[ "$label" != *"$FILTER"* ]]; then
    return 0
  fi
  ran=$((ran + 1))
  printf '%-58s ' "$label"
  local out
  if out="$("$@" 2>&1)"; then
    echo "ok"
  else
    echo "FAILED"
    failures+=("$label")
    printf '%s\n' "$out" | grep -E '^error' -A6 | head -24
  fi
}

run_pinned() {
  local label="$1"
  shift
  if [ -z "$PINNED_BIN" ]; then
    printf '%-58s %s\n' "$label" "SKIPPED (pinned toolchain '$PINNED' not installed)"
    skipped+=("$label")
    return 0
  fi
  run "$label" env PATH="$PINNED_BIN:$PATH" RUSTC="$PINNED_BIN/rustc" "$@"
}

run "workspace, default features, tests" \
  cargo check --workspace --tests
run "op-engine-ffi: editor (mobile)" \
  cargo check -p op-engine-ffi --features editor --tests
run "op-host-native: gl-host" \
  cargo check -p op-host-native --features gl-host --tests
run "op-host-native: gl-host + collab-host" \
  cargo check -p op-host-native --features gl-host,collab-host --tests
run "op-ai-skills: slim-corpus" \
  cargo check -p op-ai-skills --features slim-corpus --tests
run "op-editor-ui: slim-corpus + runtime catalogs" \
  cargo check -p op-editor-ui --features slim-corpus,runtime-icon-catalog,runtime-locale-catalog --tests
run_pinned "wasm: op-host-web (web)" \
  cargo check --target wasm32-unknown-unknown -p op-host-web --no-default-features --features web

echo
if [ "${#failures[@]}" -gt 0 ]; then
  echo "feature builds: ${#failures[@]} of $ran FAILED:"
  printf '  - %s\n' "${failures[@]}"
  exit 1
fi
if [ "${#skipped[@]}" -gt 0 ]; then
  echo "feature builds: ${#skipped[@]} combination(s) were NOT checked:"
  printf '  - %s\n' "${skipped[@]}"
  echo "feature builds: the $ran combination(s) that could be checked compile"
  exit 2
fi
echo "feature builds: all $ran combinations compile"
