-- BigQuery views over the rustviz-events log sink tables.
--
-- NOT APPLIED. playground/events/infra/setup.sh runs this after the
-- maintainer has reviewed it (it substitutes @PROJECT@).
--
-- Source tables (created by the Cloud Logging -> BigQuery sinks the first
-- time a matching line is logged; partitioned by day on `timestamp`):
--   @PROJECT@.rv_research.run_googleapis_com_stdout   event lines  (labels.rv_stream = 'rv_events')
--   @PROJECT@.rv_programs.run_googleapis_com_stdout   program lines (labels.rv_stream = 'rv_programs')
-- Each line carries the whole canonical record as one JSON string,
-- jsonPayload.rv_json (see playground/events/SCHEMA.md section 4), so these
-- views read every field with JSON_VALUE and never depend on the sink's
-- inferred column types.
--
-- `timestamp` (Cloud Logging's receive time, ~ the collector's server_ts) is
-- the time axis; `day` is its UTC date.

-- One row per event, deduplicated by event_id (client retries).
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.events` AS
SELECT * EXCEPT (rn)
FROM (
  SELECT
    JSON_VALUE(jsonPayload.rv_json, '$.event_id') AS event_id,
    JSON_VALUE(jsonPayload.rv_json, '$.event') AS event,
    SAFE_CAST(JSON_VALUE(jsonPayload.rv_json, '$.seq') AS INT64) AS seq,
    TIMESTAMP_MILLIS(SAFE_CAST(JSON_VALUE(jsonPayload.rv_json, '$.client_ts') AS INT64)) AS client_time,
    timestamp AS server_time,
    DATE(timestamp) AS day,
    JSON_VALUE(jsonPayload.rv_json, '$.surface') AS surface,
    JSON_VALUE(jsonPayload.rv_json, '$.page') AS page,
    JSON_VALUE(jsonPayload.rv_json, '$.page_load_id') AS page_load_id,
    JSON_VALUE(jsonPayload.rv_json, '$.session_id') AS session_id,
    JSON_VALUE(jsonPayload.rv_json, '$.anon_id') AS anon_id,
    JSON_VALUE(jsonPayload.rv_json, '$.consent_mode') AS consent_mode,
    SAFE_CAST(JSON_VALUE(jsonPayload.rv_json, '$.tier') AS INT64) AS tier,
    JSON_VALUE(jsonPayload.rv_json, '$.rv_version') AS rv_version,
    JSON_VALUE(jsonPayload.rv_json, '$.client_version') AS client_version,
    JSON_VALUE(jsonPayload.rv_json, '$.viewport') AS viewport,
    JSON_VALUE(jsonPayload.rv_json, '$.lang') AS lang,
    SAFE_CAST(JSON_VALUE(jsonPayload.rv_json, '$.tz_offset') AS INT64) AS tz_offset,
    JSON_VALUE(jsonPayload.rv_json, '$.referrer_host') AS referrer_host,
    JSON_VALUE(jsonPayload.rv_json, '$.cohort') AS cohort,
    JSON_QUERY(jsonPayload.rv_json, '$.props') AS props,
    ROW_NUMBER() OVER (
      PARTITION BY JSON_VALUE(jsonPayload.rv_json, '$.event_id') ORDER BY timestamp
    ) AS rn
  FROM `@PROJECT@.rv_research.run_googleapis_com_stdout`
  WHERE labels.rv_stream = 'rv_events'
)
WHERE rn = 1;

-- One row per session (session_id rotates after 30 min idle).
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.sessions` AS
SELECT
  session_id,
  ANY_VALUE(anon_id) AS anon_id,
  MIN(server_time) AS start_time,
  MAX(server_time) AS end_time,
  DATE(MIN(server_time)) AS day,
  COUNT(DISTINCT page_load_id) AS page_loads,
  COUNTIF(event = 'page_view') AS page_views,
  COUNT(*) AS events,
  STRING_AGG(DISTINCT surface ORDER BY surface) AS surfaces
FROM `@PROJECT@.rv_research.events`
WHERE session_id IS NOT NULL
GROUP BY session_id;

-- One row per UTC day.
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.daily_usage` AS
SELECT
  day,
  COUNT(DISTINCT anon_id) AS visitors,
  COUNT(DISTINCT session_id) AS sessions,
  COUNT(DISTINCT page_load_id) AS page_loads,
  COUNTIF(event = 'page_view') AS page_views,
  COUNT(DISTINCT IF(event = 'run_result', JSON_VALUE(props, '$.run_id'), NULL)) AS runs,
  COUNT(DISTINCT IF(event = 'withdraw', anon_id, NULL)) AS turned_off
FROM `@PROJECT@.rv_research.events`
GROUP BY day;

-- One row per tutorial page load: the page, how long it was actively read
-- (largest cumulative active_ms among its page_hide events) and how far it
-- was scrolled.
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.chapter_usage` AS
SELECT
  page,
  page_load_id,
  MIN(day) AS day,
  ANY_VALUE(anon_id) AS anon_id,
  ANY_VALUE(session_id) AS session_id,
  COUNTIF(event = 'page_view') AS page_views,
  MAX(IF(event = 'page_hide', SAFE_CAST(JSON_VALUE(props, '$.active_ms') AS INT64), NULL)) AS active_ms,
  MAX(IF(event = 'page_hide', SAFE_CAST(JSON_VALUE(props, '$.total_ms') AS INT64), NULL)) AS total_ms,
  MAX(IF(event = 'page_hide', SAFE_CAST(JSON_VALUE(props, '$.max_scroll_pct') AS INT64), NULL)) AS max_scroll_pct
