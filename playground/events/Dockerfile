# syntax=docker/dockerfile:1.7
#
# Cloud Run image for rustviz-events (telemetry collector + dashboard).
# NOT DEPLOYED: see playground/events/README.md before building or pushing.
#
# Build context: repo root (cargo needs the whole workspace manifest set).
#   docker build -f playground/events/Dockerfile -t rustviz-events .
#
# The collector is plain stable Rust (no rustc_private), so it builds on a
# stable image; RUSTUP_TOOLCHAIN overrides the workspace's pinned nightly in
# rust-toolchain.toml, which would otherwise download nightly + rustc-dev.

FROM rust:1.90-bookworm AS builder
ENV RUSTUP_TOOLCHAIN=1.90.0
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p rustviz-events

# Distroless: no shell, no package manager. TLS for the BigQuery API is
# rustls with bundled webpki roots, so no CA bundle or OpenSSL is needed.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /src/target/release/rustviz-events /usr/local/bin/rustviz-events
ENV RV_EVENTS_SINK=stdout
USER nonroot
ENTRYPOINT ["/usr/local/bin/rustviz-events"]
