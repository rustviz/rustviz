#!/usr/bin/env bash
# Generate N feature-combination programs from SEED, keep the ones rustc
# accepts, run RustViz's corpus checks on them, shrink and bucket the
# failures. See tools/progen/README.md.
#
#   tools/progen/run.sh N SEED [OUT]      (OUT defaults to progen-out/SEED)
#
# Env: PROGEN_TIME_LIMIT (seconds for the whole run; generation stops at
# ~55% of it, shrinking at the end), PROGEN_PLUGIN_ROOT (a `cargo install
# --root` holding the plugin to test; default: `cargo rv-plugin` on PATH),
# PROGEN_TEST_BIN (a prebuilt corpus test binary; default: built here).
set -euo pipefail
if [ $# -lt 2 ]; then
    echo "usage: $0 N SEED [OUT]" >&2
    exit 2
fi
here="$(cd "$(dirname "$0")" && pwd)"
out="${3:-progen-out/$2}"
exec python3 "$here/progen.py" run --count "$1" --seed "$2" --out "$out" \
    --time-limit "${PROGEN_TIME_LIMIT:-0}"
