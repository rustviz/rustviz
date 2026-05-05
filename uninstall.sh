#!/usr/bin/env bash
# Reverse of setup.sh. Removes RustViz-related installs from this
# host. Useful for testing setup.sh from a clean slate, and for
# tidying up after experimentation.
#
# Default behaviour (no flags):
#   - cargo uninstall the rustviz + plugin binaries from ~/.cargo/bin
#   - remove the local rustviz/rustviz-runner docker image (if present)
#   - remove playground frontend dist + node_modules + tsbuildinfo
#
# Things default-OFF — pass the flag to also remove them:
#   --toolchain    rustup uninstall the nightly pinned by rust-toolchain.toml
#                  (off by default; you might be using that nightly for
#                  other projects on this machine)
#   --target       rm -rf the cargo `target/` tree (off by default; slow
#                  to regenerate even though it's not technically a
#                  RustViz-only artifact)
#   --everything   shorthand for --toolchain --target
#
# Other flags:
#   --dry-run / -n   show what would run, don't execute
#   --help    / -h   show this help

set -euo pipefail

cd "$(dirname "$0")"

DRY_RUN=0
REMOVE_TOOLCHAIN=0
REMOVE_TARGET=0

while [ $# -gt 0 ]; do
    case "$1" in
        -n|--dry-run)  DRY_RUN=1 ;;
        --toolchain)   REMOVE_TOOLCHAIN=1 ;;
        --target)      REMOVE_TARGET=1 ;;
        --everything)  REMOVE_TOOLCHAIN=1; REMOVE_TARGET=1 ;;
        -h|--help)     sed -n '2,23p' "$0"; exit 0 ;;
        *)             echo "Unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

run() {
    echo "$ $*"
    if [ "$DRY_RUN" = 0 ]; then
        "$@"
    fi
}

# ----------------------------------------------------------------
# 1. Cargo-installed binaries.
#
# `cargo uninstall <pkg>` removes the binaries the package produced
# from ~/.cargo/bin AND the registry metadata under ~/.cargo. It's
# the symmetric undo of `cargo install`. Tolerates "not installed"
# silently so the script is idempotent — a second run is a no-op.
# ----------------------------------------------------------------
echo "==> cargo uninstall"
for pkg in rustviz2 rustviz2-plugin; do
    if cargo install --list 2>/dev/null | grep -qE "^${pkg} v"; then
        run cargo uninstall "$pkg"
    else
        echo "  - $pkg not installed; skipping"
    fi
done

# ----------------------------------------------------------------
# 2. Docker runner image. Removing the local copy doesn't touch
# anything in GHCR; subsequent `setup.sh` runs will re-pull or
# re-build as needed.
# ----------------------------------------------------------------
echo "==> docker image"
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    if docker image inspect rustviz/rustviz-runner:latest >/dev/null 2>&1; then
        run docker rmi rustviz/rustviz-runner:latest
    else
        echo "  - rustviz/rustviz-runner:latest not present; skipping"
    fi
    # Remove the GHCR-prefixed copy too, if setup.sh's pull path
    # left one behind under the original tag.
    if docker image inspect ghcr.io/rustviz/rustviz-runner:latest >/dev/null 2>&1; then
        run docker rmi ghcr.io/rustviz/rustviz-runner:latest
    fi
else
    echo "  - docker not available; skipping"
fi

# ----------------------------------------------------------------
# 3. Frontend artifacts. dist/ + node_modules/ + *.tsbuildinfo are
# all regenerable by `npm install && npm run build` (which is what
# setup.sh runs).
# ----------------------------------------------------------------
echo "==> playground frontend artifacts"
run rm -rf playground/frontend/dist \
           playground/frontend/node_modules
# tsbuildinfo files might or might not exist; let the glob fail silently
if compgen -G "playground/frontend/*.tsbuildinfo" > /dev/null; then
    run rm -f playground/frontend/*.tsbuildinfo
fi

# ----------------------------------------------------------------
# 4. Optional: rustup toolchain. Off by default — the nightly
# pinned by rust-toolchain.toml might be in use by other projects
# on this machine. Channel string is read from the same file
# rustviz2's lib.rs / rustviz init derive from, so this stays in
# sync automatically.
# ----------------------------------------------------------------
if [ "$REMOVE_TOOLCHAIN" = 1 ]; then
    CHANNEL=$(awk -F'"' '/^[[:space:]]*channel[[:space:]]*=/ {print $2; exit}' rust-toolchain.toml)
    if [ -z "$CHANNEL" ]; then
        echo "==> rustup toolchain: couldn't read channel from rust-toolchain.toml; skipping" >&2
    else
        echo "==> rustup toolchain ($CHANNEL)"
        if rustup toolchain list 2>/dev/null | grep -q "^${CHANNEL}"; then
            run rustup toolchain uninstall "$CHANNEL"
        else
            echo "  - $CHANNEL not installed; skipping"
        fi
    fi
fi

# ----------------------------------------------------------------
# 5. Optional: cargo target/ tree. Off by default because it's
# slow to regenerate (~minutes for a fresh workspace build) and
# only matters for from-scratch testing.
# ----------------------------------------------------------------
if [ "$REMOVE_TARGET" = 1 ]; then
    echo "==> cargo target/"
    run rm -rf target
fi

echo "==> done."
if [ "$DRY_RUN" = 1 ]; then
    echo "(dry run — nothing was actually removed)"
fi
