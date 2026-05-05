#!/usr/bin/env bash
# Reproduces a working build of rustviz2 on macOS/Linux.
#
# A committed Cargo.lock pins transitive deps that have since moved past
# rustc 1.80 / edition 2024 — without those pins, fresh resolution from
# crates.io fails on the pinned nightly. Toolchain refresh would remove the
# need for this; until then the lockfile is the floor.
set -euo pipefail

cd "$(dirname "$0")"

# 1. Toolchain (rust-toolchain.toml triggers rustup to auto-install
#    nightly-2025-08-20 with the rustc-dev/rust-src components we need).
rustup show active-toolchain >/dev/null

# 2. Build & install the rustc plugin (provides `cargo rv-plugin`).
cargo install --path rustviz2-plugin --locked

# 3. Build the Vite frontend that the playground serves. The build copies
#    frontend/public/ex-assets/ (helpers.js + visualization.css) into dist/
#    so they ride along with the SPA whether served by playground or a CDN.
( cd playground/frontend && npm install && npm run build )

# 4. Build the rest of the workspace.
cargo build --workspace --release

# 5. Get the sandboxed runner image into the local docker daemon. Two paths:
#    - Pull from GHCR (faster, ~30 s, what production does).
#    - Build from runner/Dockerfile locally (slower, ~5 min, but works offline
#      and lets you iterate on plugin changes without round-tripping through
#      a CI image push).
#    Pull is the default; pass --build-runner to force a local build.
#    Skipped silently when docker isn't on PATH so devs who only iterate
#    against RV_RUNNER=local don't need it installed.
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    if [ "${1:-}" = "--build-runner" ]; then
        echo "Building rustviz/rustviz-runner image locally..."
        docker build -t rustviz/rustviz-runner:latest -f runner/Dockerfile .
    elif docker image inspect rustviz/rustviz-runner:latest >/dev/null 2>&1; then
        echo "Runner image already present locally; pass --build-runner to rebuild."
    else
        echo "Pulling rustviz/rustviz-runner image from GHCR..."
        if docker pull ghcr.io/rustviz/rustviz-runner:latest && \
           docker tag ghcr.io/rustviz/rustviz-runner:latest rustviz/rustviz-runner:latest; then
            echo "Runner image pulled."
        else
            echo "Pull failed (registry unreachable, image not yet published, or no internet)." >&2
            echo "Building locally as fallback (~5 min)..." >&2
            docker build -t rustviz/rustviz-runner:latest -f runner/Dockerfile .
        fi
    fi
else
    cat <<'WARN'
Skipping runner image setup: docker is not available.

For local dev, set RV_RUNNER=local to run the plugin in-process
(NEVER do this on a public deployment — see SECURITY.md).
WARN
fi

cat <<'EOF'

Setup complete. To run the playground:

  RV_RUNNER=local cd playground && cargo run --release   # local dev
  cd playground && cargo run --release                   # docker (default)
  open http://127.0.0.1:8080/

To iterate on the frontend in dev mode (hot reload, proxies API to :8080):

  cd playground/frontend && npm run dev
  open http://127.0.0.1:3000/

To run the rustc plugin directly (host toolchain) against test-crate:

  cd test-crate && cargo rv-plugin -w
EOF
