//! End-to-end tests of the collector: HTTP in, JSONL out (file sink).

use actix_web::{http::header, test, web, App};
use rustviz_events::{read_jsonl, routes, AppState, Config, DashboardSource, SinkKind};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

const ORIGIN: &str = "https://rustviz.github.io";
const TOKEN: &str = "test-dashboard-token-0123456789";

struct Env {
    dir: PathBuf,
    config: Config,
}

impl Env {
    fn new() -> Env {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rv-events-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let events_file = dir.join("events.jsonl");
        let config = Config {
            sink: SinkKind::File,
            events_file: events_file.clone(),
            programs_file: dir.join("programs.jsonl"),
            dashboard_token: Some(TOKEN.into()),
            dashboard_source: DashboardSource::File(events_file),
            ..Config::default()
        };
        Env { dir, config }
    }
    fn events(&self) -> Vec<Value> {
        read_jsonl(&self.config.events_file)
    }
    fn programs(&self) -> Vec<Value> {
        read_jsonl(&self.config.programs_file)
    }
    fn raw_output(&self) -> String {
        std::fs::read_to_string(&self.config.events_file).unwrap_or_default()
            + &std::fs::read_to_string(&self.config.programs_file).unwrap_or_default()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.config.events_file);
        let _ = std::fs::remove_file(&self.config.programs_file);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

macro_rules! app {
    ($cfg:expr) => {{
        let state = web::Data::new(AppState::new($cfg).unwrap());
        test::init_service(App::new().app_data(state).configure(routes)).await
    }};
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn uuid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

/// A valid event with the given name and props; `who` picks the visitor.
fn ev(event: &str, seq: u64, who: u64, props: Value) -> Value {
    json!({
        "schema": 1,
        "event_id": uuid(seq + 1_000_000 * who + rand_salt()),
        "event": event,
        "seq": seq,
        "client_ts": now_ms(),
        "surface": "tutorial",
        "page": "/tutorial/ch04-01.html",
        "page_load_id": uuid(500 + who),
        "session_id": uuid(700 + who),
        "anon_id": uuid(900 + who),
        "consent_mode": "notice",
        "tier": 1,
        "rv_version": "2.1.0",
        "client_version": "0.1.0",
        "props": props,
    })
}

/// Distinct event ids across calls within one test process.
fn rand_salt() -> u64 {
    static N: AtomicU32 = AtomicU32::new(0);
    (N.fetch_add(1, Ordering::SeqCst) as u64) << 32
}

fn batch(events: Vec<Value>) -> Value {
    json!({ "events": events })
}

fn post(body: &Value) -> test::TestRequest {
    test::TestRequest::post()
        .uri("/v1/events")
        .insert_header((header::ORIGIN, ORIGIN))
        .insert_header((header::CONTENT_TYPE, "text/plain;charset=UTF-8"))
        .set_payload(body.to_string())
}

fn dwell(fig: &str, elem: &str) -> Value {
    let t = now_ms();
    json!({"figure_id": fig, "element_id": elem, "element_kind": "arrow", "event_kind": "move",
           "var": "s", "line": 5, "enter_ts": t - 800, "leave_ts": t, "dwell_ms": 800})
}

fn sha(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The two sample batches from SCHEMA.md section 5 are accepted in full.
#[actix_rt::test]
async fn schema_md_samples_are_accepted() {
    let env = Env::new();
    let mut cfg = env.config.clone();
    cfg.policy.accept_programs = true;
    let app = app!(cfg);
    let md = include_str!("../SCHEMA.md");
    let samples: Vec<&str> = md
        .split("```json\n")
        .skip(1)
        .map(|s| s.split("```").next().unwrap())
        .collect();
    let batches: Vec<Value> = samples
        .iter()
        .filter_map(|s| serde_json::from_str::<Value>(s).ok())
        .filter(|v| v.get("events").is_some())
        .collect();
    assert_eq!(batches.len(), 2, "expected two sample batches in SCHEMA.md");
    let mut total = 0;
    for b in &batches {
        let resp: Value = test::call_and_read_body_json(&app, post(b).to_request()).await;
        let n = b["events"].as_array().unwrap().len();
        assert_eq!(resp, json!({"accepted": n, "rejected": []}), "{b}");
        total += n;
    }
    // One of them is a program record, stored apart.
    assert_eq!(env.events().len() + env.programs().len(), total);
    assert_eq!(env.programs().len(), 1);
}

#[actix_rt::test]
async fn accepted_event_gets_server_fields_and_keeps_order_fields() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let e = ev(
        "tooltip_dwell",
        7,
        1,
        dwell("3fa2c1d0b9e87a65", "s:L5:move-out"),
    );
    let resp: Value =
        test::call_and_read_body_json(&app, post(&batch(vec![e.clone()])).to_request()).await;
    assert_eq!(resp["accepted"], 1);
    let out = env.events();
    assert_eq!(out.len(), 1);
    let r = &out[0];
    assert_eq!(r["seq"], 7);
    assert_eq!(r["event_id"], e["event_id"]);
    assert_eq!(r["props"]["element_id"], "s:L5:move-out");
    assert!(r["server_ts"].as_str().unwrap().ends_with('Z'));
    assert!(r["collector"]
        .as_str()
        .unwrap()
        .starts_with("rustviz-events/"));
    assert!(r.get("dropped_fields").is_none());

    // The plugin's wider element vocabulary and the crash location.
    let mut p = dwell("3fa2c1d0b9e87a65", "s:L5-L9:arm/1");
    p["element_kind"] = json!("stop_bar");
    p["event_kind"] = json!("owner_go_out_of_scope");
    p["state"] = json!("partial_privilege_gray");
    p["drop"] = json!(true);
    p["line_end"] = json!(9);
    let crash = json!({"run_id": uuid(5), "outcome": "crash", "crash_at": "expr_visitor.rs:862", "latency_ms": 10});
    let b = batch(vec![
        ev("tooltip_dwell", 8, 1, p.clone()),
        ev("run_result", 9, 1, crash),
    ]);
    let resp: Value = test::call_and_read_body_json(&app, post(&b).to_request()).await;
    assert_eq!(resp, json!({"accepted": 2, "rejected": []}));
    let out = env.events();
    assert_eq!(out[1]["props"], p);
    assert_eq!(out[2]["props"]["crash_at"], "expr_visitor.rs:862");
}

#[actix_rt::test]
async fn unknown_event_rejected_rest_of_batch_kept() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let b = batch(vec![
        ev("page_view", 0, 1, json!({})),
        ev("tooltip_hover", 1, 1, json!({})),
        ev("page_view", 2, 1, json!({})),
    ]);
    let resp: Value = test::call_and_read_body_json(&app, post(&b).to_request()).await;
    assert_eq!(
        resp,
        json!({"accepted": 2, "rejected": [{"index": 1, "reason": "unknown_event"}]})
    );
    assert_eq!(env.events().len(), 2);
}

#[actix_rt::test]
async fn unknown_fields_are_dropped_and_counted() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let mut e = ev(
        "page_view",
        0,
        1,
        json!({"nav_type": "reload", "user_email": "x@example.com"}),
    );
    e["ip"] = json!("203.0.113.9");
    e["user_agent"] = json!("Mozilla/5.0 Secret");
    let resp: Value = test::call_and_read_body_json(&app, post(&batch(vec![e])).to_request()).await;
    assert_eq!(resp["accepted"], 1);
    let r = &env.events()[0];
    assert_eq!(r["dropped_fields"], 3);
    assert_eq!(r["props"], json!({"nav_type": "reload"}));
    let raw = env.raw_output();
    for leaked in [
        "user_email",
        "x@example.com",
        "203.0.113.9",
        "Mozilla",
        "user_agent",
        "\"ip\"",
    ] {
        assert!(!raw.contains(leaked), "{leaked} leaked into {raw}");
    }
}

