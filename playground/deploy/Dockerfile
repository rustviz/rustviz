# syntax=docker/dockerfile:1.7
#
# Deploy image for the RustViz playground. Multi-stage:
#   1. rust-builder: compile playground against the workspace's pinned
#      nightly, AND install the rustviz CLI for stage 2's prerender.
#   2. frontend-builder: vite build for the SPA, including the prebuild
#      hook that prerenders every dropdown example through `rustviz svg`
#      so first-paint dropdown clicks don't pay for the cold-start
#      compile-API round-trip. Inherits from stage 1 to avoid a second
#      toolchain install.
#   3. final: docker:dind base — playground runs alongside an in-VM dockerd
#      and shells out to `docker run rustviz/rustviz-runner` per request
#      (see ../SECURITY.md). The runner image itself is published separately
#      to GHCR by .github/workflows/runner-image.yml, and entrypoint.sh
#      pulls it on first boot. This keeps the deploy image small (~135 MiB)
#      and Fly Machines stateless wrt the runner — `fly scale count N`
#      horizontal scaling needs no per-Machine bootstrap.

ARG RUST_NIGHTLY=nightly-2025-08-20

# ---------- 1. rust-builder ----------
# Alpine base shares its libc (musl) with the docker:dind final stage, so
# the playground binary we produce here drops directly into stage 3 without
# cross-compilation. We also install Node here so stage 2 can inherit and
# run the SPA build without a second toolchain install.
FROM rust:1.83-alpine AS rust-builder
ARG RUST_NIGHTLY
RUN apk add --no-cache musl-dev gcc make perl pkgconfig nodejs npm \
 && rustup toolchain install ${RUST_NIGHTLY} --profile minimal \
        --component rust-src,rustc-dev,llvm-tools-preview \
 && rustup default ${RUST_NIGHTLY}

WORKDIR /src
# Build context is the repo root. Copy enough of the workspace that cargo
# can resolve and build playground + install the rustviz CLI (needed by
# stage 2's prerender step).
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY rustviz-plugin/Cargo.toml ./rustviz-plugin/Cargo.toml
COPY rustviz-plugin/src/ ./rustviz-plugin/src/
COPY rustviz-lib/ ./rustviz-lib/
COPY rustviz-cli/ ./rustviz-cli/
COPY mdbook-rustviz/ ./mdbook-rustviz/
COPY playground/Cargo.toml ./playground/Cargo.toml
COPY playground/src/ ./playground/src/

# Build the playground binary AND install rustviz-plugin + rustviz-cli
# so the prerender script in stage 2 can shell out to `rustviz svg`.
# `--locked` mirrors setup.sh: pinned-Cargo.lock-or-bust against the
# workspace's pinned nightly.
RUN cargo build --release --locked -p rustviz-playground \
 && cargo install --path rustviz-plugin --locked \
 && cargo install --path rustviz-cli --locked

# ---------- 2. frontend-builder ----------
# Inherits stage 1's filesystem so we get:
#   - the rustviz CLI on PATH at /usr/local/cargo/bin/rustviz (used by
#     the prebuild prerender hook)
#   - the workspace tree at /src/, which is what vite.config.ts'
#     `resolve(__dirname, '../../rust-toolchain.toml')` already expects
#     when invoked from a normal local checkout — no special-case
#     copies into / required.
FROM rust-builder AS frontend-builder
WORKDIR /src/playground/frontend
COPY playground/frontend/package.json playground/frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY playground/frontend/ ./
# `npm run build` triggers `prebuild` (tsx scripts/prerender.ts → calls
# `rustviz svg` per dropdown example → writes src/prerendered.json),
# then `tsc -b && vite build`. A failed prerender aborts the build by
# design.
RUN npm run build

# ---------- 3. final (docker:dind) ----------
FROM docker:27-dind
# fuse-overlayfs is a userspace overlay implementation that lets dockerd's
# storage driver work on top of the Fly Machine's overlay rootfs (the
# kernel doesn't allow nested overlay2). Avoids needing a per-Machine ext4
# volume mounted at /var/lib/docker just for dockerd's storage.
RUN apk add --no-cache bash curl tini fuse-overlayfs

# playground binary (built on Alpine in stage 1, so already musl).
COPY --from=rust-builder /src/target/release/rustviz-playground /usr/local/bin/rustviz-playground

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
