//! rustviz-events: the telemetry collector and usage dashboard.
//!
//! * `POST /v1/events`: validates a batch against schema 1 (`SCHEMA.md`),
//!   adds `server_ts`, and writes one line per accepted event to the sink.
//!   CORS is an exact-origin allow-list. The client address is used only
//!   for an in-memory rate limit (hashed, never stored); no request header
//!   is ever copied into a record or a log.
//! * `GET /dashboard` (HTML) and `GET /dashboard/data` (JSON, bearer
//!   token): aggregate usage statistics, from BigQuery or the local file.
//!
//! The binary (`main.rs`) only reads the environment into a [`Config`] and
//! starts the server; everything testable is here.

pub mod bigquery;
pub mod dashboard;
pub mod ratelimit;
pub mod schema;
pub mod sink;

use actix_web::http::header::{self, HeaderValue};
use actix_web::{web, HttpRequest, HttpResponse};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const COLLECTOR: &str = concat!("rustviz-events/", env!("CARGO_PKG_VERSION"));

pub const DEFAULT_ORIGINS: &[&str] = &[
    "https://rustviz.github.io",
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://localhost:3001",
    "http://127.0.0.1:3001",
];

/// Dashboard tokens shorter than this are treated as unset.
pub const MIN_TOKEN_LEN: usize = 16;
const PROGRAM_DEDUPE_CAP: usize = 50_000;
const DASHBOARD_HTML: &str = include_str!("dashboard.html");