#[actix_rt::test]
async fn invalid_fields_reject_the_event() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let cases: Vec<(Value, &str)> = vec![
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["seq"] = json!("0");
                e
            },
            "invalid:seq",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["client_ts"] = json!(1_763_370_000);
                e
            },
            "invalid:client_ts",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["page"] = json!("/a.html?token=1");
                e
            },
            "invalid:page",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["schema"] = json!(2);
                e
            },
            "invalid:schema",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["anon_id"] = json!("not-a-uuid");
                e
            },
            "invalid:anon_id",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e.as_object_mut().unwrap().remove("session_id");
                e
            },
            "missing:session_id",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["consent_mode"] = json!("implied");
                e
            },
            "invalid:consent_mode",
        ),
        (
            {
                let mut e = ev("page_view", 0, 1, json!({}));
                e["surface"] = json!("book:Evil Host");
                e
            },
            "invalid:surface",
        ),
        (
            ev("tooltip_dwell", 0, 1, {
                let mut p = dwell("3FA2C1D0B9E87A65", "x");
                p["figure_id"] = json!("3FA2C1D0B9E87A65");
                p
            }),
            "invalid:figure_id",
        ),
        (
            ev(
                "tooltip_dwell",
                0,
                1,
                dwell("3fa2c1d0b9e87a65", "has space"),
            ),
            "invalid:element_id",
        ),
        (
            ev(
                "page_hide",
                0,
                1,
                json!({"active_ms": -1, "total_ms": 5, "max_scroll_pct": 10, "reason": "hidden"}),
            ),
            "invalid:active_ms",
        ),
        (
            ev(
                "pointer_trace",
                0,
                1,
                json!({"figure_id": "3fa2c1d0b9e87a65", "chunk": 0, "t0": now_ms(), "input": "mouse",
            "samples": [[0, 1.0]]}),
            ),
            "invalid:samples",
        ),
        (
            ev(
                "run_result",
                0,
                1,
                json!({"run_id": uuid(1), "outcome": "ok", "latency_ms": 5, "error_codes": ["e0382"]}),
            ),
            "invalid:error_codes",
        ),
        (
            ev(
                "figure_view",
                0,
                1,
                json!({"figure_id": "3fa2c1d0b9e87a65"}),
            ),
            "missing:figure_index",
        ),
        (
            ev(
                "run_result",
                0,
                1,
                json!({"run_id": uuid(1), "outcome": "crash", "latency_ms": 5,
            "crash_at": "/home/me/src/main.rs: line 3"}),
            ),
            "invalid:crash_at",
        ),
        (
            ev("tooltip_dwell", 0, 1, {
                let mut p = dwell("3fa2c1d0b9e87a65", "x");
                p["element_kind"] = json!("blob");
                p
            }),
            "invalid:element_kind",
        ),
        (json!([1, 2]), "not_an_object"),
    ];
    let n = cases.len();
    let (events, reasons): (Vec<Value>, Vec<&str>) = cases.into_iter().unzip();
    let resp: Value = test::call_and_read_body_json(&app, post(&batch(events)).to_request()).await;
    assert_eq!(resp["accepted"], 0, "{resp}");
    let got: Vec<&str> = resp["rejected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap())
        .collect();
    assert_eq!(got.len(), n);
    assert_eq!(got, reasons);
    assert!(env.events().is_empty());
}

