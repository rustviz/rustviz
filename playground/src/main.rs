use actix_cors::Cors;
use actix_files::Files;
use actix_governor::{Governor, GovernorConfigBuilder, KeyExtractor, SimpleKeyExtractionError};
use actix_web::{
    dev::ServiceRequest, error, http::header, middleware::Logger, web, App, HttpRequest,
    HttpResponse, HttpServer,
};
use std::net::IpAddr;
use rustviz_lib::{error_kind, ErrorKind, Rustviz};
use serde::{Deserialize, Serialize};

/// Hard cap on the JSON body Actix will accept on /submit-code.
/// 16 KiB is comfortably above the largest hand-written teaching example
/// while leaving no headroom for an attacker to ship a hostile macro corpus.
const MAX_BODY_BYTES: usize = 16 * 1024;

/// Same cap, applied to the inner `code` string after JSON parsing. Belt and
/// suspenders — the body limit is the actual gate, but checking again here
/// gives us a clean error message instead of a silent truncation if the
/// frontend ever stops constraining input.
const MAX_CODE_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
struct SubmitCodePayload {
    code: String,
}

#[derive(Deserialize, Serialize)]
struct SubmitResponse {
    code_panel: String,
    timeline_panel: String,
}

async fn submit_code(req: HttpRequest, payload: web::Json<SubmitCodePayload>) -> HttpResponse {
    let started = std::time::Instant::now();
    let code = &payload.code;
    if code.len() > MAX_CODE_BYTES {
        return HttpResponse::PayloadTooLarge()
            .body(format!("code must be at most {} bytes", MAX_CODE_BYTES));
    }

    // Rustviz::new blocks for the whole compile (up to the runner
    // timeout). Run it on actix's blocking pool so the async workers keep
    // serving other requests (and the platform health check) meanwhile.
    let code = code.clone();
    let code_for_log = log_run_results().then(|| code.clone());
    let result = web::block(move || {
        Rustviz::new(&code).map_err(|e| (error_kind(e.as_ref()), e.to_string()))
    })
    .await;
    if let Some(code) = code_for_log {
        let outcome = match &result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err((kind, msg))) => Err((*kind, msg.as_str())),
            Err(_) => Err((ErrorKind::Other, "")),
        };
        println!(
            "{}",
            run_result_line(page_load_id(&req), &code, outcome, started.elapsed().as_millis())
        );
    }
    match result {
        Ok(Ok(rv)) => HttpResponse::Ok().json(SubmitResponse {
            code_panel: rv.code_panel_string(),
            timeline_panel: rv.timeline_panel_string(),
        }),
        Ok(Err((kind, msg))) => error_response(kind, msg),
        Err(e) => HttpResponse::from_error(error::ErrorInternalServerError(e)),
    }
}

/// Response header naming the [`ErrorKind`] of a failed /submit-code
/// (`does-not-compile`, `unsupported`, `timeout`, ...). The body stays the
/// plain-text message, so frontends that predate the header still show it.
const ERROR_KIND_HEADER: &str = "X-RustViz-Error-Kind";

fn error_response(kind: ErrorKind, msg: String) -> HttpResponse {
    let mut resp = match kind {
        // The student's program, not the service: 422 Unprocessable Content.
        ErrorKind::DoesNotCompile => HttpResponse::UnprocessableEntity(),
        ErrorKind::Unsupported => {
            // Which constructs crash the plugin, for triage. The message
            // names the plugin source location, not the user's code.
            eprintln!("unsupported: {}", msg);
            HttpResponse::UnprocessableEntity()
        }
        ErrorKind::Timeout => HttpResponse::GatewayTimeout(),
        ErrorKind::SandboxUnavailable => {
            eprintln!("runner failure: {}", msg);
            return HttpResponse::ServiceUnavailable()
                .insert_header((ERROR_KIND_HEADER, kind.as_str()))
                .body("compile sandbox unavailable; try again shortly");
        }
        _ => HttpResponse::InternalServerError(),
    };
    resp.insert_header((ERROR_KIND_HEADER, kind.as_str()))
        .content_type("text/plain; charset=utf-8")
        .body(msg)
}

/// Access-log line for each request: request line, status, response size
/// and time taken. Deliberately *not* actix's default format, which also
/// prints the client IP (`%a`), the Referer and the User-Agent: the service
/// never writes a visitor's IP or browser string anywhere (see SECURITY.md,
/// "No IP addresses in logs"). The rate limiter still keys on the IP in
/// memory only.
const ACCESS_LOG_FORMAT: &str = "\"%r\" %s %b %Dms";

