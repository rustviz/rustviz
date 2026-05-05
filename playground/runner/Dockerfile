# syntax=docker/dockerfile:1.7
#
# rustviz-runner — sandboxed execution image for the RustViz playground.
#
# The plugin must be built against the same nightly that rustc_private was
# pinned to (nightly-2025-08-20), so we use a builder that installs that
# specific toolchain, then carry only the binaries + a matching runtime
# toolchain into the final image. The host (playground) invokes:
#
#   docker run --rm -i --network=none --read-only \
#     --memory=512m --cpus=1 --pids-limit=64 \
#     --tmpfs=/work:rw,size=128m --tmpfs=/tmp:rw,size=32m \
#     rustviz/rustviz-runner:latest
#
# and pipes Rust source on stdin. The container's entrypoint creates a
# throwaway crate in /work, runs `cargo rv-plugin`, and prints the SVG
# payload on stdout.

ARG RUST_NIGHTLY=nightly-2025-08-20

# ---------- builder ----------
FROM rust:1.83-bookworm AS builder
ARG RUST_NIGHTLY
RUN rustup toolchain install ${RUST_NIGHTLY} \
    --profile minimal \
    --component rust-src,rustc-dev,llvm-tools-preview \
 && rustup default ${RUST_NIGHTLY}

WORKDIR /src

# Copy only what cargo install --path needs to build rustviz2-plugin.
# (mdbook-rustviz / playground / rustviz2 are not part of the runtime image.)
COPY rust-toolchain.toml ./
COPY Cargo.lock ./
COPY rustviz2-plugin/ ./rustviz2-plugin/

# Standalone manifest so cargo install isn't tripped up by the workspace's
# other members not being present.
RUN printf '[workspace]\nmembers = ["rustviz2-plugin"]\nresolver = "2"\n' > Cargo.toml

# Compile at opt-level 2 instead of cargo install's release default
# (3). At -O3 on aarch64-unknown-linux-gnu, nightly-2025-08-20 LLVM
# SIGTRAPs partway through codegen of rustviz2-plugin (which links
# rustc_private — i.e. lots of generic instantiations from rustc's
# internal crates feeding into LLVM's optimizer all at once). -O2
# completes cleanly; the runtime difference for the plugin's hot
# paths is negligible since the heavy lifting is inside rustc
# itself, which is precompiled.
ENV CARGO_PROFILE_RELEASE_OPT_LEVEL=2
RUN cargo install --locked --path rustviz2-plugin --root /usr/local

# ---------- runtime ----------
FROM rust:1.83-bookworm AS runtime
ARG RUST_NIGHTLY
RUN rustup toolchain install ${RUST_NIGHTLY} \
    --profile minimal \
    --component rust-src,rustc-dev,llvm-tools-preview \
 && rustup default ${RUST_NIGHTLY} \
 && rm -rf /usr/local/cargo/registry/cache \
            /usr/local/cargo/registry/src \
 && useradd --create-home --uid 1000 runner

# Plugin binaries land in cargo's bindir so `cargo rv-plugin` resolves them.
COPY --from=builder /usr/local/bin/cargo-rv-plugin /usr/local/cargo/bin/
COPY --from=builder /usr/local/bin/rv-plugin-driver /usr/local/cargo/bin/

COPY runner/rv-runner.sh /usr/local/bin/rv-runner
RUN chmod +x /usr/local/bin/rv-runner

USER runner
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/rv-runner"]