#[derive(Clone, Debug, PartialEq)]
pub enum SinkKind {
    Stdout,
    File,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DashboardSource {
    None,
    File(PathBuf),
    BigQuery {
        project: String,
        dataset: String,
        token: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct Config {
    pub sink: SinkKind,
    pub events_file: PathBuf,
    pub programs_file: PathBuf,
    pub allowed_origins: Vec<String>,
    pub require_origin: bool,
    pub policy: schema::Policy,
    /// Header carrying the client address behind a proxy (Cloud Run:
    /// `X-Forwarded-For`, rightmost entry). Used for the rate limit only.
    pub client_ip_header: Option<header::HeaderName>,
    pub rate_burst: u32,
    pub rate_per_second: f64,
    pub dashboard_token: Option<String>,
    pub dashboard_source: DashboardSource,
    pub dashboard_cache: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sink: SinkKind::Stdout,
            events_file: "rv-events.jsonl".into(),
            programs_file: "rv-programs.jsonl".into(),
            allowed_origins: DEFAULT_ORIGINS.iter().map(|s| s.to_string()).collect(),
            require_origin: true,
            policy: schema::Policy::default(),
            client_ip_header: None,
            rate_burst: 60,
            rate_per_second: 1.0,
            dashboard_token: None,
            dashboard_source: DashboardSource::None,
            dashboard_cache: Duration::ZERO,
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_flag(name: &str, default: bool) -> bool {
    match env(name).as_deref() {
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        _ => default,
    }
}

impl Config {
    /// Read the `RV_EVENTS_*` environment (see the crate README).
    pub fn from_env() -> Result<Config, String> {
        let d = Config::default();
        let sink = match env("RV_EVENTS_SINK").as_deref() {
            None | Some("stdout") => SinkKind::Stdout,
            Some("file") => SinkKind::File,
            Some(other) => {
                return Err(format!(
                    "RV_EVENTS_SINK must be stdout or file, not {other:?}"
                ))
            }
        };
        let events_file: PathBuf = env("RV_EVENTS_FILE")
            .map(Into::into)
            .unwrap_or(d.events_file);
        let programs_file = env("RV_EVENTS_PROGRAMS_FILE")
            .map(Into::into)
            .unwrap_or(d.programs_file);
        let allowed_origins = match env("RV_EVENTS_ALLOWED_ORIGINS") {
            Some(list) => list
                .split(',')
                .map(|o| o.trim().trim_end_matches('/').to_string())
                .filter(|o| !o.is_empty())
                .collect(),
            None => d.allowed_origins,
        };
        let project = env("RV_EVENTS_BQ_PROJECT");
        let dashboard_source = match env("RV_EVENTS_DASHBOARD_SOURCE").as_deref() {
            Some("none") => DashboardSource::None,
            Some("file") => DashboardSource::File(events_file.clone()),
            Some("bigquery") | None if project.is_some() => DashboardSource::BigQuery {
                project: project.unwrap_or_default(),
                dataset: env("RV_EVENTS_BQ_DATASET").unwrap_or_else(|| "rv_research".into()),
                token: env("RV_EVENTS_BQ_TOKEN"),
            },
            Some("bigquery") => {
                return Err("RV_EVENTS_DASHBOARD_SOURCE=bigquery needs RV_EVENTS_BQ_PROJECT".into())
            }
            None if sink == SinkKind::File => DashboardSource::File(events_file.clone()),
            None => DashboardSource::None,
            Some(other) => {
                return Err(format!(
                    "RV_EVENTS_DASHBOARD_SOURCE must be file, bigquery or none, not {other:?}"
                ))
            }
        };
        let dashboard_cache = match dashboard_source {
            DashboardSource::BigQuery { .. } => Duration::from_secs(300),
            _ => Duration::ZERO,
        };
        let token = env("RV_EVENTS_DASHBOARD_TOKEN");
        if token.as_ref().is_some_and(|t| t.len() < MIN_TOKEN_LEN) {
            return Err(format!(
                "RV_EVENTS_DASHBOARD_TOKEN must be at least {MIN_TOKEN_LEN} characters"
            ));
        }
        Ok(Config {
            sink,
            events_file,
            programs_file,
            allowed_origins,
            require_origin: env_flag("RV_EVENTS_REQUIRE_ORIGIN", true),
            policy: schema::Policy {
                accept_programs: env_flag("RV_EVENTS_ACCEPT_PROGRAMS", false),
            },
            client_ip_header: env("RV_EVENTS_CLIENT_IP_HEADER")
                .and_then(|h| header::HeaderName::from_bytes(h.as_bytes()).ok()),
            rate_burst: env("RV_EVENTS_RATE_BURST")
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.rate_burst),
            rate_per_second: env("RV_EVENTS_RATE_PER_SECOND")
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| *v > 0.0)
                .unwrap_or(d.rate_per_second),
            dashboard_token: token,
            dashboard_source,
            dashboard_cache,
        })
    }
}

pub struct AppState {
    pub config: Config,
    sink: Box<dyn sink::Sink>,
    limiter: ratelimit::RateLimiter,
    programs_seen: Mutex<HashSet<String>>,
    bigquery: Option<bigquery::BigQuery>,
    cache: Mutex<HashMap<u32, (Instant, Arc<String>)>>,
}

impl AppState {
    pub fn new(config: Config) -> std::io::Result<AppState> {
        let sink: Box<dyn sink::Sink> = match config.sink {
            SinkKind::Stdout => Box::new(sink::StdoutSink),
            SinkKind::File => Box::new(sink::FileSink::open(
                &config.events_file,
                &config.programs_file,
            )?),
        };
        let bigquery = match &config.dashboard_source {
            DashboardSource::BigQuery {
                project,
                dataset,
                token,
            } => Some(bigquery::BigQuery::new(
                project.clone(),
                dataset.clone(),
                token.clone(),
            )),
            _ => None,
        };
        Ok(AppState {
            limiter: ratelimit::RateLimiter::new(config.rate_burst, config.rate_per_second),
            sink,
            programs_seen: Mutex::new(HashSet::new()),
            bigquery,
            cache: Mutex::new(HashMap::new()),
            config,
        })
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.config.allowed_origins.iter().any(|o| o == origin)
    }

    /// The client address, for the rate limit only.
    fn client_ip(&self, req: &HttpRequest) -> Option<IpAddr> {
        let from_header = self.config.client_ip_header.as_ref().and_then(|h| {
            req.headers()
                .get(h)?
                .to_str()
                .ok()?
                .rsplit(',')
                .next()?
                .trim()
                .parse()
                .ok()
        });
        from_header.or_else(|| req.peer_addr().map(|a| a.ip()))
    }
}

/// Register all routes. `main` and the tests both use this.
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.app_data(web::PayloadConfig::new(schema::MAX_BODY_BYTES))
        .route("/", web::get().to(index))
        .route(
            "/healthz",
            web::get().to(|| async { HttpResponse::Ok().body("ok") }),
        )
        .route("/robots.txt", web::get().to(robots))
        .route(
            "/favicon.ico",
            web::get().to(|| async { HttpResponse::NoContent().finish() }),
        )
        .route("/v1/events", web::post().to(post_events))
        .route(
            "/v1/events",
            web::method(actix_web::http::Method::OPTIONS).to(preflight),
        )
        .route("/dashboard", web::get().to(dashboard_page))
        .route("/dashboard/data", web::get().to(dashboard_data));
}

async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/plain; charset=utf-8")
        .body("rustviz-events: RustViz telemetry collector. POST /v1/events; usage dashboard at /dashboard.\n")
}

async fn robots() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/plain")
        .body("User-agent: *\nDisallow: /\n")
}

enum OriginCheck {
    /// Allowed; echo this value in Access-Control-Allow-Origin.
    Allowed(Option<HeaderValue>),
    Denied,
}

fn check_origin(req: &HttpRequest, st: &AppState) -> OriginCheck {
    match req.headers().get(header::ORIGIN) {
        Some(o) => match o.to_str() {
            Ok(s) if st.origin_allowed(s) => OriginCheck::Allowed(Some(o.clone())),
            _ => OriginCheck::Denied,
        },
        None if st.config.require_origin => OriginCheck::Denied,
        None => OriginCheck::Allowed(None),
    }
}

fn with_cors(mut resp: HttpResponse, origin: Option<HeaderValue>) -> HttpResponse {
    let h = resp.headers_mut();
    if let Some(o) = origin {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, o);
    }
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn preflight(req: HttpRequest, st: web::Data<AppState>) -> HttpResponse {
    match check_origin(&req, &st) {
        OriginCheck::Allowed(Some(origin)) => {
            let mut resp = with_cors(HttpResponse::NoContent().finish(), Some(origin));
            let h = resp.headers_mut();
            h.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("POST"),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("content-type"),
            );
            h.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("86400"),
            );
            resp
        }
        _ => with_cors(HttpResponse::Forbidden().finish(), None),
    }
}

