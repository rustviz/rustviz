//! Usage dashboard: aggregates only.
//!
//! The JSON served at `/dashboard/data` never contains an `anon_id`,
//! `session_id`, `page_load_id`, `event_id`, program text or any other
//! per-visitor value: only counts, medians, page paths, figure ids (hashes
//! of tutorial snippets) and rustc error codes. Two sources build the same
//! [`Dashboard`]: the local JSONL file (dev, aggregated here in memory) and
//! BigQuery (production, see `bigquery.rs` and `infra/bigquery/views.sql`).

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::io::{self, BufRead};
use std::path::Path;

#[derive(Serialize, Debug, Default, PartialEq)]
pub struct Totals {
    /// Distinct `anon_id`.
    pub visitors: u64,
    pub sessions: u64,
    pub page_loads: u64,
    pub page_views: u64,
    /// Playground runs with a result.
    pub runs: u64,
    /// Distinct `program_id` among runs.
    pub distinct_programs: u64,
    /// (page load, figure) pairs with at least one `figure_view`.
    pub figure_views: u64,
    pub tooltip_hovers: u64,
    /// Visitors who sent `withdraw` (turned logging off).
    pub turned_off: u64,
    /// `turned_off / visitors`, null when there are no visitors.
    pub turned_off_share: Option<f64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Day {
    pub day: String,
    pub visitors: u64,
    pub sessions: u64,
    pub page_views: u64,
    pub runs: u64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Week {
    /// Monday of the ISO week.
    pub week: String,
    pub visitors: u64,
    pub sessions: u64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Chapter {
    pub page: String,
    pub page_views: u64,
    pub visitors: u64,
    /// Median over page loads of the page's final `active_ms`.
    pub median_active_ms: Option<u64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct RunDay {
    pub day: String,
    pub total: u64,
    pub outcomes: BTreeMap<String, u64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Count {
    pub key: String,
    pub n: u64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Figure {
    pub figure_id: String,
    /// Page the figure was most often seen on.
    pub page: String,
    pub views: u64,
    pub hovers: u64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Dashboard {
    pub source: String,
    pub generated_at: String,
    pub window_days: u32,
    pub from_day: String,
    pub to_day: String,
    pub totals: Totals,
    pub daily: Vec<Day>,
    pub weekly: Vec<Week>,
    /// Tutorial pages only.
    pub chapters: Vec<Chapter>,
    pub runs_daily: Vec<RunDay>,
    pub outcomes: Vec<Count>,
    pub error_codes: Vec<Count>,
    pub figures: Vec<Figure>,
}

pub const TOP_ERROR_CODES: usize = 15;
pub const TOP_FIGURES: usize = 25;
pub const TOP_CHAPTERS: usize = 60;

pub fn window_start(today: NaiveDate, days: u32) -> NaiveDate {
    today - Duration::days(days.saturating_sub(1) as i64)
}

pub fn week_of(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

pub fn median(v: &mut [u64]) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    })
}

fn top(map: BTreeMap<String, u64>, k: usize) -> Vec<Count> {
    let mut v: Vec<Count> = map.into_iter().map(|(key, n)| Count { key, n }).collect();
    v.sort_by(|a, b| b.n.cmp(&a.n).then_with(|| a.key.cmp(&b.key)));
    v.truncate(k);
    v
}

/// Aggregate a JSONL file of canonical event records (the file sink's
/// output). Lines that don't parse are skipped. Events are deduplicated by
/// `event_id` and windowed by `server_ts` (UTC days).
pub fn from_file(path: &Path, days: u32, now: DateTime<Utc>) -> io::Result<Dashboard> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(aggregate(std::iter::empty(), days, now, "file"))
        }
        Err(e) => return Err(e),
    };
    let records = io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Value>(&l).ok());
    Ok(aggregate(records, days, now, "file"))
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k)?.as_str()
}

#[derive(Default)]
struct DayAcc {
    visitors: HashSet<String>,
    sessions: HashSet<String>,
    page_views: u64,
    runs: u64,
}

