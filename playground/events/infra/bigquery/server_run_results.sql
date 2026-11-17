-- Server-side run results logged by the compile service (rustviz-api) with
-- RV_LOG_RUN_RESULTS=1: label rv_stream = 'rv_run_results', fields under
-- jsonPayload.rv (see SCHEMA.md section 4). They share the rv_research
-- stdout table with the collector's lines (different service, different
-- label). Kept apart from views.sql because BigQuery only has the
-- jsonPayload.rv.* columns once the first such line has been logged.
--
-- NOT APPLIED. infra/setup.sh --apply views runs it (substituting @PROJECT@).
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.server_run_results` AS
SELECT
  timestamp AS server_time,
  DATE(timestamp) AS day,
  jsonPayload.rv.page_load_id AS page_load_id,
  REGEXP_EXTRACT(jsonPayload.rv.code_hash, r'^sha256:([0-9a-f]{64})$') AS program_id,
  jsonPayload.rv.outcome AS outcome,
  jsonPayload.rv.error_kind AS error_kind,
  jsonPayload.rv.error_codes AS error_codes,
  jsonPayload.rv.crash_at AS crash_at,
  SAFE_CAST(jsonPayload.rv.latency_ms AS INT64) AS latency_ms,
  SAFE_CAST(jsonPayload.rv.code_bytes AS INT64) AS code_bytes,
  SAFE_CAST(jsonPayload.rv.line_count AS INT64) AS line_count
FROM `@PROJECT@.rv_research.run_googleapis_com_stdout`
WHERE labels.rv_stream = 'rv_run_results'
  AND resource.labels.service_name = 'rustviz-api';
