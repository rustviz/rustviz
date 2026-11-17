# rustviz-events: telemetry collector and usage dashboard

**Status: not deployed.** Nothing here runs anywhere yet, and no page sends
events to it. Telemetry stays off everywhere unless an endpoint is
configured (see the telemetry proposal, plan item 3).

A small actix-web service (same stack as the playground's compile API):

- `POST /v1/events` accepts batches of typed events from `rv-telemetry.js`
  (tutorial, playground, mdBook figures), validates each one against the
  event contract in [`SCHEMA.md`](SCHEMA.md), adds `server_ts`, and writes
  one JSON line per event to a sink.
- `GET /dashboard` is a small page of usage statistics (aggregates only),
  backed by `GET /dashboard/data` (JSON, bearer token).

## Architecture

```
browser (rv-telemetry.js) ── POST /v1/events {"events":[…]} ──▶ rustviz-events (Cloud Run, max 2 instances)
                                                                   │ validate, drop unknown fields, add server_ts
                                                                   ▼
                                       stdout: one structured log line per event (label rv_stream)
                                                                   │ Cloud Logging sinks (infra/setup.sh)
                         ┌─────────────────────────────────────────┴──────────────────────┐
                         ▼                                                                ▼
     BigQuery rv_research.run_googleapis_com_stdout               BigQuery rv_programs.run_googleapis_com_stdout
     (events, partitioned by day) + views:                         (program source text, one row per program)
     events, sessions, daily_usage, chapter_usage,                 + view rv_programs.programs
     playground_runs, figure_engagement
                         ▲
                         └── /dashboard/data queries the views (service account: read rv_research only)
```

- **Sinks** (`RV_EVENTS_SINK`): `stdout` writes Cloud Logging structured
  JSON; each line carries the whole record as one string (`rv_json`) so the
  sink's BigQuery schema can't drift, and the views parse it with
  `JSON_VALUE`. `file` appends plain JSONL to `RV_EVENTS_FILE` (events) and
  `RV_EVENTS_PROGRAMS_FILE` (programs) for local dev and tests.
- **Programs** (code text) are a separate stream with no visitor ids. Each
  distinct program (`program_id` = SHA-256 of its source, checked by the
  collector) is written once per instance and kept once by the
  `rv_programs.programs` view. Events refer to it by `program_id`.
- **Dashboard sources**: BigQuery when `RV_EVENTS_BQ_PROJECT` is set (REST
  `jobs.query` with the service account's metadata-server token; results
  cached 5 minutes; each query capped at 1 GB billed), otherwise the local
  JSONL file aggregated in memory.

## Privacy guarantees (what the code enforces)

- **No IP addresses, anywhere.** The service never copies a request header
  into a record: records are built field by field from the event body
  (`src/schema.rs`). There is no request logger (actix's `Logger` would print
  the peer address). The client address is used only as an in-memory
  rate-limit key, and even then only as a 64-bit hash under a random
  per-process key (`src/ratelimit.rs`); it is never logged or written.
  Cloud Run's *own* request log does contain the address: `infra/setup.sh`
  adds an exclusion so it is never stored, and `deploy.sh` refuses to deploy
  if that exclusion is missing. Tests send `X-Forwarded-For`, `Forwarded`,
  `X-Real-IP`, `User-Agent`, `Referer` and `Cookie` and check none of them
  reaches the output.
- **The compile API's request logs are excluded too.** With
  `RV_LOG_RUN_RESULTS=1` the compile API (`rustviz-api`) logs one
  `rv_run_result` line per compile carrying the visitor's `page_load_id`;
  its Cloud Run request logs carry the client IP, so keeping both would make
  the pseudonymous events linkable to IPs. `infra/setup.sh` therefore also
  excludes `rustviz-api`'s `run.googleapis.com/requests` logs from storage
  and routes its `rv_run_results` lines to `rv_research`
  (view `server_run_results`).
  *Operational visibility lost:* no per-request entries (URL, HTTP status,
  latency, user agent, client IP) for the compile API in Logs Explorer, so
  no "which client is hammering us" by IP. *Still there:* Cloud Run's
  built-in metrics (request count by response class, latency percentiles,
  instance count, CPU/memory), the service's own stderr lines (plugin
  crashes, runner failures), and per-compile outcome/error kind/latency in
  the `rv_run_result` lines. The in-memory per-IP rate limit is unaffected.
- **Only the schema's fields are kept.** Unknown fields are dropped (and
  counted in `dropped_fields`); malformed events are rejected; sizes are
  capped (64 KiB program source, 24 KiB other events, 100 events and
  256 KiB per request; a maximal program always fits in one request).
