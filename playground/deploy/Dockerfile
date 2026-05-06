# syntax=docker/dockerfile:1.7
#
# Deploy image for the RustViz playground. Multi-stage:
#   1. rust-builder: compile playground against the workspace's pinned nightly.
#   2. frontend-builder: vite build for the SPA.
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
# cross-compilation.
FROM rust:1.83-alpine AS rust-builder
ARG RUST_NIGHTLY
RUN apk add --no-cache musl-dev gcc make perl pkgconfig \
 && rustup toolchain install ${RUST_NIGHTLY} --profile minimal \
        --component rust-src,rustc-dev,llvm-tools-preview \
 && rustup default ${RUST_NIGHTLY}

WORKDIR /src
# Build context is the repo root. Copy enough of the workspace that cargo
# can resolve and build playground. (rustviz-plugin is excluded from the
# playground binary's dep tree, so it's not compiled here even though its
# manifest is part of the workspace resolution graph.)
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY rustviz-plugin/Cargo.toml ./rustviz-plugin/Cargo.toml
COPY rustviz-plugin/src/ ./rustviz-plugin/src/
COPY rustviz-lib/ ./rustviz-lib/
COPY rustviz-cli/ ./rustviz-cli/
COPY mdbook-rustviz/ ./mdbook-rustviz/
COPY playground/Cargo.toml ./playground/Cargo.toml
COPY playground/src/ ./playground/src/

RUN cargo build --release --locked -p rustviz-playground

# ---------- 2. frontend-builder ----------
FROM node:20-alpine AS frontend-builder
WORKDIR /src
COPY playground/frontend/package.json playground/frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY playground/frontend/ ./
# vite.config.ts reads two files at build time to inject build-info
# strings into the SPA bundle (the "Compiles with rustc … and
# rustviz-plugin …" line under the Generate button):
#   - rust-toolchain.toml       -> [toolchain].channel
#   - rustviz-plugin/Cargo.toml -> [package].version
# Both lookups use `resolve(__dirname, '../../<file>')` from
# /src/vite.config.ts, so the files have to land at /<file> inside
# this stage. Locally the same relative path walks up from
# playground/frontend/ to the workspace root, so the same expressions
# work in both contexts as long as we copy the files into matching
# slots here.
COPY rust-toolchain.toml /rust-toolchain.toml
COPY rustviz-plugin/Cargo.toml /rustviz-plugin/Cargo.toml
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
COPY --from=frontend-builder /src/dist/ /app/frontend/dist/

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
