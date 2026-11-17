#!/usr/bin/env bash
# Deploy an already-pushed rustviz-events image to Cloud Run.
# NOT RUN YET: for the maintainer, after infra/setup.sh --apply.
#
#   playground/events/deploy.sh IMAGE
#
# Build and push first (repo root as context):
#   docker build -f playground/events/Dockerfile \
#       -t us-central1-docker.pkg.dev/rustviz-playground/rustviz/events:$(git rev-parse --short HEAD) .
#   docker push us-central1-docker.pkg.dev/rustviz-playground/rustviz/events:<tag>
#
# There is no CI deploy job for this service on purpose: deploy by hand
# until telemetry is switched on.
#
# Cost: request-billed, min-instances 0, so idle costs nothing.
# max-instances 2 x concurrency 80 is the ceiling; a batch takes well under
# a millisecond of CPU. The service has its own identity (rv-events@) that
# can write logs, run BigQuery queries and read the rv_research views, and
# nothing else (see infra/setup.sh).
#
# Privacy: the service never reads or logs the client address except as
# a hashed in-memory rate-limit key. Cloud Run's own request log for this
# service *does* contain the address; infra/setup.sh excludes it from
# storage. Check that exclusion exists before the first deploy:
#   gcloud logging sinks describe _Default --project rustviz-playground | grep rv-events-request-logs
set -euo pipefail

IMAGE="${1:?usage: deploy.sh IMAGE}"
SERVICE="${RV_EVENTS_SERVICE:-rustviz-events}"
PROJECT="${RV_GCP_PROJECT:-rustviz-playground}"
REGION="${RV_GCP_REGION:-us-central1}"
ORIGINS="${RV_EVENTS_ALLOWED_ORIGINS:-https://rustviz.github.io}"

if ! gcloud logging sinks describe _Default --project "$PROJECT" --format json |
        grep -q rv-events-request-logs; then
    echo "Refusing to deploy: the _Default bucket has no rv-events-request-logs exclusion," >&2
    echo "so Cloud Run request logs (with client IPs) would be stored. Run infra/setup.sh --apply." >&2
    exit 1
fi

# `^@^` switches gcloud's env-var delimiter to `@` so a comma-separated
# origin list survives.
gcloud run deploy "$SERVICE" \
    --project "$PROJECT" \
    --region "$REGION" \
    --image "$IMAGE" \
    --min-instances 0 \
    --max-instances 2 \
    --concurrency 80 \
    --cpu 1 \
    --memory 256Mi \
    --timeout 30 \
    --allow-unauthenticated \
    --service-account "rv-events@${PROJECT}.iam.gserviceaccount.com" \
    --set-env-vars "^@^RV_EVENTS_SINK=stdout@RV_EVENTS_ALLOWED_ORIGINS=${ORIGINS}@RV_EVENTS_ACCEPT_PROGRAMS=1@RV_EVENTS_CLIENT_IP_HEADER=X-Forwarded-For@RV_EVENTS_BQ_PROJECT=${PROJECT}@RV_EVENTS_BQ_DATASET=rv_research" \
    --set-secrets "RV_EVENTS_DASHBOARD_TOKEN=rv-events-dashboard-token:latest" \
    --quiet

URL=$(gcloud run services describe "$SERVICE" --project "$PROJECT" --region "$REGION" --format='value(status.url)')
echo "Deployed ${IMAGE} to ${URL}"

# Smoke test without writing an event: health, CORS preflight, an empty
# batch (accepted: 0), and a foreign origin (403).
fail() { echo "Smoke test failed: $*" >&2; exit 1; }
[ "$(curl -s "${URL}/healthz")" = ok ] || fail "/healthz"
code=$(curl -s -o /dev/null -w '%{http_code}' -X OPTIONS "${URL}/v1/events" \
    -H 'Origin: https://rustviz.github.io' -H 'Access-Control-Request-Method: POST')
[ "$code" = 204 ] || fail "preflight returned $code"
body=$(curl -s "${URL}/v1/events" -H 'Origin: https://rustviz.github.io' \
    -H 'Content-Type: text/plain' --data '{"events":[]}')
[ "$body" = '{"accepted":0,"rejected":[]}' ] || fail "empty batch returned $body"
code=$(curl -s -o /dev/null -w '%{http_code}' "${URL}/v1/events" \
    -H 'Origin: https://evil.example' --data '{"events":[]}')
[ "$code" = 403 ] || fail "foreign origin returned $code"
echo "Smoke test passed. Dashboard: ${URL}/dashboard"
