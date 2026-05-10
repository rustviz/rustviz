# syntax=docker/dockerfile:1.7
#
# Deploy image for the RustViz playground. Multi-stage:
#   1. playground-builder (alpine, musl): compile the playground binary
#      against the workspace's pinned nightly. Alpine matches the final
#      docker:dind stage's libc, so the binary drops in without
#      cross-compilation. The plugin is *not* installed here — rustc-dev
#      isn't shipped as rlibs for the musl target, so a `cargo install
#      --path rustviz-plugin` would fail with `rustc_driver required to
#      be available in rlib format, but was not found in this form`.
#   2. plugin-builder (bookworm, glibc): install rustviz-plugin and
#      rustviz-cli on a glibc base where rustc-dev rlibs exist. The
#      installed binaries here are only used by stage 3's prerender hook;
#      they don't ride into the final image.
#   3. frontend-builder (FROM plugin-builder): vite build for the SPA,
#      including the prebuild hook that prerenders every dropdown example
#      through `rustviz svg` so first-paint dropdown clicks don't pay for
#      the cold-start compile-API round-trip. Inherits plugin-builder so
#      `rustviz` is already on PATH and the workspace tree is already at
#      /src/.
#   4. final (docker:dind): playground runs alongside an in-VM dockerd
#      and shells out to `docker run rustviz/rustviz-runner` per request
#      (see ../SECURITY.md). The runner image itself is published
#      separately to GHCR by .github/workflows/runner-image.yml, and
#      entrypoint.sh pulls it on first boot. This keeps the deploy image
#      small (~135 MiB) and Fly Machines stateless wrt the runner —
#      `fly scale count N` horizontal scaling needs no per-Machine
#      bootstrap.

ARG RUST_NIGHTLY=nightly-2025-08-20

# ---------- 1. playground-builder (musl) ----------
FROM rust:1.83-alpine AS playground-builder
ARG RUST_NIGHTLY
RUN apk add --no-cache musl-dev gcc make perl pkgconfig \
 && rustup toolchain install ${RUST_NIGHTLY} --profile minimal \
 && rustup default ${RUST_NIGHTLY}

WORKDIR /src
# Build context is the repo root. Copy enough of the workspace that cargo
# can resolve + build playground. (rustviz-plugin's manifest is in the
# workspace graph but not in playground's dep tree, so we include the
# manifest for resolution but never compile its sources here.)
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY rustviz-plugin/Cargo.toml ./rustviz-plugin/Cargo.toml
COPY rustviz-plugin/src/ ./rustviz-plugin/src/
COPY rustviz-lib/ ./rustviz-lib/
COPY rustviz-cli/ ./rustviz-cli/
COPY mdbook-rustviz/ ./mdbook-rustviz/
COPY playground/Cargo.toml ./playground/Cargo.toml
COPY playground/src/ ./playground/src/
# xtask isn't compiled in either build stage, but cargo workspace
# resolution reads every member's manifest. Copy the (tiny) xtask
# crate so resolution succeeds without dragging its build in.
COPY xtask/ ./xtask/

RUN cargo build --release --locked -p rustviz-playground

# ---------- 2. plugin-builder (glibc) ----------
# rustc-dev ships rlibs for `*-unknown-linux-gnu` but not for musl, so
# any stage that does `cargo install --path rustviz-plugin` (which links
# against rustc internals via rustc_plugin / rustc_utils) needs a glibc
# base. Bookworm matches the runner image (../runner/Dockerfile) so the
# pinned nightly is exercised against the same target there too.
FROM rust:1.83-bookworm AS plugin-builder
ARG RUST_NIGHTLY
RUN apt-get update && apt-get install -y --no-install-recommends \
        nodejs npm pkg-config \
 && rm -rf /var/lib/apt/lists/* \
 && rustup toolchain install ${RUST_NIGHTLY} --profile minimal \
        --component rust-src,rustc-dev,llvm-tools-preview \
 && rustup default ${RUST_NIGHTLY}

WORKDIR /src
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY rustviz-plugin/ ./rustviz-plugin/
COPY rustviz-lib/ ./rustviz-lib/
COPY rustviz-cli/ ./rustviz-cli/
COPY mdbook-rustviz/ ./mdbook-rustviz/
COPY playground/Cargo.toml ./playground/Cargo.toml
COPY playground/src/ ./playground/src/
# Workspace resolution needs xtask's manifest readable; see stage
# 1's note. Copying just the (small) crate dir keeps the cost
# negligible and avoids touching the workspace Cargo.toml itself.
COPY xtask/ ./xtask/

# Install the plugin + CLI so stage 3's prerender (which shells out to
# `rustviz svg`) finds them on PATH at /usr/local/cargo/bin/.
# `--locked` mirrors setup.sh: pinned Cargo.lock or bust.
RUN cargo install --path rustviz-plugin --locked \
 && cargo install --path rustviz-cli --locked

# ---------- 3. frontend-builder ----------
# Inherits plugin-builder's filesystem so we get:
#   - the rustviz CLI on PATH at /usr/local/cargo/bin/rustviz (used by
#     the prebuild prerender hook)
#   - the workspace tree at /src/, which is what vite.config.ts'
#     `resolve(__dirname, '../../rust-toolchain.toml')` already expects
#     when invoked from a normal local checkout — no special-case
#     copies into / required.
FROM plugin-builder AS frontend-builder
WORKDIR /src/playground/frontend
COPY playground/frontend/package.json playground/frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY playground/frontend/ ./
# `npm run build` triggers `prebuild` (tsx scripts/prerender.ts → calls
# `rustviz svg` per dropdown example → writes src/prerendered.json),
# then `tsc -b && vite build`. A failed prerender aborts the build by
# design.
RUN npm run build

# ---------- 4. final (docker:dind) ----------
FROM docker:27-dind
# fuse-overlayfs is a userspace overlay implementation that lets dockerd's
# storage driver work on top of the Fly Machine's overlay rootfs (the
# kernel doesn't allow nested overlay2). Avoids needing a per-Machine ext4
# volume mounted at /var/lib/docker just for dockerd's storage.
RUN apk add --no-cache bash curl tini fuse-overlayfs

# playground binary (built on Alpine in stage 1, so already musl —
# drops straight into the alpine docker:dind base without
# cross-compilation).
COPY --from=playground-builder /src/target/release/rustviz-playground /usr/local/bin/rustviz-playground

# Frontend bundle (Vite output). The Vite build copies frontend/public/
# into dist/, so ex-assets/{helpers.js,visualization.css} ride along.
WORKDIR /app
COPY --from=frontend-builder /src/playground/frontend/dist/ /app/frontend/dist/

# The runner image (rustviz/rustviz-runner:latest) is built and pushed by
# .github/workflows/runner-image.yml to ghcr.io/rustviz/rustviz-runner.
# entrypoint.sh pulls it on first boot. Pre-Phase-6 deploys baked the
# runner build context into /opt/runner-context here and built at boot;
# now Machines are stateless wrt the runner image so `fly scale count N`
# horizontal scaling Just Works without per-Machine bootstrapping.

COPY playground/deploy/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENV RV_BIND=0.0.0.0:8080 \
    RV_RUNNER=docker

EXPOSE 8080
ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint.sh"]