async fn post_events(req: HttpRequest, body: web::Bytes, st: web::Data<AppState>) -> HttpResponse {
    let origin = match check_origin(&req, &st) {
        OriginCheck::Allowed(o) => o,
        OriginCheck::Denied => {
            return with_cors(HttpResponse::Forbidden().body("origin not allowed"), None)
        }
    };
    if let Some(ip) = st.client_ip(&req) {
        if !st.limiter.allow(ip) {
            return with_cors(HttpResponse::TooManyRequests().body("rate limited"), origin);
        }
    }
    let raw = match schema::parse_batch(&body) {
        Ok(events) => events,
        Err(schema::BatchError::Malformed) => {
            return with_cors(
                HttpResponse::BadRequest().body("expected {\"events\": [...]}"),
                origin,
            )
        }
        Err(schema::BatchError::TooMany) => {
            return with_cors(
                HttpResponse::PayloadTooLarge().body("too many events in one batch"),
                origin,
            )
        }
    };

    let server_ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut events = Vec::new();
    let mut programs = Vec::new();
    let mut rejected = Vec::new();
    let mut accepted = 0usize;
    for (index, ev) in raw.iter().enumerate() {
        match schema::validate(ev, &st.config.policy, &server_ts, COLLECTOR) {
            Ok(schema::Accepted::Event(rec)) => {
                accepted += 1;
                events.push(rec);
            }
            Ok(schema::Accepted::Program(rec)) => {
                accepted += 1;
                let id = rec["program_id"].as_str().unwrap_or_default().to_string();
                let mut seen = st.programs_seen.lock().unwrap_or_else(|e| e.into_inner());
                if seen.len() >= PROGRAM_DEDUPE_CAP {
                    seen.clear();
                }
                if seen.insert(id) {
                    programs.push(rec);
                }
            }
            Err(reason) => rejected.push(json!({ "index": index, "reason": reason })),
        }
    }
    if let Err(e) = st.sink.write(&events, &programs) {
        // Never echo request data here; the error is about our own sink.
        log::error!("sink write failed: {e}");
        return with_cors(
            HttpResponse::ServiceUnavailable().body("sink unavailable"),
            origin,
        );
    }
    with_cors(
        HttpResponse::Ok().json(json!({ "accepted": accepted, "rejected": rejected })),
        origin,
    )
}

async fn dashboard_page() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header((
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
             connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'",
        ))
        .insert_header((header::REFERRER_POLICY, "no-referrer"))
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .body(DASHBOARD_HTML)
}

/// Constant-time comparison (the length is not secret).
fn token_matches(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[derive(serde::Deserialize)]
struct DataQuery {
    days: Option<u32>,
}

async fn dashboard_data(
    req: HttpRequest,
    q: web::Query<DataQuery>,
    st: web::Data<AppState>,
) -> HttpResponse {
    let no_store = |mut r: HttpResponse| {
        r.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        r
    };
    let Some(expected) = st.config.dashboard_token.as_deref() else {
        return no_store(
            HttpResponse::ServiceUnavailable()
                .body("dashboard disabled: RV_EVENTS_DASHBOARD_TOKEN is not set"),
        );
    };
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    if !given.is_some_and(|g| token_matches(g, expected)) {
        return no_store(
            HttpResponse::Unauthorized()
                .insert_header((header::WWW_AUTHENTICATE, "Bearer"))
                .body("bad or missing token"),
        );
    }
    let days = q.days.unwrap_or(28).clamp(1, 366);

    if let Some((at, body)) = st
        .cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&days)
    {
        if at.elapsed() < st.config.dashboard_cache {
            return no_store(
                HttpResponse::Ok()
                    .content_type("application/json")
                    .body(body.as_str().to_owned()),
            );
        }
    }
    let st2 = st.clone();
    let result = web::block(move || -> Result<String, String> {
        let now = Utc::now();
        let d = match (&st2.config.dashboard_source, &st2.bigquery) {
            (DashboardSource::File(path), _) => dashboard::from_file(path, days, now)
                .map_err(|e| format!("reading {}: {e}", path.display()))?,
            (DashboardSource::BigQuery { .. }, Some(bq)) => bq.dashboard(days, now)?,
            _ => return Err("no dashboard data source configured".into()),
        };
        serde_json::to_string(&d).map_err(|e| e.to_string())
    })
    .await;
    match result {
        Ok(Ok(body)) => {
            st.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(days, (Instant::now(), Arc::new(body.clone())));
            no_store(
                HttpResponse::Ok()
                    .content_type("application/json")
                    .body(body),
            )
        }
        Ok(Err(e)) => {
            log::error!("dashboard: {e}");
            no_store(HttpResponse::BadGateway().body(e))
        }
        Err(e) => no_store(HttpResponse::InternalServerError().body(e.to_string())),
    }
}

/// Used by tests: parse a JSONL file into values.
pub fn read_jsonl(path: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