/// Request header in which the playground frontend sends its telemetry
/// page-load id (a random UUID, only while the visitor's usage logging is
/// on), so a `run_result` line below can be joined with the client's events.
const PAGE_LOAD_HEADER: &str = "X-RV-Page-Load";

/// `RV_LOG_RUN_RESULTS=1` prints one structured `run_result` line per
/// /submit-code on stdout (off by default). The line carries no IP address
/// and no program text: the outcome, rustc error codes, the crash location,
/// the latency, the program's size and SHA-256, and the page-load id if
/// the request had one. Cloud Logging picks it up as a structured entry.
fn log_run_results() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("RV_LOG_RUN_RESULTS").is_ok_and(|v| v == "1"))
}

/// The page-load id header, if it is a well-formed lowercase UUID.
fn page_load_id(req: &HttpRequest) -> Option<String> {
    let v = req.headers().get(PAGE_LOAD_HEADER)?.to_str().ok()?;
    let ok = v.len() == 36
        && v.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        });
    ok.then(|| v.to_string())
}

/// One `run_result` log line (JSON). `outcome` is the compile's result:
/// the error kind and message on failure (only error codes and the crash
/// location are taken from the message, never its text).
fn run_result_line(
    page_load_id: Option<String>,
    code: &str,
    outcome: Result<(), (ErrorKind, &str)>,
    latency_ms: u128,
) -> String {
    use sha2::{Digest, Sha256};
    let (name, kind, codes, crash_at) = match outcome {
        Ok(()) => ("ok", None, Vec::new(), None),
        Err((kind, msg)) => {
            let name = match kind {
                ErrorKind::DoesNotCompile => "compile_error",
                ErrorKind::Unsupported if crash_location(msg).is_some() => "crash",
                ErrorKind::Unsupported => "refused",
                ErrorKind::Timeout => "timeout",
                _ => "server_error",
            };
            let codes = if kind == ErrorKind::DoesNotCompile { error_codes(msg) } else { Vec::new() };
            (name, Some(kind.as_str()), codes, crash_location(msg))
        }
    };
    let hash: String = Sha256::digest(code.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
    serde_json::json!({
        "severity": "INFO",
        "message": "rv_run_result",
        "logging.googleapis.com/labels": { "rv_stream": "rv_run_results" },
        "rv": {
            "event": "run_result",
            "source": "server",
            "page_load_id": page_load_id,
            "outcome": name,
            "error_kind": kind,
            "error_codes": codes,
            "crash_at": crash_at,
            "latency_ms": latency_ms as u64,
            "code_hash": format!("sha256:{}", hash),
            "code_bytes": code.len(),
            "line_count": code.lines().count(),
        }
    })
    .to_string()
}

/// rustc error codes (`error[E0382]`) in a diagnostic, deduplicated.
fn error_codes(msg: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, _) in msg.match_indices("error[E") {
        let code = &msg[i + 6..];
        if code.len() >= 6 && code[1..5].bytes().all(|b| b.is_ascii_digit()) && &code[5..6] == "]" {
            let c = code[..5].to_string();
            if !out.contains(&c) && out.len() < 32 {
                out.push(c);
            }
        }
    }
    out
}

/// `expr_visitor.rs:862` from the plugin's "(internal error at
/// expr_visitor.rs:862: ...)" message.
fn crash_location(msg: &str) -> Option<String> {
    let rest = &msg[msg.find("(internal error at ")? + "(internal error at ".len()..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || "_.-:".contains(c)))?;
    let loc = rest[..end].trim_end_matches(':');
    (loc.contains(".rs:")).then(|| loc.to_string())
}

/// Rate-limit key: the real client IP.
///
/// Behind a platform proxy the TCP peer is the proxy, so keying on it
/// (actix-governor's default) puts every user in one shared bucket. When
/// `RV_CLIENT_IP_HEADER` names a header the platform sets, we key on that
/// instead:
///
/// * Fly: `Fly-Client-IP` (single value, overwritten by Fly's proxy).
/// * Cloud Run: `X-Forwarded-For`. Google's front end *appends* the client
///   address, so clients can prepend junk but not forge the last entry; we
///   take the rightmost value.
///
/// Unset (local dev) falls back to the peer address.
#[derive(Clone)]
struct ClientIp {
    header: Option<header::HeaderName>,
}

