//! Dashboard data from BigQuery, via the REST API (`jobs.query`).
//!
//! Credentials: on Cloud Run, the service account's access token from the
//! metadata server. For a local run against real data, set
//! `RV_EVENTS_BQ_TOKEN` (e.g. `$(gcloud auth print-access-token)`).
//!
//! Every query reads the views in `infra/bigquery/views.sql` and returns
//! aggregates only. `maximumBytesBilled` caps each query's cost.

use crate::dashboard::{
    Chapter, Count, Dashboard, Day, Figure, RunDay, Totals, Week, TOP_CHAPTERS, TOP_ERROR_CODES,
    TOP_FIGURES,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";
/// 1 GB per query; the dashboard's queries scan a few MB at our scale.
const MAX_BYTES_BILLED: &str = "1000000000";

pub struct BigQuery {
    pub project: String,
    pub dataset: String,
    static_token: Option<String>,
    token: Mutex<Option<(String, Instant)>>,
    agent: ureq::Agent,
}

impl BigQuery {
    pub fn new(project: String, dataset: String, static_token: Option<String>) -> Self {
        BigQuery {
            project,
            dataset,
            static_token,
            token: Mutex::new(None),
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(60))
                .build(),
        }
    }

    fn access_token(&self) -> Result<String, String> {
        if let Some(t) = &self.static_token {
            return Ok(t.clone());
        }
        let mut cached = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((t, exp)) = cached.as_ref() {
            if Instant::now() < *exp {
                return Ok(t.clone());
            }
        }
        let resp: Value = self
            .agent
            .get(METADATA_TOKEN_URL)
            .set("Metadata-Flavor", "Google")
            .call()
            .map_err(|e| format!("metadata token: {e}"))?
            .into_json()
            .map_err(|e| format!("metadata token: {e}"))?;
        let token = resp["access_token"]
            .as_str()
            .ok_or("metadata token: no access_token")?
            .to_string();
        let ttl = resp["expires_in"]
            .as_u64()
            .unwrap_or(300)
            .saturating_sub(60);
        *cached = Some((token.clone(), Instant::now() + Duration::from_secs(ttl)));
        Ok(token)
    }

    /// Run one query with an INT64 `@days` parameter; rows as JSON objects.
    pub fn query(&self, sql: &str, days: u32) -> Result<Vec<Map<String, Value>>, String> {
        let token = self.access_token()?;
        let url = format!(
            "https://bigquery.googleapis.com/bigquery/v2/projects/{}/queries",
            self.project
        );
        let body = json!({
            "query": sql,
            "useLegacySql": false,
            "parameterMode": "NAMED",
            "queryParameters": [{
                "name": "days",
                "parameterType": {"type": "INT64"},
                "parameterValue": {"value": days.to_string()}
            }],
            "defaultDataset": {"projectId": self.project, "datasetId": self.dataset},
            "maximumBytesBilled": MAX_BYTES_BILLED,
            "timeoutMs": 30000,
        });
        let mut resp: Value = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .map_err(describe)?
            .into_json()
            .map_err(|e| e.to_string())?;
        // Long queries: poll getQueryResults a few times.
        for _ in 0..4 {
            if resp["jobComplete"].as_bool() == Some(true) {
                return rows(&resp);
            }
            let job = &resp["jobReference"];
            let poll = format!(
                "https://bigquery.googleapis.com/bigquery/v2/projects/{}/queries/{}?location={}&timeoutMs=30000",
                self.project,
                job["jobId"].as_str().unwrap_or_default(),
                job["location"].as_str().unwrap_or("US"),
            );
            resp = self
                .agent
                .get(&poll)
                .set("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(describe)?
                .into_json()
                .map_err(|e| e.to_string())?;
        }
        Err("BigQuery query did not finish in time".into())
    }

    pub fn dashboard(&self, days: u32, now: DateTime<Utc>) -> Result<Dashboard, String> {
        let totals = self.query(SQL_TOTALS, days)?;
        let t = totals.first().cloned().unwrap_or_default();
        let daily = self.query(SQL_DAILY, days)?;
        let weekly = self.query(SQL_WEEKLY, days)?;
        let chapters = self.query(
            &SQL_CHAPTERS.replace("{TOP}", &TOP_CHAPTERS.to_string()),
            days,
        )?;
        let runs = self.query(SQL_RUNS, days)?;
        let codes = self.query(
            &SQL_CODES.replace("{TOP}", &TOP_ERROR_CODES.to_string()),
            days,
        )?;
        let figures = self.query(
            &SQL_FIGURES.replace("{TOP}", &TOP_FIGURES.to_string()),
            days,
        )?;
        Ok(build(
            days, now, &t, &daily, &weekly, &chapters, &runs, &codes, &figures,
        ))
    }
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            format!(
                "BigQuery HTTP {code}: {}",
                body.chars().take(500).collect::<String>()
            )
        }
        other => format!("BigQuery: {other}"),
    }
}

