#!/usr/bin/env bash
# One-time Google Cloud setup for rustviz-events. NOT RUN YET: for the
# maintainer to review and run by hand.
#
#   playground/events/infra/setup.sh            # print every command (dry run, the default)
#   playground/events/infra/setup.sh --apply    # run them
#   playground/events/infra/setup.sh --apply views   # only (re)create the SQL views
#
# What it creates (project $RV_GCP_PROJECT, default rustviz-playground):
#   1. Service account rv-events@ (the Cloud Run service's identity) with
#      only: roles/logging.logWriter (project), roles/bigquery.jobUser
#      (project; to run dashboard queries), roles/bigquery.dataViewer on
#      dataset rv_research only (NOT rv_programs, which holds code text),
#      and secretAccessor on the dashboard-token secret only.
#   2. BigQuery datasets rv_research (events) and rv_programs (code text).
#   3. Two Cloud Logging sinks from the collector's stdout, split on the
#      rv_stream label, into those datasets (partitioned tables), and a
#      third for the compile service's server-side run results
#      (rustviz-api, rv_stream=rv_run_results; only logged when that service
#      runs with RV_LOG_RUN_RESULTS=1) into rv_research.
#   4. Exclusions on the _Default log bucket so that Cloud Run request logs
#      (which contain client IPs) of BOTH the collector and the compile API
#      ($API_SERVICE) are never stored, and the collector's event/program
#      lines aren't duplicated into Cloud Logging. The compile API's run_result
#      lines carry page_load_id, so its request logs would let pseudonymous
#      events be linked to IP addresses. The BigQuery sinks still receive
#      their lines: exclusions only apply to the bucket they're set on.
#      Operational cost for the compile API: no per-request log entries
#      (URL, status, latency, user agent) in Logs Explorer. Still available:
#      Cloud Run metrics (request counts by status class, latency
#      percentiles, instances), the service's own stderr lines, and, with
#      RV_LOG_RUN_RESULTS=1, one rv_run_result line per compile (outcome,
#      error kind, latency).
#   5. Secret rv-events-dashboard-token (random, 32 bytes base64).
#   6. The SQL views (infra/bigquery/views.sql). BigQuery only creates the
#      sink tables when the first matching line arrives, so run
#      `setup.sh --apply views` again after the first real events.
#
# Retention: no table expiration is set (raw events kept until decided;
# proposal section 6). To add one later:
#   bq update --time_partitioning_expiration <seconds> <project>:rv_research.run_googleapis_com_stdout
set -euo pipefail

PROJECT="${RV_GCP_PROJECT:-rustviz-playground}"
LOCATION="${RV_BQ_LOCATION:-US}"
SERVICE="${RV_EVENTS_SERVICE:-rustviz-events}"
# The compile API (playground/cloudrun/deploy.sh's default service name).
API_SERVICE="${RV_API_SERVICE:-rustviz-api}"
SA="rv-events@${PROJECT}.iam.gserviceaccount.com"
SECRET="rv-events-dashboard-token"
HERE="$(cd "$(dirname "$0")" && pwd)"

APPLY=0
ONLY=""
for arg in "$@"; do
    case "$arg" in
        --apply) APPLY=1 ;;
        views) ONLY=views ;;
        *) echo "usage: $0 [--apply] [views]" >&2; exit 2 ;;
    esac
done

run() {
    printf '+ %s\n' "$*"
    if [ "$APPLY" = 1 ]; then "$@"; fi
}

SERVICE_LOGS="resource.type=\"cloud_run_revision\" AND resource.labels.service_name=\"${SERVICE}\""

views() {
    echo "# SQL views (needs the sink tables to exist: run after the first events arrive)"
    printf '+ sed s/@PROJECT@/%s/g %s | bq query --project_id=%s --use_legacy_sql=false\n' \
        "$PROJECT" "$HERE/bigquery/views.sql" "$PROJECT"
    if [ "$APPLY" = 1 ]; then
        sed "s/@PROJECT@/${PROJECT}/g" "$HERE/bigquery/views.sql" |
            bq query --project_id="$PROJECT" --use_legacy_sql=false
    fi
    printf '+ sed s/@PROJECT@/%s/g %s | bq query --project_id=%s --use_legacy_sql=false\n' \
        "$PROJECT" "$HERE/bigquery/server_run_results.sql" "$PROJECT"
    if [ "$APPLY" = 1 ]; then
        sed "s/@PROJECT@/${PROJECT}/g" "$HERE/bigquery/server_run_results.sql" |
            bq query --project_id="$PROJECT" --use_legacy_sql=false ||
            echo "# server_run_results skipped: no rv_run_results line logged yet"
    fi
}

if [ "$ONLY" = views ]; then
    views
    exit 0
fi

