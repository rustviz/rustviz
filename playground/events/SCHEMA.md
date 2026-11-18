# RustViz telemetry event contract, schema 1

The collector (`playground/events/`, crate `rustviz-events`) enforces exactly
what is written here, and the browser client (`rv-telemetry.js`) emits it.
The source is the telemetry proposal (sections 5 and 5a) plus the
maintainer's decisions: public sites run in consent mode `notice`,
pseudonymous ids are recorded, "turn off" sends `withdraw`, and playground
code text is collected once per distinct program as `program` records.
Element ids (`data-rv-id` and friends) are specified by the plugin in
`rustviz-plugin/docs/element-ids.md`. Adding an optional field is
backwards-compatible; anything else bumps `schema`.

Nothing here is live. Telemetry is off unless an endpoint is configured.

## 1. Transport

```
POST {endpoint}/v1/events
Content-Type: application/json        (fetch with keepalive)
Content-Type: text/plain;charset=UTF-8 (navigator.sendBeacon with a string body;
                                        avoids a CORS preflight)
Origin: https://rustviz.github.io      (must be on the collector's allow-list)

{ "events": [ <event>, <event>, ... ] }
```

- The body is parsed as JSON whatever the `Content-Type` (`application/json`,
  `text/plain`, or none). Any other top-level shape (a bare array, extra keys
  beside `events`) is a 400.
- **Origin:** a request whose `Origin` is not on the allow-list gets 403 and
  nothing is recorded. A request with **no** `Origin` header is also rejected
  (403) unless the collector runs with `RV_EVENTS_REQUIRE_ORIGIN=0` (dev only).
  Default allow-list: `https://rustviz.github.io`, `http://localhost:3000`,
  `http://127.0.0.1:3000`, `http://localhost:3001`, `http://127.0.0.1:3001`;
  override with `RV_EVENTS_ALLOWED_ORIGINS` (comma-separated, exact origins).
- No cookies, no credentials (`credentials: "omit"`). The collector never sets
  a cookie and never sends `Access-Control-Allow-Credentials`.
- **Limits (whole request rejected, nothing recorded):**
  - body > 256 KiB → 413 (but see the beacon note below)
  - more than 100 events in `events` → 413
  - malformed JSON / wrong top-level shape → 400
  - per-IP rate limit exceeded → 429 (in-memory token bucket; the IP is a
    hash-map key only, never logged or written anywhere)
- **Per-event validation:** each event is checked on its own. Invalid events
  are dropped, valid ones in the same batch are kept. Response is always 200
  with
  ```json
  { "accepted": 3, "rejected": [ { "index": 1, "reason": "unknown_event" } ] }
  ```
  The client should not retry rejected events (they will be rejected again).
  It may retry a whole batch on network error / 5xx / 429 (dedupe is by
  `event_id`, so retries are safe).
- An event serialized larger than 24 KiB is rejected (`too_large`). A
  `program` event's `source` is capped at 64 KiB of UTF-8 (`invalid:source`)
  and the whole serialized event at 152 KiB (2 × 64 KiB for worst-case JSON
  escaping + 24 KiB), so a maximal program always fits in one 256 KiB
  request alongside other events (the collector tests this).
- **Beacon note:** browsers cap `sendBeacon` / `fetch(…, {keepalive: true})`
  at 64 KiB in flight. Send ordinary batches by beacon on page hide, but send
  a `program` record with a plain `fetch` at run time (it is rarely > 16 KiB:
  the compile API itself caps code at 16 KiB).

## 2. Envelope (every event)