#[actix_rt::test]
async fn size_caps() {
    let env = Env::new();
    let app = app!(env.config.clone());

    // One oversized event (a 30 KiB `value`... dropped field, still counts toward size).
    let mut big = ev("ui", 0, 1, json!({"action": "other"}));
    big["padding"] = json!("x".repeat(30 * 1024));
    let resp: Value = test::call_and_read_body_json(
        &app,
        post(&batch(vec![big, ev("page_view", 1, 1, json!({}))])).to_request(),
    )
    .await;
    assert_eq!(
        resp,
        json!({"accepted": 1, "rejected": [{"index": 0, "reason": "too_large"}]})
    );

    // Too many events.
    let many: Vec<Value> = (0..101).map(|i| ev("page_view", i, 1, json!({}))).collect();
    let r = test::call_service(&app, post(&batch(many)).to_request()).await;
    assert_eq!(r.status(), 413);

    // Body over 256 KiB.
    let body = json!({"events": [{"pad": "y".repeat(257 * 1024)}]});
    let r = test::call_service(&app, post(&body).to_request()).await;
    assert_eq!(r.status(), 413);

    // Malformed bodies.
    for bad in [
        "not json",
        "[]",
        "{\"events\": {}}",
        "{\"events\": [], \"extra\": 1}",
    ] {
        let req = test::TestRequest::post()
            .uri("/v1/events")
            .insert_header((header::ORIGIN, ORIGIN))
            .set_payload(bad)
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), 400, "{bad}");
    }
    assert_eq!(env.events().len(), 1);
}