/// Window predicate shared by every query (UTC days, today inclusive).
/// The queries spell it out (concat! can't take a const); a test checks.
#[cfg(test)]
const WINDOW: &str = "day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY)";

macro_rules! q {
    ($($s:expr),* $(,)?) => { concat!($($s),*) };
}

const SQL_TOTALS: &str = q!(
    "SELECT COUNT(DISTINCT anon_id) AS visitors, COUNT(DISTINCT session_id) AS sessions, ",
    "COUNT(DISTINCT page_load_id) AS page_loads, COUNTIF(event = 'page_view') AS page_views, ",
    "COUNT(DISTINCT IF(event = 'run_result', JSON_VALUE(props, '$.run_id'), NULL)) AS runs, ",
    "COUNT(DISTINCT IF(event = 'run', JSON_VALUE(props, '$.program_id'), NULL)) AS distinct_programs, ",
    "COUNT(DISTINCT IF(event = 'figure_view', CONCAT(page_load_id, '/', JSON_VALUE(props, '$.figure_id')), NULL)) AS figure_views, ",
    "COUNTIF(event = 'tooltip_dwell') AS tooltip_hovers, ",
    "COUNT(DISTINCT IF(event = 'withdraw', anon_id, NULL)) AS turned_off ",
    "FROM events WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY)"
);

const SQL_DAILY: &str = q!(
    "SELECT CAST(day AS STRING) AS day, visitors, sessions, page_views, runs FROM daily_usage ",
    "WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY) ORDER BY day"
);

const SQL_WEEKLY: &str = q!(
    "SELECT CAST(DATE_TRUNC(day, WEEK(MONDAY)) AS STRING) AS week, ",
    "COUNT(DISTINCT anon_id) AS visitors, COUNT(DISTINCT session_id) AS sessions FROM events ",
    "WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY) GROUP BY week ORDER BY week"
);

const SQL_CHAPTERS: &str = q!(
    "SELECT page, SUM(page_views) AS page_views, COUNT(DISTINCT anon_id) AS visitors, ",
    "APPROX_QUANTILES(active_ms, 2)[SAFE_OFFSET(1)] AS median_active_ms FROM chapter_usage ",
    "WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY) ",
    "GROUP BY page ORDER BY page_views DESC, page LIMIT {TOP}"
);

const SQL_RUNS: &str = q!(
    "SELECT CAST(day AS STRING) AS day, outcome, COUNT(*) AS n FROM playground_runs ",
    "WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY) AND outcome != 'no_result' ",
    "GROUP BY day, outcome ORDER BY day"
);

const SQL_CODES: &str = q!(
    "SELECT code AS key, COUNT(DISTINCT run_id) AS n FROM playground_runs, UNNEST(error_codes) AS code ",
    "WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY) ",
    "GROUP BY code ORDER BY n DESC, code LIMIT {TOP}"
);