#[derive(Default)]
struct ChapterAcc {
    page_views: u64,
    visitors: HashSet<String>,
    /// page_load_id -> max active_ms
    active: BTreeMap<String, u64>,
}

#[derive(Default)]
struct FigureAcc {
    viewed_in: HashSet<String>,
    hovers: u64,
    pages: BTreeMap<String, u64>,
}

pub fn aggregate(
    records: impl Iterator<Item = Value>,
    days: u32,
    now: DateTime<Utc>,
    source: &str,
) -> Dashboard {
    let today = now.date_naive();
    let start = window_start(today, days);

    let mut seen = HashSet::new();
    let mut visitors = HashSet::new();
    let mut sessions = HashSet::new();
    let mut page_loads = HashSet::new();
    let mut turned_off = HashSet::new();
    let mut programs = HashSet::new();
    let mut page_views = 0u64;
    let mut hovers = 0u64;
    let mut daily: BTreeMap<NaiveDate, DayAcc> = BTreeMap::new();
    let mut weekly: BTreeMap<NaiveDate, (HashSet<String>, HashSet<String>)> = BTreeMap::new();
    let mut chapters: BTreeMap<String, ChapterAcc> = BTreeMap::new();
    // run_id -> (day, outcome, error codes); first result wins.
    let mut runs: BTreeMap<String, (NaiveDate, String, Vec<String>)> = BTreeMap::new();
    let mut figures: BTreeMap<String, FigureAcc> = BTreeMap::new();

    for r in records {
        let Some(id) = s(&r, "event_id") else {
            continue;
        };
        let Some(day) = s(&r, "server_ts")
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc).date_naive())
        else {
            continue;
        };
        if day < start || day > today || !seen.insert(id.to_string()) {
            continue;
        }
        let event = s(&r, "event").unwrap_or_default();
        let props = r.get("props").cloned().unwrap_or(Value::Null);
        let anon = s(&r, "anon_id").map(str::to_string);
        let session = s(&r, "session_id").map(str::to_string);
        let page_load = s(&r, "page_load_id").unwrap_or_default().to_string();
        let page = s(&r, "page").unwrap_or_default().to_string();
        let surface = s(&r, "surface").unwrap_or_default();

        let d = daily.entry(day).or_default();
        let w = weekly.entry(week_of(day)).or_default();
        if let Some(a) = &anon {
            visitors.insert(a.clone());
            d.visitors.insert(a.clone());
            w.0.insert(a.clone());
        }
        if let Some(sid) = &session {
            sessions.insert(sid.clone());
            d.sessions.insert(sid.clone());
            w.1.insert(sid.clone());
        }
        page_loads.insert(page_load.clone());

        match event {
            "page_view" => {
                page_views += 1;
                d.page_views += 1;
                if surface == "tutorial" {
                    let c = chapters.entry(page.clone()).or_default();
                    c.page_views += 1;
                    if let Some(a) = &anon {
                        c.visitors.insert(a.clone());
                    }
                }
            }
            "page_hide" if surface == "tutorial" => {
                if let Some(ms) = props.get("active_ms").and_then(Value::as_u64) {
                    let c = chapters.entry(page.clone()).or_default();
                    let e = c.active.entry(page_load.clone()).or_insert(0);
                    *e = (*e).max(ms);
                }
            }
            "run" => {
                if let Some(p) = s(&props, "program_id") {
                    programs.insert(p.to_string());
                }
            }
            "run_result" => {
                if let (Some(run), Some(outcome)) = (s(&props, "run_id"), s(&props, "outcome")) {
                    let codes = props
                        .get("error_codes")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|c| c.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                    runs.entry(run.to_string())
                        .or_insert((day, outcome.to_string(), codes));
                }
            }
            "figure_view" | "tooltip_dwell" => {
                if let Some(fid) = s(&props, "figure_id") {
                    let f = figures.entry(fid.to_string()).or_default();
                    *f.pages.entry(page.clone()).or_default() += 1;
                    if event == "figure_view" {
                        f.viewed_in.insert(page_load.clone());
                    } else {
                        f.hovers += 1;
                        hovers += 1;
                    }
                }
            }
            "withdraw" => {
                if let Some(a) = &anon {
                    turned_off.insert(a.clone());
                }
            }
            _ => {}
        }
    }

    let mut runs_daily: BTreeMap<NaiveDate, RunDay> = BTreeMap::new();
    let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
    let mut codes: BTreeMap<String, u64> = BTreeMap::new();
    for (day, outcome, errs) in runs.values() {
        daily.entry(*day).or_default().runs += 1;
        let rd = runs_daily.entry(*day).or_insert_with(|| RunDay {
            day: day.to_string(),
            total: 0,
            outcomes: BTreeMap::new(),
        });
        rd.total += 1;
        *rd.outcomes.entry(outcome.clone()).or_default() += 1;
        *outcomes.entry(outcome.clone()).or_default() += 1;
        for c in errs.iter().collect::<HashSet<_>>() {
            *codes.entry(c.clone()).or_default() += 1;
        }
    }

    let mut chapters: Vec<Chapter> = chapters
        .into_iter()
        .map(|(page, c)| Chapter {
            page,
            page_views: c.page_views,
            visitors: c.visitors.len() as u64,
            median_active_ms: median(&mut c.active.into_values().collect::<Vec<_>>()),
        })
        .collect();
    chapters.sort_by(|a, b| {
        b.page_views
            .cmp(&a.page_views)
            .then_with(|| a.page.cmp(&b.page))
    });
    chapters.truncate(TOP_CHAPTERS);

    let figure_views: u64 = figures.values().map(|f| f.viewed_in.len() as u64).sum();
    let mut figures: Vec<Figure> = figures
        .into_iter()
        .map(|(figure_id, f)| Figure {
            figure_id,
            page: f
                .pages
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                .map(|p| p.0)
                .unwrap_or_default(),
            views: f.viewed_in.len() as u64,
            hovers: f.hovers,
        })
        .collect();
    figures.sort_by(|a, b| {
        (b.views, b.hovers)
            .cmp(&(a.views, a.hovers))
            .then_with(|| a.figure_id.cmp(&b.figure_id))
    });
    figures.truncate(TOP_FIGURES);

    let visitors_n = visitors.len() as u64;
    Dashboard {
        source: source.to_string(),
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        window_days: days,
        from_day: start.to_string(),
        to_day: today.to_string(),
        totals: Totals {
            visitors: visitors_n,
            sessions: sessions.len() as u64,
            page_loads: page_loads.len() as u64,
            page_views,
            runs: runs.len() as u64,
            distinct_programs: programs.len() as u64,
            figure_views,
            tooltip_hovers: hovers,
            turned_off: turned_off.len() as u64,
            turned_off_share: (visitors_n > 0).then(|| turned_off.len() as f64 / visitors_n as f64),
        },
        daily: daily
            .into_iter()
            .map(|(day, d)| Day {
                day: day.to_string(),
                visitors: d.visitors.len() as u64,
                sessions: d.sessions.len() as u64,
                page_views: d.page_views,
                runs: d.runs,
            })
            .collect(),
        weekly: weekly
            .into_iter()
            .map(|(week, (v, s))| Week {
                week: week.to_string(),
                visitors: v.len() as u64,
                sessions: s.len() as u64,
            })
            .collect(),
        chapters,
        runs_daily: runs_daily.into_values().collect(),
        outcomes: top(outcomes, usize::MAX),
        error_codes: top(codes, TOP_ERROR_CODES),
        figures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_odd_even_empty() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [5, 1, 3]), Some(3));
        assert_eq!(median(&mut [4, 1, 3, 2]), Some(2));
    }

    #[test]
    fn weeks_start_monday() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(); // Tuesday
        assert_eq!(week_of(d).to_string(), "2026-11-16");
    }
}