/// Forwarding headers, the peer address and the user agent must never reach
/// the output, whichever header the rate limiter is configured to read.
#[actix_rt::test]
async fn no_ip_or_request_metadata_in_output() {
    for ip_header in [None, Some("x-forwarded-for")] {
        let env = Env::new();
        let mut cfg = env.config.clone();
        cfg.client_ip_header = ip_header.map(header::HeaderName::from_static);
        cfg.policy.accept_programs = true;
        let app = app!(cfg);
        let src = "fn main() { let s = String::new(); }\n";
        let b = batch(vec![
            ev("page_view", 0, 1, json!({})),
            ev(
                "tooltip_dwell",
                1,
                1,
                dwell("3fa2c1d0b9e87a65", "s:L1:bind"),
            ),
            ev(
                "program",
                2,
                1,
                json!({"program_id": sha(src), "source": src}),
            ),
        ]);
        let req = post(&b)
            .peer_addr("198.51.100.23:40000".parse().unwrap())
            .insert_header(("X-Forwarded-For", "203.0.113.77, 192.0.2.88"))
            .insert_header(("Forwarded", "for=203.0.113.78;proto=https"))
            .insert_header(("X-Real-IP", "203.0.113.79"))
            .insert_header(("X-Cloud-Trace-Context", "abc123trace/1;o=1"))
            .insert_header(("User-Agent", "Mozilla/5.0 (Macintosh) TestAgent/1.0"))
            .insert_header((
                "Referer",
                "https://rustviz.github.io/tutorial/secret-referer.html",
            ))
            .insert_header(("Cookie", "session=cookievalue"))
            .to_request();
        let resp: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(resp["accepted"], 3);
        let raw = env.raw_output();
        assert!(!raw.is_empty());
        for needle in [
            "198.51.100.23",
            "203.0.113.77",
            "203.0.113.78",
            "203.0.113.79",
            "192.0.2.88",
            "TestAgent",
            "Mozilla",
            "abc123trace",
            "secret-referer",
            "cookievalue",
        ] {
            assert!(
                !raw.contains(needle),
                "{needle} leaked (ip_header={ip_header:?})"
            );
        }
        // The program record carries no visitor ids.
        let p = &env.programs()[0];
        for id in ["anon_id", "session_id", "page_load_id", "event_id"] {
            assert!(p.get(id).is_none(), "program record has {id}");
        }
    }
}

