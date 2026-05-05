#!/usr/bin/env bash
# Container entrypoint. Reads Rust source from stdin, runs the rustviz2 plugin
# against a fresh single-file crate in the tmpfs-backed /work directory, and
# prints the plugin's stdout (which the host parses as
# "<code-svg>:::<timeline-svg>").
#
# This script does not enforce sandboxing on its own — it relies on the host
# invoking `docker run` with --network=none --read-only --tmpfs=/work …
# --memory=… --cpus=… --pids-limit=… as documented in SECURITY.md.

set -euo pipefail

cd /work
rm -rf test-crate

cargo new --lib --quiet test-crate
cd test-crate

# Pin the toolchain that rustviz2-plugin was built against; without this the
# nested cargo invocation can pick up an unrelated default.
cat > rust-toolchain.toml <<'TOML'
[toolchain]
channel = "nightly-2025-08-20"
components = ["rust-src", "rustc-dev", "llvm-tools-preview"]
TOML

# Rust source from stdin → crate's lib.rs.
cat > src/lib.rs

# Plugin emits "<code-svg>:::<timeline-svg>" on stdout; the host parses it.
exec cargo rv-plugin