impl KeyExtractor for ClientIp {
    type Key = IpAddr;
    type KeyExtractionError = SimpleKeyExtractionError<&'static str>;

    fn extract(&self, req: &ServiceRequest) -> Result<IpAddr, Self::KeyExtractionError> {
        let from_header = self.header.as_ref().and_then(|h| {
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
        from_header
            .or_else(|| req.peer_addr().map(|a| a.ip()))
            .ok_or_else(|| SimpleKeyExtractionError::new("could not determine client IP"))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // The playground compiles arbitrary user-supplied Rust, so it
    // MUST run the plugin in the sandboxed Docker runner unless an
    // operator explicitly opts out (local dev). The rustviz-lib
    // default flipped to `local` in PR C of the reorg — that's the
    // right choice for library callers + the CLI, but for this
    // binary specifically we want Docker-by-default to preserve
    // the prior security posture. Production (Cloud Run) sets
    // RV_RUNNER=sandbox in its image; this guard catches the
    // local-dev case where the env var is unset.
    if std::env::var("RV_RUNNER").is_err() {
        std::env::set_var("RV_RUNNER", "docker");
    }

    // Cloud Run tells the container which port to listen on via $PORT.
    let bind_addr = std::env::var("RV_BIND").unwrap_or_else(|_| match std::env::var("PORT") {
        Ok(port) => format!("0.0.0.0:{}", port),
        Err(_) => "127.0.0.1:8080".to_string(),
    });

    // Per-IP token bucket. `seconds_per_replenish` controls the steady-state
    // rate (one token per N seconds); `burst_size` is the bucket depth.
    // Tunable via env so the deploy can adjust without a recompile.
    let seconds_per_replenish = std::env::var("RV_RATE_SECONDS_PER_REQUEST")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &u64| n > 0)
        .unwrap_or(2u64);
    let rate_burst = std::env::var("RV_RATE_BURST")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &u32| n > 0)
        .unwrap_or(5u32);
    let client_ip = ClientIp {
        header: std::env::var("RV_CLIENT_IP_HEADER")
            .ok()
            .and_then(|h| header::HeaderName::from_bytes(h.as_bytes()).ok()),
    };
    let governor_conf = GovernorConfigBuilder::default()
        .key_extractor(client_ip)
        .per_second(seconds_per_replenish)
        .burst_size(rate_burst)
        .finish()
        .expect("governor config");

    HttpServer::new(move || {
        // CORS allowlist. Local dev serves the SPA and the API from the
        // same origin (no preflight needed); production hosts the SPA on
        // GitHub Pages and calls /submit-code on the Cloud Run origin. That
        // cross-origin path needs explicit CORS. Vite dev server origins are listed too so
        // `npm run dev` can talk to a locally-running playground.
        //
        // The allowlist is *the* control over who can drive the API from a
        // browser; widen it sparingly. Adding a new public origin here also
        // means agreeing to absorb the compute cost of its traffic.
        let cors = Cors::default()
            .allowed_origin("https://rustviz.github.io")
            .allowed_origin("http://localhost:3000")
            .allowed_origin("http://127.0.0.1:3000")
            .allowed_methods(["GET", "POST", "OPTIONS"])
            // X-RV-Page-Load: the telemetry page-load id (PAGE_LOAD_HEADER),
            // sent only by builds with usage logging, only while it's on.
            .allowed_headers([
                header::CONTENT_TYPE,
                header::HeaderName::from_static("x-rv-page-load"),
            ])
            // Cross-origin JS can only read response headers listed here.
            .expose_headers([ERROR_KIND_HEADER])
            .max_age(3600);

        App::new()
            .wrap(Logger::new(ACCESS_LOG_FORMAT))
            .wrap(cors)
            .app_data(web::JsonConfig::default().limit(MAX_BODY_BYTES))
            .service(
                web::resource("/submit-code")
                    .wrap(Governor::new(&governor_conf))
                    .route(web::post().to(submit_code)),
            )
            // Vite emits the SPA + hashed assets/ + the ex-assets/ helper
            // scripts under frontend/dist/. Single mount handles all static
            // routes (assets/*, ex-assets/*, manifest.json, /).
            .service(Files::new("/", "./frontend/dist/").index_file("index.html"))
    })
    .bind(bind_addr)?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;

    fn kind_header(resp: &HttpResponse) -> &str {
        resp.headers()
            .get(ERROR_KIND_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }

    #[test]
    fn student_errors_are_422_with_kind_header() {
        for kind in [ErrorKind::DoesNotCompile, ErrorKind::Unsupported] {
            let resp = error_response(kind, "msg".to_string());
            assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(kind_header(&resp), kind.as_str());
        }
    }

    fn line(out: Result<(), (ErrorKind, &str)>, page: Option<&str>) -> serde_json::Value {
        let l = run_result_line(page.map(String::from), "fn main() {}\n", out, 1234);
        assert!(!l.contains('\n'), "one line: {}", l);
        serde_json::from_str(&l).unwrap()
    }

    #[test]
    fn run_result_lines_carry_outcomes_not_text() {
        let v = line(Ok(()), Some("7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f"));
        assert_eq!(v["message"], "rv_run_result");
        assert_eq!(v["rv"]["outcome"], "ok");
        assert_eq!(v["rv"]["page_load_id"], "7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f");
        assert_eq!(v["rv"]["latency_ms"], 1234);
        assert_eq!(v["rv"]["line_count"], 1);
        // sha256("fn main() {}\n")
        assert!(v["rv"]["code_hash"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(v["rv"]["code_hash"].as_str().unwrap().len(), 7 + 64);

        let msg = "This program doesn't compile...\n\nerror[E0382]: borrow of moved value: `secret_name`\n\
                   error[E0499]: x\nerror[E0382]: again";
        let v = line(Err((ErrorKind::DoesNotCompile, msg)), None);
        assert_eq!(v["rv"]["outcome"], "compile_error");
        assert_eq!(v["rv"]["error_kind"], "does-not-compile");
        assert_eq!(v["rv"]["error_codes"], serde_json::json!(["E0382", "E0499"]));
        assert!(v["rv"]["page_load_id"].is_null());
        let text = v.to_string();
        assert!(!text.contains("secret_name") && !text.contains("fn main"), "{}", text);

        let crash = "RustViz can't visualize this program yet (internal error at expr_visitor.rs:862: \
                     called `Option::unwrap()` on a `None` value). Please report it";
        let v = line(Err((ErrorKind::Unsupported, crash)), None);
        assert_eq!(v["rv"]["outcome"], "crash");
        assert_eq!(v["rv"]["crash_at"], "expr_visitor.rs:862");
        let v = line(Err((ErrorKind::Unsupported, "RustViz doesn't support async yet")), None);
        assert_eq!(v["rv"]["outcome"], "refused");
        let v = line(Err((ErrorKind::Timeout, "Visualization timed out")), None);
        assert_eq!(v["rv"]["outcome"], "timeout");
        let v = line(Err((ErrorKind::SandboxUnavailable, "x")), None);
        assert_eq!(v["rv"]["outcome"], "server_error");
    }

    /// The access log must never carry the client's address or browser:
    /// none of actix's IP / header placeholders may appear in the format.
    #[test]
    fn access_log_has_no_ip_or_user_agent() {
        for bad in ["%a", "%{r}a", "%{User-Agent}i", "%{X-Forwarded-For}i", "%{Referer}i", "%{Forwarded}i"] {
            assert!(!ACCESS_LOG_FORMAT.contains(bad), "{}", bad);
        }
        assert!(!ACCESS_LOG_FORMAT.contains("}i"), "no request headers at all");
    }

    #[test]
    fn page_load_header_must_be_a_uuid() {
        use actix_web::test::TestRequest;
        let ok = TestRequest::default()
            .insert_header((PAGE_LOAD_HEADER, "7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f"))
            .to_http_request();
        assert_eq!(page_load_id(&ok).as_deref(), Some("7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f"));
        for bad in ["", "not-a-uuid", "7D3F6A52-2F4B-4C1E-8F6A-0A1B2C3D4E5F", "1.2.3.4",
                    "7d3f6a52-2f4b-4c1e-8f6a-0a1b2c3d4e5f0"] {
            let r = TestRequest::default().insert_header((PAGE_LOAD_HEADER, bad)).to_http_request();
            assert_eq!(page_load_id(&r), None, "{:?}", bad);
        }
        assert_eq!(page_load_id(&TestRequest::default().to_http_request()), None);
    }

    #[test]
    fn service_errors_keep_their_statuses() {
        let resp = error_response(ErrorKind::Timeout, "Visualization timed out".to_string());
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(kind_header(&resp), "timeout");
        let resp = error_response(ErrorKind::SandboxUnavailable, "Sandbox unavailable: x".to_string());
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let resp = error_response(ErrorKind::Other, "boom".to_string());
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(kind_header(&resp), "other");
    }
}