#[actix_rt::test]
async fn cors_allow_list() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let b = batch(vec![ev("page_view", 0, 1, json!({}))]);

    // Allowed origin: echoed, with Vary.
    let r = test::call_service(&app, post(&b).to_request()).await;
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.headers().get("access-control-allow-origin").unwrap(),
        ORIGIN
    );
    assert_eq!(r.headers().get("vary").unwrap(), "Origin");
    assert!(r
        .headers()
        .get("access-control-allow-credentials")
        .is_none());
    assert!(r.headers().get("set-cookie").is_none());

    // Disallowed origin, lookalike origin, and no origin: 403, nothing written.
    for origin in [
        Some("https://evil.example"),
        Some("https://rustviz.github.io.evil.example"),
        None,
    ] {
        let mut req = test::TestRequest::post()
            .uri("/v1/events")
            .set_payload(b.to_string());
        if let Some(o) = origin {
            req = req.insert_header((header::ORIGIN, o));
        }
        let r = test::call_service(&app, req.to_request()).await;
        assert_eq!(r.status(), 403, "{origin:?}");
        assert!(r.headers().get("access-control-allow-origin").is_none());
    }
    assert_eq!(env.events().len(), 1);

    // Preflight.
    let pre = |o: &str| {
        test::TestRequest::default()
            .method(actix_web::http::Method::OPTIONS)
            .uri("/v1/events")
            .insert_header((header::ORIGIN, o.to_string()))
            .insert_header(("Access-Control-Request-Method", "POST"))
            .insert_header(("Access-Control-Request-Headers", "content-type"))
            .to_request()
    };
    let r = test::call_service(&app, pre("http://localhost:3000")).await;
    assert_eq!(r.status(), 204);
    assert_eq!(
        r.headers().get("access-control-allow-origin").unwrap(),
        "http://localhost:3000"
    );
    assert_eq!(
        r.headers().get("access-control-allow-methods").unwrap(),
        "POST"
    );
    let r = test::call_service(&app, pre("https://evil.example")).await;
    assert_eq!(r.status(), 403);

    // Configurable list; require_origin off accepts origin-less requests (dev).
    let env2 = Env::new();
    let mut cfg = env2.config.clone();
    cfg.allowed_origins = vec!["https://course.example.edu".into()];
    cfg.require_origin = false;
    let app2 = app!(cfg);
    let r = test::call_service(&app2, post(&b).to_request()).await;
    assert_eq!(r.status(), 403, "default origin no longer allowed");
    let r = test::call_service(
        &app2,
        test::TestRequest::post()
            .uri("/v1/events")
            .set_payload(b.to_string())
            .to_request(),
    )
    .await;
    assert_eq!(r.status(), 200);
}

#[actix_rt::test]
async fn tier_rules() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let mut t1 = ev("page_view", 0, 1, json!({}));
    t1["cohort"] = json!("eecs490-w27");
    let mut t2 = ev("page_view", 1, 2, json!({}));
    t2["cohort"] = json!("eecs490-w27");
    t2["tier"] = json!(2);
    let survey = json!({"survey_version": "1", "answers": {"rust_experience": "none"}});
    let s1 = ev("survey", 2, 1, survey.clone());
    let mut s2 = ev("survey", 3, 2, survey);
    s2["tier"] = json!(2);
    let resp: Value =
        test::call_and_read_body_json(&app, post(&batch(vec![t1, t2, s1, s2])).to_request()).await;
    assert_eq!(
        resp,
        json!({"accepted": 3, "rejected": [{"index": 2, "reason": "tier"}]})
    );
    let out = env.events();
    assert!(out[0].get("cohort").is_none());
    assert_eq!(out[0]["dropped_fields"], 1);
    assert_eq!(out[1]["cohort"], "eecs490-w27");
}