FROM `@PROJECT@.rv_research.events`
WHERE surface = 'tutorial' AND event IN ('page_view', 'page_hide')
GROUP BY page, page_load_id;

-- One row per playground run: the client's `run` joined to its
-- `run_result` (outcome 'no_result' when the result never arrived).
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.playground_runs` AS
WITH r AS (
  SELECT
    JSON_VALUE(props, '$.run_id') AS run_id,
    day, server_time, anon_id, session_id, page_load_id,
    JSON_VALUE(props, '$.program_id') AS program_id,
    JSON_VALUE(props, '$.figure_id') AS figure_id,
    SAFE_CAST(JSON_VALUE(props, '$.line_count') AS INT64) AS line_count,
    JSON_VALUE(props, '$.example') AS example,
    JSON_VALUE(props, '$.source_figure_id') AS source_figure_id
  FROM `@PROJECT@.rv_research.events`
  WHERE event = 'run'
),
res AS (
  SELECT * EXCEPT (rn) FROM (
    SELECT
      JSON_VALUE(props, '$.run_id') AS run_id,
      day, server_time, anon_id, session_id, page_load_id,
      JSON_VALUE(props, '$.program_id') AS program_id,
      JSON_VALUE(props, '$.outcome') AS outcome,
      JSON_VALUE_ARRAY(props, '$.error_codes') AS error_codes,
      JSON_VALUE(props, '$.error_kind') AS error_kind,
      SAFE_CAST(JSON_VALUE(props, '$.latency_ms') AS INT64) AS latency_ms,
      SAFE_CAST(JSON_VALUE(props, '$.cached') AS BOOL) AS cached,
      SAFE_CAST(JSON_VALUE(props, '$.http_status') AS INT64) AS http_status,
      ROW_NUMBER() OVER (PARTITION BY JSON_VALUE(props, '$.run_id') ORDER BY server_time) AS rn
    FROM `@PROJECT@.rv_research.events`
    WHERE event = 'run_result'
  )
  WHERE rn = 1
)
SELECT
  run_id,
  COALESCE(r.day, res.day) AS day,
  COALESCE(r.server_time, res.server_time) AS server_time,
  COALESCE(r.anon_id, res.anon_id) AS anon_id,
  COALESCE(r.session_id, res.session_id) AS session_id,
  COALESCE(r.page_load_id, res.page_load_id) AS page_load_id,
  COALESCE(r.program_id, res.program_id) AS program_id,
  r.figure_id, r.line_count, r.example, r.source_figure_id,
  IFNULL(res.outcome, 'no_result') AS outcome,
  IFNULL(res.error_codes, []) AS error_codes,
  res.error_kind, res.latency_ms, res.cached, res.http_status
FROM r FULL OUTER JOIN res USING (run_id);

-- One row per (figure, page load): how long it was on screen, how often and
-- how long its elements were hovered.
CREATE OR REPLACE VIEW `@PROJECT@.rv_research.figure_engagement` AS
SELECT
  JSON_VALUE(props, '$.figure_id') AS figure_id,
  page_load_id,
  MIN(day) AS day,
  ANY_VALUE(page) AS page,
  ANY_VALUE(surface) AS surface,
  ANY_VALUE(rv_version) AS rv_version,
  ANY_VALUE(anon_id) AS anon_id,
  COUNTIF(event = 'figure_view') AS view_intervals,
  SUM(IF(event = 'figure_view', SAFE_CAST(JSON_VALUE(props, '$.visible_ms') AS INT64), 0)) AS visible_ms,
  COUNTIF(event = 'tooltip_dwell') AS hovers,
  SUM(IF(event = 'tooltip_dwell', SAFE_CAST(JSON_VALUE(props, '$.dwell_ms') AS INT64), 0)) AS hover_ms,
  COUNT(DISTINCT IF(event = 'tooltip_dwell', JSON_VALUE(props, '$.element_id'), NULL)) AS distinct_elements,
  COUNTIF(event = 'code_line_hover') AS code_line_hovers,
  COUNTIF(event = 'pointer_trace') AS pointer_chunks
FROM `@PROJECT@.rv_research.events`
WHERE event IN ('figure_view', 'tooltip_dwell', 'code_line_hover', 'pointer_trace')
GROUP BY figure_id, page_load_id;

-- Programs (code text), one row per program_id, first sighting kept. Lives
-- in its own dataset so access to code text can be granted separately; the
-- dashboard's service account has no access to it.
CREATE OR REPLACE VIEW `@PROJECT@.rv_programs.programs` AS
SELECT
  program_id,
  first.first_seen,
  first.source,
  first.rv_version,
  first.source_figure_id,
  first.surface
FROM (
  SELECT
    JSON_VALUE(jsonPayload.rv_json, '$.program_id') AS program_id,
    ARRAY_AGG(STRUCT(
      timestamp AS first_seen,
      JSON_VALUE(jsonPayload.rv_json, '$.source') AS source,
      JSON_VALUE(jsonPayload.rv_json, '$.rv_version') AS rv_version,
      JSON_VALUE(jsonPayload.rv_json, '$.source_figure_id') AS source_figure_id,
      JSON_VALUE(jsonPayload.rv_json, '$.surface') AS surface
    ) ORDER BY timestamp LIMIT 1)[OFFSET(0)] AS first
  FROM `@PROJECT@.rv_programs.run_googleapis_com_stdout`
  WHERE labels.rv_stream = 'rv_programs'
  GROUP BY program_id
);
