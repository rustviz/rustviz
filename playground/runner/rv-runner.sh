#!/usr/bin/env bash
# Runner entrypoint. Reads Rust source from stdin, runs the RustViz plugin
# against a fresh single-file crate in $RV_WORKDIR (default /work), and
# prints the plugin's stdout (one JSON object holding both SVG panels,
# parsed by rustviz-lib's `parse_output`).
#
# Isolation comes from whoever launches this script: `docker run
# --network=none --read-only --tmpfs=/work …` (local docker) or Cloud Run's
# per-request `sandbox do` (see SECURITY.md). The one control enforced
# here is the wall-clock limit, because a launcher that is killed from
# outside (e.g. a `docker run` client) can leave its payload running.

set -euo pipefail

WORKDIR="${RV_WORKDIR:-/work}"
mkdir -p "$WORKDIR"
cd "$WORKDIR"
rm -rf user-code

# Crate name surfaces in rustc/cargo error messages ("could not compile
# `user-code`"), so use a name that reads as "this is the user's code"
# rather than a leaked internal placeholder.
cargo new --lib --quiet --vcs none user-code
cd user-code

# Pin the toolchain that rustviz-plugin was built against; without this the
# nested cargo invocation can pick up an unrelated default.
cat > rust-toolchain.toml <<'TOML'
[toolchain]
channel = "nightly-2026-05-01"
components = ["rust-src", "rustc-dev", "llvm-tools-preview"]
TOML

# Rust source from stdin → crate's lib.rs.
cat > src/lib.rs

# Plugin prints its JSON result on stdout; the host parses it.
# Killed a little before the server's own 20 s deadline so the compile
# can't outlive the request. Some launchers (Cloud Run's `sandbox do`)
# don't pass the payload's exit status through, so a timeout is also
# announced on stderr with a sentinel the server matches on
# (rustviz-lib::RUNNER_TIMEOUT_SENTINEL).
set +e
timeout -s KILL "${RV_INNER_TIMEOUT_SECS:-18}" cargo rv-plugin
rc=$?
set -e
if [ "$rc" -eq 137 ]; then
    echo "rustviz-runner: compile timed out" >&2
    exit 124
fi
exit "$rc"