#[actix_rt::test]
async fn program_records() {
    let src = "fn main() {\n    let s = String::from(\"hi\");\n}\n";
    let prog = |seq| {
        ev(
            "program",
            seq,
            1,
            json!({"program_id": sha(src), "source": src}),
        )
    };

    // Off by default.
    let env = Env::new();
    let app = app!(env.config.clone());
    let resp: Value =
        test::call_and_read_body_json(&app, post(&batch(vec![prog(0)])).to_request()).await;
    assert_eq!(resp["rejected"][0]["reason"], "programs_disabled");
    assert!(env.programs().is_empty());

    let env = Env::new();
    let mut cfg = env.config.clone();
    cfg.policy.accept_programs = true;
    let app = app!(cfg);
    let wrong = ev(
        "program",
        9,
        1,
        json!({"program_id": sha("other"), "source": src}),
    );
    let big_src = "x".repeat(64 * 1024 + 1);
    let too_big = ev(
        "program",
        10,
        1,
        json!({"program_id": sha(&big_src), "source": big_src}),
    );
    let resp: Value = test::call_and_read_body_json(
        &app,
        post(&batch(vec![prog(0), wrong, prog(1), too_big])).to_request(),
    )
    .await;
    assert_eq!(resp["accepted"], 2);
    assert_eq!(resp["rejected"][0]["reason"], "invalid:program_id");
    assert_eq!(resp["rejected"][1]["reason"], "invalid:source");
    // Second copy deduplicated in-instance.
    let resp: Value =
        test::call_and_read_body_json(&app, post(&batch(vec![prog(2)])).to_request()).await;
    assert_eq!(resp["accepted"], 1);
    let p = env.programs();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0]["source"], src);
    assert_eq!(p[0]["program_id"], sha(src));
    // Program text never goes to the event stream.
    assert!(env.events().is_empty());

    // A run may only reference the program by id.
    let mut run = ev(
        "run",
        3,
        1,
        json!({"run_id": uuid(1), "program_id": sha(src), "line_count": 3}),
    );
    run["props"]["code"] = json!(src);
    let resp: Value =
        test::call_and_read_body_json(&app, post(&batch(vec![run])).to_request()).await;
    assert_eq!(resp["accepted"], 1);
    assert!(!std::fs::read_to_string(&env.config.events_file)
        .unwrap()
        .contains("String::from"));
}

/// The largest program the schema allows (64 KiB of source, escaping to
/// 128 KiB of JSON) fits in one request together with ordinary events.
#[actix_rt::test]
async fn max_program_fits_in_a_batch() {
    let env = Env::new();
    let mut cfg = env.config.clone();
    cfg.policy.accept_programs = true;
    let app = app!(cfg);
    // Worst common case for escaping: every byte is a quote or newline.
    let src: String = "\"\n".repeat(rustviz_events::schema::MAX_SOURCE_BYTES / 2);
    assert_eq!(src.len(), rustviz_events::schema::MAX_SOURCE_BYTES);
    let mut events = vec![ev(
        "program",
        0,
        1,
        json!({"program_id": sha(&src), "source": src}),
    )];
    events.extend((1..60).map(|i| {
        ev(
            "tooltip_dwell",
            i,
            1,
            dwell("3fa2c1d0b9e87a65", "s:L1:bind"),
        )
    }));
    let body = batch(events);
    let len = body.to_string().len();
    assert!(
        len > 128 * 1024 && len < rustviz_events::schema::MAX_BODY_BYTES,
        "{len}"
    );
    let resp: Value = test::call_and_read_body_json(&app, post(&body).to_request()).await;
    assert_eq!(resp, json!({"accepted": 60, "rejected": []}));
    assert_eq!(
        env.programs()[0]["source"].as_str().unwrap().len(),
        64 * 1024
    );

    // One byte more source is rejected (the event, not the request).
    let src2 = format!("{src}x");
    let resp: Value = test::call_and_read_body_json(
        &app,
        post(&batch(vec![ev(
            "program",
            99,
            1,
            json!({"program_id": sha(&src2), "source": src2}),
        )]))
        .to_request(),
    )
    .await;
    assert_eq!(resp["rejected"][0]["reason"], "invalid:source");
}

#[actix_rt::test]
async fn rate_limit() {
    let env = Env::new();
    let mut cfg = env.config.clone();
    cfg.rate_burst = 2;
    cfg.rate_per_second = 0.001;
    let app = app!(cfg);
    let b = batch(vec![]);
    let peer = "198.51.100.1:1".parse().unwrap();
    for expect in [200, 200, 429] {
        let r = test::call_service(&app, post(&b).peer_addr(peer).to_request()).await;
        assert_eq!(r.status(), expect);
    }
    let other = "198.51.100.2:1".parse().unwrap();
    let r = test::call_service(&app, post(&b).peer_addr(other).to_request()).await;
    assert_eq!(r.status(), 200);
}