| field | type | required | rule |
|---|---|---|---|
| `schema` | integer | yes | must be `1` |
| `event_id` | string | yes | UUID (any version, v4 recommended), lowercase hex with dashes, 36 chars. Dedupe key. |
| `event` | string | yes | one of the names in section 3; anything else → `unknown_event` |
| `seq` | integer | yes | 0 ≤ seq < 2^31. Counter per `page_load_id`, starting at 0, +1 for every event the client *creates* (so gaps mean lost events). Order within a page load is `(page_load_id, seq)`, never arrival order. |
| `client_ts` | integer | yes | ms since Unix epoch (client clock), > 1.6e12 and < 1e13 |
| `surface` | string | yes | `tutorial` \| `playground` \| `book:<host>` (host: 1-64 of `[a-z0-9.-]`) |
| `page` | string | yes | URL path only, starts with `/`, ≤ 256 chars, no `?` or `#` (query and fragment stripped by the client) |
| `page_load_id` | string | yes | UUID, fresh per page load (not stored) |
| `session_id` | string | yes | UUID; rotates after 30 min idle (localStorage) |
| `anon_id` | string | yes | UUID, persistent pseudonymous id (localStorage); cleared on `withdraw` |
| `consent_mode` | string | yes | how this visitor was asked: `notice` (no client UI: the host page's own text is the notice, and events flow from first load; the public tutorial and playground) or `opt_in` (nothing sent before an explicit opt-in; the client's default when no mode is configured) |
| `tier` | integer | yes | `1` (standard: everything below except the tier-2-only fields) or `2` (research participant who explicitly joined a study: allows `cohort`, `survey`, and code text when the server flag is on) |
| `rv_version` | string | yes | RustViz version that rendered the page, ≤ 32 of `[0-9A-Za-z.+-]` |
| `client_version` | string | yes | rv-telemetry.js version, same charset, ≤ 32 |
| `viewport` | string | no | bucketed `WxH`, each rounded to a multiple of 100 px, e.g. `1200x800`; pattern `^[0-9]{1,5}x[0-9]{1,5}$` |
| `lang` | string | no | `navigator.language`, ≤ 35 of `[A-Za-z0-9-]` |
| `tz_offset` | integer | no | minutes, `-840..=840` (`-new Date().getTimezoneOffset()`) |
| `referrer_host` | string | no | host only (no path), ≤ 253 of `[a-z0-9.-]`, omitted when same-site |
| `cohort` | string | no | tier 2 only (the server strips it on tier 1), 1-40 of `[a-z0-9-]` (from `?cohort=`) |
| `props` | object | yes | event properties (section 3); `{}` when none |

Unknown envelope fields and unknown `props` fields are **dropped** (not an
error); the server records how many in `dropped_fields`. A known field with
the wrong type or out of range rejects the event (`invalid:<field>`).

### Server-added fields

| field | meaning |
|---|---|
| `server_ts` | RFC 3339 UTC with ms, time the collector accepted the batch |
| `collector` | `rustviz-events/<crate version>` |
| `dropped_fields` | count of unknown or policy-stripped fields, present only when > 0 |

The collector **never** records the client IP, `X-Forwarded-For`,
`Forwarded`, `User-Agent`, or any other request header. Nothing outside the
event body is written.

### Ids, summarized

| id | scope | stored on device | tier |
|---|---|---|---|
| `event_id` | one event | no | all |
| `page_load_id` | one page load | no | all |
| `session_id` | ~30 min idle window | yes | all |
| `anon_id` | the browser, until withdraw / storage cleared | yes | all |
| `run_id` | one playground run, joins `run` ↔ `run_result` | no | all |

## 3. Events

Common property types:
- `figure_id`: 16-64 lowercase hex. Recommended: first 16 hex chars of
  SHA-256(snippet text + `"\n"` + rv_version). For playground runs, the hash of
  the code that was run (+ rv_version) so an edited example is a new figure.
  Tutorial snippets are never logged: mdbook-rustviz can emit a build-time
  manifest `figure_id → snippet` for re-rendering.
- `program_id`: 64 lowercase hex, SHA-256 of the exact UTF-8 source text (no
  prefix, no rv_version). The text itself only ever travels in a `program`
  event; every other event refers to it by `program_id` (and `figure_id`).
- `element_id`: the stable `data-rv-id` of a timeline element; grammar defined
  by the plugin in `rustviz-plugin/docs/element-ids.md`. The collector only checks
  1-128 characters, no whitespace or control characters.
- `ts`-suffixed fields: client ms epoch, same rule as `client_ts`.
- `*_ms` durations: integers `0..=86_400_000`.
- token: 1-32 of `[a-z0-9_]`.

