#!/usr/bin/env bash
# Deploy entrypoint: bring up dockerd, ensure the runner image is loaded,
# then exec playground. Runs as PID 1 inside the docker:dind base; tini
# wraps us so signal handling is sane.

set -euo pipefail

LOG() { printf '[entrypoint] %s\n' "$*" >&2; }

# Where to pull the runner image from. The default points at the
# rustviz/rustviz-runner package on GHCR, populated by
# .github/workflows/runner-image.yml on every push to main that touches
# runner/** or rustviz2-plugin/**. Override with RV_RUNNER_PULL_REF for
# staging/private deployments.
PULL_REF="${RV_RUNNER_PULL_REF:-ghcr.io/rustviz/rustviz-runner:latest}"
LOCAL_TAG="rustviz/rustviz-runner:latest"

# 1. Start dockerd in the background. The default dind entrypoint takes
#    care of cgroup setup, certificate handling for TLS, etc.
LOG "starting dockerd..."
# fuse-overlayfs storage driver: works on top of the Fly Machine's overlay
# rootfs without needing a per-Machine ext4 volume mounted at
# /var/lib/docker. Slightly slower than the kernel overlay2 driver
# (~10-20% per-container-start) but acceptable for our workload (one image,
# small per-request containers) and removes ~$5/mo of volume cost across a
# 10-Machine fleet. Requires fuse-overlayfs in the image and FUSE in the
# kernel (Fly Machines have it).
dockerd-entrypoint.sh dockerd \
    --host=unix:///var/run/docker.sock \
    --storage-driver=fuse-overlayfs \
    > /var/log/dockerd.log 2>&1 &

# 2. Wait up to 60s for dockerd to accept connections.
LOG "waiting for dockerd to be ready..."
for i in $(seq 1 60); do
    if docker info >/dev/null 2>&1; then
        LOG "dockerd ready after ${i}s"
        break
    fi
    sleep 1
    if [ "$i" = "60" ]; then
        LOG "ERROR: dockerd did not become ready in 60s"
        tail -50 /var/log/dockerd.log >&2 || true
        exit 1
    fi
done

# 3. Ensure the runner image is present locally. Three cases:
#
#    a) We're on a Fly Machine that already pulled this image and its
#       /var/lib/docker still has the layers. No-op, instant.
#    b) We're on a freshly-created Fly Machine (post-`fly scale count N+1`)
#       that has no docker state yet. Pull from the registry; takes ~30 s
#       for the ~600 MB image.
#    c) We're on a dev box that built the runner image locally with
#       `setup.sh`. Image is already tagged rustviz/rustviz-runner:latest;
#       the inspect succeeds and we don't try to pull (which would fail
#       in offline dev).
#
if docker image inspect "$LOCAL_TAG" >/dev/null 2>&1; then
    LOG "runner image already present locally"
else
    LOG "pulling runner image from ${PULL_REF} (~30 s on first boot)..."
    if ! docker pull "$PULL_REF"; then
        LOG "ERROR: pull failed. If you're running offline, build the image"
        LOG "       locally first: docker build -t ${LOCAL_TAG} -f runner/Dockerfile ."
        exit 1
    fi
    docker tag "$PULL_REF" "$LOCAL_TAG"
    LOG "runner image pulled and tagged as ${LOCAL_TAG}"
fi

# 4. Exec playground. Bind addr + RV_RUNNER come from the env (set in
#    Dockerfile / fly.toml).
LOG "starting playground on ${RV_BIND}"
cd /app
exec /usr/local/bin/rustviz-playground