#[actix_rt::test]
async fn dashboard_token() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let get = |auth: Option<&str>| {
        let mut r = test::TestRequest::get().uri("/dashboard/data");
        if let Some(a) = auth {
            r = r.insert_header((header::AUTHORIZATION, a.to_string()));
        }
        r.to_request()
    };
    assert_eq!(test::call_service(&app, get(None)).await.status(), 401);
    assert_eq!(
        test::call_service(&app, get(Some("Bearer wrong-token-wrong-token")))
            .await
            .status(),
        401
    );
    assert_eq!(
        test::call_service(&app, get(Some(&format!("Bearer {TOKEN}x"))))
            .await
            .status(),
        401
    );
    assert_eq!(
        test::call_service(&app, get(Some(TOKEN))).await.status(),
        401
    );
    let r = test::call_service(&app, get(Some(&format!("Bearer {TOKEN}")))).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers().get("cache-control").unwrap(), "no-store");

    // No token configured: the data endpoint is off, not open.
    let env2 = Env::new();
    let mut cfg = env2.config.clone();
    cfg.dashboard_token = None;
    let app2 = app!(cfg);
    assert_eq!(
        test::call_service(&app2, get(Some("Bearer anything-at-all-1234")))
            .await
            .status(),
        503
    );

    // The HTML page itself is public (it holds no data) and locked down.
    let r = test::call_service(
        &app,
        test::TestRequest::get().uri("/dashboard").to_request(),
    )
    .await;
    assert_eq!(r.status(), 200);
    assert!(r
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("connect-src 'self'"));
}

