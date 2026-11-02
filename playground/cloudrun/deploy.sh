#!/usr/bin/env bash
# Deploy an already-pushed playground image to Cloud Run.
#
#   playground/cloudrun/deploy.sh IMAGE [SERVICE]
#
# CI (.github/workflows/build.yml, deploy-cloudrun job) builds
# playground/cloudrun/Dockerfile, pushes it to Artifact Registry and calls
# this. Run it by hand to roll back: pass an older image digest/tag.
#
# Cost model: request-based billing with min-instances=0, so an idle
# service costs nothing and a stray bot request costs milliseconds. The
# ceiling is max-instances x (1 vCPU, 2 GiB) running flat out. There is no
# hard spend cap on Google Cloud (budgets only alert), so max-instances is
# the real limit; raise it deliberately before a planned spike.
#
# Isolation: --sandbox-launcher gives each request a fresh `sandbox do`
# sandbox (no egress, read-only image, private /tmp, none of the service's
# env/secrets/metadata). The sandbox does not inherit env, so everything
# rv-runner needs is passed explicitly in RV_SANDBOX_CMD. concurrency=1
# keeps one compile per instance so a runaway compile can't starve others.
set -euo pipefail

IMAGE="${1:?usage: deploy.sh IMAGE [SERVICE]}"
SERVICE="${2:-rustviz-api}"
PROJECT="${RV_GCP_PROJECT:-rustviz-playground}"
REGION="${RV_GCP_REGION:-us-central1}"
MAX_INSTANCES="${RV_MAX_INSTANCES:-3}"

SANDBOX_CMD="/usr/local/gcp/bin/sandbox do"
SANDBOX_CMD+=" --env PATH=/usr/local/cargo/bin:/usr/bin:/bin"
SANDBOX_CMD+=" --env RUSTUP_HOME=/usr/local/rustup"
SANDBOX_CMD+=" --env CARGO_HOME=/usr/local/cargo"
SANDBOX_CMD+=" --env HOME=/tmp"
SANDBOX_CMD+=" --env RV_WORKDIR=/tmp/work"
SANDBOX_CMD+=" --"

# `^@^` switches gcloud's env-var list delimiter from `,` to `@` so the
# spaces and `=` inside RV_SANDBOX_CMD survive.
gcloud beta run deploy "$SERVICE" \
    --project "$PROJECT" \
    --region "$REGION" \
    --image "$IMAGE" \
    --execution-environment gen2 \
    --sandbox-launcher \
    --concurrency 1 \
    --min-instances 0 \
    --max-instances "$MAX_INSTANCES" \
    --cpu 1 \
    --memory 2Gi \
    --timeout 60 \
    --allow-unauthenticated \
    --service-account "rv-runtime@${PROJECT}.iam.gserviceaccount.com" \
    --set-env-vars "^@^RV_SANDBOX_CMD=${SANDBOX_CMD}@RV_RATE_BURST=5@RV_RATE_SECONDS_PER_REQUEST=2" \
    --quiet

URL=$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" --format='value(status.url)')
echo "Deployed ${IMAGE} to ${URL}"

# Smoke test: one real compile through the sandbox.
body='{"code":"fn main() {\n    let s = String::from(\"hi\");\n    let t = s;\n}\n"}'
code=$(curl -s -o /tmp/rv-smoke.json -w '%{http_code}' --max-time 90 \
    -H 'Content-Type: application/json' -d "$body" "${URL}/submit-code")
if [ "$code" != "200" ] || ! grep -q '"timeline_panel"' /tmp/rv-smoke.json; then
    echo "Smoke test failed (HTTP $code):" >&2
    head -c 2000 /tmp/rv-smoke.json >&2
    exit 1
fi
echo "Smoke test passed"
