# syntax=docker/dockerfile:1.7
#
# Cloud Run image for the RustViz compile API: the playground server, the
# pinned nightly toolchain, the rustviz plugin and the runner script, in
# one image. Each request runs playground/runner/rv-runner.sh under Cloud
# Run's per-request sandbox launcher (`sandbox do`), configured via
# RV_SANDBOX_CMD at deploy time. No docker-in-docker, no privileged mode.
#
# Build context: repo root.
#   docker build -f playground/cloudrun/Dockerfile .

ARG RUST_NIGHTLY=nightly-2026-05-01

# ---------- builder ----------
FROM rust:1.83-bookworm AS builder
ARG RUST_NIGHTLY
RUN rustup toolchain install ${RUST_NIGHTLY} --profile minimal \
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
COPY xtask/ ./xtask/
RUN cargo install --locked --path rustviz-plugin --root /out \
 && cargo build --release --locked -p rustviz-playground \
 && cp target/release/rustviz-playground /out/bin/

# ---------- runtime ----------
# Slim base plus a fresh minimal rustup install of just the pinned nightly
# (no stable toolchain, no registry caches, no C toolchain: the plugin
# runs `cargo check` on a lib crate, which never links). Smaller image,
# faster Cloud Run cold starts.
FROM debian:bookworm-slim
ARG RUST_NIGHTLY
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path \
        --profile minimal --default-toolchain ${RUST_NIGHTLY} \
        --component rust-src,rustc-dev,llvm-tools-preview \
 && apt-get purge -y curl && apt-get autoremove -y \
 && rm -rf /var/lib/apt/lists/* /usr/local/rustup/downloads /usr/local/rustup/tmp \
 && chmod -R a+rX /usr/local/rustup /usr/local/cargo \
 && useradd --uid 1000 --create-home runner

COPY --from=builder /out/bin/cargo-rv-plugin /out/bin/rv-plugin-driver /usr/local/cargo/bin/
COPY --from=builder /out/bin/rustviz-playground /usr/local/bin/rustviz-playground
COPY playground/runner/rv-runner.sh /usr/local/bin/rv-runner
COPY playground/cloudrun/static/ /app/frontend/dist/
RUN chmod 755 /usr/local/bin/rv-runner

# The SPA lives on GitHub Pages; this service only serves /submit-code
# (plus a stub index + robots.txt so crawlers have nothing to chew on).
# RV_SANDBOX_CMD is set at deploy time (see .github/workflows/cloudrun.yml)
# because the launcher only exists on Cloud Run.
ENV RV_RUNNER=sandbox \
    RV_CLIENT_IP_HEADER=X-Forwarded-For \
    RV_WORKDIR=/tmp/work
WORKDIR /app
USER runner
CMD ["/usr/local/bin/rustviz-playground"]