#[actix_rt::test]
async fn dashboard_aggregates_file() {
    let env = Env::new();
    let app = app!(env.config.clone());
    let t = now_ms();
    let ch1 = "/tutorial/ch04-01.html";
    let ch2 = "/tutorial/ch04-02.html";
    let fig_a = "aaaaaaaaaaaaaaaa";
    let fig_b = "bbbbbbbbbbbbbbbb";
    let at = |mut e: Value, page: &str| {
        e["page"] = json!(page);
        e
    };
    let fv = |fig: &str| json!({"figure_id": fig, "figure_index": 0, "enter_ts": t - 1000, "leave_ts": t, "visible_ms": 1000});
    let hide = |ms: u64| json!({"active_ms": ms, "total_ms": ms + 1000, "max_scroll_pct": 50, "reason": "hidden"});
    let mut events = vec![
        // visitor 1: ch1 (active 10 s, then 30 s cumulative), views fig A twice, 3 hovers
        at(ev("page_view", 0, 1, json!({})), ch1),
        at(ev("figure_view", 1, 1, fv(fig_a)), ch1),
        at(ev("figure_view", 2, 1, fv(fig_a)), ch1),
        at(ev("tooltip_dwell", 3, 1, dwell(fig_a, "s:L2:bind")), ch1),
        at(
            ev("tooltip_dwell", 4, 1, dwell(fig_a, "s:L3:move-out")),
            ch1,
        ),
        at(
            ev("tooltip_dwell", 5, 1, dwell(fig_a, "s:L3:move-out")),
            ch1,
        ),
        at(ev("page_hide", 6, 1, hide(10_000)), ch1),
        at(ev("page_hide", 7, 1, hide(30_000)), ch1),
        // visitor 2: ch1 (active 50 s), fig A once, then turns logging off
        at(ev("page_view", 0, 2, json!({})), ch1),
        at(ev("figure_view", 1, 2, fv(fig_a)), ch1),
        at(ev("tooltip_dwell", 2, 2, dwell(fig_b, "t:L4:drop")), ch1),
        at(ev("page_hide", 3, 2, hide(50_000)), ch1),
        at(
            ev(
                "withdraw",
                4,
                2,
                json!({"consent_version": "2026-11", "delete_requested": false}),
            ),
            ch1,
        ),
        // visitor 3: ch2, plus playground runs
        at(ev("page_view", 0, 3, json!({})), ch2),
    ];
    let src = |n| sha(&format!("program {n}"));
    let mut pg = |seq, run: u64, outcome: &str, codes: Value| {
        let mut r = ev(
            "run",
            seq,
            3,
            json!({"run_id": uuid(run), "program_id": src(run % 2), "line_count": 3}),
        );
        r["surface"] = json!("playground");
        r["page"] = json!("/playground/");
        let mut res = ev(
            "run_result",
            seq + 1,
            3,
            json!({"run_id": uuid(run), "outcome": outcome, "error_codes": codes, "latency_ms": 900}),
        );
        res["surface"] = json!("playground");
        res["page"] = json!("/playground/");
        events.push(r);
        events.push(res);
    };
    pg(10, 1, "ok", json!([]));
    pg(12, 2, "compile_error", json!(["E0382", "E0499"]));
    pg(14, 3, "compile_error", json!(["E0382"]));
    // Retried batch: duplicate event_ids count once.
    let dup = events[0].clone();
    events.push(dup);

    let resp: Value = test::call_and_read_body_json(&app, post(&batch(events)).to_request()).await;
    assert_eq!(resp["rejected"], json!([]), "{resp}");

    let req = test::TestRequest::get()
        .uri("/dashboard/data?days=7")
        .insert_header((header::AUTHORIZATION, format!("Bearer {TOKEN}")))
        .to_request();
    let body = test::call_and_read_body(&app, req).await;
    let text = std::str::from_utf8(&body).unwrap();
    let d: Value = serde_json::from_str(text).unwrap();

    let totals = &d["totals"];
    assert_eq!(totals["visitors"], 3);
    assert_eq!(totals["sessions"], 3);
    assert_eq!(totals["page_views"], 3);
    assert_eq!(totals["runs"], 3);
    assert_eq!(totals["distinct_programs"], 2);
    assert_eq!(totals["figure_views"], 2, "fig A in two page loads");
    assert_eq!(totals["tooltip_hovers"], 4);
    assert_eq!(totals["turned_off"], 1);
    assert!((totals["turned_off_share"].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-9);

    let chapters = d["chapters"].as_array().unwrap();
    assert_eq!(
        chapters[0],
        json!({"page": ch1, "page_views": 2, "visitors": 2, "median_active_ms": 40_000})
    );
    assert_eq!(
        chapters[1],
        json!({"page": ch2, "page_views": 1, "visitors": 1, "median_active_ms": null})
    );

    assert_eq!(
        d["outcomes"],
        json!([{"key": "compile_error", "n": 2}, {"key": "ok", "n": 1}])
    );
    assert_eq!(
        d["error_codes"],
        json!([{"key": "E0382", "n": 2}, {"key": "E0499", "n": 1}])
    );
    assert_eq!(
        d["figures"][0],
        json!({"figure_id": fig_a, "page": ch1, "views": 2, "hovers": 3})
    );
    assert_eq!(
        d["figures"][1],
        json!({"figure_id": fig_b, "page": ch1, "views": 0, "hovers": 1})
    );
    assert_eq!(d["daily"].as_array().unwrap().len(), 1);
    assert_eq!(d["daily"][0]["runs"], 3);
    assert_eq!(
        d["runs_daily"][0]["outcomes"],
        json!({"compile_error": 2, "ok": 1})
    );
    assert_eq!(d["weekly"][0]["visitors"], 3);

    // Aggregates only: no visitor, session, page-load or event id in the output.
    for who in 1..=3u64 {
        for id in [uuid(500 + who), uuid(700 + who), uuid(900 + who)] {
            assert!(!text.contains(&id), "{id} in dashboard data");
        }
    }
    assert!(!text.contains("event_id") && !text.contains("anon_id"));
}