[ "$APPLY" = 1 ] || echo "# DRY RUN: nothing is changed. Re-run with --apply to execute."

echo "# 1. Service account"
run gcloud iam service-accounts create rv-events --project "$PROJECT" \
    --display-name "rustviz-events collector (Cloud Run)"
run gcloud projects add-iam-policy-binding "$PROJECT" \
    --member "serviceAccount:${SA}" --role roles/logging.logWriter --condition None
run gcloud projects add-iam-policy-binding "$PROJECT" \
    --member "serviceAccount:${SA}" --role roles/bigquery.jobUser --condition None

echo "# 2. Datasets"
run bq --location "$LOCATION" mk --dataset \
    --description "RustViz telemetry events (rustviz-events log sink). Research data: named researchers only." \
    "${PROJECT}:rv_research"
run bq --location "$LOCATION" mk --dataset \
    --description "RustViz playground programs (code text), one row per distinct program. Restricted." \
    "${PROJECT}:rv_programs"
# Dataset-level grant (needs a bq CLI with dataset IAM support, 2024+;
# otherwise add the same role on the dataset's Sharing page in the console).
run bq add-iam-policy-binding --member "serviceAccount:${SA}" \
    --role roles/bigquery.dataViewer "${PROJECT}:rv_research"

echo "# 3. Log sinks -> BigQuery (partitioned by day)"
run gcloud logging sinks create rv-events-to-bq \
    "bigquery.googleapis.com/projects/${PROJECT}/datasets/rv_research" \
    --project "$PROJECT" --use-partitioned-tables \
    --log-filter "${SERVICE_LOGS} AND labels.rv_stream=\"rv_events\""
run gcloud logging sinks create rv-programs-to-bq \
    "bigquery.googleapis.com/projects/${PROJECT}/datasets/rv_programs" \
    --project "$PROJECT" --use-partitioned-tables \
    --log-filter "${SERVICE_LOGS} AND labels.rv_stream=\"rv_programs\""
run gcloud logging sinks create rv-run-results-to-bq \
    "bigquery.googleapis.com/projects/${PROJECT}/datasets/rv_research" \
    --project "$PROJECT" --use-partitioned-tables \
    --log-filter "resource.type=\"cloud_run_revision\" AND resource.labels.service_name=\"${API_SERVICE}\" AND log_id(\"run.googleapis.com/stdout\") AND labels.rv_stream=\"rv_run_results\""
# Each sink writes as its own service identity; let it write its dataset.
for pair in rv-events-to-bq:rv_research rv-programs-to-bq:rv_programs rv-run-results-to-bq:rv_research; do
    sink="${pair%%:*}" ds="${pair##*:}"
    if [ "$APPLY" = 1 ]; then
        writer=$(gcloud logging sinks describe "$sink" --project "$PROJECT" --format 'value(writerIdentity)')
    else
        writer="<writerIdentity of ${sink}>"
    fi
    run bq add-iam-policy-binding --member "$writer" --role roles/bigquery.dataEditor "${PROJECT}:${ds}"
done

echo "# 4. Keep IPs (request logs) and event payloads out of Cloud Logging storage"
run gcloud logging sinks update _Default --project "$PROJECT" \
    --add-exclusion "name=rv-events-request-logs,description=rustviz-events request logs contain client IPs; never stored,filter=${SERVICE_LOGS} AND log_id(\"run.googleapis.com/requests\")"
run gcloud logging sinks update _Default --project "$PROJECT" \
    --add-exclusion "name=rv-api-request-logs,description=compile API request logs contain client IPs and would make run_result lines linkable to them; never stored,filter=resource.type=\"cloud_run_revision\" AND resource.labels.service_name=\"${API_SERVICE}\" AND log_id(\"run.googleapis.com/requests\")"
run gcloud logging sinks update _Default --project "$PROJECT" \
    --add-exclusion "name=rv-events-payloads,description=telemetry lines live only in BigQuery,filter=${SERVICE_LOGS} AND labels.rv_stream:*"

echo "# 5. Dashboard token"
run gcloud secrets create "$SECRET" --project "$PROJECT" --replication-policy automatic
printf '+ openssl rand -base64 32 | gcloud secrets versions add %s --data-file=-\n' "$SECRET"
if [ "$APPLY" = 1 ]; then
    openssl rand -base64 32 | tr -d '\n' |
        gcloud secrets versions add "$SECRET" --project "$PROJECT" --data-file=-
fi
run gcloud secrets add-iam-policy-binding "$SECRET" --project "$PROJECT" \
    --member "serviceAccount:${SA}" --role roles/secretmanager.secretAccessor

echo "# 6. Views"
views || echo "# (views failed: expected until the first events have been logged; re-run '$0 --apply views' later)"

echo "# Done. Read the token with: gcloud secrets versions access latest --secret $SECRET --project $PROJECT"