| event | props (`?` = optional) | notes |
|---|---|---|
| `page_view` | `nav_type?`: `navigate`\|`reload`\|`back_forward`\|`prerender` | first event of a page load after consent |
| `page_hide` | `active_ms`, `total_ms`, `max_scroll_pct` (0-100 int), `reason`: `hidden`\|`unload` | sent on every `visibilitychange→hidden`; `active_ms`/`total_ms` are cumulative since load, so take the max per page load. active = visible and focused with input in the last 30 s |
| `figure_view` | `figure_id`, `figure_index` (0-based position on page, ≤ 999), `enter_ts`, `leave_ts`, `visible_ms` | one per visible interval (≥ 50 % in viewport) |
| `tooltip_dwell` | `figure_id`, `element_id`, `element_kind` (`data-rv-kind`): `header`\|`dot`\|`call`\|`fn`\|`arrow`\|`line`\|`branch_line`\|`stop_bar`\|`bracket`\|`code_line`\|`other`, `event_kind?` token (first token of `data-rv-event`: the plugin's event names, `acquire`, `move`, `copy`, `static_borrow`, `mutable_borrow`, `owner_go_out_of_scope`, …), `var?` (≤ 64 chars, `data-rv-var`), `line?` (1-100000, `data-rv-line`), `line_end?` (1-100000, `data-rv-line-end`), `state?` token (`data-rv-state`), `drop?` bool (`data-rv-drop`), `enter_ts`, `leave_ts`, `dwell_ms` | every hover, no minimum dwell. Element vocabulary: `rustviz-plugin/docs/element-ids.md` |
| `code_line_hover` | `figure_id`, `line`, `enter_ts`, `leave_ts`, `dwell_ms` | |
| `pointer_trace` | `figure_id`, `chunk` (0-based per figure per page load), `t0` (ts of first sample), `input`: `mouse`\|`touch`\|`pen`, `samples`: array of `[dt_ms, x, y]` (≤ 600 per chunk), `taps?`: array of `[dt_ms, x, y]` (≤ 100) | `dt_ms` int 0..=600000 relative to `t0`; `x`,`y` finite numbers in the figure SVG's user coordinates (viewBox units), rounded to 1 decimal, abs ≤ 100000. 10 Hz while the pointer is inside the figure; flush a chunk when it reaches 600 samples or with the batch |
| `ui` | `action`: `theme_change`\|`panel_resize`\|`open_in_playground`\|`example_select`\|`import`\|`export`\|`share`\|`privacy_panel`\|`other`, `value?` (≤ 64 chars), `source_figure_id?` | `value` is a short enum-like string (theme name, example id), never free text |
| `program` | `program_id`, `source` (string, ≤ 64 KiB UTF-8), `source_figure_id?` (figure_id of the example/tutorial figure it was edited from) | the only event that carries code text. Sent only when the client is configured with `collectCode: true` (playground: true; library default: false), the first time this page load sees a `program_id`, before the `run`/`edit` that references it. The collector rejects it unless SHA-256(`source`) == `program_id` (`invalid:program_id`), and unless it runs with `RV_EVENTS_ACCEPT_PROGRAMS=1` (`programs_disabled`; default off, the Cloud Run deploy turns it on). Stored once per `program_id` in its own stream/table (section 4); `first_seen` = earliest `server_ts`. |
| `edit` | `ms_since_last_run?`, `chars_changed` (0-1000000), `program_id?`, `program_omitted?`: `too_large` | an edit snapshot: at most one per 10 s; when `collectCode` is on, a `program` record for the snapshot precedes it (same dedupe) |
| `run` | `run_id` (UUID), `program_id`, `figure_id?` (of the figure this run renders), `line_count` (0-100000), `example?` (≤ 64 chars, dropdown id it started from), `source_figure_id?`, `program_omitted?`: `too_large` | `program_id` is sent even when `collectCode` is off (the hash alone, no text). `program_omitted: "too_large"` (on `run` or `edit`) marks a `program_id` whose source exceeds the 64 KiB `source` cap, so no `program` record was sent (truncating would break the hash) |
| `run_result` | `run_id`, `program_id?`, `figure_id?`, `outcome`: `ok`\|`compile_error`\|`refused`\|`crash`\|`timeout`\|`rate_limited`\|`network_error`\|`server_error`, `error_codes?` (≤ 32 strings matching `^E[0-9]{4}$`), `error_kind?` (value of `X-RustViz-Error-Kind`, ≤ 64 of `[a-z0-9-]`), `crash_at?` (≤ 64 of `[A-Za-z0-9_.:-]`, plugin source location such as `expr_visitor.rs:862`, only with `outcome: "crash"`), `latency_ms`, `cached?` bool, `http_status?` (100-599) | client-side view; the compile service may later write an authoritative server-side copy joined by `page_load_id` |
| `consent` | `consent_version` (≤ 32 of `[0-9A-Za-z.-]`), `choice`: `opt_in` (explicit opt-in, `opt_in` mode or joining a study), `age_18_plus?` bool | see consent rules below |
| `withdraw` | `consent_version` | the visitor turned logging off. Last event this browser sends until it turns logging back on; it carries the ids, so a visitor who e-mails their ID (shown in the privacy page's Your data section) can have their rows found and deleted. The dashboard's "turned logging off" share = distinct `anon_id` with a `withdraw` / distinct `anon_id` |
| `survey` | `survey_version` (≤ 32), `answers`: object of ≤ 20 entries, keys 1-32 of `[a-z0-9_]`, values strings ≤ 200 | tier 2 only (tier 1 → rejected `tier`) |

`figure_view`, `tooltip_dwell`, `code_line_hover` and `pointer_trace` on a
playground figure use that run's `figure_id` and may add `program_id?`.

### Consent rules (enforced by the client)

- `consent_mode` is a client config value. The client's default when nothing
  is configured is `opt_in` (safest): **no event at all** before the visitor
  opts in, then `consent` (`choice: "opt_in"`) followed by `page_view`.
- The public tutorial and playground configure `notice`: the client shows no
  UI. The host page's own text is the notice (the playground info panel's
  Privacy section; a paragraph at the bottom of the tutorial's front page).
  Events flow from first load unless the visitor turned
  logging off in the privacy page's Your data section, which sends `withdraw`. No `consent`
  event is sent in `notice` mode.
- There is no decline event: declining in `opt_in` mode sends nothing.
- `withdraw` stops all logging on this device and clears `anon_id` and
  `session_id`.
- Do Not Track / Global Privacy Control set → send nothing, in every mode,
  **only when the site keeps the client's default** `respectPrivacySignals:
  true`. The RustViz sites (playground, tutorial) set it to false: they never
  sell or share the data, so only Turn off stops logging there.

## 4. What the collector writes

One JSON line per accepted event (the canonical record):

```json
{"schema":1,"event_id":"…","event":"tooltip_dwell","seq":7,"client_ts":1763370000123,
 "surface":"tutorial","page":"/rustviz/tutorial/ch04-01.html","page_load_id":"…",
 "session_id":"…","anon_id":"…","consent_mode":"notice","tier":1,"rv_version":"2.1.0",
 "client_version":"0.1.0","props":{…},
 "server_ts":"2026-11-17T09:20:00.456Z","collector":"rustviz-events/0.1.0"}
```

- `RV_EVENTS_SINK=file` (dev/tests): the record above, appended to
  `RV_EVENTS_FILE` (default `./rv-events.jsonl`).
- `RV_EVENTS_SINK=stdout` (Cloud Run): one Cloud Logging structured-log line
  per record. The whole record is carried as one JSON **string**, `rv_json`,
  so the BigQuery sink's auto-generated schema is a single STRING column that
  can never conflict between event types (and never meets an array of
  arrays, e.g. pointer samples, which BigQuery can't store):
  ```
  {"severity":"INFO","message":"rv_event","event":"tooltip_dwell",
   "logging.googleapis.com/labels":{"rv_stream":"rv_events"},
   "rv_json":"{\"anon_id\":…,\"props\":{…},…}"}
  ```
  Sink filter: `resource.type="cloud_run_revision" AND
  resource.labels.service_name="rustviz-events" AND labels.rv_stream="rv_events"`.
  The SQL views read fields with `JSON_VALUE(jsonPayload.rv_json, '$.anon_id')`.
- `program` events are written to a **separate stream**: label
  `rv_stream: "rv_programs"`, message `rv_program`, and the record is only
  `{program_id, source, source_figure_id?, rv_version, surface, server_ts,
  collector}`: **no** `anon_id`, `session_id`, `page_load_id` or `event_id`
  (who ran what is recoverable only by joining `run.program_id`). A second
  log sink routes them to their own dataset (`rv_programs`), so code text can
  get tighter access than the event table. The collector skips a
  `program_id` it has already written in this instance's lifetime (bounded
  in-memory set); the `rv_programs.programs` view keeps the first row per
  `program_id` (`first_seen`). In file mode they go to
  `RV_EVENTS_PROGRAMS_FILE` (default `./rv-programs.jsonl`).
- Not through the collector: the compile service (`rustviz-api`, behind
  `RV_LOG_RUN_RESULTS=1`, off by default) logs its own authoritative
  server-side `run_result` lines, label `rv_stream: "rv_run_results"`,
  message `rv_run_result`, fields `rv.{event, source:"server", page_load_id,
  outcome, error_kind, error_codes[], crash_at, latency_ms,
  code_hash:"sha256:<program_id>", code_bytes, line_count}`. A third log sink
  routes them to `rv_research`; view `rv_research.server_run_results` joins
  to client events on `page_load_id` and `program_id`.

## 5. Samples

`curl` these against a local collector
(`RV_EVENTS_SINK=file cargo run -p rustviz-events`, port 8090):

```sh
curl -s localhost:8090/v1/events -H 'Origin: http://localhost:3000' \
  -H 'Content-Type: text/plain;charset=UTF-8' --data-binary @batch.json
```

Tier 1 (`notice` mode), tutorial page with a figure hover and a pointer chunk:

```json
{"events":[
 {"schema":1,"event_id":"0b6b3c1e-6f0e-4a59-9a43-5d1f4c2a9e01","event":"page_view","seq":0,
  "client_ts":1763370000000,"surface":"tutorial","page":"/rustviz/tutorial/ch04-01-what-is-ownership.html",
  "page_load_id":"7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f","session_id":"2b3c4d5e-6f70-4812-9a3b-4c5d6e7f8091","anon_id":"11111111-2222-4333-8444-555555555555","consent_mode":"notice","tier":1,"rv_version":"2.1.0",
  "client_version":"0.1.0","viewport":"1400x900","lang":"en-US","tz_offset":-300,"props":{"nav_type":"navigate"}},
 {"schema":1,"event_id":"0b6b3c1e-6f0e-4a59-9a43-5d1f4c2a9e02","event":"figure_view","seq":1,
  "client_ts":1763370004000,"surface":"tutorial","page":"/rustviz/tutorial/ch04-01-what-is-ownership.html",
  "page_load_id":"7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f","session_id":"2b3c4d5e-6f70-4812-9a3b-4c5d6e7f8091","anon_id":"11111111-2222-4333-8444-555555555555","consent_mode":"notice","tier":1,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"figure_id":"3fa2c1d0b9e87a65","figure_index":0,"enter_ts":1763370001000,"leave_ts":1763370004000,"visible_ms":3000}},
 {"schema":1,"event_id":"0b6b3c1e-6f0e-4a59-9a43-5d1f4c2a9e03","event":"tooltip_dwell","seq":2,
  "client_ts":1763370003500,"surface":"tutorial","page":"/rustviz/tutorial/ch04-01-what-is-ownership.html",
  "page_load_id":"7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f","session_id":"2b3c4d5e-6f70-4812-9a3b-4c5d6e7f8091","anon_id":"11111111-2222-4333-8444-555555555555","consent_mode":"notice","tier":1,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"figure_id":"3fa2c1d0b9e87a65","element_id":"s:L5:move-out","element_kind":"arrow","event_kind":"move",
           "var":"s","line":5,"enter_ts":1763370002700,"leave_ts":1763370003500,"dwell_ms":800}},
 {"schema":1,"event_id":"0b6b3c1e-6f0e-4a59-9a43-5d1f4c2a9e04","event":"pointer_trace","seq":3,
  "client_ts":1763370003600,"surface":"tutorial","page":"/rustviz/tutorial/ch04-01-what-is-ownership.html",
  "page_load_id":"7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f","session_id":"2b3c4d5e-6f70-4812-9a3b-4c5d6e7f8091","anon_id":"11111111-2222-4333-8444-555555555555","consent_mode":"notice","tier":1,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"figure_id":"3fa2c1d0b9e87a65","chunk":0,"t0":1763370002500,"input":"mouse",
           "samples":[[0,120.5,40.0],[100,121.0,44.5],[200,122.5,52.0]]}},
 {"schema":1,"event_id":"0b6b3c1e-6f0e-4a59-9a43-5d1f4c2a9e05","event":"page_hide","seq":4,
  "client_ts":1763370060000,"surface":"tutorial","page":"/rustviz/tutorial/ch04-01-what-is-ownership.html",
  "page_load_id":"7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f","session_id":"2b3c4d5e-6f70-4812-9a3b-4c5d6e7f8091","anon_id":"11111111-2222-4333-8444-555555555555","consent_mode":"notice","tier":1,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"active_ms":52000,"total_ms":60000,"max_scroll_pct":85,"reason":"hidden"}}
]}
```

Tier 2 (`opt_in` study participant), playground consent + program + run + result
(the collector needs `RV_EVENTS_ACCEPT_PROGRAMS=1` for the `program` event):

```json
{"events":[
 {"schema":1,"event_id":"5c1d2e3f-0000-4000-8000-000000000001","event":"consent","seq":0,
  "client_ts":1763371000000,"surface":"playground","page":"/rustviz/playground/",
  "page_load_id":"9e8d7c6b-5a49-4382-a1b0-c9d8e7f6a5b4","session_id":"1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
  "anon_id":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","consent_mode":"opt_in","tier":2,"rv_version":"2.1.0","client_version":"0.1.0",
  "cohort":"eecs490-w27","props":{"consent_version":"2026-11","choice":"opt_in","age_18_plus":true}},
 {"schema":1,"event_id":"5c1d2e3f-0000-4000-8000-000000000004","event":"program","seq":1,
  "client_ts":1763371004900,"surface":"playground","page":"/rustviz/playground/",
  "page_load_id":"9e8d7c6b-5a49-4382-a1b0-c9d8e7f6a5b4","session_id":"1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
  "anon_id":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","consent_mode":"opt_in","tier":2,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"program_id":"3feb7add7ab31b1344a8b4996fe291e2ef4a53bb842d82eb03ae1748510a5ad5",
           "source":"fn main() {\n    let s = String::from(\"hi\");\n    let t = s;\n    println!(\"{}\", s);\n}\n"}},
 {"schema":1,"event_id":"5c1d2e3f-0000-4000-8000-000000000002","event":"run","seq":2,
  "client_ts":1763371005000,"surface":"playground","page":"/rustviz/playground/",
  "page_load_id":"9e8d7c6b-5a49-4382-a1b0-c9d8e7f6a5b4","session_id":"1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
  "anon_id":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","consent_mode":"opt_in","tier":2,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"run_id":"c0ffee00-1111-4222-8333-444455556666",
           "program_id":"3feb7add7ab31b1344a8b4996fe291e2ef4a53bb842d82eb03ae1748510a5ad5",
           "line_count":4,"example":"move-string"}},
 {"schema":1,"event_id":"5c1d2e3f-0000-4000-8000-000000000003","event":"run_result","seq":3,
  "client_ts":1763371007000,"surface":"playground","page":"/rustviz/playground/",
  "page_load_id":"9e8d7c6b-5a49-4382-a1b0-c9d8e7f6a5b4","session_id":"1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
  "anon_id":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee","consent_mode":"opt_in","tier":2,"rv_version":"2.1.0","client_version":"0.1.0",
  "props":{"run_id":"c0ffee00-1111-4222-8333-444455556666",
           "program_id":"3feb7add7ab31b1344a8b4996fe291e2ef4a53bb842d82eb03ae1748510a5ad5","outcome":"compile_error",
           "error_codes":["E0382"],"error_kind":"does-not-compile","latency_ms":1840,"cached":false,"http_status":422}}
]}
```

Expected response to each: `{"accepted":5,"rejected":[]}` /
`{"accepted":4,"rejected":[]}`.
