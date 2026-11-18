#!/usr/bin/env bash
# Compatibility wrapper: the font notices are now staged together with
# THIRD_PARTY_NOTICES.md by stage-notices.sh. Old callers keep working.
set -euo pipefail
exec bash "$(dirname "${BASH_SOURCE[0]}")/stage-notices.sh" "$@"