const SQL_FIGURES: &str = q!(
    "SELECT figure_id, APPROX_TOP_COUNT(page, 1)[OFFSET(0)].value AS page, ",
    "COUNTIF(view_intervals > 0) AS views, SUM(hovers) AS hovers FROM figure_engagement ",
    "WHERE day >= DATE_SUB(CURRENT_DATE(), INTERVAL @days - 1 DAY) ",
    "GROUP BY figure_id ORDER BY views DESC, hovers DESC, figure_id LIMIT {TOP}"
);

#[cfg(test)]
const ALL_SQL: &[&str] = &[
    SQL_TOTALS,
    SQL_DAILY,
    SQL_WEEKLY,
    SQL_CHAPTERS,
    SQL_RUNS,
    SQL_CODES,
    SQL_FIGURES,
];

/// Convert a `jobs.query` / `getQueryResults` response to rows.
pub fn rows(resp: &Value) -> Result<Vec<Map<String, Value>>, String> {
    let fields = resp["schema"]["fields"]
        .as_array()
        .ok_or("BigQuery response without schema")?;
    let empty = Vec::new();
    let raw_rows = resp["rows"].as_array().unwrap_or(&empty);
    raw_rows
        .iter()
        .map(|row| {
            let cells = row["f"].as_array().ok_or("row without cells")?;
            Ok(fields
                .iter()
                .zip(cells)
                .map(|(f, c)| {
                    let name = f["name"].as_str().unwrap_or_default().to_string();
                    (name, cell(f, &c["v"]))
                })
                .collect())
        })
        .collect()
}

fn cell(field: &Value, v: &Value) -> Value {
    if field["mode"] == "REPEATED" {
        let scalar = json!({"type": field["type"].clone(), "fields": field["fields"].clone()});
        return Value::Array(
            v.as_array()
                .map(|a| a.iter().map(|x| cell(&scalar, &x["v"])).collect())
                .unwrap_or_default(),
        );
    }
    let Some(s) = v.as_str() else {
        return v.clone();
    };
    match field["type"].as_str() {
        Some("INTEGER" | "INT64") => s.parse::<i64>().map(Value::from).unwrap_or(Value::Null),
        Some("FLOAT" | "FLOAT64" | "NUMERIC") => {
            s.parse::<f64>().map(Value::from).unwrap_or(Value::Null)
        }
        Some("BOOLEAN" | "BOOL") => Value::Bool(s == "true"),
        _ => Value::String(s.to_string()),
    }
}