- **Code text is opt-in on the server** (`RV_EVENTS_ACCEPT_PROGRAMS=1`,
  default off; `deploy.sh` turns it on per the maintainer's decision) and
  goes to its own dataset that the dashboard cannot read.
- **CORS allow-list**, exact origins only; no cookies, no credentials.
  Requests without an `Origin` header are refused unless
  `RV_EVENTS_REQUIRE_ORIGIN=0`.
- **Dashboard shows aggregates only**: counts, medians, page paths, figure
  ids (hashes) and rustc error codes. No `anon_id`, `session_id`,
  `page_load_id`, event id or code appears in `/dashboard/data` (tested).
  The data endpoint is disabled unless `RV_EVENTS_DASHBOARD_TOKEN` (≥ 16
  chars) is set; the page asks for the token once and keeps it in the
  browser's localStorage.

What this service does *not* decide: the consent model, Do Not
Track / GPC, and which ids exist are the client's job (`rv-telemetry.js`);
see `SCHEMA.md` "Consent rules". Google's front end necessarily sees client
addresses; the privacy page should say so.

## Local development

```sh
RV_EVENTS_SINK=file RV_EVENTS_DASHBOARD_TOKEN=dev-token-0123456789 \
  cargo run -p rustviz-events
# listening on http://127.0.0.1:8090; events go to ./rv-events.jsonl
```

Post the samples from `SCHEMA.md` (extract one of its JSON blocks into
`batch.json`):

```sh
curl -s localhost:8090/v1/events -H 'Origin: http://localhost:3000' \
  -H 'Content-Type: text/plain;charset=UTF-8' --data-binary @batch.json
# {"accepted":5,"rejected":[]}
```

Then open <http://127.0.0.1:8090/dashboard> and enter the token.
Set `RV_EVENTS_ACCEPT_PROGRAMS=1` to accept `program` events locally.

Tests: `cargo test -p rustviz-events` (schema validation, size caps, CORS,
no IP/header leakage, programs, rate limit, token check, file-mode
aggregation, BigQuery response parsing).

### Configuration

| variable | default | meaning |
|---|---|---|
| `RV_EVENTS_SINK` | `stdout` | `stdout` (Cloud Logging) or `file` |
| `RV_EVENTS_FILE` | `rv-events.jsonl` | file sink: events |
| `RV_EVENTS_PROGRAMS_FILE` | `rv-programs.jsonl` | file sink: programs |
| `RV_EVENTS_ALLOWED_ORIGINS` | `https://rustviz.github.io` + localhost/127.0.0.1 on ports 3000, 3001 | comma-separated exact origins |
| `RV_EVENTS_REQUIRE_ORIGIN` | `1` | `0` accepts requests with no `Origin` (dev only) |
| `RV_EVENTS_ACCEPT_PROGRAMS` | `0` | `1` accepts `program` events (code text) |
| `RV_EVENTS_CLIENT_IP_HEADER` | unset (peer address) | proxy header for the rate-limit key; Cloud Run: `X-Forwarded-For` (rightmost entry) |
| `RV_EVENTS_RATE_BURST` / `RV_EVENTS_RATE_PER_SECOND` | `60` / `1` | per-client token bucket (requests, not events) |
| `RV_EVENTS_DASHBOARD_TOKEN` | unset (dashboard data off) | bearer token, ≥ 16 chars |
| `RV_EVENTS_DASHBOARD_SOURCE` | `bigquery` if a project is set, else `file` with the file sink, else none | `file`, `bigquery` or `none` |
| `RV_EVENTS_BQ_PROJECT` / `RV_EVENTS_BQ_DATASET` | unset / `rv_research` | BigQuery views location |
| `RV_EVENTS_BQ_TOKEN` | unset (metadata server) | access token for running the dashboard locally against BigQuery, e.g. `$(gcloud auth print-access-token)` |
| `PORT` / `RV_BIND` | `127.0.0.1:8090` | Cloud Run sets `PORT` (binds `0.0.0.0:$PORT`) |

## Deploying (maintainer, after review)

Nothing below has been run. In order:

1. Review `infra/setup.sh` and run it with no arguments (a dry run that
   prints every command), then with `--apply`. It creates the `rv-events`
   service account and its narrow grants, datasets `rv_research` and
   `rv_programs`, log sinks into them (collector events, programs, and the
   compile service's server-side run results), the `_Default`-bucket exclusions
   (request logs with IPs; telemetry lines), and the dashboard-token secret.
2. Build and push the image (repo root as context):
   ```sh
   TAG=us-central1-docker.pkg.dev/rustviz-playground/rustviz/events:$(git rev-parse --short HEAD)
   docker build -f playground/events/Dockerfile -t "$TAG" . && docker push "$TAG"
   ```
3. `playground/events/deploy.sh "$TAG"`: Cloud Run, min 0 / max 2
   instances, its own service account, the token from Secret Manager. It
   refuses to deploy without the request-log exclusion and finishes with a
   smoke test that writes no event.
4. Once the first real events have arrived (the sink tables only exist
   then): `playground/events/infra/setup.sh --apply views`.
5. Point the clients at the service URL (`rv-telemetry.js` config,
   mdbook-rustviz `telemetry-endpoint`), which is a separate change.

There is deliberately no CI deploy job for this service; deploy by hand
until telemetry is turned on. To turn it off: `gcloud run services delete
rustviz-events` (clients drop events silently when the endpoint fails).

Retention, access to `rv_research` / `rv_programs` (named researchers), and
deletion requests (a visitor e-mails the ID shown in the privacy page's Your data section; `anon_id`) are
still open decisions; `infra/setup.sh` sets no expiration.