fn u(r: &Map<String, Value>, k: &str) -> u64 {
    r.get(k)
        .and_then(Value::as_i64)
        .map_or(0, |n| n.max(0) as u64)
}
fn st(r: &Map<String, Value>, k: &str) -> String {
    r.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    days: u32,
    now: DateTime<Utc>,
    t: &Map<String, Value>,
    daily: &[Map<String, Value>],
    weekly: &[Map<String, Value>],
    chapters: &[Map<String, Value>],
    runs: &[Map<String, Value>],
    codes: &[Map<String, Value>],
    figures: &[Map<String, Value>],
) -> Dashboard {
    let today = now.date_naive();
    let visitors = u(t, "visitors");
    let turned_off = u(t, "turned_off");
    let mut runs_daily: BTreeMap<String, RunDay> = BTreeMap::new();
    let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
    for r in runs {
        let day = st(r, "day");
        let (outcome, n) = (st(r, "outcome"), u(r, "n"));
        let rd = runs_daily.entry(day.clone()).or_insert_with(|| RunDay {
            day,
            total: 0,
            outcomes: BTreeMap::new(),
        });
        rd.total += n;
        *rd.outcomes.entry(outcome.clone()).or_default() += n;
        *outcomes.entry(outcome).or_default() += n;
    }
    let mut outcomes: Vec<Count> = outcomes
        .into_iter()
        .map(|(key, n)| Count { key, n })
        .collect();
    outcomes.sort_by(|a, b| b.n.cmp(&a.n).then_with(|| a.key.cmp(&b.key)));
    Dashboard {
        source: "bigquery".into(),
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        window_days: days,
        from_day: crate::dashboard::window_start(today, days).to_string(),
        to_day: today.to_string(),
        totals: Totals {
            visitors,
            sessions: u(t, "sessions"),
            page_loads: u(t, "page_loads"),
            page_views: u(t, "page_views"),
            runs: u(t, "runs"),
            distinct_programs: u(t, "distinct_programs"),
            figure_views: u(t, "figure_views"),
            tooltip_hovers: u(t, "tooltip_hovers"),
            turned_off,
            turned_off_share: (visitors > 0).then(|| turned_off as f64 / visitors as f64),
        },
        daily: daily
            .iter()
            .map(|r| Day {
                day: st(r, "day"),
                visitors: u(r, "visitors"),
                sessions: u(r, "sessions"),
                page_views: u(r, "page_views"),
                runs: u(r, "runs"),
            })
            .collect(),
        weekly: weekly
            .iter()
            .map(|r| Week {
                week: st(r, "week"),
                visitors: u(r, "visitors"),
                sessions: u(r, "sessions"),
            })
            .collect(),
        chapters: chapters
            .iter()
            .map(|r| Chapter {
                page: st(r, "page"),
                page_views: u(r, "page_views"),
                visitors: u(r, "visitors"),
                median_active_ms: r
                    .get("median_active_ms")
                    .and_then(Value::as_i64)
                    .map(|n| n.max(0) as u64),
            })
            .collect(),
        runs_daily: runs_daily.into_values().collect(),
        outcomes,
        error_codes: codes
            .iter()
            .map(|r| Count {
                key: st(r, "key"),
                n: u(r, "n"),
            })
            .collect(),
        figures: figures
            .iter()
            .take(TOP_FIGURES)
            .map(|r| Figure {
                figure_id: st(r, "figure_id"),
                page: st(r, "page"),
                views: u(r, "views"),
                hovers: u(r, "hovers"),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_query_uses_the_window_and_no_ids() {
        for sql in ALL_SQL {
            assert!(sql.contains(WINDOW), "{sql}");
            // Ids may be counted, never selected.
            for id in ["anon_id", "session_id", "page_load_id"] {
                for (i, _) in sql.match_indices(id) {
                    let before = &sql[..i];
                    assert!(
                        before.ends_with("DISTINCT ")
                            || before.ends_with("DISTINCT IF(event = 'withdraw', ")
                            || before.ends_with("CONCAT("),
                        "{id} selected in: {sql}"
                    );
                }
            }
        }
    }

    #[test]
    fn parses_query_response() {
        let resp = json!({
            "jobComplete": true,
            "schema": {"fields": [
                {"name": "day", "type": "STRING", "mode": "NULLABLE"},
                {"name": "visitors", "type": "INTEGER", "mode": "NULLABLE"},
                {"name": "codes", "type": "STRING", "mode": "REPEATED"},
                {"name": "share", "type": "FLOAT", "mode": "NULLABLE"},
                {"name": "median", "type": "INTEGER", "mode": "NULLABLE"}
            ]},
            "rows": [{"f": [
                {"v": "2026-11-17"}, {"v": "42"},
                {"v": [{"v": "E0382"}, {"v": "E0499"}]}, {"v": "0.25"}, {"v": null}
            ]}]
        });
        let r = rows(&resp).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["day"], "2026-11-17");
        assert_eq!(r[0]["visitors"], 42);
        assert_eq!(r[0]["codes"], json!(["E0382", "E0499"]));
        assert_eq!(r[0]["share"], 0.25);
        assert_eq!(r[0]["median"], Value::Null);
    }
}
